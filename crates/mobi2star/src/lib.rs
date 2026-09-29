//! Bundle orchestration. Parsers and renderers do not depend on this crate or CLI.
#![forbid(unsafe_code)]
mod bundle;
mod transaction;
mod verify;
pub use bundle::{convert, FileDigest, Manifest, Report};
pub use verify::verify;

mod dispatch;
mod source_bundle;
pub use dispatch::{
    convert_with_backend, dictionary_root, verify_bundle, Backend, ConversionReport,
};
pub use source_bundle::{convert_source, verify_source, SourceReport};
