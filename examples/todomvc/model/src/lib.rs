//! `todomvc-model` — the TodoMVC domain logic, written against the
//! **view-agnostic Leptos core** (signals, memos, effects — no DOM, no
//! `csr`/`ssr`/`hydrate` features).
//!
//! With the `plum` feature enabled, this crate also compiles to a wasm cdylib.
//! `index.ts` wraps that build as the `@todomvc/model` package the app uses.

mod platform_data;
pub mod runtime;
pub mod storage;
mod todos;

pub use platform_data::PlatformData;
pub use todos::*;
