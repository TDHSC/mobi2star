//! Shared streaming output primitives for all input adapters.
use crate::{IndexEntry, Synonym, compare_synonyms, compare_words, validate_key};
use lexicon_core::{Error, Limits, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs::{File,OpenOptions}, io::{BufWriter,Write}, path::Path};
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq,PartialOrd,Ord)]
pub struct Payload { pub offset:u64, pub size:u32 }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CatalogItem { pub id:u64, pub word:String, pub payload:Payload }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CatalogAlias { pub word:String, pub target_id:u64 }
#[derive(Debug)]
pub struct Catalog { pub index:Vec<IndexEntry>, pub synonyms:Vec<Synonym>, pub ordinals:BTreeMap<u64,u32> }
pub struct PayloadWriter { writer:BufWriter<File>, bytes:u64, limit:u64, entry_limit:usize, offset_bits:u8 }
fn create(path:&Path)->Result<BufWriter<File>> { Ok(BufWriter::new(OpenOptions::new().create_new(true).write(true).open(path)?)) }
fn finish(mut writer:BufWriter<File>)->Result<()> {writer.flush()?;writer.get_ref().sync_all()?;Ok(())}
impl PayloadWriter {
    pub fn new(root:&Path,limits:&Limits,bits:u8)->Result<Self> {
        if !matches!(bits,32|64) {return Err(Error::Unsupported("StarDict offsets must be 32 or 64 bit".into()));}
        Ok(Self{writer:create(&root.join("dictionary.dict"))?,bytes:0,limit:limits.output_bytes,entry_limit:limits.entry_bytes,offset_bits:bits})
    }
    pub fn append(&mut self,bytes:&[u8])->Result<Payload> {
        if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {return Err(Error::Incomplete("StarDict payload must be nonempty UTF-8 HTML without NUL".into()));}
        if bytes.len()>self.entry_limit {return Err(Error::Limit("rendered article byte budget".into()));}
        let size=u32::try_from(bytes.len()).map_err(|_|Error::Limit("StarDict article length".into()))?;
        let end=self.bytes.checked_add(size as u64).ok_or_else(||Error::Limit("StarDict offset overflow".into()))?;
        if end>self.limit || self.offset_bits==32 && end>u32::MAX as u64 {return Err(Error::Limit("StarDict output budget/offset width".into()));}
        let payload=Payload{offset:self.bytes,size}; self.writer.write_all(bytes)?;self.bytes=end;Ok(payload)
    }
    pub fn finish(self)->Result<u64> {finish(self.writer)?;Ok(self.bytes)}
}
pub fn write_catalog(root:&Path,title:&str,items:&[CatalogItem],aliases:&[CatalogAlias],bytes:u64,bits:u8,limits:&Limits)->Result<Catalog> {
    if !matches!(bits,32|64) {return Err(Error::Unsupported("StarDict offset width".into()));}
    if items.len()>limits.entries || items.len()>u32::MAX as usize || aliases.len()>limits.aliases {return Err(Error::Limit("StarDict catalog count".into()));}
    if title.contains(['\r','\n','\0']) {return Err(Error::Incomplete("multiline/NUL StarDict book title".into()));}
    let mut ordered:Vec<&CatalogItem>=items.iter().collect();
    ordered.sort_by(|a,b|compare_words(&a.word,&b.word).then(a.id.cmp(&b.id)));
    let ordinals:BTreeMap<u64,u32>=ordered.iter().enumerate().map(|(i,x)|(x.id,i as u32)).collect();
    if ordinals.len()!=items.len() {return Err(Error::Incomplete("catalog IDs must be unique".into()));}
    // Exact shared article ranges are valid. Partial overlaps and gaps are errors.
    let mut ranges:Vec<Payload>=items.iter().map(|x|x.payload).collect();ranges.sort();ranges.dedup();
    let mut end=0u64;
    for p in ranges {if p.offset!=end {return Err(Error::Incomplete("uncovered/partially overlapping StarDict payload ranges".into()));} end=end.checked_add(p.size as u64).ok_or_else(||Error::Limit("payload range overflow".into()))?;}
    if end!=bytes {return Err(Error::Incomplete("unindexed payload bytes".into()));}
    let mut idx=create(&root.join("dictionary.idx"))?;let mut index=Vec::new();let mut idx_bytes=0u64;
    for item in ordered {
        validate_key(&item.word)?;idx.write_all(item.word.as_bytes())?;idx.write_all(&[0])?;
        if bits==64 {idx.write_all(&item.payload.offset.to_be_bytes())?;}
        else {let offset=u32::try_from(item.payload.offset).map_err(|_|Error::Limit("32-bit index offset".into()))?;idx.write_all(&offset.to_be_bytes())?;}
        idx.write_all(&item.payload.size.to_be_bytes())?;
        idx_bytes+=item.word.len() as u64+1+u64::from(bits/8)+4;
        index.push(IndexEntry{word:item.word.clone(),offset:item.payload.offset,size:item.payload.size});
    }
    finish(idx)?;
    let mut synonyms=Vec::new();
    for alias in aliases {
        validate_key(&alias.word)?;
        let target=*ordinals.get(&alias.target_id).ok_or_else(||Error::Incomplete("synonym target ID missing".into()))?;
        synonyms.push(Synonym{word:alias.word.clone(),target});
    }
    synonyms.sort_by(compare_synonyms);
    let mut syn=create(&root.join("dictionary.syn"))?;
    for s in &synonyms {syn.write_all(s.word.as_bytes())?;syn.write_all(&[0])?;syn.write_all(&s.target.to_be_bytes())?;}
    finish(syn)?;
    let mut ifo=create(&root.join("dictionary.ifo"))?;
    writeln!(ifo,"StarDict's dict ifo file\nversion=3.0.0\nbookname={title}\nwordcount={}\nsynwordcount={}\nidxfilesize={idx_bytes}\nidxoffsetbits={bits}\nsametypesequence=h",items.len(),synonyms.len())?;
    finish(ifo)?;Ok(Catalog{index,synonyms,ordinals})
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn shared_payloads_and_duplicate_headwords_roundtrip(){
        let dir=tempfile::tempdir().unwrap();let limits=Limits::default();let mut out=PayloadWriter::new(dir.path(),&limits,32).unwrap();
        let shared=out.append(b"<b>shared meaning</b>").unwrap();let separate=out.append(b"<b>another meaning</b>").unwrap();let bytes=out.finish().unwrap();
        let items=vec![CatalogItem{id:0,word:"run".into(),payload:shared},CatalogItem{id:1,word:"running".into(),payload:shared},CatalogItem{id:2,word:"run".into(),payload:separate}];
        let aliases=vec![CatalogAlias{word:"runs".into(),target_id:0},CatalogAlias{word:"runs".into(),target_id:2}];
        let catalog=write_catalog(dir.path(),"Test",&items,&aliases,bytes,32,&limits).unwrap();let parsed=crate::open(dir.path(),&limits).unwrap();
        assert_eq!(catalog.index,parsed.entries);assert_eq!(catalog.synonyms,parsed.synonyms);assert_eq!(parsed.lookup("run").len(),2);assert_eq!(parsed.lookup("runs").len(),2);
        assert_eq!(parsed.entries[parsed.lookup("running")[0]].offset,shared.offset);
    }
    #[test]fn partial_overlap_is_rejected(){
        let dir=tempfile::tempdir().unwrap();let limits=Limits::default();
        let items=vec![CatalogItem{id:0,word:"a".into(),payload:Payload{offset:0,size:4}},CatalogItem{id:1,word:"b".into(),payload:Payload{offset:2,size:4}}];
        assert!(write_catalog(dir.path(),"Test",&items,&[],6,32,&limits).is_err());
    }
    #[test]fn payload_budget_and_nul_are_errors(){
        let dir=tempfile::tempdir().unwrap();let limits=Limits{entry_bytes:4,..Limits::default()};let mut writer=PayloadWriter::new(dir.path(),&limits,32).unwrap();
        assert!(writer.append(b"12345").is_err());assert!(writer.append(b"a\0b").is_err());assert!(writer.append(&[0xff]).is_err());
    }
}
