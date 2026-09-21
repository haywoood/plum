//! plum-macro: proc-macro crate for the plum Leptos-to-TS adapter.
//!
//! Provides two macros, both applied in the **core model crate** (feature-gated):
//!
//! - `#[derive(PlumModel)]` — on the model struct. Generates a `#[wasm_bindgen]`
//!   wrapper class (`Wasm{Name}`) with watch methods, unwatch, and dispose.
//! - `#[plum_actions]` — attribute on the `impl` block. Generates a forwarding
//!   `#[wasm_bindgen] impl Wasm{Name}` with camelCase js_names.
//!
//! The core crate must have `wasm-bindgen`, `js-sys`, `plum-wasm`, and
//! `serde_json` as dependencies (behind the `plum` feature).

mod actions_attr;
mod derive_model;
mod naming;
mod ts_type;

use proc_macro::TokenStream;
use syn::parse_macro_input;

/// Derive macro: generates a `#[wasm_bindgen]` wrapper struct with watch methods.
///
/// ```rust,ignore
/// #[derive(PlumModel)]
/// pub struct Todos {
///     #[plum(watch)]
///     list: RwSignal<Vec<Todo>>,
///     // ...
/// }
/// ```
#[proc_macro_derive(PlumModel, attributes(plum))]
pub fn plum_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    derive_model::expand(&input).into()
}

/// Attribute macro: generates a `#[wasm_bindgen]` forwarding impl for ops.
///
/// ```rust,ignore
/// #[plum_actions]
/// impl Todos {
///     pub fn add(&self, text: &str) -> i32 { ... }
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn plum_actions(_attr: TokenStream, item: TokenStream) -> TokenStream {
    actions_attr::expand(item.into()).into()
}
