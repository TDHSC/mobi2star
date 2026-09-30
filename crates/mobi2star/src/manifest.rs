//! Checks shared by both manifest formats. They run before strict parsing, so
//! a bundle from another mobi2star version gets a clear message instead of a
//! missing-field JSON error. Verification regenerates or replays output with
//! the producing code, so it is version-specific by design.
use crate::Profile;
use lexicon_core::{checked_member, read_bounded, Error, Result};
use serde::Deserialize;
use std::path::Path;

pub(crate) const TOOL: &str = "mobi2star";

#[derive(Deserialize)]
struct Header {
    schema: u32,
    tool: String,
    version: String,
}

/// Reads a bundle's manifest.json. Dictionary-only output has none; its
/// report records the profile, so it gets its own message rather than the
/// missing-file error a damaged full bundle gets.
pub(crate) fn read(root: &Path, cap: usize) -> Result<Vec<u8>> {
    if root.join("manifest.json").symlink_metadata().is_err()
        && recorded_profile(root) == Some(Profile::Stardict)
    {
        return Err(Error::Unsupported(
            "dictionary-only output (convert --profile stardict) has no manifest and cannot be verified; convert with the default bundle profile to get a verifiable bundle".into(),
        ));
    }
    read_bounded(&checked_member(root, "manifest.json")?, cap)
}

/// The profile `report.json` records, when it can be read.
fn recorded_profile(root: &Path) -> Option<Profile> {
    #[derive(Deserialize)]
    struct Recorded {
        #[serde(default)]
        profile: Profile,
    }
    let bytes = read_bounded(&checked_member(root, "report.json").ok()?, 1024 * 1024).ok()?;
    serde_json::from_slice::<Recorded>(&bytes)
        .ok()
        .map(|report| report.profile)
}

pub(crate) fn check_header(manifest: &[u8], schema: u32) -> Result<()> {
    let header: Header = serde_json::from_slice(manifest)?;
    if header.tool != TOOL {
        return Err(Error::Verify("not a mobi2star bundle manifest".into()));
    }
    let current = env!("CARGO_PKG_VERSION");
    if header.version != current {
        return Err(Error::Unsupported(format!(
            "bundle was produced by mobi2star {}; verify it with that version or reconvert with {current}",
            header.version
        )));
    }
    if header.schema != schema {
        return Err(Error::Verify(format!(
            "manifest schema {} (expected {schema})",
            header.schema
        )));
    }
    Ok(())
}
