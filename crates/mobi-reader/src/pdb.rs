use lexicon_core::{bytes::{be16, be32, slice}, Error, Result, Span};

#[derive(Debug)]
pub struct PalmDatabase { pub records: Vec<Span> }
impl PalmDatabase {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if slice(data, 60, 8)? != b"BOOKMOBI" { return Err(Error::Unsupported("expected a BOOK/MOBI Palm database".into())); }
        if be16(data, 32)? & 1 != 0 { return Err(Error::Unsupported("Palm resource database".into())); }
        if be32(data, 72)? != 0 { return Err(Error::Unsupported("chained Palm record lists".into())); }
        let count = be16(data, 76)? as usize;
        if count == 0 { return Err(Error::Malformed("empty Palm record table".into())); }
        let table_end = 78 + count * 8;
        slice(data, 0, table_end)?;
        let mut offsets = Vec::with_capacity(count + 1);
        for i in 0..count {
            let offset = be32(data, 78 + 8 * i)? as usize;
            if offset < table_end || offset > data.len() { return Err(Error::Malformed(format!("PDB record {i} offset {offset}"))); }
            if offsets.last().is_some_and(|&previous| previous >= offset) {
                return Err(Error::Malformed("non-increasing/empty PDB records".into()));
            }
            offsets.push(offset);
        }
        offsets.push(data.len());
        let records = offsets.windows(2).map(|w| Span { start: w[0], end: w[1] }).collect();
        Ok(Self { records })
    }
    pub fn record<'a>(&self, source: &'a [u8], n: usize) -> Result<&'a [u8]> {
        self.records.get(n).ok_or_else(|| Error::Malformed(format!("PDB record {n} not present")))?.bytes(source)
    }
}
