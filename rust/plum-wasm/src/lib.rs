//! The runtime of plum, a Leptos-to-TypeScript adapter.
//!
//! `plum-macro` generates a wasm-bindgen class for a model; this crate is
//! what that generated code calls.
//!
//! - [`Bridge`] holds the subscriptions of one model: every JS `watch` is a
//!   Leptos `Effect` that hands values to a JS callback, `unwatch` stops it,
//!   and `flush` delivers pending changes before an action returns.
//! - [`Payload`] is the value that crosses, converted from anything
//!   `Serialize`.
//! - [`runtime::init`] sets up the executor and the root owner that signals
//!   and async actions need outside of a Leptos app.
//! - [`js`] (wasm only) has a few helpers for calling host JS functions
//!   without web-sys.

pub mod bridge;
pub mod runtime;

#[doc(hidden)]
#[path = "rt.rs"]
pub mod __rt;

/// Dynamic host-JS helpers (wasm target only).
#[cfg(target_arch = "wasm32")]
pub mod js;

pub use bridge::{Bridge, Payload};
pub use plum_macro::{plum_actions, PlumModel};
