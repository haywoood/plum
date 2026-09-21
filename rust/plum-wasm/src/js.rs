//! Minimal helpers for calling host JavaScript APIs dynamically
//! (localStorage, fetch, timers) without pulling in web-sys.
//!
//! js-sys's `Function` type is `no_upcast`, so a function value obtained via
//! `Reflect::get` cannot be cast into it. These helpers declare a tiny
//! external `JsFn` type whose `call` methods accept arbitrary argument
//! counts (JS ignores surplus arguments, so one `call2` also covers
//! zero/one-argument calls).

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// An opaque JS function value (e.g. obtained from `Reflect::get`).
#[wasm_bindgen]
extern "C" {
    pub type JsFn;

    #[wasm_bindgen(method, catch, js_name = call)]
    pub fn call0(this: &JsFn, context: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = call)]
    pub fn call1(this: &JsFn, context: &JsValue, arg1: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = call)]
    pub fn call2(
        this: &JsFn,
        context: &JsValue,
        arg1: &JsValue,
        arg2: &JsValue,
    ) -> Result<JsValue, JsValue>;
}

/// The global JS object, as a `JsValue` (for `Reflect::get` lookups).
pub fn global() -> JsValue {
    js_sys::global().into()
}

/// Maps a caught JS error to a plain Rust error string: the message of an
/// `Error`, a thrown string as it is, anything else in debug form.
pub fn js_err(e: &JsValue) -> String {
    match e.dyn_ref::<js_sys::Error>() {
        Some(error) => error.message().into(),
        None => e.as_string().unwrap_or_else(|| format!("{e:?}")),
    }
}

/// Convenience: look up a property on an object and treat it as a function.
pub fn get_fn(obj: &JsValue, prop: &str) -> Result<JsFn, String> {
    let f = js_sys::Reflect::get(obj, &JsValue::from_str(prop)).map_err(|e| js_err(&e))?;
    Ok(f.unchecked_into())
}
