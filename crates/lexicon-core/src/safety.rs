use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(Error::Limit(format!(
            "{} exceeds {} bytes",
            path.display(),
            limit
        )));
    }
    let mut data = Vec::new();
    (&mut file).take(limit as u64 + 1).read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(Error::Limit("input grew beyond limit while reading".into()));
    }
    Ok(data)
}
pub fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut f = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut len = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
        len = len
            .checked_add(n as u64)
            .ok_or_else(|| Error::Limit("file size overflow".into()))?;
    }
    Ok((len, format!("{:x}", digest.finalize())))
}
/// Manifest names are untrusted. Refuse absolute paths, dot components and links.
/// This is not a defense against a same-user process racing filesystem changes.
pub fn checked_member(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name);
    if name.is_empty()
        || name.contains('\\')
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::Verify(format!("unsafe member name {name:?}")));
    }
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part.as_os_str());
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::Verify("symlink in bundle".into()));
        }
    }
    if !path.is_file() {
        return Err(Error::Verify(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_hash() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn path_traversal_rejected_before_io() {
        assert!(checked_member(Path::new("/no-such-bundle"), "../x").is_err());
        assert!(checked_member(Path::new("/no-such-bundle"), "/etc/passwd").is_err());
    }
}
