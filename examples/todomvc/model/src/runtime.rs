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

use std::cell::Cell;
use std::sync::Once;

use leptos::prelude::*;

static EXECUTOR_INIT: Once = Once::new();

thread_local! {
    static OWNER_SET: Cell<bool> = const { Cell::new(false) };
}

/// Configures the global executor (once) and sets the root owner for the
/// current thread. Cheap to call repeatedly; call it before creating signals
/// or spawning async work.
pub fn ensure_init() {
    EXECUTOR_INIT.call_once(|| {
        #[cfg(target_arch = "wasm32")]
        let _ = any_spawner::Executor::init_wasm_bindgen();
        #[cfg(not(target_arch = "wasm32"))]
        let _ = any_spawner::Executor::init_futures_executor();
    });
    OWNER_SET.with(|set| {
        if !set.get() {
            let owner = Owner::new();
            owner.set();
            set.set(true);
        }
    });
}

/// Drives the native executor until spawned tasks are idle (cargo test /
/// examples). No-op on wasm, where the host event loop drives tasks.
#[cfg(not(target_arch = "wasm32"))]
pub fn pump() {
    any_spawner::Executor::poll_local();
}

#[cfg(target_arch = "wasm32")]
pub fn pump() {}
