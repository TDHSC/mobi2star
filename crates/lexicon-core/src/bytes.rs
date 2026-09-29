use crate::{Error, Result};

pub fn slice(data: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    let end = start
        .checked_add(len)
        .ok_or_else(|| Error::Malformed("offset overflow".into()))?;
    data.get(start..end).ok_or_else(|| {
        Error::Malformed(format!("range {start}..{end} outside {} bytes", data.len()))
    })
}
pub fn be16(data: &[u8], at: usize) -> Result<u16> {
    let b = slice(data, at, 2)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}
pub fn be32(data: &[u8], at: usize) -> Result<u32> {
    let b = slice(data, at, 4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
pub fn be64(data: &[u8], at: usize) -> Result<u64> {
    let b = slice(data, at, 8)?;
    Ok(u64::from_be_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// MOBI's high bit terminates a big-endian base-128 integer; not LEB128.
pub fn vint(data: &[u8], cursor: &mut usize) -> Result<u32> {
    let mut value = 0u32;
    for _ in 0..5 {
        let b = *data
            .get(*cursor)
            .ok_or_else(|| Error::Malformed("truncated variable integer".into()))?;
        *cursor += 1;
        value = value
            .checked_mul(128)
            .and_then(|n| n.checked_add(u32::from(b & 0x7f)))
            .ok_or_else(|| Error::Malformed("variable integer overflow".into()))?;
        if b & 0x80 != 0 {
            return Ok(value);
        }
    }
    Err(Error::Malformed("unterminated variable integer".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endian_and_bounds() {
        assert_eq!(be32(&[0, 1, 0, 2], 0).unwrap(), 65538);
        assert!(slice(&[1], usize::MAX, 2).is_err());
        assert!(be16(&[1], 0).is_err());
    }
    #[test]
    fn variable_integers() {
        assert_eq!(vint(&[0x81], &mut 0).unwrap(), 1);
        assert_eq!(vint(&[1, 0x80], &mut 0).unwrap(), 128);
        assert_eq!(vint(&[15, 127, 127, 127, 255], &mut 0).unwrap(), u32::MAX);
        assert!(vint(&[16, 127, 127, 127, 255], &mut 0).is_err());
        assert!(vint(&[0, 0, 0], &mut 0).is_err());
    }
}
