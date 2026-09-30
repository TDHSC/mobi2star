//! Every `--reader` choice previewed in each reader it is for, on
//! dictionaries the converter actually produces.
use lexicon_core::{Limits, TargetReader};
use mobi2star::{Backend, OutputOptions};
use reader_view::{koreader, App, Dictionary, Outcome, View};

const SRCS: &[u8] = include_bytes!("../../../tests/fixtures/srcs.mobi");
const COMPILED: &[u8] = include_bytes!("../../../tests/fixtures/uncompressed.mobi");

fn dictionary(book: &[u8], reader: TargetReader) -> Dictionary {
    let archive = mobi2star::convert_dictionary_zip(
        book.to_vec(),
        &Limits::browser(),
        OutputOptions {
            reader,
            ..OutputOptions::default()
        },
        Backend::Auto,
        &mut |_| {},
    )
    .unwrap();
    Dictionary::from_zip(archive.zip, &Limits::browser()).unwrap()
}

fn view(outcome: Outcome) -> View {
    match outcome {
        Outcome::View(view) => view,
        other => panic!("expected a view, got {other:?}"),
    }
}

/// The class every scoped rule of the dictionary's stylesheet starts with.
fn scope_class(d: &Dictionary) -> String {
    let css = d
        .resource("dictionary.css")
        .map(|css| String::from_utf8_lossy(css).into_owned());
    let css = css
        .or_else(|| d.companion_css().map(str::to_owned))
        .unwrap();
    let start = css.find(".m2s_").unwrap();
    css[start..start + 21].to_owned()
}

#[test]
fn each_choice_styles_its_readers_in_the_preview() {
    for book in [SRCS, COMPILED] {
        for &target in TargetReader::ALL {
            let d = dictionary(book, target);
            let class = scope_class(&d);
            for &app in App::for_target(target) {
                let found = view(reader_view::search(&d, app, "run").unwrap());
                let document = match app {
                    App::Koreader => {
                        koreader::result_document(&d, "run", found.results[0].entry, 0).unwrap()
                    }
                    _ => found.documents[0].clone(),
                };
                let styled = document
                    .split("<style>")
                    .skip(1)
                    .any(|style| style[..style.find("</style>").unwrap()].contains(&class));
                assert!(styled, "{target:?} in {app:?}: no style with {class}");
            }
        }
    }
}

#[test]
fn a_koreader_output_is_bare_in_readest() {
    let d = dictionary(SRCS, TargetReader::Koreader);
    let class = scope_class(&d);
    let found = view(reader_view::search(&d, App::Readest, "run").unwrap());
    assert!(
        !found.documents[0].contains(&format!("{class} ")),
        "no stylesheet reaches Readest"
    );
}

/// Follows the first `bword://` link of `word`'s first result in `app`.
fn follow_first_link(d: &Dictionary, app: App, word: &str) -> Outcome {
    let shown = view(reader_view::search(d, app, word).unwrap());
    let entry = shown.results[0].entry;
    let payload = d.payload(entry).unwrap();
    let start = payload.find("href=\"bword://").unwrap() + 6;
    let href = &payload[start..start + payload[start..].find('"').unwrap()];
    reader_view::follow(d, app, href, entry).unwrap()
}

#[test]
fn links_resolve_in_koreader_for_both_backends() {
    for book in [SRCS, COMPILED] {
        let d = dictionary(book, TargetReader::Koreader);
        let followed = view(follow_first_link(&d, App::Koreader, "run"));
        assert_eq!(followed.scroll_to, None);
        assert!(!followed.results.is_empty());
    }
}

#[test]
fn goldendict_looks_up_then_scrolls() {
    let d = dictionary(SRCS, TargetReader::Goldendict);
    let followed = view(follow_first_link(&d, App::GoldendictNg, "run"));
    let anchor = followed.scroll_to.expect("an anchor to scroll to");
    assert!(followed.documents[0].contains(&format!("id=\"{anchor}\"")));
}

#[test]
fn readers_without_link_support_stay() {
    let d = dictionary(SRCS, TargetReader::Universal);
    for app in [App::Readest, App::GoldendictMobile, App::KoboPyglossary] {
        assert!(
            matches!(follow_first_link(&d, app, "run"), Outcome::Stay { .. }),
            "{app:?}"
        );
    }
}
