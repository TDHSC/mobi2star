use lexicon_core::{bytes::{be16, be32, slice}, hex, Encoding, Error, ExthRecord, Metadata, Result};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Header {
    pub compression: u16,
    pub text_length: usize,
    pub text_records: usize,
    pub text_record_size: usize,
    pub encryption: u16,
    pub version: u32,
    pub encoding_number: u32,
    pub orth_index: Option<usize>,
    pub infl_index: Option<usize>,
    pub extra_indexes: Vec<usize>,
    pub first_image: Option<usize>,
    pub huff_start: Option<usize>,
    pub huff_count: usize,
    pub extra_flags: u16,
    pub hybrid: bool,
    pub metadata: Metadata,
}
fn optional_record(value: u32) -> Option<usize> {
    if value == u32::MAX { None } else { Some(value as usize) }
}
impl Header {
    pub fn parse(r: &[u8]) -> Result<Self> {
        if slice(r, 16, 4)? != b"MOBI" { return Err(Error::Unsupported("missing MOBI header".into())); }
        let len = be32(r, 20)? as usize;
        if len < 116 { return Err(Error::Unsupported(format!("short/legacy MOBI header ({len} bytes)"))); }
        let end = 16usize.checked_add(len).ok_or_else(|| Error::Malformed("MOBI header overflow".into()))?;
        slice(r, 16, len)?;
        let get32 = |p| -> Result<u32> {
            if p + 4 > end { return Err(Error::Malformed("field outside declared MOBI header".into())); }
            be32(r, p)
        };
        let encoding_number = get32(28)?;
        let encoding = Encoding::from_mobi(encoding_number)?;
        let title_pos = get32(84)? as usize;
        let title_len = get32(88)? as usize;
        let mut metadata = Metadata {
            title: encoding.decode(slice(r, title_pos, title_len)?)?,
            input_language: get32(96)?, output_language: get32(100)?, ..Metadata::default()
        };
        let mut hybrid = false;
        if end >= 132 && get32(128)? & 0x40 != 0 {
            if slice(r, end, 4)? != b"EXTH" { return Err(Error::Malformed("EXTH flag set but EXTH missing".into())); }
            let exth_len = be32(r, end + 4)? as usize;
            if exth_len < 12 { return Err(Error::Malformed("short EXTH".into())); }
            let exth = slice(r, end, exth_len)?;
            let count = be32(exth, 8)? as usize;
            if count > exth_len / 8 { return Err(Error::Malformed("impossible EXTH count".into())); }
            let mut p = 12;
            for _ in 0..count {
                let kind = be32(exth, p)?;
                let size = be32(exth, p + 4)? as usize;
                if size < 8 { return Err(Error::Malformed("EXTH record shorter than its header".into())); }
                let content = slice(exth, p + 8, size - 8)?;
                if kind == 100 { metadata.authors.push(encoding.decode(content)?); }
                if kind == 503 { metadata.title = encoding.decode(content)?; }
                if kind == 121 && content.len() == 4 && be32(content, 0)? != u32::MAX { hybrid = true; }
                metadata.exth.push(ExthRecord { kind, data_hex: hex(content) });
                p += size;
            }
            if exth[p..].iter().any(|&b| b != 0) { return Err(Error::Malformed("unaccounted EXTH payload".into())); }
        }
        let mut extra_indexes = Vec::new();
        for p in (48..=76).step_by(4) {
            if let Some(n) = optional_record(get32(p)?) { extra_indexes.push(n); }
        }
        let extra_flags = if end >= 244 { be16(r, 242)? } else { 0 };
        Ok(Self {
            compression: be16(r, 0)?, text_length: be32(r, 4)? as usize,
            text_records: be16(r, 8)? as usize, text_record_size: be16(r, 10)? as usize,
            encryption: be16(r, 12)?, version: get32(36)?, encoding_number,
            orth_index: optional_record(get32(40)?), infl_index: optional_record(get32(44)?),
            extra_indexes, first_image: optional_record(get32(108)?),
            huff_start: optional_record(get32(112)?), huff_count: get32(116)? as usize,
            extra_flags, hybrid, metadata,
        })
    }
    pub fn format_gate(&self) -> Result<()> {
        if self.encryption != 0 { return Err(Error::Unsupported("encrypted/DRM input; no decryption is performed".into())); }
        if self.hybrid || self.version >= 8 { return Err(Error::Unsupported("KF8 or hybrid MOBI: do not silently discard a rendition".into())); }
        if self.version < 5 { return Err(Error::Unsupported(format!("MOBI version {}", self.version))); }
        if self.orth_index.is_none() { return Err(Error::Unsupported("no compiled orthographic dictionary index".into())); }
        if !matches!(self.compression, 1 | 2 | 17480) { return Err(Error::Unsupported(format!("compression {}", self.compression))); }
        Ok(())
    }
    pub fn conversion_gate(&self) -> Result<()> {
        self.format_gate()?;
        if !self.extra_indexes.is_empty() { return Err(Error::Unsupported("additional indexes require the SRCS backend".into())); }
        Ok(())
    }
}
