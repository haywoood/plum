//! Effect-driven bridge from view-agnostic Leptos state to JavaScript.
//!
//! JS registers a callback for a reactive source; the bridge creates one
//! Leptos `Effect` per subscription. Because the effect *reads* the source,
//! Leptos tracks the dependency and re-runs the effect whenever the value
//! changes, pushing the new value to the JS callback synchronously. The
//! first push also happens synchronously inside [`Bridge::watch`], so JS
//! consumers never observe a stale value before the first effect tick.
//!
//! On native targets the observer call is a no-op, so the crate still
//! compiles for `cargo test`/`cargo check`.

use std::cell::Cell;
use std::collections::HashMap;

use js_sys::Function;
use reactive_graph::effect::Effect;
use reactive_graph::owner::LocalStorage;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

/// The value types that can cross the bridge to JS.
///
/// Conversion to `JsValue` happens in [`Notifier::call`] on the wasm target
/// only, so the same code compiles natively (as a no-op).
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    /// `None` (maps to JS `null`, matching the generated `T | null` types).
    Null,
    Bool(bool),
    I32(i32),
    /// Floating-point numbers (JSON numbers that are not integers in range).
    F64(f64),
    Str(String),
    /// Arrays of payloads (for collections).
    Vec(Vec<Payload>),
    /// Objects as key/value payload lists.
    Obj(Vec<(String, Payload)>),
}

impl From<i32> for Payload {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}
impl From<bool> for Payload {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<String> for Payload {
    fn from(v: String) -> Self {
        Self::Str(v)
    }
}
impl From<Option<i32>> for Payload {
    fn from(v: Option<i32>) -> Self {
        match v {
            Some(v) => Self::I32(v),
            None => Self::Null,
        }
    }
}
impl From<Option<String>> for Payload {
    fn from(v: Option<String>) -> Self {
        match v {
            Some(v) => Self::Str(v),
            None => Self::Null,
        }
    }
}
impl<T: Into<Payload>> From<Vec<T>> for Payload {
    fn from(v: Vec<T>) -> Self {
        Self::Vec(v.into_iter().map(Into::into).collect())
    }
}

impl From<serde_json::Value> for Payload {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                        Self::I32(i as i32)
                    } else {
                        Self::F64(i as f64)
                    }
                } else {
                    Self::F64(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            serde_json::Value::String(s) => Self::Str(s),
            serde_json::Value::Array(arr) => Self::Vec(arr.into_iter().map(Self::from).collect()),
            serde_json::Value::Object(map) => Self::Obj(
                map.into_iter()
                    .map(|(k, val)| (k, Self::from(val)))
                    .collect(),
            ),
        }
    }
}

/// A synchronous JS observer for one bridged value.
///
/// Holds the JS callback directly (`js_sys::Function` is `Clone` +
/// `Send` + `Sync`), so no `JsValue` round-trip is needed. Cheaply `Clone`
/// so observers can be captured into effects and async tasks.
#[derive(Clone)]
pub struct Notifier(
    // Only read on wasm32; the native build never invokes the observer.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    std::sync::Arc<std::sync::RwLock<Option<Function>>>,
);

impl Notifier {
    /// Creates an observer with no JS function registered yet.
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::RwLock::new(None)))
    }

    /// Registers (or replaces) the JS observer.
    pub fn set(&self, f: Function) {
        *self.0.write().expect("observer lock poisoned") = Some(f);
    }

    /// Pushes a payload to the JS observer, if one is registered.
    #[cfg(target_arch = "wasm32")]
    pub fn call(&self, payload: &Payload) {
        let value = to_js(payload);
        if let Some(f) = self.0.read().expect("observer lock poisoned").clone() {
            let _ = f.call1(&JsValue::UNDEFINED, &value);
        }
    }

    /// No-op on native targets (tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn call(&self, _payload: &Payload) {}
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Recursively converts a [`Payload`] into a JS value (wasm only).
#[cfg(target_arch = "wasm32")]
fn to_js(payload: &Payload) -> JsValue {
    match payload {
        Payload::Null => JsValue::NULL,
        Payload::Bool(b) => JsValue::from(*b),
        Payload::I32(v) => JsValue::from(*v),
        Payload::F64(v) => JsValue::from(*v),
        Payload::Str(s) => JsValue::from_str(s),
        Payload::Vec(items) => {
            let arr = js_sys::Array::new();
            for item in items {
                arr.push(&to_js(item));
            }
            JsValue::from(arr)
        }
        Payload::Obj(pairs) => {
            let obj = js_sys::Object::new();
            for (key, value) in pairs {
                let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(key), &to_js(value));
            }
            JsValue::from(obj)
        }
    }
}

/// The bridge: a registry of active JS subscriptions.
///
/// Each subscription is backed by one Leptos `Effect` that reads the source
/// and pushes to a private [`Notifier`]. Single-threaded (wasm), so a plain
/// `RefCell` registry is enough.
pub struct Bridge {
    subs: std::cell::RefCell<HashMap<u32, Effect<LocalStorage>>>,
    next: Cell<u32>,
}

impl Bridge {
    pub fn new() -> Self {
        Self {
            subs: std::cell::RefCell::new(HashMap::new()),
            next: Cell::new(1),
        }
    }

    /// Subscribes `cb` to a reactive source.
    ///
    /// `read` is re-invoked whenever the value it reads changes (Leptos
    /// tracks the reads inside the effect). `cb` is invoked **synchronously**
    /// with the current payload first (so JS is correct immediately), and
    /// again (synchronously) on every subsequent change. Returns a
    /// subscription id; call [`Bridge::unwatch`] to stop.
    pub fn watch<F>(&self, read: F, cb: Function) -> u32
    where
        F: Fn() -> Payload + 'static,
    {
        let notifier = Notifier::new();
        notifier.set(cb);
        // Synchronous first push so the JS store is correct immediately.
        notifier.call(&read());
        // Effect-driven updates for all later changes.
        let effect = Effect::new(move |_prev: Option<Payload>| {
            let value = read();
            notifier.call(&value);
            value
        });
        let id = self.next.get();
        self.next.set(id + 1);
        self.subs.borrow_mut().insert(id, effect);
        id
    }

    /// Subscribes `cb` to a reactive source of any `Serialize` value.
    ///
    /// Each read is converted through `serde_json` into a [`Payload`] (JSON
    /// scalars, arrays, and objects map onto the payload variants). Semantics
    /// are identical to [`Bridge::watch`]: synchronous first push, then an
    /// Effect-driven push on every change.
    ///
    /// Serialization failures (e.g. non-finite floats, non-string map keys)
    /// push `Null`; they are unreachable for plain model data.
    pub fn watch_json<T, F>(&self, read: F, cb: Function) -> u32
    where
        T: serde::Serialize + 'static,
        F: Fn() -> T + 'static,
    {
        self.watch(
            move || {
                serde_json::to_value(read())
                    .unwrap_or(serde_json::Value::Null)
                    .into()
            },
            cb,
        )
    }

    /// Stops the subscription with the given id (no further pushes).
    pub fn unwatch(&self, id: u32) {
        if let Some(effect) = self.subs.borrow_mut().remove(&id) {
            effect.stop();
        }
    }

    /// Stops every active subscription.
    pub fn unwatch_all(&self) {
        let ids = self.subs.borrow().keys().copied().collect::<Vec<u32>>();
        for id in ids {
            self.unwatch(id);
        }
    }
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reactive_graph::signal::RwSignal;
    use reactive_graph::traits::{Get, Set};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Once;

    static INIT: Once = Once::new();
    thread_local! {
        static OWNER_SET: Cell<bool> = const { Cell::new(false) };
    }

    /// Native test bootstrap: global executor + thread-local root owner.
    fn ensure_init() {
        INIT.call_once(|| {
            let _ = any_spawner::Executor::init_futures_executor();
        });
        OWNER_SET.with(|set| {
            if !set.get() {
                let owner = reactive_graph::owner::Owner::new();
                owner.set();
                set.set(true);
            }
        });
    }

    fn pump() {
        any_spawner::Executor::poll_local();
    }

    /// `Effect::stop` before the first tick must prevent the first run.
    #[test]
    fn stop_before_first_run_prevents_it() {
        ensure_init();
        let sig = RwSignal::new(0);
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        let eff = Effect::new(move |_prev: Option<i32>| {
            h.set(h.get() + 1);
            sig.get()
        });
        eff.stop();
        sig.set(1);
        pump();
        assert_eq!(hits.get(), 0, "stopped effect must never run");
    }

    /// `Effect::stop` after the first tick must stop re-runs on change.
    #[test]
    fn stop_after_first_run_prevents_reruns() {
        ensure_init();
        let sig = RwSignal::new(0);
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        let eff = Effect::new(move |_prev: Option<i32>| {
            h.set(h.get() + 1);
            sig.get()
        });
        pump();
        assert_eq!(hits.get(), 1, "first run happens on the next tick");
        eff.stop();
        sig.set(1);
        pump();
        assert_eq!(hits.get(), 1, "stopped effect must not re-run on change");
    }

    #[test]
    fn from_json_scalar_mappings() {
        use serde_json::json;
        assert_eq!(Payload::from(json!(null)), Payload::Null);
        assert_eq!(Payload::from(json!(true)), Payload::Bool(true));
        assert_eq!(Payload::from(json!(false)), Payload::Bool(false));
        assert_eq!(Payload::from(json!(42)), Payload::I32(42));
        assert_eq!(Payload::from(json!(-7)), Payload::I32(-7));
        assert_eq!(Payload::from(json!(3.5)), Payload::F64(3.5));
        assert_eq!(Payload::from(json!(2.0)), Payload::F64(2.0));
        assert_eq!(Payload::from(json!("hi")), Payload::Str("hi".to_string()));
    }

    #[test]
    fn from_json_large_integer_becomes_f64() {
        use serde_json::json;
        // 7_000_000_000 exceeds i32::MAX (2_147_483_647)
        let p: Payload = Payload::from(json!(7_000_000_000i64));
        assert_eq!(p, Payload::F64(7_000_000_000.0));
    }

    #[test]
    fn from_json_i32_boundaries() {
        use serde_json::json;
        assert_eq!(
            Payload::from(json!(2_147_483_647i64)),
            Payload::I32(i32::MAX)
        );
        assert_eq!(
            Payload::from(json!(-2_147_483_648i64)),
            Payload::I32(i32::MIN)
        );
        // Just above i32::MAX
        assert_eq!(
            Payload::from(json!(2_147_483_648i64)),
            Payload::F64(2_147_483_648.0)
        );
    }

    #[test]
    fn from_json_nested_array() {
        use serde_json::json;
        let p: Payload = Payload::from(json!([1, "two", [true, null]]));
        assert_eq!(
            p,
            Payload::Vec(vec![
                Payload::I32(1),
                Payload::Str("two".to_string()),
                Payload::Vec(vec![Payload::Bool(true), Payload::Null]),
            ])
        );
    }

    #[test]
    fn from_json_object() {
        use serde_json::json;
        let p: Payload = Payload::from(json!({"id": 1, "done": true}));
        let Payload::Obj(pairs) = &p else {
            panic!("expected Obj, got {p:?}")
        };
        // BTreeMap-backed JSON objects iterate in sorted key order; the
        // pair order is not semantically meaningful, so check membership.
        assert_eq!(pairs.len(), 2);
        assert!(pairs.contains(&("id".to_string(), Payload::I32(1))));
        assert!(pairs.contains(&("done".to_string(), Payload::Bool(true))));
    }

    #[test]
    fn from_json_empty_collections() {
        use serde_json::json;
        assert_eq!(Payload::from(json!([])), Payload::Vec(Vec::new()));
        assert_eq!(Payload::from(json!({})), Payload::Obj(Vec::new()));
    }
}
