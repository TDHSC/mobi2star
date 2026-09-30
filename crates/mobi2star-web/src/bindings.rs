//! WebAssembly exports for the page's worker. wasm-bindgen generates unsafe
//! glue for them, so this module alone allows unsafe code.
#![allow(unsafe_code)]
use js_sys::Function;
use std::sync::Mutex;
use wasm_bindgen::prelude::*;

/// The last panic message. WebAssembly reports a panic only as an
/// "unreachable" trap, so the worker asks for the message afterwards.
static PANIC: Mutex<Option<String>> = Mutex::new(None);

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut last) = PANIC.lock() {
            *last = Some(info.to_string());
        }
    }));
}

/// The message of the panic that stopped the last call, if there was one.
/// Running out of memory aborts without a panic, so it leaves none.
#[wasm_bindgen(js_name = lastPanic)]
pub fn last_panic() -> Option<String> {
    PANIC.lock().ok().and_then(|last| last.clone())
}

#[wasm_bindgen]
pub fn version() -> String {
    crate::version().into()
}

fn thrown(failure: crate::Failure) -> JsError {
    JsError::new(&format!("{}: {}", failure.code, failure.message))
}

/// The file to convert, filled chunk by chunk from `File.stream()` so the
/// page never holds a second whole copy of it.
#[wasm_bindgen]
pub struct Source(crate::SourceBuffer);
#[wasm_bindgen]
impl Source {
    /// Reserves room for `size` bytes. Throws `LIMIT: ...` for files over
    /// the browser limit or too large for memory.
    #[wasm_bindgen(constructor)]
    pub fn new(size: usize) -> Result<Source, JsError> {
        crate::SourceBuffer::new(size).map(Source).map_err(thrown)
    }
    /// Reserves room for a converted dictionary's zip of `size` bytes, which
    /// may be as large as the browser's output limit.
    #[wasm_bindgen(js_name = forArchive)]
    pub fn for_archive(size: usize) -> Result<Source, JsError> {
        crate::SourceBuffer::for_archive(size)
            .map(Source)
            .map_err(thrown)
    }
    /// Appends the next chunk of the file.
    pub fn push(&mut self, chunk: &[u8]) -> Result<(), JsError> {
        self.0.push(chunk).map_err(thrown)
    }
}

/// The page's reader and label choices as JSON.
#[wasm_bindgen]
pub fn choices() -> String {
    crate::choices()
}

/// A finished conversion.
#[wasm_bindgen]
pub struct Converted(crate::Converted);
#[wasm_bindgen]
impl Converted {
    /// The zip archive, moved out so memory holds it only once.
    #[wasm_bindgen(js_name = takeZip)]
    pub fn take_zip(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0.zip)
    }
    #[wasm_bindgen(getter, js_name = fileName)]
    pub fn file_name(&self) -> String {
        self.0.file_name.clone()
    }
    /// JSON: version, folder, file name, zip size and the conversion report.
    #[wasm_bindgen(getter)]
    pub fn summary(&self) -> String {
        self.0.summary.clone()
    }
}

/// Converts a MOBI dictionary, consuming `source`. `progress` is called with
/// each stage as JSON, e.g. `{"stage":"rendering","done":10,"total":200}`.
/// Throws an Error whose message is `CODE: detail`.
#[wasm_bindgen]
pub fn convert(source: Source, choices: &str, progress: &Function) -> Result<Converted, JsError> {
    let mut report = |stage: mobi2star::Stage| {
        if let Ok(json) = serde_json::to_string(&stage) {
            // A failing progress callback must not stop the conversion.
            let _ = progress.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
    };
    let source = source.0.finish().map_err(thrown)?;
    crate::convert(source, choices, &mut report)
        .map(Converted)
        .map_err(thrown)
}

/// A converted dictionary opened for previewing it as a reader shows it.
/// Methods answer in JSON; see `crate::preview`.
#[wasm_bindgen]
pub struct Preview(crate::preview::Session);
#[wasm_bindgen]
impl Preview {
    /// Opens the zip in `source` (filled through `Source.forArchive`) for
    /// the readers of the `--reader` choice `reader`.
    #[wasm_bindgen(constructor)]
    pub fn new(source: Source, reader: &str) -> Result<Preview, JsError> {
        let zip = source.0.finish().map_err(thrown)?;
        crate::preview::Session::open(zip, reader)
            .map(Preview)
            .map_err(thrown)
    }
    pub fn info(&self) -> String {
        self.0.info()
    }
    pub fn search(&self, app: &str, word: &str) -> Result<String, JsError> {
        self.0.search(app, word).map_err(thrown)
    }
    pub fn follow(&self, app: &str, href: &str, current: usize) -> Result<String, JsError> {
        self.0.follow(app, href, current).map_err(thrown)
    }
    pub fn suggest(&self, prefix: &str, limit: usize) -> String {
        self.0.suggest(prefix, limit)
    }
    #[wasm_bindgen(js_name = headwordAt)]
    pub fn headword_at(&self, fraction: f64) -> Option<String> {
        self.0.headword_at(fraction)
    }
    #[wasm_bindgen(js_name = koreaderGeometry)]
    pub fn koreader_geometry(screen: &str, font_size: u32) -> Option<String> {
        crate::preview::koreader_geometry(screen, font_size)
    }
    #[wasm_bindgen(js_name = koreaderFonts)]
    pub fn koreader_fonts() -> String {
        crate::preview::koreader_fonts()
    }
    /// The KOReader font file for a MuPDF font request, if any.
    #[wasm_bindgen(js_name = koreaderFont)]
    pub fn koreader_font(family: &str, script: &str, bold: bool, italic: bool) -> Option<String> {
        reader_view::koreader::font_for(family, script, bold, italic).map(str::to_owned)
    }
    /// The font KOReader draws characters with that no other font has.
    #[wasm_bindgen(js_name = koreaderLastFont)]
    pub fn koreader_last_font() -> String {
        reader_view::koreader::LAST_FONT.into()
    }
    /// `html` with the characters in `chars` set in the last font.
    #[wasm_bindgen(js_name = koreaderWithLastFont)]
    pub fn koreader_with_last_font(html: &str, chars: &str) -> String {
        reader_view::koreader::with_last_font(html, chars)
    }
}
