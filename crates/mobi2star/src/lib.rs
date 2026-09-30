//! Bundle orchestration. Parsers and renderers do not depend on this crate or CLI.
#![forbid(unsafe_code)]
mod bundle;
mod dictionary;
mod manifest;
mod output;
mod transaction;
mod tree;
mod verify;
pub use bundle::{convert, FileDigest, Manifest, Report};
pub use output::{OutputOptions, Profile, Stage};
pub use verify::verify;

mod dispatch;
mod source_bundle;
pub use dispatch::{
    convert_dictionary, convert_dictionary_zip, convert_with_backend, dictionary_root,
    verify_bundle, Backend, ConversionReport, DictionaryArchive,
};
pub use source_bundle::{convert_source, verify_source, SourceReport};
