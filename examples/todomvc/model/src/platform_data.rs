//! Platform data: named, reactive JSON values that outlive whoever reads
//! them. It is the state of the platform; models are logic over it.
//!
//! - A model written in Rust *defines* its stores with a type. The platform
//!   holds the value and rejects a write of any other shape, wherever it
//!   comes from.
//! - Code with no Rust model of its own, such as a microfrontend, *creates*
//!   stores from JS and keeps whatever it likes in them.
//!
//! Either way a store is reached by name, from Rust and from JS, and can be
//! subscribed to before it exists.

use std::collections::HashMap;
use std::sync::Arc;

use leptos::prelude::*;
use plum_macro::{plum_actions, PlumModel};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

type Check = Arc<dyn Fn(&Value) -> Result<(), String> + Send + Sync>;

struct Slot {
    // Reference counted rather than owned by a reactive scope, so the value
    // is still there when the last listener has gone.
    value: ArcRwSignal<Value>,
    /// A slot appears as soon as anyone asks for it; it is created once.
    created: bool,
    /// For a store defined with a Rust type: what a write has to look like.
    check: Option<Check>,
}

#[derive(PlumModel, Clone, Copy)]
pub struct PlatformData {
    slots: StoredValue<HashMap<String, Slot>>,
    /// The names of the stores that exist.
    #[plum(watch)]
    keys: RwSignal<Vec<String>>,
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
                keys: RwSignal::new(Vec::new()),
            };
            provide_context(data);
            data
        })
    }

    fn slot(&self, key: &str) -> ArcRwSignal<Value> {
        if let Some(value) = self
            .slots
            .with_value(|slots| slots.get(key).map(|s| s.value.clone()))
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
                    check: None,
                },
            );
        });
        value
    }

    fn create(&self, key: &str, initial: Value, check: Option<Check>) -> Result<(), String> {
        let value = self.slot(key);
        let fresh = self.slots.try_update_value(|slots| {
            let slot = slots.get_mut(key).expect("slot() made it");
            let fresh = !slot.created;
            slot.created = true;
            slot.check = check;
            fresh
        });
        if fresh != Some(true) {
            return Err(format!("a store named {key:?} already exists"));
        }
        value.set(initial);
        self.keys.update(|keys| keys.push(key.to_string()));
        Ok(())
    }

    /// Defines a store that holds a `T`. Writes from JS that are not a `T`
    /// are refused. For models; panics if the name is taken.
    #[plum(skip)]
    pub fn define<T>(&self, key: &str, initial: &T)
    where
        T: Serialize + DeserializeOwned + 'static,
    {
        let check: Check = Arc::new(|value| {
            serde_json::from_value::<T>(value.clone())
                .map(drop)
                .map_err(|e| e.to_string())
        });
        let initial = serde_json::to_value(initial).expect("serializable");
        self.create(key, initial, Some(check))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// A store read as a `T`, for models.
    #[plum(skip)]
    pub fn lens<T>(&self, key: &str) -> Memo<T>
    where
        T: DeserializeOwned + Default + PartialEq + Clone + Send + Sync + 'static,
    {
        let value = self.slot(key);
        Memo::new(move |_| serde_json::from_value(value.get()).unwrap_or_default())
    }

    /// Writes a store from a model, which knows its own types.
    #[plum(skip)]
    pub fn write<T: Serialize>(&self, key: &str, value: &T) {
        self.slot(key)
            .set(serde_json::to_value(value).expect("serializable"));
    }

    /// Creates a store that can hold anything.
    pub fn create_store(&self, key: &str, initial: Value) -> Result<(), String> {
        self.create(key, initial, None)
    }

    /// The store with this name. It can be subscribed to before it exists,
    /// and then holds `null`.
    #[plum(watch, set = "set")]
    pub fn get_store(&self, key: &str) -> ArcRwSignal<Value> {
        self.slot(key)
    }

    /// Writes a store. A store defined with a type only takes that type.
    pub fn set(&self, key: &str, value: Value) -> Result<(), String> {
        let check = self
            .slots
            .with_value(|slots| slots.get(key).and_then(|slot| slot.check.clone()));
        if let Some(check) = check {
            check(&value).map_err(|e| format!("{key:?} does not take that value: {e}"))?;
        }
        self.slot(key).set(value);
        Ok(())
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

    #[test]
    fn every_model_finds_the_same_platform_data() {
        let a = PlatformData::new();
        a.create_store("cart", serde_json::json!({ "items": 0 }))
            .unwrap();
        let b = PlatformData::new();
        assert_eq!(b.get_store("cart").get(), serde_json::json!({ "items": 0 }));
        assert_eq!(b.keys.get(), ["cart"]);
    }

    #[test]
    fn a_store_can_be_watched_before_it_is_created() {
        let data = PlatformData::new();
        let early = data.get_store("late");
        assert_eq!(early.get(), Value::Null);
        data.create_store("late", serde_json::json!(1)).unwrap();
        assert_eq!(early.get(), serde_json::json!(1));
        assert!(data.create_store("late", serde_json::json!(2)).is_err());
    }

    #[test]
    fn a_defined_store_only_takes_its_type() {
        let data = PlatformData::new();
        data.define::<Vec<i32>>("numbers", &vec![1, 2]);
        let numbers = data.lens::<Vec<i32>>("numbers");
        assert_eq!(numbers.get(), [1, 2]);

        assert!(data.set("numbers", serde_json::json!([3])).is_ok());
        assert_eq!(numbers.get(), [3]);
        let refused = data.set("numbers", serde_json::json!("three")).unwrap_err();
        assert!(
            refused.contains("\"numbers\" does not take that value"),
            "{refused}"
        );
        assert_eq!(numbers.get(), [3]);
    }
}
