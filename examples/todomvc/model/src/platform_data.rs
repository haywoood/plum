//! Platform data: a service that keeps named, reactive values for whoever
//! is on the page.
//!
//! The data is ad hoc. Teams create the stores they need and put what they
//! like in them, so this layer neither knows nor checks what a store holds:
//! a value is JSON, and the service keeps it, hands it out and tells
//! subscribers when it changes. What a value means is the business of
//! whoever reads it.
//!
//! A store is reached by name, from JS and from Rust. It can be subscribed
//! to before it exists, and it outlives its listeners.

use std::collections::HashMap;

use leptos::prelude::*;
use plum_macro::{plum_actions, PlumModel};
use serde_json::Value;

struct Slot {
    // Reference counted rather than owned by a reactive scope, so the value
    // is still there when the last listener has gone.
    value: ArcRwSignal<Value>,
    /// A slot appears as soon as anyone asks for it; it is created once.
    created: bool,
}

#[derive(PlumModel, Clone, Copy)]
pub struct PlatformData {
    slots: StoredValue<HashMap<String, Slot>>,
}

#[plum_actions]
impl PlatformData {
    /// The platform data of this thread. The first call creates it and
    /// provides it as context; every later call, from any model, finds it.
    pub fn new() -> Self {
        crate::runtime::ensure_init();
        use_context::<Self>().unwrap_or_else(|| {
            let data = Self {
                slots: StoredValue::new(HashMap::new()),
            };
            provide_context(data);
            data
        })
    }

    fn slot(&self, key: &str) -> ArcRwSignal<Value> {
        if let Some(value) = self
            .slots
            .with_value(|slots| slots.get(key).map(|slot| slot.value.clone()))
        {
            return value;
        }
        let value = ArcRwSignal::new(Value::Null);
        self.slots.update_value(|slots| {
            slots.insert(
                key.to_string(),
                Slot {
                    value: value.clone(),
                    created: false,
                },
            );
        });
        value
    }

    /// Creates a store. The name has to be free: by convention it starts
    /// with something unique to the team that owns it.
    pub fn create_store(&self, key: &str, initial: Value) -> Result<(), String> {
        let value = self.slot(key);
        let fresh = self.slots.try_update_value(|slots| {
            let slot = slots.get_mut(key).expect("slot() made it");
            !std::mem::replace(&mut slot.created, true)
        });
        if fresh != Some(true) {
            return Err(format!("a store named {key:?} already exists"));
        }
        value.set(initial);
        Ok(())
    }

    /// The store with this name. Until it is created it holds `null`.
    #[plum(watch, set = "set")]
    pub fn get_store(&self, key: &str) -> ArcRwSignal<Value> {
        self.slot(key)
    }

    /// Writes a store.
    pub fn set(&self, key: &str, value: Value) {
        self.slot(key).set(value);
    }
}

impl Default for PlatformData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_model_finds_the_same_platform_data() {
        PlatformData::new()
            .create_store("cart", json!({ "items": 0 }))
            .unwrap();
        let elsewhere = PlatformData::new();
        assert_eq!(elsewhere.get_store("cart").get(), json!({ "items": 0 }));
    }

    #[test]
    fn a_store_can_be_watched_before_it_is_created() {
        let data = PlatformData::new();
        let early = data.get_store("late");
        assert_eq!(early.get(), Value::Null);
        data.create_store("late", json!(1)).unwrap();
        assert_eq!(early.get(), json!(1));
        assert!(data.create_store("late", json!(2)).is_err());
    }

    #[test]
    fn a_store_holds_whatever_it_is_given() {
        let data = PlatformData::new();
        data.create_store("anything", json!([1, 2])).unwrap();
        data.set("anything", json!("now a string"));
        data.set("anything", json!({ "now": { "an": "object" } }));
        assert_eq!(
            data.get_store("anything").get(),
            json!({ "now": { "an": "object" } })
        );
    }
}
