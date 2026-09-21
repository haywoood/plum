//! One-time runtime bootstrap for plum-managed models.
//!
//! Configures the Leptos executor and sets the root owner for the current
//! thread. Safe to call repeatedly (idempotent). Must be called before
//! creating signals or spawning async work.

use std::cell::Cell;
use std::sync::Once;

use reactive_graph::owner::Owner;

static EXECUTOR_INIT: Once = Once::new();

thread_local! {
    static OWNER_SET: Cell<bool> = const { Cell::new(false) };
}

/// Initialize the Leptos executor and root owner. Idempotent.
pub fn init() {
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
