//! Checks shared by both manifest formats. They run before strict parsing, so
//! a bundle from another mobi2star version gets a clear message instead of a
//! missing-field JSON error. Verification regenerates or replays output with
//! the producing code, so it is version-specific by design.
use lexicon_core::{Error, Result};
use serde::Deserialize;

pub(crate) const TOOL: &str = "mobi2star";

#[derive(Deserialize)]
struct Header {
    schema: u32,
    tool: String,
    version: String,
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
