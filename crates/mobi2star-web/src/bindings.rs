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

/// Converts a MOBI dictionary. `progress` is called with each stage as
/// JSON, e.g. `{"stage":"rendering","done":10,"total":200}`. Throws an
/// Error whose message is `CODE: detail`.
#[wasm_bindgen]
pub fn convert(source: Vec<u8>, choices: &str, progress: &Function) -> Result<Converted, JsError> {
    let mut report = |stage: mobi2star::Stage| {
        if let Ok(json) = serde_json::to_string(&stage) {
            // A failing progress callback must not stop the conversion.
            let _ = progress.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
    };
    crate::convert(source, choices, &mut report)
        .map(Converted)
        .map_err(|failure| JsError::new(&format!("{}: {}", failure.code, failure.message)))
}
