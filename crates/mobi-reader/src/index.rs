use crate::pdb::PalmDatabase;
use lexicon_core::{
    bytes::{be16, be32, slice, vint},
    Encoding, Error, Result, Span,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub struct IndexRow {
    pub label: Vec<u8>,
    pub tags: BTreeMap<u8, Vec<u32>>,
    pub record: usize,
    pub ordinal: usize,
}
impl IndexRow {
    pub fn scalar(&self, tag: u8) -> Result<u32> {
        let values = self.tags.get(&tag).ok_or_else(|| {
            Error::Incomplete(format!("index row {} missing tag {tag}", self.ordinal))
        })?;
        if values.len() != 1 {
            return Err(Error::Unsupported(format!("tag {tag} is not a scalar")));
        }
        Ok(values[0])
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Index {
    pub meta_record: usize,
    pub declared_entries: usize,
    pub encoding: Encoding,
    pub encoding_number: u32,
    pub ordt_type: u32,
    pub ordt: Vec<u16>,
    pub descriptors: Vec<Descriptor>,
    pub rows: Vec<IndexRow>,
    pub cncx_records: Vec<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Descriptor {
    pub tag: u8,
    pub arity: u8,
    pub mask: u8,
    pub end: u8,
}

fn check_header(r: &[u8]) -> Result<usize> {
    if slice(r, 0, 4)? != b"INDX" {
        return Err(Error::Malformed("expected INDX record".into()));
    }
    let n = be32(r, 4)? as usize;
    if n < 56 {
        return Err(Error::Malformed("INDX header shorter than 56 bytes".into()));
    }
    slice(r, 0, n)?;
    Ok(n)
}

impl Index {
    pub fn parse(pdb: &PalmDatabase, source: &[u8], start: usize, limit: usize) -> Result<Self> {
        let meta = pdb.record(source, start)?;
        let hlen = check_header(meta)?;
        if slice(meta, hlen, 4)? != b"TAGX" {
            return Err(Error::Unsupported("index without TAGX descriptors".into()));
        }
        let record_count = be32(meta, 24)? as usize;
        let declared_entries = be32(meta, 36)? as usize;
        if declared_entries > limit || record_count > pdb.records.len().saturating_sub(start + 1) {
            return Err(Error::Limit("INDX record or entry count".into()));
        }
        let enc = be32(meta, 28)?;
        let encoding = Encoding::from_mobi(match enc {
            u32::MAX => 1252,
            65002 => 65001,
            n => n,
        })?;
        if be32(meta, 48)? != 0 {
            return Err(Error::Unsupported(
                "LIGT index label transformations require a validated decoder".into(),
            ));
        }
        let mut ordt = Vec::new();
        let mut ordt_type = 0;
        if hlen >= 180 && be32(meta, 168)? != 0 {
            let count = be32(meta, 168)? as usize;
            let pos = be32(meta, 176)? as usize;
            ordt_type = be32(meta, 164)?;
            if !matches!(ordt_type, 1 | 2) {
                return Err(Error::Unsupported(format!("ORDT type {ordt_type}")));
            }
            if count > 65536 || slice(meta, pos, 4)? != b"ORDT" {
                return Err(Error::Malformed("ORDT bounds/signature".into()));
            }
            slice(meta, pos + 4, count * 2)?;
            for i in 0..count {
                ordt.push(be16(meta, pos + 4 + i * 2)?);
            }
        }
        let tagx_len = be32(meta, hlen + 4)? as usize;
        let control_count = be32(meta, hlen + 8)? as usize;
        if tagx_len < 12 || (tagx_len - 12) % 4 != 0 || control_count > 64 {
            return Err(Error::Malformed("invalid TAGX length/control count".into()));
        }
        let descriptor_bytes = slice(meta, hlen + 12, tagx_len - 12)?;
        let descriptors: Vec<Descriptor> = descriptor_bytes
            .chunks_exact(4)
            .map(|b| Descriptor {
                tag: b[0],
                arity: b[1],
                mask: b[2],
                end: b[3],
            })
            .collect();
        if descriptors.iter().filter(|d| d.end == 1).count() != control_count
            || descriptors.iter().any(|d| d.end > 1)
        {
            return Err(Error::Malformed(
                "TAGX control-byte delimiters mismatch".into(),
            ));
        }
        let mut rows = Vec::new();
        for record in start + 1..start + 1 + record_count {
            let bytes = pdb.record(source, record)?;
            let header_end = check_header(bytes)?;
            let count = be32(bytes, 24)? as usize;
            if count > limit.saturating_sub(rows.len()) {
                return Err(Error::Limit("INDX row count".into()));
            }
            let table = be32(bytes, 20)? as usize;
            if table < header_end || slice(bytes, table, 4)? != b"IDXT" {
                return Err(Error::Malformed("invalid IDXT offset".into()));
            }
            slice(
                bytes,
                table + 4,
                count
                    .checked_mul(2)
                    .ok_or_else(|| Error::Limit("IDXT multiplication".into()))?,
            )?;
            let mut offsets: Vec<usize> = (0..count)
                .map(|n| be16(bytes, table + 4 + 2 * n).map(usize::from))
                .collect::<Result<_>>()?;
            offsets.push(table);
            for n in 0..count {
                let (a, b) = (offsets[n], offsets[n + 1]);
                if a < header_end || a >= b || b > table {
                    return Err(Error::Malformed(
                        "overlapping/out-of-order INDX rows".into(),
                    ));
                }
                let row = &bytes[a..b];
                let label_len = usize::from(row[0]);
                let label = slice(row, 1, label_len)?.to_vec();
                let mut cursor = 1 + label_len;
                let tags = decode_tags(row, &mut cursor, control_count, &descriptors)?;
                // Alignment padding is allowed only after the final row, and only up to 3 NULs.
                let rest = &row[cursor..];
                if !rest.is_empty()
                    && !(n + 1 == count && rest.len() <= 3 && rest.iter().all(|&b| b == 0))
                {
                    return Err(Error::Unsupported(format!(
                        "unparsed bytes in INDX record {record}, row {n}"
                    )));
                }
                rows.push(IndexRow {
                    label,
                    tags,
                    record,
                    ordinal: rows.len(),
                });
            }
        }
        if rows.len() != declared_entries {
            return Err(Error::Incomplete(format!(
                "INDX declared {declared_entries} rows, parsed {}",
                rows.len()
            )));
        }
        let cncx_count = be32(meta, 52)? as usize;
        let cncx_start = start + 1 + record_count;
        if cncx_count > pdb.records.len().saturating_sub(cncx_start) {
            return Err(Error::Malformed("CNCX records beyond PDB".into()));
        }
        let cncx_records = (cncx_start..cncx_start + cncx_count).collect();
        Ok(Self {
            meta_record: start,
            declared_entries,
            encoding,
            encoding_number: enc,
            ordt_type,
            ordt,
            descriptors,
            rows,
            cncx_records,
        })
    }
    pub fn decode_label(&self, label: &[u8]) -> Result<String> {
        decode_label(label, self.encoding, self.ordt_type, &self.ordt)
    }
    /// Resolve explicit physical ranges and tag-22 shared-definition references.
    /// Iterative three-colour traversal rejects cycles and avoids recursion overflow.
    pub fn definition_spans(&self, bound: usize) -> Result<Vec<Span>> {
        let mut states = vec![0u8; self.rows.len()];
        let mut spans = vec![Span { start: 0, end: 0 }; self.rows.len()];
        for start in 0..self.rows.len() {
            if states[start] == 2 {
                continue;
            }
            let mut path = Vec::new();
            let mut at = start;
            let span = loop {
                let row = self.rows.get(at).ok_or_else(|| {
                    Error::Malformed("shared-definition reference out of range".into())
                })?;
                if states[at] == 2 {
                    break spans[at];
                }
                if states[at] == 1 {
                    return Err(Error::Malformed("shared-definition reference cycle".into()));
                }
                states[at] = 1;
                path.push(at);
                if row.tags.contains_key(&1) {
                    if row.tags.contains_key(&22) {
                        return Err(Error::Unsupported(
                            "both physical and shared-definition tags".into(),
                        ));
                    }
                    let s = Span::new(row.scalar(1)? as usize, row.scalar(2)? as usize, bound)?;
                    if s.is_empty() {
                        return Err(Error::Incomplete("zero-length compiled definition".into()));
                    }
                    break s;
                }
                if row.tags.contains_key(&2) {
                    return Err(Error::Malformed(
                        "definition length without position".into(),
                    ));
                }
                at = row.scalar(22)? as usize;
            };
            for at in path {
                spans[at] = span;
                states[at] = 2;
            }
        }
        Ok(spans)
    }
    pub fn require_tags(&self, allowed: &[u8]) -> Result<()> {
        for d in &self.descriptors {
            if d.end == 0 && !allowed.contains(&d.tag) {
                return Err(Error::Unsupported(format!(
                    "index {} declares unknown tag {}",
                    self.meta_record, d.tag
                )));
            }
        }
        Ok(())
    }
    pub fn cncx_flat(
        &self,
        pdb: &PalmDatabase,
        source: &[u8],
        offset: u32,
        len: usize,
    ) -> Result<Vec<u8>> {
        let number = (offset >> 16) as usize;
        let inside = (offset & 0xffff) as usize;
        let record = *self
            .cncx_records
            .get(number)
            .ok_or_else(|| Error::Malformed("CNCX segment does not exist".into()))?;
        Ok(slice(pdb.record(source, record)?, inside, len)?.to_vec())
    }
    pub fn cncx_string(&self, pdb: &PalmDatabase, source: &[u8], offset: u32) -> Result<String> {
        let number = (offset >> 16) as usize;
        let mut inside = (offset & 0xffff) as usize;
        let record = *self
            .cncx_records
            .get(number)
            .ok_or_else(|| Error::Malformed("CNCX string segment does not exist".into()))?;
        let bytes = pdb.record(source, record)?;
        let len = vint(bytes, &mut inside)? as usize;
        self.encoding.decode(slice(bytes, inside, len)?)
    }
}

fn decode_label(label: &[u8], encoding: Encoding, ordt_type: u32, ordt: &[u16]) -> Result<String> {
    if ordt.is_empty() {
        return encoding.decode(label);
    }
    let codes: Vec<u16> = match ordt_type {
        1 => label.iter().map(|&x| u16::from(x)).collect(),
        2 => {
            if label.len() % 2 != 0 {
                return Err(Error::Malformed("odd two-byte ORDT label".into()));
            }
            label
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect()
        }
        _ => return Err(Error::Unsupported("ORDT label width".into())),
    };
    let units: Vec<u16> = codes
        .into_iter()
        .map(|c| ordt.get(c as usize).copied().unwrap_or(c))
        .collect();
    if units.iter().any(|&x| x <= 5) {
        return Err(Error::Unsupported(
            "ligature escape requires LIGT mapping".into(),
        ));
    }
    let text =
        String::from_utf16(&units).map_err(|_| Error::Malformed("ORDT UTF-16 sequence".into()))?;
    if text.contains('\0') {
        return Err(Error::Malformed("NUL in ORDT label".into()));
    }
    Ok(text)
}

fn decode_tags(
    data: &[u8],
    p: &mut usize,
    count: usize,
    descriptors: &[Descriptor],
) -> Result<BTreeMap<u8, Vec<u32>>> {
    let controls = slice(data, *p, count)?;
    *p += count;
    let mut control = 0;
    let mut used_masks = vec![0u8; count];
    // Presence/count descriptors precede all values, including byte-length forms.
    enum Shape {
        Values(usize),
        Bytes(usize),
    }
    let mut shapes = Vec::new();
    let mut present = BTreeSet::new();
    for d in descriptors {
        if d.end == 1 {
            control += 1;
            continue;
        }
        let c = *controls
            .get(control)
            .ok_or_else(|| Error::Malformed("TAGX control index".into()))?;
        if d.mask == 0 || d.arity == 0 {
            return Err(Error::Malformed("TAGX zero mask/arity".into()));
        }
        if used_masks[control] & d.mask != 0 {
            return Err(Error::Malformed("overlapping TAGX masks".into()));
        }
        used_masks[control] |= d.mask;
        let n = c & d.mask;
        if n == 0 {
            continue;
        }
        if !present.insert(d.tag) {
            return Err(Error::Unsupported("duplicate TAGX tag descriptor".into()));
        }
        let shape = if n == d.mask && d.mask.count_ones() > 1 {
            Shape::Bytes(vint(data, p)? as usize)
        } else {
            let instances = if n == d.mask {
                1
            } else {
                usize::from(n >> d.mask.trailing_zeros())
            };
            Shape::Values(instances * usize::from(d.arity))
        };
        shapes.push((d.tag, d.arity, shape));
    }
    for (&used, &actual) in used_masks.iter().zip(controls) {
        if actual & !used != 0 {
            return Err(Error::Unsupported(
                "unmapped control bits in INDX row".into(),
            ));
        }
    }
    let mut tags = BTreeMap::new();
    for (id, arity, shape) in shapes {
        let mut values = Vec::new();
        match shape {
            Shape::Values(n) => {
                for _ in 0..n {
                    values.push(vint(data, p)?);
                }
            }
            Shape::Bytes(n) => {
                let payload = slice(data, *p, n)?;
                let mut local = 0;
                while local < payload.len() {
                    values.push(vint(payload, &mut local)?);
                }
                *p += n;
                if values.len() % usize::from(arity) != 0 {
                    return Err(Error::Malformed("TAGX tuple arity mismatch".into()));
                }
            }
        }
        tags.insert(id, values);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_count_precedes_value_stream() {
        let d = vec![
            Descriptor {
                tag: 1,
                arity: 1,
                mask: 1,
                end: 0,
            },
            Descriptor {
                tag: 42,
                arity: 1,
                mask: 6,
                end: 0,
            },
            Descriptor {
                tag: 0,
                arity: 0,
                mask: 0,
                end: 1,
            },
        ];
        let mut p = 0;
        let tags = decode_tags(&[7, 0x82, 0x89, 0x81, 0x82], &mut p, 1, &d).unwrap();
        assert_eq!(tags[&1], vec![9]);
        assert_eq!(tags[&42], vec![1, 2]);
        assert_eq!(p, 5);
    }
    #[test]
    fn unknown_bits_fail_closed() {
        let d = vec![
            Descriptor {
                tag: 1,
                arity: 1,
                mask: 1,
                end: 0,
            },
            Descriptor {
                tag: 0,
                arity: 0,
                mask: 0,
                end: 1,
            },
        ];
        assert!(decode_tags(&[2], &mut 0, 1, &d).is_err());
    }
}

#[cfg(test)]
mod shared_and_ordt_tests {
    use super::*;
    fn index(tags: Vec<BTreeMap<u8, Vec<u32>>>) -> Index {
        let rows = tags
            .into_iter()
            .enumerate()
            .map(|(ordinal, tags)| IndexRow {
                label: b"x".to_vec(),
                tags,
                record: 1,
                ordinal,
            })
            .collect::<Vec<_>>();
        Index {
            meta_record: 0,
            declared_entries: rows.len(),
            encoding: Encoding::Utf8,
            encoding_number: 65001,
            ordt_type: 0,
            ordt: Vec::new(),
            descriptors: Vec::new(),
            rows,
            cncx_records: Vec::new(),
        }
    }
    #[test]
    fn shared_ranges_resolve_transitively() {
        let idx = index(vec![
            BTreeMap::from([(22, vec![1])]),
            BTreeMap::from([(22, vec![2])]),
            BTreeMap::from([(1, vec![3]), (2, vec![5])]),
        ]);
        assert_eq!(
            idx.definition_spans(10).unwrap(),
            vec![Span { start: 3, end: 8 }; 3]
        );
    }
    #[test]
    fn shared_cycles_and_bounds_are_errors() {
        assert!(index(vec![
            BTreeMap::from([(22, vec![1])]),
            BTreeMap::from([(22, vec![0])])
        ])
        .definition_spans(10)
        .is_err());
        assert!(index(vec![BTreeMap::from([(22, vec![99])])])
            .definition_spans(10)
            .is_err());
        assert!(index(vec![BTreeMap::from([(1, vec![3]), (2, vec![8])])])
            .definition_spans(10)
            .is_err());
    }
    #[test]
    fn conflicting_or_missing_physical_lengths_fail() {
        assert!(index(vec![BTreeMap::from([(1, vec![3])])])
            .definition_spans(10)
            .is_err());
        assert!(index(vec![BTreeMap::from([(1, vec![3]), (2, vec![0])])])
            .definition_spans(10)
            .is_err());
        assert!(index(vec![BTreeMap::from([
            (1, vec![3]),
            (2, vec![2]),
            (22, vec![0])
        ])])
        .definition_spans(10)
        .is_err());
    }
    #[test]
    fn long_reference_chain_is_iterative() {
        let mut tags = (0..10000)
            .map(|i| BTreeMap::from([(22, vec![i + 1])]))
            .collect::<Vec<_>>();
        tags.push(BTreeMap::from([(1, vec![0]), (2, vec![2])]));
        assert!(index(tags)
            .definition_spans(2)
            .unwrap()
            .iter()
            .all(|s| s.len() == 2));
    }
    #[test]
    fn ordt_preserves_unicode_and_surrogate_pairs() {
        assert_eq!(
            decode_label(&[0, 1, 2], Encoding::Utf8, 1, &[0x63, 0x61, 0xe9]).unwrap(),
            "caé"
        );
        assert_eq!(
            decode_label(&[0, 0, 0, 1], Encoding::Utf8, 2, &[0xd83d, 0xde00]).unwrap(),
            "😀"
        );
        assert!(decode_label(&[0], Encoding::Utf8, 2, &[0x61]).is_err());
        assert!(decode_label(&[0], Encoding::Utf8, 1, &[0xd800]).is_err());
    }
}
