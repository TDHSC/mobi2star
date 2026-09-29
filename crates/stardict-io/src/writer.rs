use crate::{compare_words, IndexEntry, Synonym};
use lexicon_core::{sha256, Document, Entry, Limits, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WrittenEntry {
    pub entry: Entry,
    pub ordinal: u32,
    pub offset: u64,
    pub size: u32,
    pub source_sha256: String,
    pub rendered_sha256: String,
}
#[derive(Debug)]
pub struct WrittenDictionary {
    pub entries: Vec<WrittenEntry>,
    pub synonyms: Vec<Synonym>,
    pub index: Vec<IndexEntry>,
    pub dictionary_bytes: u64,
}
/// Legacy-model adapter to the same streaming writer used by the SRCS backend.
pub fn write(root: &Path, document: &Document, limits: &Limits, offset_bits: u8,
    mut render: impl FnMut(&Entry) -> Result<String>) -> Result<WrittenDictionary> {
    use crate::{CatalogItem, CatalogAlias, PayloadWriter, write_catalog};
    let mut ordered: Vec<&Entry> = document.entries.iter().collect();
    ordered.sort_by(|a,b| compare_words(&a.headword,&b.headword).then(a.id.cmp(&b.id)));
    let mut payloads=PayloadWriter::new(root,limits,offset_bits)?;
    let mut items=Vec::new(); let mut aliases=Vec::new(); let mut entries=Vec::new();
    for entry in ordered {
        let html=render(entry)?; let payload=payloads.append(html.as_bytes())?;
        items.push(CatalogItem{id:entry.id,word:entry.headword.clone(),payload});
        for alias in &entry.aliases { aliases.push(CatalogAlias{word:alias.word.clone(),target_id:entry.id}); }
        aliases.push(CatalogAlias{word:entry.internal_key(&document.namespace),target_id:entry.id});
        entries.push(WrittenEntry{entry:entry.clone(),ordinal:0,offset:payload.offset,size:payload.size,
            source_sha256:sha256(entry.span.bytes(&document.rawml)?),rendered_sha256:sha256(html.as_bytes())});
    }
    let dictionary_bytes=payloads.finish()?;
    let title=if document.metadata.title.is_empty(){"Converted MOBI dictionary"}else{&document.metadata.title};
    let catalog=write_catalog(root,title,&items,&aliases,dictionary_bytes,offset_bits,limits)?;
    for entry in &mut entries {entry.ordinal=catalog.ordinals[&entry.entry.id];}
    Ok(WrittenDictionary{entries,synonyms:catalog.synonyms,index:catalog.index,dictionary_bytes})
}
