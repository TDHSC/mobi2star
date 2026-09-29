//! Independent compiled index/text reconciliation, including alias multiplicity.
use crate::{plain_text, SourceBook};
use lexicon_core::{sha256, Error, Limits, Result, Span};
use mobi_reader::{Container, index::Index, inflection::apply_rule};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct DefinitionMatch {
    pub entry_id: usize,
    pub compiled: Span,
    pub source_text_sha256: String,
    pub compiled_text_sha256: String,
}
#[derive(Debug, Serialize)]
pub struct FormMatch {
    pub entry_id: usize, pub orth_ordinal: usize, pub base: String, pub value: String,
    pub group: usize, pub rule: usize, pub group_name: String,
}
#[derive(Debug, Serialize)]
pub struct Crosscheck {
    pub orth: Index,
    pub inflections: Option<Index>,
    pub ancillary: BTreeMap<usize, Index>,
    pub definitions: Vec<DefinitionMatch>,
    pub forms: Vec<FormMatch>,
    pub record_roles: Vec<String>,
}
fn counter<T:Ord>(items:impl IntoIterator<Item=T>) -> BTreeMap<T,usize> {
    let mut out=BTreeMap::new(); for item in items { *out.entry(item).or_insert(0)+=1; } out
}
pub fn crosscheck(mobi:&Container<'_>,book:&SourceBook,rawml:&[u8],limits:&Limits) -> Result<Crosscheck> {
    if mobi.header.encoding_number!=65001 { return Err(Error::Unsupported("SRCS crosscheck currently requires UTF-8 compiled text".into())); }
    let start=mobi.header.orth_index.ok_or_else(||Error::Incomplete("missing compiled headword index".into()))?;
    let orth=mobi.index(start,limits.entries)?; orth.require_tags(&[1,2,22,42])?;
    let inflections=mobi.header.infl_index.map(|i|mobi.index(i,limits.entries)).transpose()?;
    if let Some(infl)=&inflections { infl.require_tags(&[5,26,27])?; check_reverse_rules(infl)?; }
    let spans=orth.definition_spans(rawml.len())?;
    let labels=orth.rows.iter().map(|row|orth.decode_label(&row.label)).collect::<Result<Vec<_>>>()?;
    if counter(labels.iter()) != counter(book.orths.iter().map(|o|&o.value)) {
        return Err(Error::Incomplete("compiled/source headword multisets differ".into()));
    }
    let mut groups=BTreeMap::<Span,Vec<String>>::new();
    for (span,label) in spans.iter().zip(&labels) {groups.entry(*span).or_default().push(label.clone());}
    if groups.len()!=book.entries.len() {return Err(Error::Incomplete(format!("compiled definitions {}, source definitions {}",groups.len(),book.entries.len())));}
    let order=book.ordered_entries()?;
    let mut ownership=BTreeMap::new(); let mut definitions=Vec::new();
    for (&eid,(span,words)) in order.iter().zip(groups) {
        let entry=&book.entries[eid];
        if counter(words.iter()) != counter(entry.orths.iter().map(|&i|&book.orths[i].value)) {
            return Err(Error::Incomplete(format!("definition {eid} headwords differ across source/compiled reading order")));
        }
        let source_text=plain_text(entry.span.bytes(&book.files[&entry.file])?)?;
        let compiled_text=plain_text(span.bytes(rawml)?)?;
        if source_text!=compiled_text {
            return Err(Error::Incomplete(format!("definition {eid} visible text differs; source {} bytes, compiled {} bytes after whitespace normalization",source_text.len(),compiled_text.len())));
        }
        definitions.push(DefinitionMatch{entry_id:eid,compiled:span,source_text_sha256:sha256(source_text.as_bytes()),compiled_text_sha256:sha256(compiled_text.as_bytes())});
        ownership.insert(span,eid);
    }
    let mut forms=Vec::new();
    for (i,row) in orth.rows.iter().enumerate() {
        for &gid in row.tags.get(&42).into_iter().flatten() {
            let infl=inflections.as_ref().ok_or_else(||Error::Incomplete("referenced inflection index absent".into()))?;
            let group=infl.rows.get(gid as usize).ok_or_else(||Error::Malformed("inflection group out of range".into()))?;
            let names=group.tags.get(&5).ok_or_else(||Error::Malformed("missing inflection group names".into()))?;
            let rules=group.tags.get(&26).ok_or_else(||Error::Malformed("missing inflection group rules".into()))?;
            if names.len()!=rules.len() {return Err(Error::Malformed("inflection name/rule counts differ".into()));}
            for (&name,&rid) in names.iter().zip(rules) {
                if forms.len()>=limits.aliases {return Err(Error::Limit("compiled inflection count".into()));}
                let rule=infl.rows.get(rid as usize).ok_or_else(||Error::Malformed("inflection rule out of range".into()))?;
                // ORDT is decoded before the UTF-8 edit rule is applied.
                let bytes=apply_rule(labels[i].as_bytes(),&rule.label,4096)?;
                let value=String::from_utf8(bytes).map_err(|_|Error::Malformed("inflection rule produced invalid UTF-8".into()))?;
                let group_name=infl.cncx_string(&mobi.pdb,mobi.source,name)?;
                forms.push(FormMatch{entry_id:ownership[&spans[i]],orth_ordinal:i,base:labels[i].clone(),value,group:gid as usize,rule:rid as usize,group_name});
            }
        }
    }
    let a=counter(forms.iter().map(|f|(f.entry_id,&f.base,&f.value)));
    let b=counter(book.forms.iter().map(|f|(f.entry_id,&book.orths[f.orth_id].value,&f.value)));
    if a!=b {return Err(Error::Incomplete("compiled/source inflection associations or multiplicities differ".into()));}
    let mut ancillary=BTreeMap::new();
    for n in 1..mobi.pdb.records.len() {
        let bytes=mobi.record(n)?;
        if bytes.starts_with(b"INDX") && bytes.get(lexicon_core::bytes::be32(bytes,4)? as usize..).is_some_and(|tail|tail.starts_with(b"TAGX")) && n!=start && Some(n)!=mobi.header.infl_index {
            ancillary.insert(n,mobi.index(n,limits.entries)?);
        }
    }
    // The extra dictionary-name index maps a named wordlist to this dictionary.
    // Other advertised lookup structures require explicit query semantics.
    for n in &mobi.header.extra_indexes {
        let index=ancillary.get(n).ok_or_else(||Error::Incomplete(format!("advertised extra index {n} not parsed")))?;
        index.require_tags(&[1])?;
        if index.rows.len()!=1 || index.rows[0].scalar(1)?!=0 {return Err(Error::Unsupported("multiple/additional lookup indexes need query adapter".into()));}
    }
    let record_roles=classify(mobi,&orth,inflections.as_ref(),&ancillary,book.archive_record)?;
    Ok(Crosscheck{orth,inflections,ancillary,definitions,forms,record_roles})
}
fn check_reverse_rules(infl:&Index) -> Result<()> {
    let mut forward=Vec::new(); let mut reverse=Vec::new();
    for row in &infl.rows {
        if let Some(rules)=row.tags.get(&26) {
            let names=row.tags.get(&5).ok_or_else(||Error::Malformed("inflection names absent".into()))?;
            if names.len()!=rules.len() {return Err(Error::Malformed("inflection group arity".into()));}
            for &rule in rules {forward.push((row.ordinal,rule as usize));}
        }
        for &group in row.tags.get(&27).into_iter().flatten() {reverse.push((group as usize,row.ordinal));}
    }
    // Older compiled indexes omit reverse edges entirely. When present, verify them all.
    if !reverse.is_empty() && counter(forward)!=counter(reverse) {return Err(Error::Incomplete("inflection forward/reverse edge multisets differ".into()));}
    Ok(())
}
fn classify(m:&Container<'_>,orth:&Index,infl:Option<&Index>,other:&BTreeMap<usize,Index>,srcs:usize)->Result<Vec<String>> {
    let mut roles=vec![String::new();m.pdb.records.len()]; roles[0]="header".into();
    for r in roles.iter_mut().take(m.header.text_records+1).skip(1) {*r="text".into();}
    let mut assign=|n:usize,role:&str|->Result<()> {
        let r=roles.get_mut(n).ok_or_else(||Error::Malformed("record role out of range".into()))?;
        if !r.is_empty() {return Err(Error::Malformed(format!("overlapping record roles at {n}")));}
        *r=role.into(); Ok(())
    };
    for index in std::iter::once(orth).chain(infl).chain(other.values()) {
        let count=lexicon_core::bytes::be32(m.record(index.meta_record)?,24)? as usize;
        for n in index.meta_record..index.meta_record+1+count {assign(n,"parsed_index")?;}
        for &n in &index.cncx_records {assign(n,"index_strings")?;}
    }
    if m.header.compression==17480 {
        let start=m.header.huff_start.ok_or_else(||Error::Malformed("HUFF start".into()))?;
        for n in start..start+m.header.huff_count {assign(n,"huff_dictionary")?;}
    }
    for image in m.resources()? {assign(image.pdb_record,"image")?;}
    assign(srcs,"embedded_source_zip")?;
    for (n,role) in roles.iter_mut().enumerate() {
        if !role.is_empty() {continue;}
        let raw=m.record(n)?;
        if raw==b"\0\0" && n==m.header.text_records+1 { *role="text_terminator".into(); }
        else if raw==b"\xe9\x8e\x0d\x0a" || [b"FLIS".as_slice(),b"FCIS",b"RESC",b"7MDI",b"8MDI",b"CMET"].iter().any(|sig|raw.starts_with(sig)) {
            *role="retained_container_metadata".into();
        } else {return Err(Error::Unsupported(format!("unclassified PDB record {n}")));}
    }
    Ok(roles)
}
