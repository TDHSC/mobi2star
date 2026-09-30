//! Helpers shared by the integration tests. Each test binary compiles this
//! module separately and uses only some of it.
#![allow(dead_code)]
use lexicon_core::{sha256, Limits};
use std::{fs, path::Path};

/// Reads a bundle's manifest.json, lets `edit` change it, and writes it
/// back. The manifest is not in its own digest list, so nothing is rehashed.
pub fn edit_manifest(bundle: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = bundle.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

/// Replaces a bundle file and updates its manifest digest, so only
/// source-derived checks can catch the change.
pub fn rewrite_with_digest(bundle: &Path, name: &str, bytes: &[u8]) {
    fs::write(bundle.join(name), bytes).unwrap();
    edit_manifest(bundle, |manifest| {
        for file in manifest["files"].as_array_mut().unwrap() {
            if file["path"] == name {
                file["bytes"] = bytes.len().into();
                file["sha256"] = sha256(bytes).into();
            }
        }
    });
}

/// Payload of the first index entry for `word` in the StarDict files at `root`.
pub fn payload(root: &Path, word: &str) -> String {
    let limits = Limits::default();
    let parsed = stardict_io::open(root, &limits).unwrap();
    let mut dict = fs::File::open(&parsed.dictionary_path).unwrap();
    let entry = &parsed.entries[parsed.lookup(word)[0]];
    stardict_io::read_payload(&mut dict, entry, limits.entry_bytes).unwrap()
}
