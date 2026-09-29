use crate::uri::{collision_key, validate_path};
use lexicon_core::{sha256, Error, Limits, Result};
use std::{collections::{BTreeMap, BTreeSet}, io::{Cursor, Read}};

pub struct SourceArchive { pub record: usize, pub sha256: String, pub files: BTreeMap<String, Vec<u8>> }
impl SourceArchive {
    /// Bound entry count *before* ZipArchive allocates its central-directory map.
    /// This adapter deliberately accepts classic, single-disk ZIP only.
    pub fn read(raw: &[u8], record: usize, limits: &Limits) -> Result<Self> {
        preflight(raw)?;
        let mut zip = zip::ZipArchive::new(Cursor::new(raw)).map_err(|e| Error::Malformed(format!("SRCS ZIP: {e}")))?;
        if zip.len() > 10_000 { return Err(Error::Limit("ZIP entry count".into())); }
        let mut files = BTreeMap::new();
        let mut paths = BTreeMap::<String, String>::new();
        let mut explicit = BTreeSet::new();
        let mut used = 0usize;
        for i in 0..zip.len() {
            let mut item = zip.by_index(i).map_err(|e| Error::Malformed(format!("ZIP member {i}: {e}")))?;
            let raw_name = std::str::from_utf8(item.name_raw()).map_err(|_| Error::Unsupported("non-UTF8 ZIP filename".into()))?;
            if raw_name != item.name() { return Err(Error::Unsupported("ZIP filename requires a codepage adapter".into())); }
            let directory = item.is_dir();
            let name = item.name().trim_end_matches('/').to_owned();
            validate_path(&name)?;
            if !explicit.insert(collision_key(&name)) { return Err(Error::Malformed(format!("duplicate ZIP entry {name}"))); }
            // Also account for implicit parent directory casing and normalization.
            let mut prefix = String::new();
            for component in name.split('/') {
                if !prefix.is_empty() { prefix.push('/'); } prefix.push_str(component);
                let key = collision_key(&prefix);
                if paths.get(&key).is_some_and(|prior| prior != &prefix) { return Err(Error::Malformed(format!("macOS path collision at {prefix}"))); }
                paths.insert(key, prefix.clone());
            }
            if item.unix_mode().is_some_and(|mode| { let kind = mode & 0o170000; kind != 0 && kind != 0o100000 && kind != 0o040000 }) {
                return Err(Error::Unsupported("ZIP symlink or special file".into()));
            }
            if directory {
                if item.size() != 0 { return Err(Error::Malformed("ZIP directory contains payload bytes".into())); }
                continue;
            }
            let declared = usize::try_from(item.size()).map_err(|_| Error::Limit("ZIP member length".into()))?;
            if declared > limits.text_bytes.saturating_sub(used) { return Err(Error::Limit("ZIP expanded byte budget".into())); }
            if !matches!(item.compression(), zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated) {
                return Err(Error::Unsupported("ZIP compression requires an adapter".into()));
            }
            let mut bytes = Vec::new();
            // Take one extra byte to detect understated sizes; reaching EOF runs CRC validation.
            item.by_ref().take(declared as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() != declared { return Err(Error::Malformed("ZIP decoded size mismatch".into())); }
            let mut tail = [0u8; 1];
            if item.read(&mut tail)? != 0 { return Err(Error::Malformed("ZIP extra decoded bytes".into())); }
            used += bytes.len();
            files.insert(name, bytes);
        }
        for path in files.keys() {
            let mut current = path.as_str();
            while let Some((parent, _)) = current.rsplit_once('/') {
                if files.contains_key(parent) { return Err(Error::Malformed("ZIP file/directory collision".into())); }
                current = parent;
            }
        }
        Ok(Self { record, sha256: sha256(raw), files })
    }
}
fn le16(raw: &[u8], at: usize) -> Result<u16> {
    let b = raw.get(at..at + 2).ok_or_else(|| Error::Malformed("ZIP truncated integer".into()))?;
    Ok(u16::from_le_bytes([b[0],b[1]]))
}
fn le32(raw: &[u8], at: usize) -> Result<u32> {
    let b = raw.get(at..at + 4).ok_or_else(|| Error::Malformed("ZIP truncated integer".into()))?;
    Ok(u32::from_le_bytes([b[0],b[1],b[2],b[3]]))
}
fn preflight(raw: &[u8]) -> Result<()> {
    let begin = raw.len().saturating_sub(65557);
    let end = (begin..raw.len().saturating_sub(21)).rev().find(|&p| raw.get(p..p+4) == Some(b"PK\x05\x06")).ok_or_else(|| Error::Malformed("ZIP end record missing".into()))?;
    if le16(raw,end+4)? != 0 || le16(raw,end+6)? != 0 || le16(raw,end+8)? != le16(raw,end+10)? {
        return Err(Error::Unsupported("multi-disk ZIP".into()));
    }
    let count = le16(raw,end+10)?;
    if count > 10_000 { return Err(Error::Limit("ZIP entry count".into())); }
    if le32(raw,end+12)? == u32::MAX || le32(raw,end+16)? == u32::MAX { return Err(Error::Unsupported("ZIP64 source archive".into())); }
    let cd_start = le32(raw,end+16)? as usize;
    let cd_len = le32(raw,end+12)? as usize;
    if cd_start.checked_add(cd_len) != Some(end) || end + 22 + le16(raw,end+20)? as usize != raw.len() {
        return Err(Error::Malformed("ZIP central-directory/trailing-byte boundaries".into()));
    }
    let mut pos = cd_start;
    for _ in 0..count {
        if raw.get(pos..pos+4) != Some(b"PK\x01\x02") { return Err(Error::Malformed("ZIP central entry signature".into())); }
        if le16(raw,pos+8)? & 1 != 0 { return Err(Error::Unsupported("encrypted ZIP member".into())); }
        let name = le16(raw,pos+28)? as usize; let extra = le16(raw,pos+30)? as usize; let comment = le16(raw,pos+32)? as usize;
        pos = pos.checked_add(46 + name + extra + comment).ok_or_else(|| Error::Limit("ZIP central-directory overflow".into()))?;
        if pos > end { return Err(Error::Malformed("ZIP central entry out of bounds".into())); }
    }
    if pos != end { return Err(Error::Malformed("ZIP central entry count mismatch".into())); }
    Ok(())
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn zip_truncation_rejected() { for n in 0..100 { assert!(preflight(&vec![0;n]).is_err()); } }
    #[test] fn empty_zip_bounded() { let mut zip = b"PK\x05\x06".to_vec(); zip.resize(22,0); assert!(preflight(&zip).is_ok()); }
}

#[cfg(test)]mod adversarial_tests{
    use super::*;
    use std::io::Write;
    fn archive(items:&[(&str,&[u8])])->Vec<u8>{
        let mut zip=zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for &(name,bytes) in items{zip.start_file(name,options).unwrap();zip.write_all(bytes).unwrap();}
        zip.finish().unwrap().into_inner()
    }
    #[test]fn stored_zip_bytes_preserved(){let bytes=archive(&[("OEBPS/a.txt",b"hello")]);assert_eq!(SourceArchive::read(&bytes,9,&Limits::default()).unwrap().files["OEBPS/a.txt"],b"hello");}
    #[test]fn zip_path_traversal_and_mac_collisions_fail(){
        for names in [vec![("../outside",b"a".as_slice())],vec![("A/file",b"a".as_slice()),("a/other",b"b".as_slice())],vec![("É.txt",b"a".as_slice()),("e\u{301}.TXT",b"b".as_slice())]]{
            let bytes=archive(&names);assert!(SourceArchive::read(&bytes,1,&Limits::default()).is_err());
        }
    }
    #[test]fn crc_corruption_is_an_error(){
        let mut bytes=archive(&[("file",b"hello")]);let pos=bytes.windows(5).position(|s|s==b"hello").unwrap();bytes[pos]^=1;
        assert!(SourceArchive::read(&bytes,1,&Limits::default()).is_err());
    }
    #[test]fn expanded_archive_budget_is_enforced(){
        let bytes=archive(&[("file",b"hello")]);let limits=Limits{text_bytes:4,..Limits::default()};assert!(SourceArchive::read(&bytes,1,&limits).is_err());
    }
}
