//! Publisher-source stylesheets use the bounded grammar of the shared scoper:
//! constructs whose meaning after extraction is not established are errors.
use html_preserve::css::Grammar;
use lexicon_core::Result;
pub fn scope(css: &str, scope: &str) -> Result<String> {
    srcs_reader::markup::check_inline_style(css)?;
    html_preserve::css::scope(css, scope, Grammar::Bounded)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_scoped() {
        assert_eq!(
            scope("body { color:red } .entry,b{font-weight:bold}", "dict").unwrap(),
            ".dict{ color:red }.dict .entry,.dict b{font-weight:bold}"
        );
    }
    #[test]
    fn media_and_comments() {
        assert_eq!(
            scope("/*c*/@media screen {.a{color:red}}", "d").unwrap(),
            "@media screen{.d .a{color:red}}"
        );
    }
    #[test]
    fn resources_and_unknown_grammar_fail() {
        for s in [
            "@import 'x';",
            "p{background:url(x)}",
            "p:hover{color:red}",
            "p{font:bad",
        ] {
            assert!(scope(s, "d").is_err());
        }
    }
}
