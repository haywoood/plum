//! One-time runtime bootstrap for plum-managed models.
//!
//! Signals need an owner and async actions an executor; a Leptos app gets
//! both from its renderer, a headless model from here. Safe to call
//! repeatedly. Call it before creating signals or dispatching actions.

use std::cell::RefCell;
use std::sync::Once;

use reactive_graph::owner::Owner;

static EXECUTOR_INIT: Once = Once::new();

thread_local! {
    // `Owner::set` keeps only a weak reference, so the root has to be held
    // here or it is gone as soon as `init` returns, and with it everything
    // that hangs off an owner: `provide_context`, `on_cleanup`, child owners.
    static ROOT: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

/// Sets up the executor and, unless the thread already has one, a root owner.
pub fn init() {
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
