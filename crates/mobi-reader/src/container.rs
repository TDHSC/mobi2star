//! Shared, bounded MOBI container access for compiled-text and SRCS adapters.
use crate::{
    compression::{palmdoc, strip_trailing, Huff},
    header::Header,
    index::Index,
    pdb::PalmDatabase,
};
use lexicon_core::{bytes::be32, Encoding, Error, Limits, Resource, Result};
use serde::Serialize;

pub struct Container<'a> {
    pub source: &'a [u8],
    pub pdb: PalmDatabase,
    pub header: Header,
}
#[derive(Debug, Clone, Serialize)]
pub struct TextRecord {
    pub record: usize,
    pub decoded_bytes: usize,
    pub compressed_bytes: usize,
    pub trailer_bytes: usize,
}
impl<'a> Container<'a> {
    pub fn open(source: &'a [u8], limits: &Limits) -> Result<Self> {
        if source.len() > limits.input_bytes {
            return Err(Error::Limit("input byte budget".into()));
        }
        let pdb = PalmDatabase::parse(source)?;
        let header = Header::parse(pdb.record(source, 0)?)?;
        header.format_gate()?;
        Ok(Self {
            source,
            pdb,
            header,
        })
    }
    pub fn record(&self, n: usize) -> Result<&'a [u8]> {
        self.pdb.record(self.source, n)
    }
    pub fn index(&self, record: usize, limit: usize) -> Result<Index> {
        Index::parse(&self.pdb, self.source, record, limit)
    }
    /// An SRCS record contains a ZIP at its declared offset. Multiple archives
    /// need an explicit rendition merger and are rejected here.
    pub fn source_archive(&self) -> Result<Option<(usize, &'a [u8])>> {
        let mut found = None;
        for n in 1..self.pdb.records.len() {
            let bytes = self.record(n)?;
            if bytes.starts_with(b"SRCS") {
                if found.is_some() {
                    return Err(Error::Unsupported("multiple SRCS archives".into()));
                }
                let start = be32(bytes, 4)? as usize;
                if start < 8 {
                    return Err(Error::Malformed("SRCS archive overlaps its header".into()));
                }
                let zip = bytes
                    .get(start..)
                    .ok_or_else(|| Error::Malformed("SRCS offset".into()))?;
                if !zip.starts_with(b"PK\x03\x04") {
                    return Err(Error::Malformed("SRCS ZIP signature".into()));
                }
                found = Some((n, zip));
            }
        }
        Ok(found)
    }
    pub fn rawml(&self, limits: &Limits) -> Result<(Vec<u8>, Vec<TextRecord>)> {
        decode_text(&self.pdb, self.source, &self.header, limits)
    }
    /// Identify every compiled raster image, retaining record numbers and bytes.
    /// These resource formats need separate usable-output adapters.
    pub fn resources(&self) -> Result<Vec<Resource>> {
        let mut result = Vec::new();
        if let Some(first) = self.header.first_image {
            if first >= self.pdb.records.len() {
                return Err(Error::Malformed(
                    "first image record is out of range".into(),
                ));
            }
            for n in first..self.pdb.records.len() {
                let bytes = self.record(n)?;
                if [b"FONT".as_slice(), b"AUDI", b"VIDE"]
                    .iter()
                    .any(|s| bytes.starts_with(s))
                {
                    return Err(Error::Unsupported(format!(
                        "special media at record {n}; usable-output adapter required"
                    )));
                }
                if let Some((extension, media_type)) = image_type(bytes) {
                    let recindex = u32::try_from(n - first + 1)
                        .map_err(|_| Error::Limit("image record index".into()))?;
                    result.push(Resource {
                        recindex,
                        pdb_record: n,
                        source_span: self.pdb.records[n],
                        filename: format!("mobi-{recindex:06}.{extension}"),
                        media_type: media_type.into(),
                    });
                }
            }
        }
        Ok(result)
    }
}
pub fn image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("jpg", "image/jpeg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else if bytes.starts_with(b"BM") {
        Some(("bmp", "image/bmp"))
    } else {
        None
    }
}
pub fn decode_text(
    pdb: &PalmDatabase,
    source: &[u8],
    h: &Header,
    limits: &Limits,
) -> Result<(Vec<u8>, Vec<TextRecord>)> {
    if h.text_length > limits.text_bytes
        || h.text_records == 0
        || h.text_records >= pdb.records.len()
    {
        return Err(Error::Limit("declared text byte/record budget".into()));
    }
    let mut budget = limits.operations;
    let huff = if h.compression == 17480 {
        let start = h
            .huff_start
            .ok_or_else(|| Error::Malformed("HUFF record offset absent".into()))?;
        if h.huff_count < 2 || h.huff_count > pdb.records.len().saturating_sub(start) {
            return Err(Error::Malformed("HUFF/CDIC record count".into()));
        }
        let cdics = (start + 1..start + h.huff_count)
            .map(|n| pdb.record(source, n))
            .collect::<Result<Vec<_>>>()?;
        Some(Huff::parse(
            pdb.record(source, start)?,
            &cdics,
            limits.entries,
        )?)
    } else {
        None
    };
    let mut text = Vec::new();
    let mut records = Vec::new();
    for n in 1..=h.text_records {
        let raw = pdb.record(source, n)?;
        let input = strip_trailing(raw, h.extra_flags)?;
        let cap = h.text_length.saturating_sub(text.len());
        let block = match h.compression {
            1 => {
                if input.len() > cap {
                    return Err(Error::Incomplete("text exceeds declaration".into()));
                }
                input.to_vec()
            }
            2 => palmdoc(input, cap)?,
            17480 => huff
                .as_ref()
                .ok_or_else(|| Error::Malformed("missing HUFF decoder".into()))?
                .decompress(input, cap, &mut budget)?,
            n => return Err(Error::Unsupported(format!("MOBI compression {n}"))),
        };
        records.push(TextRecord {
            record: n,
            decoded_bytes: block.len(),
            compressed_bytes: input.len(),
            trailer_bytes: raw.len() - input.len(),
        });
        text.extend(block);
    }
    if text.len() != h.text_length {
        return Err(Error::Incomplete(format!(
            "declared text {}, decoded {}",
            h.text_length,
            text.len()
        )));
    }
    Encoding::from_mobi(h.encoding_number)?.decode(&text)?;
    Ok((text, records))
}
