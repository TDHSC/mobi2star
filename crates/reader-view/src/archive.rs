//! Loading a dictionary from the zip the web page offers for download.
use crate::Dictionary;
use lexicon_core::{read_limited, Error, Limits, Result};
use std::{collections::BTreeMap, io::Cursor};
use zip::ZipArchive;

fn malformed(error: zip::result::ZipError) -> Error {
    Error::Malformed(format!("zip archive: {error}"))
}

impl Dictionary {
    /// Reads the one StarDict dictionary in `zip`: its `.ifo`, `.idx`,
    /// `.syn`, `.dict`, the `.css` next to the `.ifo`, and `res/`. Sizes
    /// are checked against `limits` before anything is inflated, and the
    /// zip itself is released once its members are read. The zip and its
    /// unpacked files are in memory together until then, so the two
    /// together must fit `limits.output_bytes`, the same budget a
    /// conversion in the browser has for its output.
    pub fn from_zip(zip: Vec<u8>, limits: &Limits) -> Result<Self> {
        let zip_bytes = zip.len() as u64;
        let mut archive = ZipArchive::new(Cursor::new(zip)).map_err(malformed)?;
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
        let ifo = {
            let mut ifos = names.iter().filter(|name| name.ends_with(".ifo"));
            match (ifos.next(), ifos.next()) {
                (Some(ifo), None) => ifo.clone(),
                _ => {
                    return Err(Error::Unsupported(
                        "the zip must hold exactly one StarDict dictionary".into(),
                    ))
                }
            }
        };
        let base = ifo.strip_suffix(".ifo").unwrap_or(&ifo).to_owned();
        let folder = base
            .rfind('/')
            .map_or("", |slash| &base[..=slash])
            .to_owned();

        let mut total = 0u64;
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index).map_err(malformed)?;
            total = total.saturating_add(file.size());
        }
        let needed = total.saturating_add(zip_bytes);
        if needed > limits.output_bytes {
            return Err(Error::Limit(format!(
                "the dictionary unpacks to {total} bytes, {needed} with its zip; the limit is {}",
                limits.output_bytes
            )));
        }
        let dict_limit = usize::try_from(limits.output_bytes).unwrap_or(usize::MAX);
        let mut read = |name: &str, limit: usize| -> Result<Vec<u8>> {
            let file = archive.by_name(name).map_err(malformed)?;
            let length = file.size();
            read_limited(file, length, limit, &name)
        };
        let ifo_bytes = read(&ifo, 65536)?;
        let idx = read(&format!("{base}.idx"), limits.input_bytes)?;
        let syn = if names.contains(&format!("{base}.syn")) {
            read(&format!("{base}.syn"), limits.input_bytes)?
        } else {
            Vec::new()
        };
        let dict = read(&format!("{base}.dict"), dict_limit)?;
        let companion_css = if names.contains(&format!("{base}.css")) {
            let css = read(&format!("{base}.css"), limits.text_bytes)?;
            Some(String::from_utf8_lossy(&css).into_owned())
        } else {
            None
        };
        let mut resources = BTreeMap::new();
        let res = format!("{folder}res/");
        for name in names.iter().filter(|name| !name.ends_with('/')) {
            if let Some(path) = name.strip_prefix(&res) {
                resources.insert(path.to_owned(), read(name, limits.input_bytes)?);
            }
        }
        drop(archive);
        Dictionary::from_parts(
            &ifo_bytes,
            &idx,
            &syn,
            dict,
            companion_css,
            resources,
            limits,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mobi2star::{Backend, OutputOptions};

    const SRCS: &[u8] = include_bytes!("../../../tests/fixtures/srcs.mobi");

    fn converted() -> Vec<u8> {
        mobi2star::convert_dictionary_zip(
            SRCS.to_vec(),
            &Limits::browser(),
            OutputOptions::default(),
            Backend::Auto,
            &mut |_| {},
        )
        .unwrap()
        .zip
    }

    #[test]
    fn reads_the_zip_the_page_offers() {
        let dictionary = Dictionary::from_zip(converted(), &Limits::browser()).unwrap();
        assert_eq!(dictionary.bookname(), "Original Rust SRCS fixture");
        let run = dictionary.index().lookup("run");
        assert_eq!(run.len(), 2, "both homographs");
        assert!(dictionary.payload(run[0]).unwrap().contains("run"));
        assert!(dictionary.companion_css().is_some());
        assert!(dictionary.resource("dictionary.css").is_some());
        assert!(dictionary.resource("source/OEBPS/image.png").is_some());
    }

    #[test]
    fn sizes_are_checked_before_inflating() {
        let limits = Limits {
            output_bytes: 100,
            ..Limits::browser()
        };
        assert!(matches!(
            Dictionary::from_zip(converted(), &limits),
            Err(Error::Limit(_))
        ));
        assert!(matches!(
            Dictionary::from_zip(b"not a zip".to_vec(), &Limits::browser()),
            Err(Error::Malformed(_))
        ));
        // The zip counts too: it is held while its members are inflated.
        let zip = converted();
        let mut archive = ZipArchive::new(Cursor::new(zip.clone())).unwrap();
        let unpacked: u64 = (0..archive.len())
            .map(|n| archive.by_index_raw(n).unwrap().size())
            .sum();
        let budget = |output_bytes| Limits {
            output_bytes,
            ..Limits::browser()
        };
        let needed = unpacked + zip.len() as u64;
        assert!(Dictionary::from_zip(zip.clone(), &budget(needed)).is_ok());
        assert!(matches!(
            Dictionary::from_zip(zip, &budget(needed - 1)),
            Err(Error::Limit(_))
        ));
    }
}
