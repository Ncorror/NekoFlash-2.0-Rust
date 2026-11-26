//! Android sparse container v1. Format and acceptance rules follow
//! AndroidSparseImage.kt and AOSP libsparse/sparse_format.h (Apache-2.0).
#![forbid(unsafe_code)]
use std::io::{self, Read, Seek, SeekFrom, Write};

const MAGIC: u32 = 0xed26ff3a;
const RAW: u16 = 0xcac1;
const FILL: u16 = 0xcac2;
const SKIP: u16 = 0xcac3;
const CRC: u16 = 0xcac4;

#[derive(Debug, PartialEq, Eq)]
pub struct SparseInfo {
    pub block_size: u32,
    pub blocks: u32,
    pub chunks: u32,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn u32_at(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}
fn u16_at(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes(b[n..n + 2].try_into().unwrap())
}

/// Inspect structure in constant memory. CRC chunks are checked for correct shape.
pub fn inspect<R: Read + Seek>(
    input: &mut R,
    mut check_cancelled: impl FnMut() -> io::Result<()>,
) -> io::Result<SparseInfo> {
    let size = input.seek(SeekFrom::End(0))?;
    input.rewind()?;
    let mut header = [0; 28];
    input.read_exact(&mut header)?;
    let header_size = u64::from(u16_at(&header, 8));
    let chunk_size = u64::from(u16_at(&header, 10));
    let block_size = u32_at(&header, 12);
    let blocks = u32_at(&header, 16);
    let chunks = u32_at(&header, 20);
    if u32_at(&header, 0) != MAGIC
        || u16_at(&header, 4) != 1
        || header_size < 28
        || header_size > size
        || chunk_size < 12
        || !block_size.is_power_of_two()
        || !(512..=i32::MAX as u32).contains(&block_size)
        || blocks == 0
        || u64::from(chunks) > (size - header_size) / chunk_size
    {
        return Err(invalid("invalid Android sparse header"));
    }
    let mut offset = header_size;
    let mut remaining = blocks;
    for _ in 0..chunks {
        check_cancelled()?;
        input.seek(SeekFrom::Start(offset))?;
        let mut h = [0; 12];
        input.read_exact(&mut h)?;
        let count = u32_at(&h, 4);
        let total = u64::from(u32_at(&h, 8));
        let payload = match u16_at(&h, 0) {
            RAW => u64::from(count) * u64::from(block_size),
            FILL => 4,
            SKIP => 0,
            CRC if count == 0 => 4,
            _ => return Err(invalid("unsupported or malformed sparse chunk")),
        };
        if total != chunk_size + payload || total > size - offset || count > remaining {
            return Err(invalid("invalid sparse chunk bounds"));
        }
        offset += total;
        remaining -= count;
    }
    if offset != size || remaining != 0 {
        return Err(invalid("incomplete or trailing sparse data"));
    }
    Ok(SparseInfo {
        block_size,
        blocks,
        chunks,
    })
}

/// Write raw input as bounded RAW/FILL chunks. Zero blocks are explicit FILLs so
/// old on-device contents are overwritten. Caller publishes output only on success.
pub fn from_raw<R: Read, W: Write + Seek>(
    input: &mut R,
    output: &mut W,
    length: u64,
    mut check_cancelled: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    const BLOCK: usize = 4096;
    if length == 0 || !length.is_multiple_of(BLOCK as u64) {
        return Err(invalid("raw image must contain whole 4096-byte blocks"));
    }
    let blocks =
        u32::try_from(length / BLOCK as u64).map_err(|_| invalid("too many sparse blocks"))?;
    if output.stream_position()? != 0 || output.seek(SeekFrom::End(0))? != 0 {
        return Err(invalid("output must be empty"));
    }
    output.write_all(&MAGIC.to_le_bytes())?;
    for v in [1u16, 0, 28, 12] {
        output.write_all(&v.to_le_bytes())?;
    }
    for v in [BLOCK as u32, blocks, 0, 0] {
        output.write_all(&v.to_le_bytes())?;
    }
    let mut buf = vec![0; 256 * 1024];
    let mut remaining = length;
    let mut chunks = 0u32;
    while remaining > 0 {
        check_cancelled()?;
        let n = remaining.min(buf.len() as u64) as usize;
        let data = &mut buf[..n];
        input.read_exact(data)?;
        let fill = data
            .as_chunks::<4>()
            .0
            .iter()
            .all(|word| word == &data[..4]);
        let (ty, payload) = if fill {
            (FILL, &data[..4])
        } else {
            (RAW, &data[..])
        };
        output.write_all(&ty.to_le_bytes())?;
        output.write_all(&0u16.to_le_bytes())?;
        output.write_all(&((n / BLOCK) as u32).to_le_bytes())?;
        output.write_all(&(12 + payload.len() as u32).to_le_bytes())?;
        output.write_all(payload)?;
        remaining -= n as u64;
        chunks += 1;
    }
    let end = output.stream_position()?;
    output.seek(SeekFrom::Start(20))?;
    output.write_all(&chunks.to_le_bytes())?;
    output.seek(SeekFrom::Start(end))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn sample() -> Vec<u8> {
        let mut raw = vec![0; 256 * 1024 + 4096];
        *raw.last_mut().unwrap() = 1;
        let mut out = Cursor::new(Vec::new());
        from_raw(&mut &raw[..], &mut out, raw.len() as u64, || Ok(())).unwrap();
        out.into_inner()
    }
    #[test]
    fn explicit_zero_fill_and_raw_round_trip() {
        let b = sample();
        assert_eq!(u16_at(&b, 28), FILL);
        assert_eq!(u16_at(&b, 44), RAW);
        assert_eq!(b[b.len() - 1], 1);
        assert_eq!(
            inspect(&mut Cursor::new(b), || Ok(())).unwrap(),
            SparseInfo {
                block_size: 4096,
                blocks: 65,
                chunks: 2
            }
        );
    }
    #[test]
    fn corruption_and_cancellation() {
        let b = sample();
        for n in [0, 27, 40, b.len() - 1] {
            assert!(inspect(&mut Cursor::new(&b[..n]), || Ok(())).is_err());
        }
        let mut corrupt = b.clone();
        corrupt.push(0);
        assert!(inspect(&mut Cursor::new(corrupt), || Ok(())).is_err());
        assert_eq!(
            inspect(&mut Cursor::new(b), || Err(
                io::ErrorKind::Interrupted.into()
            ))
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
    }
}

pub mod ext4;
