//! Services carried on a single authorized ADB stream. SYNC follows the AOSP
//! v1 file_sync protocol. The caller retains file selection and publication.
use crate::{
    connection::Service,
    packet_io::{Link, MAX_PAYLOAD},
};
use std::io::{self, Read, Seek, SeekFrom, Write};

fn invalid(s: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}

fn record(output: &mut impl Write, id: &[u8; 4], value: u32) -> io::Result<()> {
    let mut header = [0; 8];
    header[..4].copy_from_slice(id);
    header[4..].copy_from_slice(&value.to_le_bytes());
    output.write_all(&header)
}
fn read_record(input: &mut impl Read) -> io::Result<([u8; 4], u32)> {
    let mut header = [0; 8];
    input.read_exact(&mut header)?;
    Ok((
        header[..4].try_into().unwrap(),
        u32::from_le_bytes(header[4..].try_into().unwrap()),
    ))
}
fn path_request(output: &mut impl Write, id: &[u8; 4], path: &str) -> io::Result<()> {
    if path.is_empty() || path.len() > 1024 || path.contains('\0') {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    record(output, id, path.len() as u32)?;
    output.write_all(path.as_bytes())
}

fn remote_failure(input: &mut impl Read, len: u32) -> io::Result<io::Error> {
    if len > MAX_PAYLOAD {
        return Err(invalid("oversized SYNC failure"));
    }
    let mut msg = vec![0; len as usize];
    input.read_exact(&mut msg)?;
    Ok(io::Error::other(String::from_utf8_lossy(&msg).into_owned()))
}

/// SEND a regular file on an open sync: stream. The remote must confirm OKAY.
/// The length check stops a shortened source from being finalized.
pub fn push<S: Read + Write>(
    stream: &mut S,
    path: &str,
    source: &mut impl Read,
    length: u64,
    mtime: u32,
    mut progress: impl FnMut(u64) -> io::Result<()>,
) -> io::Result<()> {
    if path.is_empty() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    path_request(stream, b"SEND", &format!("{path},{}", 0o100644))?;
    let mut buffer = vec![0; 65536];
    let mut sent = 0;
    while sent < length {
        progress(sent)?;
        let n = (length - sent).min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..n])?;
        record(stream, b"DATA", n as u32)?;
        stream.write_all(&buffer[..n])?;
        sent += n as u64;
    }
    progress(sent)?;
    record(stream, b"DONE", mtime)?;
    match read_record(stream)? {
        (id, 0) if &id == b"OKAY" => Ok(()),
        (id, len) if &id == b"FAIL" => Err(remote_failure(stream, len)?),
        _ => Err(invalid("missing SYNC push confirmation")),
    }
}

/// RECV into a staging writer. Publish the staged file only after DONE.
pub fn pull<S: Read + Write>(
    stream: &mut S,
    path: &str,
    output: &mut impl Write,
    mut progress: impl FnMut(u64) -> io::Result<()>,
) -> io::Result<u64> {
    path_request(stream, b"RECV", path)?;
    let mut buf = vec![0; 65536];
    let mut total = 0u64;
    loop {
        let (id, len) = read_record(stream)?;
        match &id {
            b"DONE" => return Ok(total),
            b"FAIL" => return Err(remote_failure(stream, len)?),
            // Accept the same peer DATA range as AdbProtocol.kt.
            b"DATA" if len <= 256 * 1024 => {
                let mut remaining = len as usize;
                while remaining > 0 {
                    let n = remaining.min(buf.len());
                    stream.read_exact(&mut buf[..n])?;
                    output.write_all(&buf[..n])?;
                    total = total
                        .checked_add(n as u64)
                        .ok_or_else(|| invalid("SYNC length overflow"))?;
                    remaining -= n;
                    progress(total)?;
                }
            }
            _ => return Err(invalid("malformed SYNC pull reply")),
        }
    }
}

pub fn shell_stdin(stream: &mut impl Write, input: &[u8]) -> io::Result<()> {
    for chunk in input.chunks(MAX_PAYLOAD as usize) {
        shell_header(stream, 0, chunk.len() as u32)?;
        stream.write_all(chunk)?;
    }
    Ok(())
}
fn shell_header(stream: &mut impl Write, id: u8, len: u32) -> io::Result<()> {
    let mut header = [id; 5];
    header[1..].copy_from_slice(&len.to_le_bytes());
    stream.write_all(&header)
}
pub fn close_stdin(stream: &mut impl Write) -> io::Result<()> {
    shell_header(stream, 4, 0)
}

/// Decode binary stdout and stderr separately. EOF without EXIT is a failure.
pub fn shell_output(
    input: &mut impl Read,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    let mut buf = vec![0; 65536];
    loop {
        let mut header = [0; 5];
        input.read_exact(&mut header)?;
        let len = u32::from_le_bytes(header[1..].try_into().unwrap());
        if len > MAX_PAYLOAD {
            return Err(invalid("oversized shell v2 frame"));
        }
        if header[0] == 3 && len == 1 {
            input.read_exact(&mut buf[..1])?;
            return Ok(buf[0]);
        }
        let target: &mut dyn Write = match header[0] {
            1 => stdout,
            2 => stderr,
            _ => return Err(invalid("unexpected shell v2 frame")),
        };
        let mut remaining = len as usize;
        while remaining > 0 {
            let n = remaining.min(buf.len());
            input.read_exact(&mut buf[..n])?;
            target.write_all(&buf[..n])?;
            remaining -= n;
        }
    }
}

const SIDELOAD_BLOCK: u64 = 65536;
pub fn sideload_service(size: u64) -> io::Result<String> {
    if size == 0 || size.div_ceil(SIDELOAD_BLOCK) > 100_000_000 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(format!("sideload-host:{size}:{SIDELOAD_BLOCK}"))
}

/// Recovery pulls blocks by decimal index and can repeat them. DONE confirms
/// only stream completion. Recovery's installation verdict is a separate step.
pub fn sideload<T: Link>(
    stream: &mut Service<'_, T>,
    source: &mut (impl Read + Seek),
    size: u64,
    mut progress: impl FnMut(u64) -> io::Result<()>,
) -> io::Result<()> {
    sideload_service(size)?;
    let blocks = size.div_ceil(SIDELOAD_BLOCK);
    let mut acknowledged = vec![0u8; blocks.div_ceil(8) as usize];
    let mut buf = vec![0; SIDELOAD_BLOCK as usize];
    let mut served = 0;
    loop {
        let mut request = [0; 8];
        stream.read_exact(&mut request)?;
        if &request == b"DONEDONE" {
            return Ok(());
        }
        if &request == b"FAILFAIL" {
            return Err(io::Error::other("recovery refused sideload"));
        }
        let token = std::str::from_utf8(&request)
            .map_err(|_| invalid("invalid recovery block request"))?
            .trim_matches(['\0', ' ']);
        if token == "-1" {
            stream.write_empty_packet()?;
            continue;
        }
        if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("invalid recovery block index"));
        }
        let block = token
            .parse::<u64>()
            .map_err(|_| invalid("invalid recovery block index"))?;
        if block >= blocks {
            return Err(invalid("recovery requested data outside file"));
        }
        let offset = block * SIDELOAD_BLOCK;
        let n = (size - offset).min(SIDELOAD_BLOCK) as usize;
        source.seek(SeekFrom::Start(offset))?;
        source.read_exact(&mut buf[..n])?;
        stream.write_all(&buf[..n])?;
        let bit = 1 << (block % 8);
        let flags = &mut acknowledged[(block / 8) as usize];
        if *flags & bit == 0 {
            *flags |= bit;
            served += n as u64;
        }
        progress(served)?;
    }
}

impl<T: Link> crate::connection::Connection<T> {
    pub fn shell_v2(
        &mut self,
        command: &str,
        deadline: std::time::Instant,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> io::Result<u8> {
        let mut service = self.open(&format!("shell,v2,raw:{command}"), deadline)?;
        service.finish_shell_input()?;
        let exit = shell_output(&mut service, stdout, stderr)?;
        service.finish()?;
        Ok(exit)
    }
    /// A legacy shell does not report an exit code.
    pub fn shell_legacy(
        &mut self,
        command: &str,
        deadline: std::time::Instant,
        output: &mut impl Write,
    ) -> io::Result<()> {
        let mut service = self.open(&format!("shell:{command}"), deadline)?;
        io::copy(&mut service, output)?;
        service.finish()
    }
    pub fn push_file(
        &mut self,
        path: &str,
        source: &mut impl Read,
        length: u64,
        mtime: u32,
        deadline: std::time::Instant,
        progress: impl FnMut(u64) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut service = self.open("sync:", deadline)?;
        push(&mut service, path, source, length, mtime, progress)?;
        service.finish()
    }
    pub fn pull_file(
        &mut self,
        path: &str,
        output: &mut impl Write,
        deadline: std::time::Instant,
        progress: impl FnMut(u64) -> io::Result<()>,
    ) -> io::Result<u64> {
        let mut service = self.open("sync:", deadline)?;
        let n = pull(&mut service, path, output, progress)?;
        service.finish()?;
        Ok(n)
    }
    pub fn sideload_file(
        &mut self,
        source: &mut (impl Read + Seek),
        size: u64,
        deadline: std::time::Instant,
        progress: impl FnMut(u64) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut service = self.open(&sideload_service(size)?, deadline)?;
        sideload(&mut service, source, size, progress)?;
        service.finish()
    }
}
