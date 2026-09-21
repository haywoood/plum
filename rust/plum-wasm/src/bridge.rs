//! The bridge from view-agnostic Leptos state to JavaScript.
//!
//! JS registers a callback for a reactive source; the bridge creates one
//! Leptos `Effect` per subscription. The effect reads the source, so Leptos
//! tracks the dependency and re-runs it when the value changes.
//!
//! Leptos schedules effects on a later tick. JS should not have to wait for
//! that after calling into Rust, so the generated action wrappers call
//! [`Bridge::flush`] before they return: every subscription whose sources
//! changed is delivered right away. Changes that do not come from a JS call
//! (an async load finishing, an autosave) arrive through the effect as usual.
//! Each subscription remembers the last payload it delivered and skips
//! repeats, so the two paths never deliver the same value twice.
//!
//! On native targets the JS callback is never invoked, so the crate still
//! compiles for `cargo test`/`cargo check`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::Function;
use reactive_graph::effect::Effect;
use reactive_graph::graph::{untrack, ReactiveNode, Subscriber, ToAnySubscriber, WithObserver};
use reactive_graph::owner::LocalStorage;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

/// The value types that can cross the bridge to JS.
///
/// Conversion to `JsValue` happens on the wasm target only, so the same code
/// compiles natively.
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

/// Calls the JS callback with a payload (wasm only; a no-op natively).
#[cfg(target_arch = "wasm32")]
fn push(cb: &Function, payload: &Payload) {
    let _ = cb.call1(&JsValue::UNDEFINED, &to_js(payload));
}

#[cfg(not(target_arch = "wasm32"))]
fn push(_cb: &Function, _payload: &Payload) {}

struct Subscription {
    effect: Effect<LocalStorage>,
    /// Reads the source and hands the payload to the sink if it changed.
    deliver: Rc<dyn Fn()>,
}

/// The bridge: a registry of active subscriptions, one Leptos `Effect` each.
pub struct Bridge {
    subs: RefCell<HashMap<u32, Subscription>>,
    next: Cell<u32>,
}

impl Bridge {
    pub fn new() -> Self {
        Self {
            subs: RefCell::new(HashMap::new()),
            next: Cell::new(1),
        }
    }

    /// Subscribes the JS function `cb` to a reactive source.
    ///
    /// `cb` is called with the current payload before this returns, and again
    /// whenever a value that `read` reads changes. Returns a subscription id;
    /// call [`Bridge::unwatch`] to stop.
    pub fn watch<F>(&self, read: F, cb: Function) -> u32
    where
        F: Fn() -> Payload + 'static,
    {
        self.watch_with(read, move |payload| push(&cb, payload))
    }

    /// [`Bridge::watch`] with a Rust sink instead of a JS function.
    pub fn watch_with<F, S>(&self, read: F, sink: S) -> u32
    where
        F: Fn() -> Payload + 'static,
        S: Fn(&Payload) + 'static,
    {
        let last = RefCell::new(None::<Payload>);
        let deliver: Rc<dyn Fn()> = Rc::new(move || {
            let value = read();
            if last.borrow().as_ref() != Some(&value) {
                // Recorded before the sink runs, in case it calls back in.
                *last.borrow_mut() = Some(value.clone());
                sink(&value);
            }
        });
        untrack(|| deliver());
        let effect = Effect::new({
            let deliver = Rc::clone(&deliver);
            move |_: Option<()>| deliver()
        });
        let id = self.next.get();
        self.next.set(id + 1);
        self.subs
            .borrow_mut()
            .insert(id, Subscription { effect, deliver });
        id
    }

    /// Delivers every pending change now instead of on the next tick.
    ///
    /// This asks each effect the question its own run loop asks
    /// (`update_if_necessary`) and re-runs it on the spot. The scheduled run
    /// then finds nothing left to do.
    pub fn flush(&self) {
        // Collected first: a sink may call `watch`/`unwatch` while we iterate.
        let pending: Vec<_> = self
            .subs
            .borrow()
            .values()
            .map(|sub| (sub.effect.to_any_subscriber(), Rc::clone(&sub.deliver)))
            .collect();
        for (subscriber, deliver) in pending {
            if subscriber.with_observer(|| subscriber.update_if_necessary()) {
                subscriber.clear_sources(&subscriber);
                subscriber.with_observer(|| deliver());
            }
        }
    }

    /// Subscribes `cb` to a reactive source of any `Serialize` value.
    ///
    /// Each read is converted through `serde_json` into a [`Payload`] (JSON
    /// scalars, arrays, and objects map onto the payload variants). Semantics
    /// are identical to [`Bridge::watch`].
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
        if let Some(sub) = self.subs.borrow_mut().remove(&id) {
            sub.effect.stop();
        }
    }

    /// Stops every active subscription.
    pub fn unwatch_all(&self) {
        let subs: Vec<_> = self.subs.borrow_mut().drain().collect();
        for (_, sub) in subs {
            sub.effect.stop();
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
    use reactive_graph::computed::Memo;
    use reactive_graph::signal::RwSignal;
    use reactive_graph::traits::{Get, Set};

    /// A bridge, a signal, a memo over it, and the payloads each one delivered.
    #[allow(clippy::type_complexity)]
    fn setup() -> (
        Bridge,
        RwSignal<i32>,
        Rc<RefCell<Vec<Payload>>>,
        Rc<RefCell<Vec<Payload>>>,
        u32,
    ) {
        crate::runtime::init();
        let bridge = Bridge::new();
        let count = RwSignal::new(1);
        let double = Memo::new(move |_| count.get() * 2);
        let counts = Rc::new(RefCell::new(Vec::new()));
        let doubles = Rc::new(RefCell::new(Vec::new()));
        let id = bridge.watch_with(move || count.get().into(), {
            let seen = Rc::clone(&counts);
            move |p| seen.borrow_mut().push(p.clone())
        });
        bridge.watch_with(move || double.get().into(), {
            let seen = Rc::clone(&doubles);
            move |p| seen.borrow_mut().push(p.clone())
        });
        (bridge, count, counts, doubles, id)
    }

    fn tick() {
        any_spawner::Executor::poll_local();
    }

    #[test]
    fn first_value_is_delivered_by_watch_itself() {
        let (_bridge, _count, counts, doubles, _) = setup();
        assert_eq!(*counts.borrow(), [Payload::I32(1)]);
        assert_eq!(*doubles.borrow(), [Payload::I32(2)]);
    }

    #[test]
    fn flush_delivers_signals_and_memos_without_a_tick() {
        let (bridge, count, counts, doubles, _) = setup();
        count.set(5);
        bridge.flush();
        assert_eq!(*counts.borrow(), [Payload::I32(1), Payload::I32(5)]);
        assert_eq!(*doubles.borrow(), [Payload::I32(2), Payload::I32(10)]);
    }

    #[test]
    fn nothing_is_delivered_twice() {
        let (bridge, count, counts, doubles, _) = setup();
        tick(); // the effects' own first run
        count.set(5);
        bridge.flush();
        tick(); // the scheduled re-run that flush got ahead of
        bridge.flush();
        assert_eq!(*counts.borrow(), [Payload::I32(1), Payload::I32(5)]);
        assert_eq!(*doubles.borrow(), [Payload::I32(2), Payload::I32(10)]);
    }

    #[test]
    fn changes_arrive_on_the_next_tick_without_flush() {
        let (_bridge, count, counts, _doubles, _) = setup();
        tick();
        count.set(7);
        assert_eq!(counts.borrow().len(), 1);
        tick();
        assert_eq!(*counts.borrow(), [Payload::I32(1), Payload::I32(7)]);
    }

    #[test]
    fn unwatch_stops_delivery() {
        let (bridge, count, counts, doubles, id) = setup();
        bridge.unwatch(id);
        count.set(9);
        bridge.flush();
        tick();
        assert_eq!(*counts.borrow(), [Payload::I32(1)]);
        assert_eq!(doubles.borrow().last(), Some(&Payload::I32(18)));
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
