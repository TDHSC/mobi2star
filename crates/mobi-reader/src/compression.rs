use lexicon_core::{bytes::{be16, be32, slice}, Error, Result};

pub fn strip_trailing(record: &[u8], flags: u16) -> Result<&[u8]> {
    let mut end = record.len();
    // Trailers occur in bit order; remove highest-bit trailers first.
    for bit in (1..16).rev() {
        if flags & (1 << bit) == 0 { continue; }
        let mut size = 0usize;
        let mut terminated = false;
        let mut used = 0;
        for shift in (0..28).step_by(7) {
            used += 1;
            let p = end.checked_sub(used).ok_or_else(|| Error::Malformed("truncated record trailer".into()))?;
            let byte = record[p];
            size |= usize::from(byte & 127) << shift;
            if byte & 128 != 0 { terminated = true; break; }
        }
        if !terminated || size < used || size > end { return Err(Error::Malformed("invalid record trailer length".into())); }
        end -= size;
    }
    if flags & 1 != 0 {
        let last = *record.get(end.checked_sub(1).ok_or_else(|| Error::Malformed("missing multibyte trailer".into()))?)
            .ok_or_else(|| Error::Malformed("missing multibyte trailer".into()))?;
        end = end.checked_sub(usize::from(last & 3) + 1).ok_or_else(|| Error::Malformed("invalid multibyte trailer".into()))?;
    }
    Ok(&record[..end])
}

fn append(out: &mut Vec<u8>, bytes: &[u8], cap: usize) -> Result<()> {
    if bytes.len() > cap.saturating_sub(out.len()) { return Err(Error::Limit("decompression output".into())); }
    out.extend_from_slice(bytes);
    Ok(())
}
pub fn palmdoc(input: &[u8], cap: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < input.len() {
        let b = input[p]; p += 1;
        match b {
            0 | 9..=127 => append(&mut out, &[b], cap)?,
            1..=8 => { let raw = slice(input, p, b as usize)?; append(&mut out, raw, cap)?; p += b as usize; }
            128..=191 => {
                let low = *input.get(p).ok_or_else(|| Error::Malformed("truncated PalmDOC backreference".into()))?; p += 1;
                let packed = (u16::from(b) << 8) | u16::from(low);
                let distance = usize::from((packed >> 3) & 0x7ff);
                let length = usize::from(packed & 7) + 3;
                if distance == 0 || distance > out.len() { return Err(Error::Malformed("invalid PalmDOC backreference distance".into())); }
                if length > cap.saturating_sub(out.len()) { return Err(Error::Limit("PalmDOC expansion".into())); }
                for _ in 0..length { let copied = out[out.len() - distance]; out.push(copied); }
            }
            192..=255 => append(&mut out, &[b' ', b ^ 0x80], cap)?,
        }
    }
    Ok(out)
}

#[derive(Debug)]
struct Phrase { bytes: Vec<u8>, literal: bool }
#[derive(Debug)]
pub struct Huff {
    quick: Vec<u32>,
    min: Vec<u64>,
    max: Vec<u64>,
    phrases: Vec<Phrase>,
}
impl Huff {
    pub fn parse(huff: &[u8], cdics: &[&[u8]], max_phrases: usize) -> Result<Self> {
        if slice(huff, 0, 4)? != b"HUFF" || be32(huff, 4)? < 24 { return Err(Error::Malformed("invalid HUFF header".into())); }
        let q = be32(huff, 8)? as usize;
        let l = be32(huff, 12)? as usize;
        let quick = (0..256).map(|i| be32(huff, q + i * 4)).collect::<Result<Vec<_>>>()?;
        let mut min = vec![0u64; 33];
        let mut max = vec![0u64; 33];
        for length in 1..=32 {
            let lo = u64::from(be32(huff, l + (length - 1) * 8)?);
            let hi = u64::from(be32(huff, l + (length - 1) * 8 + 4)?);
            min[length] = lo << (32 - length);
            max[length] = ((hi + 1) << (32 - length)).saturating_sub(1);
        }
        if cdics.is_empty() { return Err(Error::Malformed("missing CDIC".into())); }
        let total = be32(cdics[0], 8)? as usize;
        let width = be32(cdics[0], 12)? as usize;
        if total == 0 || total > max_phrases || !(1..=16).contains(&width) { return Err(Error::Limit("CDIC phrase table".into())); }
        let mut phrases = Vec::with_capacity(total);
        for &record in cdics {
            if slice(record, 0, 4)? != b"CDIC" || be32(record, 4)? != 16 || be32(record, 8)? as usize != total || be32(record, 12)? as usize != width {
                return Err(Error::Malformed("inconsistent CDIC header".into()));
            }
            let count = (total - phrases.len()).min(1usize << width);
            if count == 0 { return Err(Error::Malformed("extra CDIC record".into())); }
            for n in 0..count {
                let off = be16(record, 16 + 2 * n)? as usize;
                if off < count * 2 { return Err(Error::Malformed("CDIC phrase overlaps offset table".into())); }
                let size = be16(record, 16 + off)?;
                phrases.push(Phrase { bytes: slice(record, 18 + off, usize::from(size & 0x7fff))?.to_vec(), literal: size & 0x8000 != 0 });
            }
        }
        if phrases.len() != total { return Err(Error::Incomplete("CDIC phrase count mismatch".into())); }
        Ok(Self { quick, min, max, phrases })
    }
    pub fn decompress(&self, input: &[u8], cap: usize, budget: &mut usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.expand(input, &mut out, cap, budget, &mut Vec::new())?;
        Ok(out)
    }
    fn expand(&self, input: &[u8], out: &mut Vec<u8>, cap: usize, budget: &mut usize, chain: &mut Vec<usize>) -> Result<()> {
        if chain.len() > 64 { return Err(Error::Limit("HUFF recursion depth".into())); }
        let total_bits = input.len().checked_mul(8).ok_or_else(|| Error::Limit("HUFF bit length".into()))?;
        let mut position = 0;
        while position < total_bits {
            *budget = budget.checked_sub(1).ok_or_else(|| Error::Limit("HUFF operation budget".into()))?;
            // A bounded, zero-padded, MSB-first lookahead. No unchecked bit shifts.
            let mut code = 0u64;
            for bit in 0..32 {
                code <<= 1;
                let p = position + bit;
                if p < total_bits { code |= u64::from((input[p / 8] >> (7 - p % 8)) & 1); }
            }
            let q = self.quick[(code >> 24) as usize];
            let mut length = (q & 31) as usize;
            if length == 0 { return Err(Error::Malformed("HUFF zero-length code".into())); }
            let ceiling;
            if q & 0x80 != 0 {
                ceiling = ((u64::from(q >> 8) + 1) << (32 - length)) - 1;
            } else {
                while length <= 32 && code < self.min[length] { length += 1; }
                if length > 32 { return Err(Error::Malformed("HUFF code outside tables".into())); }
                ceiling = self.max[length];
            }
            // Final incomplete code is padding; total decompressed length is checked by the caller.
            if length > total_bits - position { break; }
            if code > ceiling { return Err(Error::Malformed("HUFF code above maximum".into())); }
            let phrase_id = ((ceiling - code) >> (32 - length)) as usize;
            let phrase = self.phrases.get(phrase_id).ok_or_else(|| Error::Malformed("HUFF phrase reference out of bounds".into()))?;
            position += length;
            if phrase.literal {
                append(out, &phrase.bytes, cap)?;
            } else {
                if chain.contains(&phrase_id) { return Err(Error::Malformed("cyclic HUFF phrase".into())); }
                chain.push(phrase_id);
                self.expand(&phrase.bytes, out, cap, budget, chain)?;
                chain.pop();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palmdoc_overlap_and_literals() {
        assert_eq!(palmdoc(&[b'a', 0x80, 0x0b], 100).unwrap(), b"aaaaaaa");
        assert_eq!(palmdoc(&[0xc1, 2, 1, 2, b'z'], 100).unwrap(), b" A\x01\x02z");
    }
    #[test]
    fn invalid_compression_is_not_ignored() {
        assert!(palmdoc(&[0x80], 100).is_err());
        assert!(palmdoc(&[0x80, 0x08], 100).is_err());
        assert!(palmdoc(&[b'a', 0x80, 0x0f], 3).is_err());
    }
    #[test]
    fn trailers() {
        assert_eq!(strip_trailing(b"abc\x00", 1).unwrap(), b"abc");
        assert_eq!(strip_trailing(b"abcX\x82", 2).unwrap(), b"abc");
        assert_eq!(strip_trailing(b"abc\x00X\x82", 3).unwrap(), b"abc");
        assert!(strip_trailing(b"\x7f", 2).is_err());
    }
    #[test]
    fn huffman_single_literal_and_cycle() {
        let mut h = Huff { quick: vec![(255 << 8) | 0x80 | 8; 256], min: vec![0; 33], max: vec![0; 33], phrases: vec![Phrase { bytes: b"hello".to_vec(), literal: true }] };
        assert_eq!(h.decompress(&[255], 100, &mut 100).unwrap(), b"hello");
        h.phrases[0] = Phrase { bytes: vec![255], literal: false };
        assert!(h.decompress(&[255], 100, &mut 100).is_err());
    }
}
