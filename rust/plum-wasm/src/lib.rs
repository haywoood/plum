//! `plum-wasm` — the Rust half of the plum adapter.
//!
//! Plum's job: let a **view-agnostic Leptos** logic crate (plain signals,
//! memos, actions — no DOM, no view features) be consumed by TypeScript
//! frontends, with **Nanostores** as the public reactive surface.
//!
//! This crate provides the generic machinery that per-app adapter crates use:
//!
//! - [`bridge::Bridge`] — an effect-driven subscription registry: each JS
//!   `watch` becomes one Leptos `Effect` that pushes values to a JS callback;
//!   `unwatch` stops the effect. No model code lives here.
//! - [`bridge::Payload`] — the value types that cross to JS (app crates
//!   build payloads explicitly in their watch closures).
//! - [`js`] (wasm only) — minimal helpers for dynamic host I/O
//!   (localStorage, fetch, timers) without web-sys.
//!
//! The model crate (e.g. `examples/todomvc/model`) generates its own
//! `#[wasm_bindgen]` classes and call [`bridge::Bridge::watch`] for each
//! reactive value they expose.

pub mod bridge;
pub mod runtime;

/// Dynamic host-JS helpers (wasm target only).
#[cfg(target_arch = "wasm32")]
pub mod js;

pub use bridge::{Bridge, Notifier, Payload};

/// Declarative bridge generation. See the `plum-macro` crate for details.
///
/// ```rust,ignore
/// #[derive(PlumModel)]
/// pub struct Todos {
///     #[plum(watch)]
///     list: RwSignal<Vec<Todo>>,
///     // ...
/// }
///
/// #[plum_actions]
/// impl Todos {
///     pub fn add(&self, text: &str) -> i32 { ... }
///     // ...
/// }
/// ```
pub use plum_macro::{plum_actions, PlumModel};
