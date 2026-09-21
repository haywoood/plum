---
name: plum
description: Integrate plum, the Leptos-to-TypeScript adapter, into a Rust codebase. plum turns Leptos signals and memos into Nanostores and `pub` methods into typed functions, compiled to WebAssembly, so a React, Vue, Svelte or plain TypeScript front-end can consume Rust state and logic without seeing Rust. Use this skill whenever the task involves plum, `#[derive(PlumModel)]`, `#[plum_actions]`, `plum-macro` or `plum-wasm`, or when someone wants to expose, share or reuse Leptos or Rust state and logic in a TypeScript or JavaScript front-end, ship Rust logic as an npm package of stores, move state out of Leptos components into a model, or give microfrontends shared reactive state backed by Rust, even if they never say "plum".
---

# plum

plum lets a team that keeps its logic in Leptos hand that logic to people who
work in TypeScript. The Rust team annotates a model; plum generates a
wasm-bindgen class and a TypeScript binding; the Rust team wraps those in an
npm package; the front-end team installs it and sees Nanostores and typed
functions. Rust, Leptos and WebAssembly do not appear in front-end code.

Repository and full example: https://github.com/haywoood/plum
(`examples/todomvc` is a complete integration; read it when in doubt).

## The three layers

Keep these apart. Most mistakes are one layer's knowledge leaking into
another.

1. **plum** (`plum-macro`, `plum-wasm`): generic. Knows nothing about any
   model.
2. **The model package**, owned by the Rust team: a Leptos crate with no view
   code, plus a small `index.ts` and `package.json` next to it. Everything
   about wasm loading, constructing models and startup lives here, behind one
   `init()`.
3. **The app**, owned by the front-end team: calls `init()` once, then uses
   stores and actions. It imports the package and nothing else: no wasm
   paths, no generated files.

## Workflow

Work through these in order. Steps 1 and 3 are where the effort is.

### 1. Get the state into a crate with no view code

The model crate has to build for `wasm32-unknown-unknown` without a
renderer. It depends on `leptos` with `default-features = false` and none of
`csr`, `ssr`, `hydrate`. An existing Leptos app keeps its view features in
its own crate and depends on the model crate.

If signals are created inside components today, extract them:

- Inventory every `signal`, `RwSignal::new`, `Memo::new`, `Resource`,
  `Action` and every closure that mutates one. Those are the model.
- Make one struct that owns the signals as fields, created in `new()`.
  Derived values become `Memo` fields. Every mutation becomes a `pub fn` that
  takes `&self` (signal handles are `Copy`, so `&self` is enough).
- Components shrink to reading the model and calling its methods. Provide the
  model to Leptos views through context; views in other modules may want
  accessor methods that return the signal, which plum ignores.
- Host I/O (storage, fetch) goes behind a seam the model is constructed
  with, such as a struct of async closures, so the model runs natively in
  tests with a fake. `examples/todomvc/model/src/storage.rs` shows one.

A Leptos app gets its reactive owner and executor from the renderer. A model
used without one (native tests, or under plum) needs them set up before the
first signal is created. Under plum the generated factory does this. For
native tests, add a bootstrap as a public module (`pub mod runtime;`, so
that `pump` is not dead code) and call `ensure_init()` at the top of the
constructor:

```rust
use std::cell::RefCell;
use std::sync::Once;
use leptos::prelude::*;

static EXECUTOR_INIT: Once = Once::new();
thread_local! {
    // Owner::set keeps only a weak reference; hold the root here.
    static ROOT: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

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

/// Native tests call this to let effects and dispatched actions run.
#[cfg(not(target_arch = "wasm32"))]
pub fn pump() {
    any_spawner::Executor::poll_local();
}
```

That needs `any_spawner = "0.3"`. If the model uses `Effect`, also depend on
`reactive_graph = { version = "0.2", features = ["effects"] }`: `leptos` only
enables effects through its view features, and without it effects silently
never run.

### 2. Cargo.toml of the model crate

```toml
[lib]
crate-type = ["rlib", "cdylib"]

[dependencies]
leptos = { version = "0.8", default-features = false }
serde = { version = "1", features = ["derive"] }
ts-rs = "12"
plum-macro = "0.1"

# wasm build only
plum-wasm = { version = "0.1", optional = true }
wasm-bindgen = { version = "0.2", optional = true }

[features]
plum = ["dep:plum-wasm", "dep:wasm-bindgen"]
```

The feature has to be named `plum`: everything generated for wasm is behind
`#[cfg(feature = "plum")]`, so native builds and the team's own Leptos app
get none of it. It is a feature and not a target check because that Leptos
app builds the crate for wasm32 too. `plum-macro` is not optional, because
the attributes have to resolve in every build. The two crates are
released together, so keep them on the same version.

### 3. Annotate the model

A complete model. Everything in it is used by the later steps.

```rust
pub mod runtime; // the bootstrap from step 1

use std::future::Future;
use std::pin::Pin;

use leptos::prelude::*;
use plum_macro::{plum_actions, PlumModel};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Item {
    pub id: u32,
    pub name: String,
    pub price_cents: u32,
    pub qty: u32,
}

/// Host I/O behind a seam: the real one calls the payment API, a test passes
/// a fake. The future touches JS, so it is not `Send`.
pub type Charge = Box<dyn Fn(u32) -> Pin<Box<dyn Future<Output = Result<(), String>>>>>;

#[derive(PlumModel)]
pub struct Cart {
    #[plum(watch)]
    items: RwSignal<Vec<Item>>, // store: items
    #[plum(watch, set)]
    coupon: RwSignal<String>, // writable store: coupon.set("...")
    #[plum(watch)]
    total: Memo<u32>,
    #[plum(watch)]
    is_empty: Memo<bool>, // so the view never writes items.length === 0
    #[plum(watch)]
    paying: Memo<bool>, // the action's pending()
    #[plum(watch)]
    error: Memo<Option<String>>, // a refusal, or else the action's failure
    charge: Action<u32, Result<(), String>>,
    refused: RwSignal<Option<String>>,
    next_id: RwSignal<u32>, // not watched: never leaves Rust
}

#[plum_actions]
impl Cart {
    /// Becomes `createCart(apiUrl)`.
    pub fn new(api_url: &str) -> Self {
        let _ = api_url; // build the real `Charge` from it
        Self::with_charge(Box::new(|_cents| Box::pin(async { Ok(()) })))
    }

    /// Not exported: only `new` and methods that take `&self` are.
    pub fn with_charge(charge: Charge) -> Self {
        crate::runtime::ensure_init();

        let items = RwSignal::new(Vec::<Item>::new());
        let coupon = RwSignal::new(String::new());
        let refused = RwSignal::new(None);

        let subtotal =
            Memo::new(move |_| items.get().iter().map(|i| i.price_cents * i.qty).sum::<u32>());
        let total = Memo::new(move |_| {
            let off = coupon.get().trim().eq_ignore_ascii_case("PLUM10");
            subtotal.get() - if off { subtotal.get() / 10 } else { 0 }
        });
        let is_empty = Memo::new(move |_| items.get().is_empty());

        // Async work is a Leptos Action. It applies its own result.
        let charge = Action::new_local(move |cents: &u32| {
            let charging = charge(*cents);
            async move {
                charging.await?;
                items.set(Vec::new());
                Ok(())
            }
        });
        let paying = charge.pending();
        let charged = charge.value();
        let error = Memo::new(move |_| {
            refused.get().or_else(|| charged.get().and_then(Result::err))
        });

        Self {
            items, coupon, total, is_empty, paying, error, charge, refused,
            next_id: RwSignal::new(1),
        }
    }

    pub fn add(&self, name: &str, price_cents: u32) -> u32 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.items.update(|items| {
            items.push(Item { id, name: name.to_string(), price_cents, qty: 1 })
        });
        id
    }

    pub fn change_qty(&self, id: u32, delta: i32) {
        self.items.update(|items| {
            if let Some(item) = items.iter_mut().find(|i| i.id == id) {
                item.qty = item.qty.saturating_add_signed(delta);
            }
            items.retain(|i| i.qty > 0);
        });
    }

    /// The whole "pay" event. The view calls it and renders `paying`/`error`.
    pub fn pay(&self) {
        if self.is_empty.get() {
            self.refused.set(Some("the cart is empty".into()));
            return;
        }
        self.refused.set(None);
        self.charge.dispatch_local(self.total.get());
    }

    /// A store that takes an argument: `stores.qtyOf(id)`.
    #[plum(watch)]
    pub fn qty_of(&self, id: u32) -> Memo<u32> {
        let items = self.items;
        Memo::new(move |_| items.get().iter().find(|i| i.id == id).map_or(0, |i| i.qty))
    }

    #[plum(skip)]
    pub fn debug_dump(&self) -> String {
        format!("{} items", self.items.get().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pay_runs_the_action_and_reports_through_stores() {
        let cart = Cart::with_charge(Box::new(|cents| {
            Box::pin(async move { if cents > 1000 { Err("declined".into()) } else { Ok(()) } })
        }));
        let id = cart.add("Plum", 250);
        cart.change_qty(id, 1);
        assert_eq!((cart.total.get(), cart.qty_of(id).get()), (500, 2));

        cart.pay();
        crate::runtime::pump(); // let the dispatched action run
        assert!(cart.is_empty.get() && cart.error.get().is_none());
    }
}
```

The binding that comes out of it:

```ts
export interface CartStores {
  items: ReadableAtom<Array<Item>>;
  coupon: WritableAtom<string>;
  total: ReadableAtom<number>;
  isEmpty: ReadableAtom<boolean>;
  paying: ReadableAtom<boolean>;
  error: ReadableAtom<string | null>;
  qtyOf(id: number): ReadableAtom<number>;
}

export interface CartActions {
  add(name: string, priceCents: number): number;
  changeQty(id: number, delta: number): void;
  pay(): void;
}
```

What the macros need:

- The derive and the `#[plum_actions]` block in the same module, one such
  block per model.
- A watched field implements Leptos' `Get` (`RwSignal`, `Memo`, `Signal`,
  their `Arc` versions, an action's `pending()` or `value()`, your own
  type). It can be private. No accessor is needed.
- Watched values are `Serialize + TS`; parameters `Deserialize + TS`; return
  values `Serialize + TS`. Numbers, `bool`, strings, your own structs and
  enums, `Vec`, `Option`, tuples and maps all work. The macros never look at
  type names, so there is no list of supported types.

What comes out:

| Rust                                               | TypeScript                                                                                                  |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `#[plum(watch)]` field                             | `stores.name: ReadableAtom<T>` (camelCase, or `js = "Name"`)                                                |
| `#[plum(watch, set)]` field                        | `WritableAtom<T>`; `set` writes the field                                                                   |
| `#[plum(watch, set = "method")]` field or method   | `WritableAtom<T>`; `set` calls that method, which can validate. The method is no longer listed as an action |
| `#[plum(watch)]` method returning a signal or memo | a store; with parameters, `stores.name(args)`                                                               |
| any other `pub fn (&self, ..)`                     | `actions.name(args)`; `#[plum(js = "...")]` renames it                                                      |
| `pub fn new(..) -> Self` or `-> Result<Self, E>`   | `createCart(..)` on the wasm module                                                                         |
| `dispose(&self)`, optional                         | called by the generated `dispose()`                                                                         |

Left out: `#[plum(skip)]`, non-`pub` methods, `&mut self` methods,
associated functions other than `new`, and unmarked methods that return a
signal or memo.

Behaviour worth designing around:

- **Writes are synchronous for JS.** When an action or `store.set` returns,
  every store it affected, on any model on the page, already holds the new
  value. A store can therefore back a controlled text input.
- **Stores are lazy.** A store subscribes in Rust while it has listeners and
  lets go about a second after the last one leaves. Unused stores cost
  nothing.
- **Stores with arguments return the same object for the same arguments**
  (compared by `JSON.stringify`), so they can be called during render.
- **A `#[plum(watch)]` method runs once per subscription, in a scope that is
  disposed when the store goes idle.** Memos it creates are meant to go
  away. State it creates goes away too, so return existing handles, or
  use `ArcRwSignal` for get-or-create state that must survive.
- **Errors are exceptions.** A returned `Err` is thrown as a JS `Error`; an
  argument of the wrong shape throws one naming the action and parameter.
- `None` is `null` both ways; 64-bit integers are JS numbers.

### 4. Async work

Actions are synchronous; a `pub async fn` in the block is a compile error.
Put async work in a Leptos `Action` and dispatch it from a plain method, as
`pay()` does above. The action's `pending()`, `value()`, `input()` and
`version()` implement `Get`, so they are watched like any field: `paying` is
`charge.pending()`, and `error` is a memo over `charge.value()`. `paying` is
already `true` when `pay()` returns to JS, and the outcome arrives in
`error`.

Use `Action::new_local` and `dispatch_local` whenever the future touches JS:
those futures are not `Send`. In native tests, call `runtime::pump()` after
dispatching so the future runs.

### 5. Build

```sh
cargo test __plum_export                       # writes plum_gen/<model>.ts
wasm-pack build --target web --features plum   # writes pkg/
```

Both land in the crate directory and are build output: gitignore `pkg/` and
`plum_gen/`. The binding is written by a test the derive generates, because
only there do real types exist for ts-rs to name. So the test has to run
before anything imports the binding; make the package's build script run
both commands in this order. Plain `cargo test` writes it too.

`plum_gen/<model>.ts` declares the data types, `<Model>Stores`,
`<Model>Actions`, and `bind<Model>(model)`, which returns
`{ stores, actions, dispose }`.

### 6. Wrap it as a package

plum stops at `bind<Model>`. Write the entry point next to the crate:

```ts
// index.ts
import initWasm, { createCart } from "./pkg/my_model.js";
import { bindCart } from "./plum_gen/cart";

export type { Item } from "./plum_gen/cart";

export let cart: ReturnType<typeof bindCart>;

export async function init(): Promise<void> {
  await initWasm();
  cart = bindCart(createCart("https://api.example.com"));
}
```

```json
{
  "name": "@acme/model",
  "type": "module",
  "exports": "./index.ts",
  "dependencies": { "nanostores": "^1.0.0" }
}
```

`./pkg/<crate_name_with_underscores>.js` is wasm-pack's glue. It finds the
`.wasm` file with `new URL(..., import.meta.url)`, which Vite handles in dev
and production with no configuration. Put model-specific startup (kicking
off a load, creating several models) in `init()`.

### 7. The front-end

```ts
import { init } from "@acme/model";
await init(); // once, before rendering
```

```tsx
import { useStore } from "@nanostores/react";
import { cart, type Item } from "@acme/model";

export default function PayButton() {
  const total = useStore(cart.stores.total);
  const isEmpty = useStore(cart.stores.isEmpty);
  const paying = useStore(cart.stores.paying);
  return (
    <button disabled={isEmpty} onClick={cart.actions.pay}>
      {paying ? "Paying…" : `Pay ${total}`}
    </button>
  );
}

function Line({ item }: { item: Item }) {
  const qty = useStore(cart.stores.qtyOf(item.id)); // same id, same store
  return (
    <li>
      {item.name} x{qty}
    </li>
  );
}
```

`cart` is assigned inside `init()`, so read it inside components and
functions, not at the top level of a module. Actions are plain functions and
can be passed as handlers. Any Nanostores binding works (React, Vue, Svelte,
Solid, or `store.subscribe`).

## Where logic goes

The point of the arrangement is that the model is the logic layer. When you
write or review the front-end, keep components to three things: render a
store, branch on a boolean the model supplied, hand a DOM event to an
action. Anything else moves into the model:

- a comparison (`count > 0`, `selectedId === id`, `list.length === 0`)
  becomes a `Memo<bool>` store, or a store with an argument for per-item
  questions such as `isSelected(id)`;
- a constant that encodes domain knowledge (the list of filters, labels)
  becomes a store;
- `useState` for form text becomes a writable store;
- an `if` inside an event handler becomes a method that handles the event,
  such as `submit()` or `edit_key(key)`.

DOM plumbing (`e.preventDefault()`, `e.target.value`, `e.key`) is all a
handler does before calling the model. This is cheap because writes are
synchronous and per-item stores only re-render the item that changed.

## Verify

- `cargo test` passes natively with the `plum` feature off.
- `cargo clippy --target wasm32-unknown-unknown --features plum` is clean.
- `plum_gen/<model>.ts` has the stores and action signatures you expect.
  Read it: it is the contract the front-end team gets.
- The front-end typechecks against it with no casts.

## More

- `references/troubleshooting.md`: what each compile error and runtime
  surprise means. Read it as soon as something does not build.
- `references/platform-data.md`: a service model that keeps named JSON
  values for code with no Rust model of its own, such as microfrontends.
  Read it when several apps on a page need to share state.
