//! Headless Leptos runtime bootstrap for this logic crate.
//!
//! A Leptos *app* gets its executor and root owner from its view framework;
//! a view-agnostic crate used outside a view (headless, in tests, or inside
//! a wasm adapter) has to bootstrap them itself:
//!
//! - **wasm**: the `wasm-bindgen-futures` runtime — driven by the browser
//!   event loop, and by Node microtasks in tests;
//! - **native**: the `futures` executor — a thread-local pool that tests and
//!   examples drive with [`pump`].

use std::cell::RefCell;
use std::sync::Once;

use leptos::prelude::*;

static EXECUTOR_INIT: Once = Once::new();

thread_local! {
    // `Owner::set` keeps only a weak reference; the root is held here.
    static ROOT: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

/// Configures the global executor (once) and, unless the thread already has
/// one, a root owner. Under plum the generated factory has done this by the
/// time a model is constructed; tests and native hosts get it from here.
pub fn ensure_init() {
    EXECUTOR_INIT.call_once(|| {
        #[cfg(target_arch = "wasm32")]
        let _ = any_spawner::Executor::init_wasm_bindgen();
        #[cfg(not(target_arch = "wasm32"))]
        let _ = any_spawner::Executor::init_futures_executor();
    });
    if Owner::current().is_none() {
        let root = Owner::new();
        root.set();
        ROOT.with(|slot| *slot.borrow_mut() = Some(root));
    }
}

/// Drives the native executor until spawned tasks are idle (cargo test /
/// examples). No-op on wasm, where the host event loop drives tasks.
#[cfg(not(target_arch = "wasm32"))]
pub fn pump() {
    any_spawner::Executor::poll_local();
}

#[cfg(target_arch = "wasm32")]
pub fn pump() {}
