//! Static offline viewer. Conversion emits bytes; it starts no browser/server.
use crate::{Plan,escape,browser_path,browser_anchor};
use lexicon_core::Result;
use srcs_reader::{SourceBook,uri::percent_encode};

pub fn lookup_data(book:&SourceBook)->Result<Vec<u8>>{
    let mut rows=Vec::new();
    for orth in &book.orths{
        let entry=&book.entries[orth.entry_id];
        let url=format!("{}#{}",percent_encode(&browser_path(&entry.file),true),browser_anchor(entry.id));
        rows.push(serde_json::json!([orth.value,url,orth.value,entry.id]));
        for &id in &orth.forms{rows.push(serde_json::json!([book.forms[id].value,url,orth.value,entry.id]));}
    }
    let json=serde_json::to_string(&rows)?.replace('<',"\\u003c").replace('\u{2028}',"\\u2028").replace('\u{2029}',"\\u2029");
    Ok(format!("window.MOBI2STAR_LOOKUP={json};\n").into_bytes())
}
pub fn index(book:&SourceBook,plan:&Plan)->Vec<u8>{
    let title=escape(&book.package.title);let mut nav=String::new();
    let mut paths=book.package.spine.clone();
    for path in plan.page_ids.keys(){if !paths.contains(path){paths.push(path.clone());}}
    for path in paths{let page=&book.pages[&path];nav.push_str(&format!("<a href=\"{}\">{}</a> ",escape(&percent_encode(&browser_path(&path),true)),escape(&page.title)));}
    format!(r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title}</title><link rel="stylesheet" href="viewer.css"></head><body><h1>{title}</h1><p>本地离线词典 · 完整章节 · 显式词形</p><form id="search"><label for="word">单词或词形</label><input id="word" autocomplete="off" required><button>查询</button></form><p id="status" aria-live="polite"></p><section id="results"></section><h2>原书目录</h2><nav>{nav}</nav><p><a href="images.html">全部原始图片与编译图片</a></p><script src="lookup-data.js"></script><script src="viewer.js"></script></body></html>"#).into_bytes()
}
pub const CSS:&str="body{font-family:system-ui,sans-serif;max-width:65rem;margin:2rem auto;padding:0 1rem;line-height:1.6}input,button{font:inherit;padding:.6rem}label{display:block}#results a{display:block;padding:.25rem}nav{display:flex;flex-wrap:wrap;gap:.6rem}";
pub const JS:&str=r#"'use strict';
const exact=new Map(),folded=new Map();
for(const row of window.MOBI2STAR_LOOKUP){
  for(const [map,key] of [[exact,row[0]],[folded,row[0].toLocaleLowerCase('en')]]){
    if(!map.has(key)) map.set(key,[]);
    map.get(key).push(row);
  }
}
document.getElementById('search').addEventListener('submit',event=>{
  event.preventDefault();
  const raw=document.getElementById('word').value;
  const q=exact.has(raw)?raw:raw.trim();
  const rows=exact.get(q)||folded.get(q.toLocaleLowerCase('en'))||[];
  const box=document.getElementById('results');box.replaceChildren();
  const seen=new Set();
  for(const row of rows){
    if(seen.has(row[3]))continue;seen.add(row[3]);
    const link=document.createElement('a');link.href=row[1];
    link.textContent=row[0]===row[2]?row[2]:row[0]+' → '+row[2];box.appendChild(link);
  }
  document.getElementById('status').textContent=seen.size?'找到 '+seen.size+' 个释义块':'未找到精确匹配，可按原书目录浏览。';
});
"#;
