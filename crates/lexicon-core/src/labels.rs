//! Text that mobi2star itself adds to a bundle: lookup keys for content outside
//! the source headword index, image-gallery titles and the offline viewer UI.
//! Source text is never translated.
use serde::{Deserialize, Serialize};

/// Language of generated labels. The labels are part of the output that
/// verification regenerates byte for byte, so bundles record their choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
pub enum LabelLanguage {
    /// English
    #[default]
    En,
    /// Chinese (Simplified)
    Zh,
}

/// Fixed generated strings for one language.
#[derive(Debug)]
pub struct LabelText {
    /// Lookup key and heading of the gallery of publisher-source images.
    pub source_images: &'static str,
    /// Lookup key and heading of the gallery of compiled MOBI images.
    pub compiled_images: &'static str,
    /// Title of the viewer page that shows every image.
    pub all_images: &'static str,
    /// Value of the viewer's `<html lang>` attribute.
    pub html_lang: &'static str,
    pub viewer_tagline: &'static str,
    pub viewer_search_label: &'static str,
    pub viewer_search_button: &'static str,
    pub viewer_contents: &'static str,
    /// Search status; `{n}` is replaced with the number of matching definitions.
    pub viewer_found: &'static str,
    pub viewer_not_found: &'static str,
}

const EN: LabelText = LabelText {
    source_images: "[Source images]",
    compiled_images: "[Compiled MOBI images]",
    all_images: "All source and compiled images",
    html_lang: "en",
    viewer_tagline: "Offline dictionary · full chapters · explicit word forms",
    viewer_search_label: "Word or inflected form",
    viewer_search_button: "Look up",
    viewer_contents: "Contents",
    viewer_found: "Definitions found: {n}",
    viewer_not_found: "No exact match. Browse the contents below.",
};

const ZH: LabelText = LabelText {
    source_images: "〔原始源文件图片〕",
    compiled_images: "〔MOBI 编译图片〕",
    all_images: "全部原始图片与编译图片",
    html_lang: "zh-CN",
    viewer_tagline: "本地离线词典 · 完整章节 · 显式词形",
    viewer_search_label: "单词或词形",
    viewer_search_button: "查询",
    viewer_contents: "原书目录",
    viewer_found: "找到 {n} 个释义块",
    viewer_not_found: "未找到精确匹配，可按原书目录浏览。",
};

impl LabelLanguage {
    pub fn text(self) -> &'static LabelText {
        match self {
            Self::En => &EN,
            Self::Zh => &ZH,
        }
    }
    /// Lookup key for compiled text not covered by any indexed headword; `number` starts at 1.
    pub fn supplement_key(self, number: usize) -> String {
        match self {
            Self::En => format!("[Supplement {number:06}]"),
            Self::Zh => format!("〔原书补充内容 {number:06}〕"),
        }
    }
    /// Start of a chapter's lookup key, followed by the chapter title; `number` starts at 1.
    pub fn chapter_key_prefix(self, number: usize) -> String {
        match self {
            Self::En => format!("[Chapter {number:06}] "),
            Self::Zh => format!("〔原书章节 {number:06}〕 "),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn english_is_the_default() {
        assert_eq!(LabelLanguage::default(), LabelLanguage::En);
        assert_eq!(
            LabelLanguage::default().supplement_key(1),
            "[Supplement 000001]"
        );
        assert_eq!(
            LabelLanguage::default().chapter_key_prefix(12),
            "[Chapter 000012] "
        );
    }
    #[test]
    fn chinese_keys_are_unchanged() {
        assert_eq!(
            LabelLanguage::Zh.supplement_key(1),
            "〔原书补充内容 000001〕"
        );
        assert_eq!(
            LabelLanguage::Zh.chapter_key_prefix(1),
            "〔原书章节 000001〕 "
        );
        assert_eq!(LabelLanguage::Zh.text().source_images, "〔原始源文件图片〕");
    }
    #[test]
    fn manifest_spelling_is_stable() {
        assert_eq!(serde_json::to_string(&LabelLanguage::En).unwrap(), "\"en\"");
        assert_eq!(
            serde_json::from_str::<LabelLanguage>("\"zh\"").unwrap(),
            LabelLanguage::Zh
        );
    }
    #[test]
    fn gallery_keys_differ_within_each_language() {
        for language in [LabelLanguage::En, LabelLanguage::Zh] {
            let text = language.text();
            assert_ne!(text.source_images, text.compiled_images);
            assert!(text.viewer_found.contains("{n}"));
        }
    }
}
