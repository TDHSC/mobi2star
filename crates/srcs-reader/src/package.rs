//! Package manifest and reading order; unknown files remain in the source archive.
use crate::{
    markup::{entities, utf8, xml_attrs},
    model::*,
    uri,
};
use lexicon_core::{Error, Result};
use quick_xml::{events::Event, Reader};
use std::collections::{BTreeMap, BTreeSet};

pub fn parse(files: &BTreeMap<String, Vec<u8>>, pages: &BTreeMap<String, Page>) -> Result<Package> {
    let paths: Vec<&String> = files
        .keys()
        .filter(|p| p.to_ascii_lowercase().ends_with(".opf"))
        .collect();
    if paths.len() != 1 {
        return Err(Error::Unsupported("SRCS requires one OPF package".into()));
    }
    let file = paths[0];
    let mut reader = Reader::from_reader(files[file].as_slice());
    let mut out = Package {
        file: file.clone(),
        ..Package::default()
    };
    let mut refs = Vec::new();
    let mut title = false;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > 200_000 {
            return Err(Error::Limit("OPF event count".into()));
        }
        let e = reader
            .read_event()
            .map_err(|e| Error::Malformed(format!("OPF XML: {e}")))?;
        match e {
            Event::Start(e) | Event::Empty(e) => {
                let name = utf8(e.name().as_ref())?.to_owned();
                let attrs = xml_attrs(&e)?;
                let local = name.rsplit(':').next().unwrap_or(&name);
                if local == "title" {
                    title = true;
                }
                if local == "item" {
                    let id = attrs
                        .get("id")
                        .ok_or_else(|| Error::Malformed("OPF item id".into()))?;
                    let href = attrs
                        .get("href")
                        .ok_or_else(|| Error::Malformed("OPF item href".into()))?;
                    let media = attrs
                        .get("media-type")
                        .ok_or_else(|| Error::Malformed("OPF media-type".into()))?;
                    let target = uri::resolve(file, href)?
                        .ok_or_else(|| Error::Unsupported("remote OPF item".into()))?;
                    if !target.anchor.is_empty() || !files.contains_key(&target.file) {
                        return Err(Error::Incomplete(format!("missing OPF item {href}")));
                    }
                    if !matches!(
                        media.as_str(),
                        "application/xhtml+xml"
                            | "text/html"
                            | "text/css"
                            | "image/png"
                            | "image/jpeg"
                            | "image/gif"
                            | "image/bmp"
                            | "application/x-dtbncx+xml"
                    ) {
                        return Err(Error::Unsupported(format!("OPF media type {media}")));
                    }
                    if out
                        .manifest
                        .insert(
                            id.clone(),
                            ManifestItem {
                                file: target.file,
                                media_type: media.clone(),
                            },
                        )
                        .is_some()
                    {
                        return Err(Error::Malformed("duplicate OPF item id".into()));
                    }
                }
                if local == "itemref" {
                    refs.push(
                        attrs
                            .get("idref")
                            .ok_or_else(|| Error::Malformed("OPF spine idref".into()))?
                            .clone(),
                    );
                }
            }
            Event::End(e) => {
                if utf8(e.name().as_ref())?.rsplit(':').next() == Some("title") {
                    title = false;
                }
            }
            Event::Text(e) => {
                if title {
                    out.title.push_str(&entities(utf8(e.as_ref())?)?);
                }
            }
            Event::GeneralRef(e) => {
                if title {
                    out.title
                        .push_str(&entities(&format!("&{};", utf8(e.as_ref())?))?);
                }
            }
            Event::DocType(e) if e.as_ref().contains(&b'[') => {
                return Err(Error::Unsupported("OPF DTD subset".into()))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    for id in refs {
        let item = out
            .manifest
            .get(&id)
            .ok_or_else(|| Error::Incomplete(format!("OPF spine item {id} missing")))?;
        if !pages.contains_key(&item.file) {
            return Err(Error::Unsupported("non-HTML spine item".into()));
        }
        if !seen.insert(item.file.clone()) {
            return Err(Error::Unsupported("repeated OPF spine occurrence".into()));
        }
        out.spine.push(item.file.clone());
    }
    if out.spine.is_empty() {
        return Err(Error::Incomplete("empty OPF reading order".into()));
    }
    Ok(out)
}
