//! The macros of plum, a Leptos-to-TypeScript adapter.
//!
//! - `#[derive(PlumModel)]` on the model struct generates the wasm-bindgen
//!   class (`Wasm{Name}`) with a watch method for every `#[plum(watch)]`
//!   field, and a test that writes the TypeScript binding to `plum_gen/`.
//! - `#[plum_actions]` on the model's `impl` block exports its methods as
//!   actions and turns `new(..)` into a `create{Name}(..)` factory.
//!
//! Everything for wasm is generated behind `#[cfg(feature = "plum")]`, so
//! the crate builds without `plum-wasm` or `wasm-bindgen` when the feature
//! is off. The derive and the `impl` block have to be in the same module.

mod actions_attr;
mod derive_model;
mod naming;

use proc_macro::TokenStream;
use syn::parse_macro_input;

/// ```rust,ignore
/// #[derive(PlumModel)]
/// pub struct Todos {
///     #[plum(watch, js = "Todos")]
///     list: RwSignal<Vec<Todo>>,
///     #[plum(watch)]
///     remaining: Memo<i32>,
///     next_id: RwSignal<i32>, // not watched: never leaves Rust
/// }
/// ```
///
/// A watched field can be anything that implements Leptos' `Get`, as long
/// as its value is `Serialize` and `TS`.
#[proc_macro_derive(PlumModel, attributes(plum))]
pub fn plum_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    derive_model::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// ```rust,ignore
/// #[plum_actions]
/// impl Todos {
///     pub fn new(storage_key: &str) -> Self { ... }   // createTodos(storageKey)
///     pub fn add(&self, text: &str) -> i32 { ... }    // add(text): number
///     pub fn import(&self, todos: Vec<Todo>) { ... }  // import(todos: Array<Todo>)
///     pub async fn load(&self) -> Result<(), String> { ... }
///
///     #[plum(skip)]
///     pub fn debug_dump(&self) -> String { ... }
/// }
/// ```
///
/// Parameters must be `Deserialize` and `TS`, return values `Serialize` and
/// `TS`. A returned `Err` is thrown in JS.
#[proc_macro_attribute]
pub fn plum_actions(_attr: TokenStream, item: TokenStream) -> TokenStream {
    actions_attr::expand(item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
