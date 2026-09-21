//! Support for the code `plum-macro` generates. Not a public API.

use std::fmt::Display;

use js_sys::Error;
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::JsValue;

pub use any_spawner;
pub use js_sys;

/// Converts an argument that arrived from JS. A value of the wrong shape
/// becomes a JS exception that names the action and the parameter.
pub fn arg<T: DeserializeOwned>(action: &str, param: &str, value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| Error::new(&format!("{action}: bad argument `{param}`: {e}")).into())
}

/// Converts a return value for JS: `None` is `null`, maps are plain objects,
/// which is how watched values arrive too.
pub fn ret<T: Serialize + ?Sized>(action: &str, value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| Error::new(&format!("{action}: cannot return value: {e}")).into())
}

/// An `Err` returned by an action, as a JS `Error` to throw.
pub fn err(e: impl Display) -> JsValue {
    Error::new(&e.to_string()).into()
}
