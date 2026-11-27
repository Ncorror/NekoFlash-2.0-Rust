//! Read a Fastboot TCP partition range into a new file.
use nekoflash_fastboot::session::{Session, TcpPackets};
use nekoflash_transport::Tcp;
use std::{
    env,
    error::Error,
    ffi::OsStr,
    io,
    net::SocketAddr,
    path::Path,
    process::ExitCode,
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;

fn number(input: &OsStr) -> Result<u64, Box<dyn Error>> {
    let text = input
        .to_str()
        .ok_or("expected a decimal or hexadecimal integer")?;
    Ok(
        match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            Some(hex) => u64::from_str_radix(hex, 16)?,
            None => text.parse()?,
        },
    )
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [peer, partition, destination, offset, size] = args.as_slice() else {
        return Err("usage: fetch_range IP:PORT PARTITION OUTPUT OFFSET SIZE".into());
    };
    let peer: SocketAddr = peer
        .to_str()
        .ok_or("expected IP:PORT or [IPv6]:PORT")?
        .parse()?;
    let partition = partition
        .to_str()
        .ok_or("expected an ASCII partition name")?;
    let offset = number(offset)?;
    let size = u32::try_from(number(size)?)?;
    let destination = Path::new(destination);
    if destination.try_exists()? {
        return Err(
            io::Error::new(io::ErrorKind::AlreadyExists, "destination already exists").into(),
        );
    }
    let directory = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = NamedTempFile::new_in(directory)?;

    let connect_by = Instant::now() + Duration::from_secs(5);
    let tcp = Tcp::connect(peer, connect_by)?;
    let mut session = Session::new(TcpPackets::handshake(tcp, connect_by)?);
    let finish_by = Instant::now() + Duration::from_secs(120);
    session.fetch_range(
        partition,
        offset,
        size,
        staged.as_file_mut(),
        finish_by,
        |_| Ok(()),
    )?;
    staged.as_file().sync_all()?;
    staged.persist_noclobber(destination)?;
    println!(
        "Fetched {size} bytes from {partition} at {offset:#x} to {}",
        destination.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fetch_range: {err}");
            ExitCode::FAILURE
        }
    }
}
