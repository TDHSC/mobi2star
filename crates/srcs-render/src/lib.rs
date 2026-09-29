//! Source-preserving rendering. Edits carry byte ranges and replayable reasons.
#![forbid(unsafe_code)]
pub mod css;
pub mod browser;
pub mod readability;
use lexicon_core::{Error,Result,Span};
use serde::{Deserialize,Serialize};
use srcs_reader::{SourceBook,Definition,Page,Target,uri};
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub struct Edit {pub span:Span,pub replacement:String,pub reason:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ResolvedLink {pub source_file:String,pub source_value:Span,pub target:Target,pub target_entry:Option<usize>,pub route:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct PagePlan {pub layout_counts:readability::Counts,pub stardict:Vec<Edit>,pub browser:Vec<Edit>,pub scoped_css:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Plan {pub layout_profile:String,pub namespace:String,pub scope:String,pub page_ids:BTreeMap<String,usize>,pub pages:BTreeMap<String,PagePlan>,pub links:Vec<ResolvedLink>}
#[derive(Debug)]
pub struct Rendered {pub bytes:Vec<u8>,pub prefix_bytes:usize,pub fragment_bytes:usize,pub suffix_bytes:usize}
pub fn escape(s:&str)->String{s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&#39;")}
pub fn entry_route(namespace:&str,id:usize)->String{format!("m2s-{namespace}-e{id}")}
pub fn page_route(namespace:&str,id:usize)->String{format!("m2s-{namespace}-p{id}")}
pub fn browser_path(file:&str)->String{format!("content/{file}.html")}
pub fn browser_anchor(id:usize)->String{format!("m2s-entry-{id}")}
impl Plan {
    pub fn build(book:&SourceBook,namespace:&str)->Result<Self>{
        if namespace.len()!=64||!namespace.bytes().all(|b|b.is_ascii_hexdigit()){return Err(Error::Malformed("source namespace must be SHA-256 hex".into()));}
        let stylesheet_texts = book.pages.values().flat_map(|p| p.stylesheets.iter())
            .map(|name| srcs_reader::markup::utf8(&book.files[name])).collect::<Result<Vec<_>>>()?;
        let readable=readability::applies(&book.package.title,&stylesheet_texts);
        let mut out=Self{layout_profile:if readable{readability::PROFILE.into()}else{"source".into()},namespace:namespace.into(),scope:format!("m2s_{}",&namespace[..16]),
            page_ids:book.pages.keys().enumerate().map(|(i,n)|(n.clone(),i)).collect(),pages:BTreeMap::new(),links:Vec::new()};
        let source_keys:BTreeSet<&str>=book.orths.iter().map(|o|o.value.as_str()).chain(book.forms.iter().map(|f|f.value.as_str())).collect();
        for id in 0..book.entries.len(){if source_keys.contains(entry_route(namespace,id).as_str()){return Err(Error::Incomplete("internal route collides with source word".into()));}}
        for id in out.page_ids.values(){if source_keys.contains(page_route(namespace,*id).as_str()){return Err(Error::Incomplete("chapter route collides with source word".into()));}}
        // Output extensions are explicit, with preflight for macOS path aliases.
        let mut output_paths=BTreeSet::new();
        for file in book.files.keys(){
            let path=if book.pages.contains_key(file){browser_path(file)}else{format!("content/{file}")};
            if !output_paths.insert(uri::collision_key(&path)){return Err(Error::Incomplete("browser output path collision".into()));}
        }
        for page in book.pages.values(){
            let mut plan=PagePlan{layout_counts:readability::Counts::default(),stardict:Vec::new(),browser:Vec::new(),scoped_css:String::new()};
            if readable {
                plan.scoped_css=readability::CSS.into();
                let (layout,counts)=readability::edits(&book.files[&page.file],false)?;
                plan.stardict.extend(layout.clone());plan.browser.extend(layout);
                plan.layout_counts=counts;
                // The browser body receives the profile class, keeping original attributes.
                let raw=&book.files[&page.file];
                let html_preserve::tokenizer::Token::Tag(tag)=html_preserve::tokenizer::Tokenizer::new(page.body_tag.bytes(raw)?).next()
                    .ok_or_else(||Error::Malformed("missing source body".into()))?? else {return Err(Error::Malformed("source body tag".into()));};
                let (at,value)=if let Some(value)=tag.attr("class").and_then(|a|a.value){
                    (page.body_tag.start+value.end," m2s-readable".to_owned())
                }else{(page.body_tag.end-1," class=\"m2s-readable\"".to_owned())};
                plan.browser.push(Edit{span:Span{start:at,end:at},replacement:value,reason:"browser_layout_class".into()});
                plan.browser.push(Edit{span:Span{start:page.head_end,end:page.head_end},replacement:format!("<style>{}</style>",readability::CSS),reason:"reader_portable_css".into()});
            } else {
                for style in &page.stylesheets{
                    let text=srcs_reader::markup::utf8(&book.files[style])?;
                    plan.scoped_css.push_str(&css::scope(text,&out.scope)?);
                }
            }
            for link in &page.links{
                let Some(target)=&link.target else{continue};
                let dest=&book.pages[&target.file];
                let anchor=if target.anchor.is_empty(){None}else{Some(&dest.ids[&target.anchor])};
                if anchor.is_some_and(|a| !dest.body.contains(a.position)&&!dest.body_tag.contains(a.position)){
                    return Err(Error::Unsupported("link targets source head/root outside rendered body".into()));
                }
                let target_entry=anchor.and_then(|a|a.entry_id);
                let route=target_entry.map(|id|entry_route(namespace,id)).unwrap_or_else(||page_route(namespace,out.page_ids[&target.file]));
                let suffix=if target.anchor.is_empty(){String::new()}else{format!("#{}",uri::percent_encode(&target.anchor,false))};
                plan.stardict.push(Edit{span:link.value,replacement:escape(&format!("bword://{route}{suffix}")),reason:"resolved_internal_link".into()});
                let relative=uri::relative(&browser_path(&page.file),&browser_path(&target.file));
                plan.browser.push(Edit{span:link.value,replacement:escape(&format!("{}{suffix}",uri::percent_encode(&relative,true))),reason:"browser_local_link".into()});
                out.links.push(ResolvedLink{source_file:page.file.clone(),source_value:link.value,target:target.clone(),target_entry,route});
            }
            for image in &page.images{
                let target=image.target.as_ref().ok_or_else(||Error::Incomplete("unresolved image".into()))?;
                plan.stardict.push(Edit{span:image.value,replacement:escape(&uri::percent_encode(&format!("source/{}",target.file),true)),reason:"image_resource_path".into()});
            }
            for &id in &page.entries{
                let anchor=browser_anchor(id);
                if page.ids.contains_key(&anchor){return Err(Error::Incomplete("generated browser anchor collision".into()));}
                let start=book.entries[id].span.start;
                plan.browser.push(Edit{span:Span{start,end:start},replacement:format!("<a id=\"{anchor}\"></a>"),reason:"browser_lookup_anchor".into()});
            }
            plan.browser.push(Edit{span:Span{start:page.head_end,end:page.head_end},replacement:"<meta charset=\"utf-8\"/>".into(),reason:"explicit_html_encoding".into()});
            for edits in [&mut plan.stardict,&mut plan.browser]{
                edits.sort_by_key(|e|(e.span.start,e.span.end));
                for pair in edits.windows(2){if pair[0].span.end>pair[1].span.start{return Err(Error::Incomplete("overlapping source edits".into()));}}
            }
            out.pages.insert(page.file.clone(),plan);
        }
        Ok(out)
    }
    pub fn definition(&self,book:&SourceBook,entry:&Definition)->Result<Rendered>{self.render(book,&book.pages[&entry.file],entry.span,&entry.ancestors)}
    pub fn chapter(&self,book:&SourceBook,page:&Page)->Result<Rendered>{self.render(book,page,page.body,&[])}
    fn render(&self,book:&SourceBook,page:&Page,span:Span,ancestors:&[srcs_reader::Ancestor])->Result<Rendered>{
        let raw=&book.files[&page.file];let plan=&self.pages[&page.file];
        let body=srcs_reader::markup::utf8(page.body_tag.bytes(raw)?)?.replacen("<body","<div",1);
        let mut prefix=format!("<style>{}</style><div class=\"{}\">{body}",plan.scoped_css,if self.layout_profile==readability::PROFILE{format!("{} m2s-readable",self.scope)}else{self.scope.clone()}).into_bytes();
        for ancestor in ancestors{prefix.extend_from_slice(ancestor.start_tag.bytes(raw)?);}
        let mut suffix=String::new();for ancestor in ancestors.iter().rev(){suffix.push_str(&format!("</{}>",ancestor.name));}suffix.push_str("</div></div>");
        let fragment=replay(raw,&plan.stardict,span)?;
        let prefix_bytes=prefix.len();let fragment_bytes=fragment.len();let suffix_bytes=suffix.len();
        prefix.extend(fragment);prefix.extend_from_slice(suffix.as_bytes());
        Ok(Rendered{bytes:prefix,prefix_bytes,fragment_bytes,suffix_bytes})
    }
    pub fn browser_page(&self,book:&SourceBook,page:&Page)->Result<Vec<u8>>{
        let raw=&book.files[&page.file];replay(raw,&self.pages[&page.file].browser,Span{start:0,end:raw.len()})
    }
}
pub fn replay(raw:&[u8],edits:&[Edit],span:Span)->Result<Vec<u8>>{
    span.bytes(raw)?;let mut cursor=span.start;let mut out=Vec::new();
    let first=edits.partition_point(|e|e.span.start<span.start);
    if first>0&&edits[first-1].span.end>span.start{return Err(Error::Incomplete("edit crosses fragment start".into()));}
    for edit in &edits[first..]{
        if edit.span.start>=span.end{break;}
        if edit.span.start<cursor||edit.span.end<edit.span.start||edit.span.end>span.end{return Err(Error::Incomplete("edit overlap/boundary".into()));}
        out.extend_from_slice(&raw[cursor..edit.span.start]);out.extend_from_slice(edit.replacement.as_bytes());cursor=edit.span.end;
    }
    out.extend_from_slice(&raw[cursor..span.end]);Ok(out)
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn splice_preserves_every_other_byte(){let e=vec![Edit{span:Span{start:3,end:4},replacement:"long".into(),reason:"test".into()}];assert_eq!(replay(b"abcXdef",&e,Span{start:0,end:7}).unwrap(),b"abclongdef");}
    #[test]fn crossings_rejected(){let e=vec![Edit{span:Span{start:1,end:4},replacement:"x".into(),reason:"test".into()}];assert!(replay(b"abcdef",&e,Span{start:2,end:6}).is_err());assert!(replay(b"abcdef",&e,Span{start:0,end:3}).is_err());}
    #[test]fn escaping_and_namespace(){assert_eq!(escape("a<&\"'"),"a&lt;&amp;&quot;&#39;");assert_ne!(entry_route(&"a".repeat(64),0),entry_route(&"b".repeat(64),0));}
}
