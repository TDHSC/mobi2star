//! KOReader's dictionary popup: the HTML document it gives MuPDF, how it
//! looks words up and follows links, the popup's geometry and its fonts.
//!
//! Ported from KOReader (AGPL-3.0-or-later) at commit b539d24b:
//! - frontend/ui/widget/htmlboxwidget.lua: `mupdf_css_fixes`, and
//!   `setContent`'s document template and `<br>` rewrite;
//! - frontend/ui/widget/dictquicklookup.lua: `getHtmlDictionaryCss`,
//!   `addQueryWordToResult`, and the popup and definition sizes;
//! - frontend/apps/reader/modules/readerdictionary.lua:
//!   `onHtmlDictionaryLinkTapped` and `startSdcv`;
//! - frontend/ui/size.lua and koreader-base fe41d769 ffi/framebuffer.lua:
//!   `Size` and `scaleBySize`;
//! - koreader-fonts 04697698: the font files.
use crate::{
    html::{anchors, inline_images, wrap_characters},
    url::{clean_path, percent_decode},
    App, Dictionary, Match, Outcome, Stay, View,
};
use lexicon_core::{bytes::image_type, Result};
use serde::Serialize;

const MUPDF_CSS_FIXES: &str =
    "article, aside, button, canvas, datalist, details, dialog, dir, fieldset, figcaption,
figure, footer, form, frame, frameset, header, hgroup, iframe, legend, listing,
main, map, marquee, multicol, nav, noembed, noframes, noscript, optgroup, output,
plaintext, search, select, summary, template, textarea, video, xmp {
  display: block;
}
";

/// `getHtmlDictionaryCss` with the default `dict_justify` (on).
const DICTIONARY_CSS: &str = "        @page {
            margin: 0;
            font-family: 'Noto Sans';
        }

        body {
            margin: 0;
            line-height: 1.3;
            text-align: justify;
        }

        blockquote, dd {
            margin: 0 1em;
        }

        ol, ul, menu {
            margin: 0; padding: 0 1.7em;
        }
    ";

/// `html:gsub("%<br ?/?%>", "&nbsp;<div></div>")`: `<br>`, `<br/>`,
/// `<br >` and `<br />`, lowercase only.
fn rewrite_br(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<br") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 3..];
        let after_space = after.strip_prefix(' ').unwrap_or(after);
        let after_slash = after_space.strip_prefix('/').unwrap_or(after_space);
        if let Some(tail) = after_slash.strip_prefix('>') {
            out.push_str("&nbsp;<div></div>");
            rest = tail;
        } else {
            out.push_str("<br");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The document KOReader renders for one result. `query` is set for the
/// first result, which carries the looked-up word underneath.
pub fn document(dictionary: &Dictionary, entry: usize, query: Option<&str>) -> Result<String> {
    // sdcv starts an HTML definition on a new line.
    let mut body = format!("\n{}", dictionary.payload(entry)?);
    if let Some(word) = query {
        body.push_str("<br/>_______<br/>");
        body.push_str(&format!("(query : {word})"));
    }
    // MuPDF opens the definition with res/ as its directory: paths are
    // URL-decoded and cleaned. It decodes PNG, JPEG and GIF data URIs.
    let body = inline_images(&body, |src| {
        if src.contains("://") || src.starts_with("data:") {
            return None;
        }
        let bytes = dictionary.resource(&clean_path(&percent_decode(src)))?;
        matches!(image_type(bytes), Some(("png" | "jpg" | "gif", _))).then_some(bytes)
    });
    let css = format!(
        "{DICTIONARY_CSS}{}",
        dictionary.companion_css().unwrap_or("")
    );
    Ok(rewrite_br(&format!(
        "<html><head><style>\n{MUPDF_CSS_FIXES}\n{css}</style></head><body>{body}</body></html>"
    )))
}

/// The document for result `n` of a lookup of `query`: the first result
/// carries the query line (`addQueryWordToResult`).
pub fn result_document(
    dictionary: &Dictionary,
    query: &str,
    entry: usize,
    n: usize,
) -> Result<String> {
    document(dictionary, entry, (n == 0).then_some(query))
}

/// `startSdcv`: the word, then its lowercase form, each looked up exactly.
/// The view carries no documents; see `result_document`.
pub fn search(dictionary: &Dictionary, word: &str) -> Result<Outcome> {
    let mut hits = dictionary.lookup_exact(word);
    let lowercase = word.to_lowercase();
    if lowercase != word {
        for hit in dictionary.lookup_exact(&lowercase) {
            if !hits.contains(&hit) {
                hits.push(hit);
            }
        }
    }
    if hits.is_empty() {
        return Ok(Outcome::NotFound { word: word.into() });
    }
    let results = hits
        .iter()
        .map(|&entry| {
            Ok(Match {
                headword: dictionary.entry(entry)?.word.clone(),
                entry,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Outcome::View(View {
        app: App::Koreader,
        query: word.into(),
        results,
        documents: Vec::new(),
        scroll_to: None,
    }))
}

/// `onHtmlDictionaryLinkTapped`, given the link URI exactly as MuPDF
/// reports it and the entry being shown. MuPDF treats a link whose
/// `#fragment` names an element of that entry as internal, which KOReader
/// ignores without scrolling. A new lookup starts at its first result.
pub fn follow(dictionary: &Dictionary, uri: &str, current: usize) -> Result<Outcome> {
    if let Some((_, fragment)) = uri.split_once('#') {
        if anchors(&dictionary.payload(current)?).contains(fragment) {
            return Ok(Outcome::Stay {
                reason: Stay::AnchorInEntry,
            });
        }
    }
    let word = match uri.strip_prefix("bword://") {
        Some(word) => word,
        None if uri.contains("://") => {
            return Ok(Outcome::Stay {
                reason: Stay::ExternalLink,
            })
        }
        None => uri,
    };
    if word.is_empty() {
        return Ok(Outcome::Stay {
            reason: Stay::NotFollowed,
        });
    }
    search(dictionary, word)
}

/// The size of KOReader's dictionary text box, in device pixels, and the
/// font size MuPDF lays it out with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub em: u32,
}

/// The `dict_font_size` setting's range and default
/// (frontend/apps/reader/modules/readerdictionary.lua).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FontSizes {
    pub min: u32,
    pub max: u32,
    pub default: u32,
}
pub const FONT_SIZES: FontSizes = FontSizes {
    min: 8,
    max: 32,
    default: 20,
};

impl Geometry {
    /// The normal (not large) popup on a `width`×`height` screen with no DPI
    /// override and the footer hidden.
    pub fn for_screen(width: u32, height: u32, font_size: u32) -> Self {
        let scale = f64::from(width.min(height)) / 600.0;
        // scaleBySize: ceil(px * (size_scale + dpi_scale) / 2)
        let size = |px: f64| (px * (scale + scale) / 2.0).ceil() as u32;
        let round = |x: f64| (x + 0.5).floor() as u32;
        let window = width.saturating_sub(size(80.0));
        let inner = window.saturating_sub(2 * size(1.5));
        let content = inner.saturating_sub(2 * size(10.0));
        let box_width = content.saturating_sub(size(6.0) + size(12.0));
        let available = height.saturating_sub(2 * size(5.0));
        let definition = (f64::from(available) * 0.5 * 0.7).floor();
        let em = size(f64::from(font_size));
        let line = round(1.3 * f64::from(em)).max(1);
        let lines = round(definition / f64::from(line)).max(1);
        Self {
            width: box_width,
            height: lines * line,
            em,
        }
    }
}

/// A device screen to lay the popup out for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Screen {
    pub id: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Common screens, the first being the default.
pub const SCREENS: &[Screen] = &[
    Screen {
        id: "6in-300ppi",
        width: 1072,
        height: 1448,
    },
    Screen {
        id: "6.8in-300ppi",
        width: 1236,
        height: 1648,
    },
    Screen {
        id: "7in-300ppi",
        width: 1264,
        height: 1680,
    },
    Screen {
        id: "10.3in-227ppi",
        width: 1404,
        height: 1872,
    },
    Screen {
        id: "phone",
        width: 1080,
        height: 2400,
    },
];

/// koreader-fonts files for 'Noto Sans', the family KOReader sets, by
/// bold and italic.
pub const NOTO_SANS: [&str; 4] = [
    "NotoSans-Regular.ttf",
    "NotoSans-Italic.ttf",
    "NotoSans-Bold.ttf",
    "NotoSans-BoldItalic.ttf",
];
/// KOReader's fallbacks for characters the page's fonts lack: Noto Sans
/// CJK SC for every script, then FreeSerif where MuPDF would try its own
/// symbol fonts.
pub const FALLBACK_FONT: &str = "NotoSansCJKsc-Regular.otf";
pub const LAST_FONT: &str = "FreeSerif.ttf";
/// A family no entry uses, which `font_for` maps to `LAST_FONT`.
pub const LAST_FONT_FAMILY: &str = "mobi2star last resort";

/// The scripts Noto Sans CJK SC has characters for, by the names MuPDF's
/// WebAssembly build asks with.
const FALLBACK_SCRIPTS: &[&str] = &[
    "SC",
    "TC",
    "JP",
    "KR",
    "Latin",
    "Greek",
    "Cyrillic",
    "Common",
    "Inherited",
];

/// The font file KOReader's MuPDF would draw with for a request from
/// MuPDF's WebAssembly build, or `None` to let MuPDF use its own.
///
/// That build asks by family with the script "undefined", and for a
/// fallback by script alone ("TC", "Hebrew", ...). KOReader's MuPDF
/// (koreader-base thirdparty/mupdf/external_fonts.patch, at koreader
/// b539d24) maps exactly the family "Noto Sans" to koreader-fonts' files,
/// other families to the URW fonts MuPDF also builds in, and every
/// fallback to Noto Sans CJK SC, then FreeSerif. The hook answers one font
/// per script, so a script the CJK font has no characters for goes
/// straight to FreeSerif, where KOReader ends up.
pub fn font_for(family: &str, script: &str, bold: bool, italic: bool) -> Option<&'static str> {
    if family == "Noto Sans" {
        return Some(NOTO_SANS[usize::from(bold) * 2 + usize::from(italic)]);
    }
    if family == LAST_FONT_FAMILY {
        return Some(LAST_FONT);
    }
    if !matches!(family, "" | "undefined") || matches!(script, "" | "undefined") {
        return None;
    }
    Some(if FALLBACK_SCRIPTS.contains(&script) {
        FALLBACK_FONT
    } else {
        LAST_FONT
    })
}

/// `html` with every character in `chars` set in `LAST_FONT`.
///
/// Past its fallback, MuPDF's WebAssembly build has no font to try, so a
/// character that neither the page's font nor Noto Sans CJK SC has comes
/// out as a box where KOReader draws it with FreeSerif. The preview finds
/// such characters after a layout and lays the page out again with them
/// in FreeSerif, which only they use.
pub fn with_last_font(html: &str, chars: &str) -> String {
    wrap_characters(
        html,
        |c| chars.contains(c),
        &format!("<span style=\"font-family:'{LAST_FONT_FAMILY}'\">"),
        "</span>",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::tests::build;

    fn sample() -> Dictionary {
        build(
            &[
                (1, "Run", r#"<p id="top">Run<br/>more</p>"#),
                (
                    2,
                    "run",
                    r##"<a href="bword://walk#w1">walk</a><a href="#top">up</a><img src="a%20b.png">"##,
                ),
                (3, "walk", r#"<p id="w1">walk</p>"#),
            ],
            &[("walk#w1", 3)],
            Some(".m2s{color:red}"),
            &[("a b.png", b"\x89PNG\r\n\x1a\n.")],
        )
    }

    #[test]
    fn the_document_is_koreaders() {
        let d = sample();
        let run = d.lookup_exact("Run")[0];
        let html = document(&d, run, Some("Run")).unwrap();
        assert!(html.starts_with("<html><head><style>\narticle, aside,"));
        assert!(html.contains("xmp {\n  display: block;\n}\n\n        @page {"));
        assert!(html.contains(
            "    .m2s{color:red}</style></head><body>\n<p id=\"top\">Run&nbsp;<div></div>more</p>"
        ));
        assert!(
            html.ends_with("&nbsp;<div></div>_______&nbsp;<div></div>(query : Run)</body></html>")
        );
        assert!(!document(&d, run, None).unwrap().contains("(query"));
        let other = document(&d, d.lookup_exact("run")[0], None).unwrap();
        assert!(
            other.contains("<img src=\"data:image/png;base64,"),
            "{other}"
        );
    }

    #[test]
    fn br_is_rewritten_like_the_lua_pattern() {
        assert_eq!(
            rewrite_br("<br><br/><br /><br ><BR><br class=x><brr>"),
            "&nbsp;<div></div>".repeat(4) + "<BR><br class=x><brr>"
        );
    }

    #[test]
    fn lookups_add_the_lowercase_form() {
        let d = sample();
        let Outcome::View(view) = search(&d, "Run").unwrap() else {
            panic!()
        };
        let words: Vec<_> = view.results.iter().map(|m| m.headword.as_str()).collect();
        assert_eq!(words, ["Run", "run"]);
        assert!(view.documents.is_empty());
        let result = |n: usize| result_document(&d, &view.query, view.results[n].entry, n).unwrap();
        assert!(result(0).contains("(query : Run)"));
        assert!(!result(1).contains("(query"));
        assert!(matches!(search(&d, "RUN").unwrap(), Outcome::View(v) if v.results.len() == 1));
        assert!(matches!(
            search(&d, "fly").unwrap(),
            Outcome::NotFound { .. }
        ));
    }

    #[test]
    fn links_follow_koreaders_rules() {
        let d = sample();
        let run = d.lookup_exact("run")[0];
        let Outcome::View(view) = follow(&d, "bword://walk#w1", run).unwrap() else {
            panic!()
        };
        assert_eq!(view.results[0].headword, "walk");
        assert_eq!(view.scroll_to, None, "KOReader starts at the top");
        let upper = d.lookup_exact("Run")[0];
        assert_eq!(
            follow(&d, "bword://Run#top", upper).unwrap(),
            Outcome::Stay {
                reason: Stay::AnchorInEntry
            }
        );
        assert_eq!(
            follow(&d, "https://example.com", run).unwrap(),
            Outcome::Stay {
                reason: Stay::ExternalLink
            }
        );
        assert!(matches!(follow(&d, "walk", run).unwrap(), Outcome::View(_)));
        assert!(
            matches!(
                follow(&d, "bword://walk%231", run).unwrap(),
                Outcome::NotFound { .. }
            ),
            "not URL-decoded"
        );
    }

    #[test]
    fn geometry_follows_koreaders_formulas() {
        // 1072×1448: scale 1072/600; scaleBySize(80) = ceil(142.93) = 143,
        // (1.5) = 3, (10) = 18, (6) = 11, (12) = 22, (5) = 9, (20) = 36.
        // width: 1072 - 143 - 2*3 - 2*18 - (11 + 22) = 854
        // height: floor((1448 - 18) * 0.35) = 500; line round(46.8) = 47;
        // round(500 / 47) = 11 lines = 517.
        assert_eq!(
            Geometry::for_screen(1072, 1448, 20),
            Geometry {
                width: 854,
                height: 517,
                em: 36
            }
        );
        assert_eq!(
            Geometry::for_screen(1448, 1072, 20).width,
            1448 - 143 - 6 - 36 - 33
        );
    }

    /// The requests below are the ones MuPDF 1.27.0's WebAssembly build
    /// makes, recorded in Node.
    #[test]
    fn fonts_are_koreaders() {
        assert_eq!(
            font_for("Noto Sans", "undefined", false, false),
            Some("NotoSans-Regular.ttf")
        );
        assert_eq!(
            font_for("Noto Sans", "undefined", true, true),
            Some("NotoSans-BoldItalic.ttf")
        );
        assert_eq!(
            font_for("Noto Sans", "undefined", true, false),
            Some("NotoSans-Bold.ttf")
        );
        assert_eq!(font_for("noto sans", "undefined", false, false), None);
        assert_eq!(
            font_for(LAST_FONT_FAMILY, "undefined", true, false),
            Some(LAST_FONT)
        );
        assert_eq!(font_for("serif", "undefined", false, false), None);
        for script in ["TC", "JP", "KR", "Latin", "Common"] {
            assert_eq!(
                font_for("undefined", script, false, false),
                Some(FALLBACK_FONT)
            );
        }
        for script in ["Arabic", "Thai", "Hebrew", "Devanagari"] {
            assert_eq!(font_for("undefined", script, true, false), Some(LAST_FONT));
        }
    }

    #[test]
    fn characters_can_be_set_in_the_last_font() {
        assert_eq!(
            with_last_font("<p>\u{25b8} run \u{25c6}</p>", "\u{25b8}"),
            "<p><span style=\"font-family:'mobi2star last resort'\">\u{25b8}</span> run \u{25c6}</p>"
        );
    }
}
