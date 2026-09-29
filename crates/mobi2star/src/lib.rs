//! Bundle orchestration. Parsers and renderers do not depend on this crate or CLI.
#![forbid(unsafe_code)]
mod bundle;
mod transaction;
mod verify;
pub use bundle::{convert, FileDigest, Manifest, Report};
pub use verify::verify;

mod source_bundle;
mod dispatch;
pub use source_bundle::{convert_source,verify_source,SourceReport};
pub use dispatch::{Backend,ConversionReport,convert_with_backend,verify_bundle,dictionary_root};
