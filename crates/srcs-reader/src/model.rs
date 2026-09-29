use lexicon_core::Span;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct SourceBook {
    pub files: BTreeMap<String, Vec<u8>>,
    pub archive_sha256: String,
    pub archive_record: usize,
    pub pages: BTreeMap<String, Page>,
    pub entries: Vec<Definition>,
    pub orths: Vec<Orth>,
    pub forms: Vec<Inflection>,
    pub package: Package,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub id: usize,
    pub file: String,
    pub span: Span,
    pub orths: Vec<usize>,
    pub ancestors: Vec<Ancestor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ancestor {
    pub name: String,
    pub start_tag: Span,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Orth {
    pub id: usize,
    pub entry_id: usize,
    pub value: String,
    pub span: Span,
    pub forms: Vec<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inflection {
    pub id: usize,
    pub entry_id: usize,
    pub orth_id: usize,
    pub value: String,
    pub attributes: BTreeMap<String, String>,
    pub position: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub position: usize,
    pub entry_id: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub file: String,
    pub anchor: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub value: Span,
    pub url: String,
    pub target: Option<Target>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub file: String,
    pub title: String,
    pub body: Span,
    pub body_tag: Span,
    pub head_end: usize,
    pub entries: Vec<usize>,
    pub ids: BTreeMap<String, Anchor>,
    pub links: Vec<Reference>,
    pub images: Vec<Reference>,
    pub stylesheets: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Package {
    pub file: String,
    pub title: String,
    pub spine: Vec<String>,
    pub manifest: BTreeMap<String, ManifestItem>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestItem {
    pub file: String,
    pub media_type: String,
}
