//! The host I/O seam implementation for the `Todos` model.
//!
//! The core never touches the DOM or host APIs — this module is where the
//! "host" is implemented per target:
//!
//! - **wasm**: real `localStorage`, reached dynamically through
//!   `plum_wasm::js` helpers (no web-sys). `save` includes a short settle
//!   delay so the `saving` state is observable in the UI.
//! - **native**: a stub that errors (core tests use their own fakes).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{Storage, Todo};

#[cfg(all(target_arch = "wasm32", feature = "plum"))]
mod wasm_impl {
    use super::*;
    use js_sys::Reflect;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    use plum_wasm::js::{self, JsFn};

    /// Simulated round-trip latency for saves so the UI's `saving` state is
    /// visible.
    const SAVE_SETTLE_MS: u32 = 120;

    async fn delay_ms(ms: u32) -> Result<(), String> {
        let make = js_sys::Function::new_no_args(&format!(
            "return new Promise(function (resolve) {{ setTimeout(resolve, {ms}); }});"
        ));
        let promise: js_sys::Promise = make
            .call0(&JsValue::UNDEFINED)
            .map_err(|e| js::js_err(&e))?
            .unchecked_into();
        JsFuture::from(promise).await.map_err(|e| js::js_err(&e))?;
        Ok(())
    }

    pub fn local_storage(key: &str) -> Storage {
        let (load_key, save_key) = (key.to_string(), key.to_string());
        Storage {
            load: Arc::new(move || {
                let key = load_key.clone();
                Box::pin(async move {
                    let global = js::global();
                    let ls = Reflect::get(&global, &JsValue::from_str("localStorage"))
                        .map_err(|e| js::js_err(&e))?;
                    let get: JsFn = plum_wasm::js::get_fn(&ls, "getItem")?;
                    let raw = get
                        .call1(&ls, &JsValue::from_str(&key))
                        .map_err(|e| js::js_err(&e))?;
                    let raw = raw.as_string();
                    match raw {
                        None => Ok(Vec::new()),
                        Some(s) if s.is_empty() => Ok(Vec::new()),
                        Some(s) => {
                            let parsed = js_sys::JSON::parse(&s).map_err(|e| js::js_err(&e))?;
                            serde_wasm_bindgen::from_value(parsed).map_err(|e| e.to_string())
                        }
                    }
                }) as Pin<Box<dyn Future<Output = Result<Vec<Todo>, String>>>>
            }),
            save: Arc::new(move |todos: Vec<Todo>| {
                let key = save_key.clone();
                Box::pin(async move {
                    let value = serde_wasm_bindgen::to_value(&todos).map_err(|e| e.to_string())?;
                    let json = js_sys::JSON::stringify(&value).map_err(|e| js::js_err(&e))?;
                    delay_ms(SAVE_SETTLE_MS).await?;
                    let global = js::global();
                    let ls = Reflect::get(&global, &JsValue::from_str("localStorage"))
                        .map_err(|e| js::js_err(&e))?;
                    let set: JsFn = plum_wasm::js::get_fn(&ls, "setItem")?;
                    set.call2(&ls, &JsValue::from_str(&key), &JsValue::from(json))
                        .map_err(|e| js::js_err(&e))?;
                    Ok(())
                }) as Pin<Box<dyn Future<Output = Result<(), String>>>>
            }),
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", feature = "plum")))]
mod native_impl {
    use super::*;

    pub fn local_storage(_key: &str) -> Storage {
        Storage {
            load: Arc::new(move || {
                Box::pin(async move { Err("storage is unavailable in native builds".to_string()) })
                    as Pin<Box<dyn Future<Output = Result<Vec<Todo>, String>>>>
            }),
            save: Arc::new(move |_todos: Vec<Todo>| {
                Box::pin(async move { Err("storage is unavailable in native builds".to_string()) })
                    as Pin<Box<dyn Future<Output = Result<(), String>>>>
            }),
        }
    }
}

/// The storage backend for the current target.
pub fn local_storage(key: &str) -> Storage {
    #[cfg(all(target_arch = "wasm32", feature = "plum"))]
    {
        wasm_impl::local_storage(key)
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "plum")))]
    {
        native_impl::local_storage(key)
    }
}
