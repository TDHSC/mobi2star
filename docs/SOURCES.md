# Format and library references

Primary references used for the architecture and library interfaces:

- StarDict file format: https://stardict-4.sourceforge.net/StarDictFileFormat
- Cargo dependency resolution and lock files: https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html
- Rust installation: https://rust-lang.org/tools/install/
- quick-xml project and event API: https://github.com/tafia/quick-xml and https://docs.rs/quick-xml/0.38.3/quick_xml/events/enum.Event.html
- zip crate project and API: https://github.com/zip-rs/zip2 and https://docs.rs/zip/
- image crate project and API: https://github.com/image-rs/image and https://docs.rs/image/

MOBI/SRCS handling is an independent implementation, developed against the observed structure of real dictionary containers and this project's synthetic fixtures. The project does not vendor mobi2stardict source, .NET binaries or their dependency tree.

Reader behavior references for the readability profile are listed in [READABILITY.md](READABILITY.md).
