//! Source-preserving rendering. Edits carry byte ranges and replayable reasons.
#![forbid(unsafe_code)]
pub mod browser;
pub mod css;
pub mod readability;
use lexicon_core::{Error, Result, Span, StyleDelivery, LINK_TAG};
use serde::{Deserialize, Serialize};
use srcs_reader::{uri, Definition, Page, SourceBook, Target};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edit {
    pub span: Span,
    pub replacement: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedLink {
    pub source_file: String,
    pub source_value: Span,
    pub target: Target,
    pub target_entry: Option<usize>,
    pub route: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PagePlan {
    pub layout_counts: readability::Counts,
    pub stardict: Vec<Edit>,
    pub browser: Vec<Edit>,
    /// Index into [`Plan::style_sets`].
    pub style_set: usize,
}
/// CSS shared by every page with the same ordered stylesheet list, scoped
/// under `class`, which is also the class of those pages' payload wrappers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StyleSet {
    /// Source stylesheets in page order; empty for a layout profile's CSS.
    pub stylesheets: Vec<String>,
    pub class: String,
    pub css: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub layout_profile: String,
    pub namespace: String,
    pub scope: String,
    pub page_ids: BTreeMap<String, usize>,
    pub pages: BTreeMap<String, PagePlan>,
    pub style_sets: Vec<StyleSet>,
    pub links: Vec<ResolvedLink>,
}
#[derive(Debug)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub prefix_bytes: usize,
    pub fragment_bytes: usize,
    pub suffix_bytes: usize,
}
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn entry_route(namespace: &str, id: usize) -> String {
    format!("m2s-{namespace}-e{id}")
}
pub fn page_route(namespace: &str, id: usize) -> String {
    format!("m2s-{namespace}-p{id}")
}
pub fn browser_path(file: &str) -> String {
    format!("content/{file}.html")
}
pub fn browser_anchor(id: usize) -> String {
    format!("m2s-entry-{id}")
}
/// Groups pages by their ordered stylesheet list. The first list keeps the
/// book scope as its class, so books with one list render exactly as before;
/// later lists get `{scope}-s{n}`. A set's selectors only match wrappers that
/// carry its class, so all sets can share one stylesheet file without
/// changing any page's cascade.
fn style_sets<'a>(
    scope: &str,
    pages: &[&[String]],
    text: impl Fn(&str) -> Result<&'a str>,
) -> Result<(Vec<StyleSet>, Vec<usize>)> {
    let mut sets: Vec<StyleSet> = Vec::new();
    let mut assigned = Vec::with_capacity(pages.len());
    for &stylesheets in pages {
        let index = match sets.iter().position(|s| s.stylesheets == stylesheets) {
            Some(index) => index,
            None => {
                let class = if sets.is_empty() {
                    scope.to_owned()
                } else {
                    format!("{scope}-s{}", sets.len())
                };
                let mut css_text = String::new();
                for file in stylesheets {
                    css_text.push_str(&css::scope(text(file)?, &class)?);
                }
                sets.push(StyleSet {
                    stylesheets: stylesheets.to_vec(),
                    class,
                    css: css_text,
                });
                sets.len() - 1
            }
        };
        assigned.push(index);
    }
    Ok((sets, assigned))
}
impl Plan {
    pub fn build(book: &SourceBook, namespace: &str) -> Result<Self> {
        if namespace.len() != 64 || !namespace.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Malformed(
                "source namespace must be SHA-256 hex".into(),
            ));
        }
        let stylesheet_texts = book
            .pages
            .values()
            .flat_map(|p| p.stylesheets.iter())
            .map(|name| srcs_reader::markup::utf8(&book.files[name]))
            .collect::<Result<Vec<_>>>()?;
        let readable = readability::applies(&book.package.title, &stylesheet_texts);
        let mut out = Self {
            layout_profile: if readable {
                readability::PROFILE.into()
            } else {
                "source".into()
            },
            namespace: namespace.into(),
            scope: format!("m2s_{}", &namespace[..16]),
            page_ids: book
                .pages
                .keys()
                .enumerate()
                .map(|(i, n)| (n.clone(), i))
                .collect(),
            pages: BTreeMap::new(),
            style_sets: Vec::new(),
            links: Vec::new(),
        };
        let page_sets = if readable {
            out.style_sets.push(StyleSet {
                stylesheets: Vec::new(),
                class: out.scope.clone(),
                css: readability::CSS.into(),
            });
            vec![0; book.pages.len()]
        } else {
            let lists: Vec<&[String]> = book
                .pages
                .values()
                .map(|p| p.stylesheets.as_slice())
                .collect();
            let (sets, assigned) = style_sets(&out.scope, &lists, |file| {
                srcs_reader::markup::utf8(&book.files[file])
            })?;
            out.style_sets = sets;
            assigned
        };
        let source_keys: BTreeSet<&str> = book
            .orths
            .iter()
            .map(|o| o.value.as_str())
            .chain(book.forms.iter().map(|f| f.value.as_str()))
            .collect();
        for id in 0..book.entries.len() {
            if source_keys.contains(entry_route(namespace, id).as_str()) {
                return Err(Error::Incomplete(
                    "internal route collides with source word".into(),
                ));
            }
        }
        for id in out.page_ids.values() {
            if source_keys.contains(page_route(namespace, *id).as_str()) {
                return Err(Error::Incomplete(
                    "chapter route collides with source word".into(),
                ));
            }
        }
        // Output extensions are explicit, with preflight for macOS path aliases.
        let mut output_paths = BTreeSet::new();
        for file in book.files.keys() {
            let path = if book.pages.contains_key(file) {
                browser_path(file)
            } else {
                format!("content/{file}")
            };
            if !output_paths.insert(uri::collision_key(&path)) {
                return Err(Error::Incomplete("browser output path collision".into()));
            }
        }
        for (page, &style_set) in book.pages.values().zip(&page_sets) {
            let mut plan = PagePlan {
                layout_counts: readability::Counts::default(),
                stardict: Vec::new(),
                browser: Vec::new(),
                style_set,
            };
            if readable {
                let (layout, counts) = readability::edits(&book.files[&page.file], false)?;
                plan.stardict.extend(layout.clone());
                plan.browser.extend(layout);
                plan.layout_counts = counts;
                // The browser body receives the profile class, keeping original attributes.
                let raw = &book.files[&page.file];
                let html_preserve::tokenizer::Token::Tag(tag) =
                    html_preserve::tokenizer::Tokenizer::new(page.body_tag.bytes(raw)?)
                        .next()
                        .ok_or_else(|| Error::Malformed("missing source body".into()))??
                else {
                    return Err(Error::Malformed("source body tag".into()));
                };
                let (at, value) = if let Some(value) = tag.attr("class").and_then(|a| a.value) {
                    (page.body_tag.start + value.end, " m2s-readable".to_owned())
                } else {
                    (page.body_tag.end - 1, " class=\"m2s-readable\"".to_owned())
                };
                plan.browser.push(Edit {
                    span: Span { start: at, end: at },
                    replacement: value,
                    reason: "browser_layout_class".into(),
                });
                plan.browser.push(Edit {
                    span: Span {
                        start: page.head_end,
                        end: page.head_end,
                    },
                    replacement: format!("<style>{}</style>", readability::CSS),
                    reason: "reader_portable_css".into(),
                });
            }
            for link in &page.links {
                let Some(target) = &link.target else { continue };
                let dest = &book.pages[&target.file];
                let anchor = if target.anchor.is_empty() {
                    None
                } else {
                    Some(&dest.ids[&target.anchor])
                };
                if anchor.is_some_and(|a| {
                    !dest.body.contains(a.position) && !dest.body_tag.contains(a.position)
                }) {
                    return Err(Error::Unsupported(
                        "link targets source head/root outside rendered body".into(),
                    ));
                }
                let target_entry = anchor.and_then(|a| a.entry_id);
                let route = target_entry
                    .map(|id| entry_route(namespace, id))
                    .unwrap_or_else(|| page_route(namespace, out.page_ids[&target.file]));
                let suffix = if target.anchor.is_empty() {
                    String::new()
                } else {
                    format!("#{}", uri::percent_encode(&target.anchor, false))
                };
                plan.stardict.push(Edit {
                    span: link.value,
                    replacement: escape(&format!("bword://{route}{suffix}")),
                    reason: "resolved_internal_link".into(),
                });
                let relative =
                    uri::relative(&browser_path(&page.file), &browser_path(&target.file));
                plan.browser.push(Edit {
                    span: link.value,
                    replacement: escape(&format!(
                        "{}{suffix}",
                        uri::percent_encode(&relative, true)
                    )),
                    reason: "browser_local_link".into(),
                });
                out.links.push(ResolvedLink {
                    source_file: page.file.clone(),
                    source_value: link.value,
                    target: target.clone(),
                    target_entry,
                    route,
                });
            }
            for image in &page.images {
                let target = image
                    .target
                    .as_ref()
                    .ok_or_else(|| Error::Incomplete("unresolved image".into()))?;
                plan.stardict.push(Edit {
                    span: image.value,
                    replacement: escape(&uri::percent_encode(
                        &format!("source/{}", target.file),
                        true,
                    )),
                    reason: "image_resource_path".into(),
                });
            }
            for &id in &page.entries {
                let anchor = browser_anchor(id);
                if page.ids.contains_key(&anchor) {
                    return Err(Error::Incomplete(
                        "generated browser anchor collision".into(),
                    ));
                }
                let start = book.entries[id].span.start;
                plan.browser.push(Edit {
                    span: Span { start, end: start },
                    replacement: format!("<a id=\"{anchor}\"></a>"),
                    reason: "browser_lookup_anchor".into(),
                });
            }
            plan.browser.push(Edit {
                span: Span {
                    start: page.head_end,
                    end: page.head_end,
                },
                replacement: "<meta charset=\"utf-8\"/>".into(),
                reason: "explicit_html_encoding".into(),
            });
            for edits in [&mut plan.stardict, &mut plan.browser] {
                edits.sort_by_key(|e| (e.span.start, e.span.end));
                for pair in edits.windows(2) {
                    if pair[0].span.end > pair[1].span.start {
                        return Err(Error::Incomplete("overlapping source edits".into()));
                    }
                }
            }
            out.pages.insert(page.file.clone(), plan);
        }
        Ok(out)
    }
    pub fn definition(
        &self,
        book: &SourceBook,
        entry: &Definition,
        style: StyleDelivery,
    ) -> Result<Rendered> {
        self.render(
            book,
            &book.pages[&entry.file],
            entry.span,
            &entry.ancestors,
            style,
        )
    }
    pub fn chapter(
        &self,
        book: &SourceBook,
        page: &Page,
        style: StyleDelivery,
    ) -> Result<Rendered> {
        self.render(book, page, page.body, &[], style)
    }
    /// The dictionary-wide stylesheet: every style set's CSS, in set order.
    pub fn stylesheet(&self) -> String {
        self.style_sets.iter().map(|set| set.css.as_str()).collect()
    }
    /// Style set of generated pages (image galleries), present when a layout
    /// profile styles the whole book.
    pub fn profile_style_set(&self) -> Option<usize> {
        (self.layout_profile == readability::PROFILE).then_some(0)
    }
    /// Stylesheet references a payload using style set `set` starts with.
    /// A set without CSS needs none.
    pub fn style_prefix(&self, set: usize, style: StyleDelivery) -> String {
        let css = &self.style_sets[set].css;
        let mut out = String::new();
        if css.is_empty() {
            return out;
        }
        if style.link {
            out.push_str(LINK_TAG);
        }
        if style.inline {
            out.push_str("<style>");
            out.push_str(css);
            out.push_str("</style>");
        }
        out
    }
    fn render(
        &self,
        book: &SourceBook,
        page: &Page,
        span: Span,
        ancestors: &[srcs_reader::Ancestor],
        style: StyleDelivery,
    ) -> Result<Rendered> {
        let raw = &book.files[&page.file];
        let plan = &self.pages[&page.file];
        let body =
            srcs_reader::markup::utf8(page.body_tag.bytes(raw)?)?.replacen("<body", "<div", 1);
        let set = &self.style_sets[plan.style_set];
        let class = if self.layout_profile == readability::PROFILE {
            format!("{} m2s-readable", set.class)
        } else {
            set.class.clone()
        };
        let mut prefix = format!(
            "{}<div class=\"{class}\">{body}",
            self.style_prefix(plan.style_set, style)
        )
        .into_bytes();
        for ancestor in ancestors {
            prefix.extend_from_slice(ancestor.start_tag.bytes(raw)?);
        }
        let mut suffix = String::new();
        for ancestor in ancestors.iter().rev() {
            suffix.push_str(&format!("</{}>", ancestor.name));
        }
        suffix.push_str("</div></div>");
        let fragment = replay(raw, &plan.stardict, span)?;
        let prefix_bytes = prefix.len();
        let fragment_bytes = fragment.len();
        let suffix_bytes = suffix.len();
        prefix.extend(fragment);
        prefix.extend_from_slice(suffix.as_bytes());
        Ok(Rendered {
            bytes: prefix,
            prefix_bytes,
            fragment_bytes,
            suffix_bytes,
        })
    }
    pub fn browser_page(&self, book: &SourceBook, page: &Page) -> Result<Vec<u8>> {
        let raw = &book.files[&page.file];
        replay(
            raw,
            &self.pages[&page.file].browser,
            Span {
                start: 0,
                end: raw.len(),
            },
        )
    }
}
pub fn replay(raw: &[u8], edits: &[Edit], span: Span) -> Result<Vec<u8>> {
    span.bytes(raw)?;
    let mut cursor = span.start;
    let mut out = Vec::new();
    let first = edits.partition_point(|e| e.span.start < span.start);
    if first > 0 && edits[first - 1].span.end > span.start {
        return Err(Error::Incomplete("edit crosses fragment start".into()));
    }
    for edit in &edits[first..] {
        if edit.span.start >= span.end {
            break;
        }
        if edit.span.start < cursor || edit.span.end < edit.span.start || edit.span.end > span.end {
            return Err(Error::Incomplete("edit overlap/boundary".into()));
        }
        out.extend_from_slice(&raw[cursor..edit.span.start]);
        out.extend_from_slice(edit.replacement.as_bytes());
        cursor = edit.span.end;
    }
    out.extend_from_slice(&raw[cursor..span.end]);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splice_preserves_every_other_byte() {
        let e = vec![Edit {
            span: Span { start: 3, end: 4 },
            replacement: "long".into(),
            reason: "test".into(),
        }];
        assert_eq!(
            replay(b"abcXdef", &e, Span { start: 0, end: 7 }).unwrap(),
            b"abclongdef"
        );
    }
    #[test]
    fn crossings_rejected() {
        let e = vec![Edit {
            span: Span { start: 1, end: 4 },
            replacement: "x".into(),
            reason: "test".into(),
        }];
        assert!(replay(b"abcdef", &e, Span { start: 2, end: 6 }).is_err());
        assert!(replay(b"abcdef", &e, Span { start: 0, end: 3 }).is_err());
    }
    #[test]
    fn pages_are_grouped_by_ordered_stylesheet_list() {
        let files: BTreeMap<&str, &str> = [
            ("a.css", "p { color: red }"),
            ("b.css", "p { color: blue }"),
        ]
        .into();
        let lists: Vec<Vec<String>> = vec![
            vec!["a.css".into()],
            vec!["a.css".into()],
            vec!["b.css".into(), "a.css".into()],
            vec![],
            vec!["a.css".into(), "b.css".into()],
        ];
        let pages: Vec<&[String]> = lists.iter().map(Vec::as_slice).collect();
        let (sets, assigned) = style_sets("m2s_x", &pages, |f| Ok(files[f])).unwrap();
        assert_eq!(assigned, [0, 0, 1, 2, 3]);
        // The first list keeps the book scope; later lists get their own class.
        let classes: Vec<&str> = sets.iter().map(|s| s.class.as_str()).collect();
        assert_eq!(classes, ["m2s_x", "m2s_x-s1", "m2s_x-s2", "m2s_x-s3"]);
        assert!(sets[0].css.contains(".m2s_x p") && !sets[0].css.contains("-s"));
        // Each set is scoped only under its own class, in its own file order.
        let s1 = &sets[1].css;
        assert!(!s1.contains(".m2s_x p") && s1.find("blue").unwrap() < s1.find("red").unwrap());
        assert!(sets[2].css.is_empty());
        let s3 = &sets[3].css;
        assert!(s3.find("red").unwrap() < s3.find("blue").unwrap());
    }
    #[test]
    fn style_prefix_follows_the_delivery() {
        let plan = Plan {
            layout_profile: "source".into(),
            namespace: "0".repeat(64),
            scope: "m2s_x".into(),
            page_ids: BTreeMap::new(),
            pages: BTreeMap::new(),
            style_sets: vec![
                StyleSet {
                    stylesheets: vec!["a.css".into()],
                    class: "m2s_x".into(),
                    css: ".m2s_x p{}".into(),
                },
                StyleSet {
                    stylesheets: vec![],
                    class: "m2s_x-s1".into(),
                    css: String::new(),
                },
            ],
            links: Vec::new(),
        };
        let both = StyleDelivery {
            link: true,
            inline: true,
        };
        let link = StyleDelivery {
            link: true,
            inline: false,
        };
        assert_eq!(
            plan.style_prefix(0, StyleDelivery::INLINE),
            "<style>.m2s_x p{}</style>"
        );
        assert_eq!(plan.style_prefix(0, link), LINK_TAG);
        assert_eq!(
            plan.style_prefix(0, both),
            format!("{LINK_TAG}<style>.m2s_x p{{}}</style>")
        );
        assert_eq!(
            plan.style_prefix(1, both),
            "",
            "a set without CSS needs no reference"
        );
        assert_eq!(plan.stylesheet(), ".m2s_x p{}");
        assert_eq!(plan.profile_style_set(), None);
    }
    #[test]
    fn escaping_and_namespace() {
        assert_eq!(escape("a<&\"'"), "a&lt;&amp;&quot;&#39;");
        assert_ne!(
            entry_route(&"a".repeat(64), 0),
            entry_route(&"b".repeat(64), 0)
        );
    }
}
