//! Length-prefixed frames: Native Messaging's framing (a 32-bit length in native byte order —
//! little-endian on every platform Omni Download runs on — then UTF-8 JSON), used on the local
//! pipe too.

use std::io::{self, Read, Write};

/// Largest message accepted from an extension. Browsers allow up to 4 GiB; real messages are a
/// few kilobytes.
pub const MAX_REQUEST: usize = 64 << 20;
/// Largest message a host may send to the browser (Chrome's limit).
pub const MAX_REPLY: usize = 1 << 20;

/// Reads one frame. `None` at the end of the stream (also when it ends inside a frame).
pub fn read_frame<R: Read>(r: &mut R, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {n} bytes (limit {max})"),
        ));
    }
    let mut buf = vec![0; n];
    match r.read_exact(&mut buf) {
        Ok(()) => Ok(Some(buf)),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn write_frame<W: Write>(w: &mut W, data: &[u8]) -> io::Result<()> {
    let n = u32::try_from(data.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    w.write_all(&n.to_le_bytes())?;
    w.write_all(data)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_frames() {
        let mut buf = Vec::new();
        write_frame(&mut buf, br#"{"id":"1","type":"ping"}"#).unwrap();
        write_frame(&mut buf, b"").unwrap();
        assert_eq!(&buf[..4], &24u32.to_le_bytes());
        let mut r = &buf[..];
        assert_eq!(
            read_frame(&mut r, 100).unwrap().unwrap(),
            br#"{"id":"1","type":"ping"}"#
        );
        assert_eq!(read_frame(&mut r, 100).unwrap().unwrap(), b"");
        assert_eq!(read_frame(&mut r, 100).unwrap(), None);
    }

    #[test]
    fn rejects_oversized_and_survives_truncation() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &[b'x'; 50]).unwrap();
        assert!(read_frame(&mut &buf[..], 10).is_err());
        assert_eq!(read_frame(&mut &buf[..20], 100).unwrap(), None);
        assert_eq!(read_frame(&mut &buf[..2], 100).unwrap(), None);
    }
}
