# Platform data: shared state for code with no Rust model

A pattern built on plum, not part of it. It is a model that keeps named JSON
values for whoever is on the page: "platform, store this and make it
accessible". Microfrontends use it like this:

```ts
platformdata.createStore("cart/state", { items: 0 });
const cart = useStore(platformdata.getStore<Cart>("cart/state"));
platformdata.getStore<Cart>("cart/state").set({ items: 1 });
```

The working version is `examples/todomvc/model/src/platform_data.rs` in the
plum repository.

## When to use it, and when not

Use it for ad hoc state that teams own and that has no Rust model: what a
microfrontend would otherwise keep in a global or in its own store, and
wants to share by name.

Do not move a Rust model's own state into it. A model's signals are in the
same wasm instance anyway, and typed; going through the generic layer costs
a JSON round trip per write and loses the types for nothing.

The service does not know or check what a store holds. Do not add schemas,
validators or typed accessors to it: the data is decided by the teams using
it, so there is nothing for this layer to know. The type parameter of
`getStore<T>` on the TypeScript side is the caller's claim.

Values are JSON on purpose. Holding JS values as they are would skip the
serialization, but then platform state could contain functions and class
instances, and would stop being something you can print, diff or persist.

## The model

Needs `serde_json = "1"` and `ts-rs` with the `serde-json-impl` feature,
which names arbitrary JSON `JsonValue` in the binding.

```rust
use std::collections::HashMap;

use leptos::prelude::*;
use plum_macro::{plum_actions, PlumModel};
use serde_json::Value;

struct Slot {
    // Reference counted, not owned by a reactive scope: the value is still
    // there when the last listener has gone.
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
    /// The first call creates it and provides it as context; every later
    /// call, from any model, finds the same one.
    pub fn new() -> Self {
        use_context::<Self>().unwrap_or_else(|| {
            let data = Self { slots: StoredValue::new(HashMap::new()) };
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
            slots.insert(key.to_string(), Slot { value: value.clone(), created: false });
        });
        value
    }

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

    /// Until it is created, the store holds `null`.
    #[plum(watch, set = "set")]
    pub fn get_store(&self, key: &str) -> ArcRwSignal<Value> {
        self.slot(key)
    }

    pub fn set(&self, key: &str, value: Value) {
        self.slot(key).set(value);
    }
}
```

Three details that are easy to get wrong:

- **"Exists" means "was created", not "was asked for".** Microfrontends load
  in no guaranteed order, so one may subscribe to a store before its owner
  creates it. `get_store` therefore gets or makes the slot, and
  `create_store` tracks creation separately. If a lookup counted as
  existence, the owner's `createStore` would fail.
- **The values are `ArcRwSignal`, not `RwSignal`.** A `#[plum(watch)]` method
  runs in a scope that is disposed when its store goes idle. A plain signal
  created there would be disposed with it and the state lost.
- **`get_store` names `set` as its writer**, so the binding gives
  `getStore(key): WritableAtom<JsonValue>` and drops `set` from the actions.

## The package entry

```ts
import type { WritableAtom } from "nanostores";
import initWasm, { createPlatformData } from "./pkg/my_model.js";
import { bindPlatformData, type JsonValue } from "./plum_gen/platform-data";

let platform: ReturnType<typeof bindPlatformData>;

export const platformdata = {
  createStore: (key: string, initial: JsonValue): void =>
    platform.actions.createStore(key, initial),
  getStore: <T extends JsonValue = JsonValue>(key: string) =>
    platform.stores.getStore(key) as WritableAtom<T>,
};

export async function init(): Promise<void> {
  await initWasm();
  platform = bindPlatformData(createPlatformData());
}
```

## One instance for everyone

"The same name gives the same store" only holds if every microfrontend uses
one instance of the package: one wasm instance and one store cache. It has
to be a shared module (an import map, or the bundler's shared/external
mechanism), not bundled into each microfrontend. Two copies means two
platforms that never see each other.

Rust models on the same page can use the service too: `PlatformData::new()`
finds it through Leptos context.
