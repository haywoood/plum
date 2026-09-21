# plum

plum exposes the state of a Leptos crate to TypeScript. Signals and memos
become [Nanostores](https://github.com/nanostores/nanostores) stores, and
`pub` methods become function calls. The crate is compiled to WebAssembly
with wasm-bindgen.

It is for a team that keeps its logic in Leptos and wants people who work in
React, Vue, Svelte, Solid or plain TypeScript to be able to build on it. The
Rust team publishes an npm package. The front-end team installs it and reads
stores. Rust, Leptos and WebAssembly do not appear in their code.

Status: 0.1. Not on crates.io. The API will change. There is one example, in
`examples/todomvc`.

## What is in the repository

- `rust/plum-macro`: `#[derive(PlumModel)]` and `#[plum_actions]`. They
  generate the wasm-bindgen wrapper for a model and a TypeScript binding file.
- `rust/plum-wasm`: the runtime the generated code calls. One Leptos `Effect`
  per subscription, conversion of values to JS, executor and owner setup.
- `examples/todomvc/model`: a Leptos crate that uses both, and the 15-line
  `index.ts` that turns its wasm build into an npm package.
- `examples/todomvc/app`: a React app that uses that package.

## Requirements

- A recent stable Rust (CI uses 1.98) with the `wasm32-unknown-unknown` target
- [wasm-pack](https://rustwasm.github.io/wasm-pack/)
- Leptos 0.8
- Node 20 or later for the TypeScript side

## Integrating

### 1. Get the state into a crate with no view code

The crate that holds your state has to build for `wasm32-unknown-unknown`
without a renderer, so it depends on `leptos` with `default-features = false`
and none of `csr`, `ssr` or `hydrate`. If the model uses `Effect`, depend on
`reactive_graph` with the `effects` feature as well. `leptos` only turns that
on through its view features.

If your signals are created inside components today, this is the step that
takes work: move them into a struct that owns its signals and has methods for
everything that changes them. plum does not help with that.

### 2. Cargo.toml

```toml
[lib]
crate-type = ["rlib", "cdylib"]

[dependencies]
leptos = { version = "0.8", default-features = false }
reactive_graph = { version = "0.2", features = ["effects"] }
any_spawner = "0.3" # generated code for `async fn` actions spawns through it
serde = { version = "1", features = ["derive"] }
ts-rs = "12"        # for structs and enums that appear in watched fields
plum-macro = { git = "<this repository>" }

# wasm build only
plum-wasm = { git = "<this repository>", optional = true }
wasm-bindgen = { version = "0.2", optional = true }
js-sys = { version = "0.3", optional = true }
serde_json = { version = "1", optional = true }
serde-wasm-bindgen = { version = "0.6", optional = true }

[features]
plum = [
  "dep:plum-wasm",
  "dep:wasm-bindgen",
  "dep:js-sys",
  "dep:serde_json",
  "dep:serde-wasm-bindgen",
]
```

Everything the macros generate is behind `#[cfg(feature = "plum")]`, so
`cargo test` and your Leptos app build the crate with no wasm dependencies.
`plum-macro` is not optional because the attributes have to resolve either
way. The feature has to be named `plum`.

### 3. Annotate the model

```rust
use leptos::prelude::*;
use plum_macro::{plum_actions, PlumModel};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Todo {
    pub id: i32,
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    All,
    Active,
    Completed,
}

#[derive(PlumModel, Clone)]
pub struct Todos {
    #[plum(watch, js = "Todos")]
    list: RwSignal<Vec<Todo>>,
    #[plum(watch)]
    filter: RwSignal<Filter>,
    #[plum(watch)]
    remaining: Memo<i32>,
    next_id: RwSignal<i32>, // not watched, never leaves Rust
}

#[plum_actions]
impl Todos {
    pub fn new() -> Self { /* create the signals and memos */ }

    // one accessor per watched field, named after the field
    pub fn list(&self) -> RwSignal<Vec<Todo>> { self.list }
    pub fn filter(&self) -> RwSignal<Filter> { self.filter }
    pub fn remaining(&self) -> Memo<i32> { self.remaining }

    pub fn add(&self, text: &str) -> i32 { /* ... */ }
    pub fn set_filter(&self, filter: Filter) { self.filter.set(filter); }
    pub async fn load(&self) -> Result<(), String> { /* ... */ }

    pub fn dispose(&self) { /* stop effects you created */ }
}
```

The full version is `examples/todomvc/model/src/todos.rs`.

The macros expect the following from your code. They do not check it up
front, so a missing piece shows up as a compile error inside generated code.

- `new()` with no arguments. The generated `createTodos()` calls it.
- `dispose(&self)`.
- For every `#[plum(watch)]` field, a method with the same name that returns
  the signal or memo.
- `use leptos::prelude::*` (or `reactive_graph::traits::Get`) in scope where
  the derive is used.
- `Clone` on the model if it has `async fn` actions.
- `Serialize` and `TS` on structs and enums used in watched fields.
  `Deserialize` on types used as action parameters.
- At most one `PlumModel` per module.

What is exposed:

- Watched fields can be `RwSignal`, `Memo`, `ReadSignal` or `Signal` of a
  number, `bool`, `String`, a struct or enum of yours, or a `Vec` or `Option`
  of those. Maps, tuples and references are rejected at compile time. The
  store is named after the field in camelCase, or after the `js` override.
- Every `pub fn` that takes `&self` becomes an action under its camelCase
  name. `dispose`, methods that return a signal type, `&mut self` methods and
  associated functions are left out.

Action parameters and return values:

- Numbers, `bool`, `String` and `&str` go through wasm-bindgen unchanged.
- Any other parameter type is passed from JS as a string and parsed with
  serde. In practice that means enums with unit variants:
  `actions.setFilter("completed")`. Struct parameters do not work yet.
- Any other return type is converted with serde-wasm-bindgen. Its TypeScript
  type is `any`.
- A call to an `async fn` action returns nothing, immediately. The future is
  spawned on the Leptos executor and its result is dropped. Report progress
  and failure through signals, as the example does with `saving` and
  `last_error`.

### 4. Build

```sh
cargo test                                     # writes plum_gen/types.ts
wasm-pack build --target web --features plum   # writes pkg/ and plum_gen/todos.ts
```

All output lands in the crate directory.

- `pkg/` is wasm-pack's output: the `.wasm` file, its JS glue, and a `.d.ts`
  with the `WasmTodos` class and `createTodos()`.
- `plum_gen/todos.ts` is written by the derive while the crate compiles. It
  exports `TodosStores` and `bindTodos(model)`, which returns
  `{ stores, actions, dispose }`.
- `plum_gen/types.ts` holds the TypeScript declarations of your data types,
  from ts-rs. A test that the derive adds writes it, so it appears when you
  run `cargo test`, not when you build.

Both directories are build output. The example gitignores them.

### 5. Wrap it as a package

plum stops at `bindTodos`. Loading the wasm file, constructing models and
whatever has to happen at startup are specific to your crate, so you write
the entry point. This is the example's, in full:

```ts
import initWasm, { createTodos, type WasmTodos } from "./pkg/todomvc_model.js";
import { bindTodos } from "./plum_gen/todos";

export type { Todo, Filter } from "./plum_gen/types";

export let todos: ReturnType<typeof bindTodos<WasmTodos>>;

export async function init(): Promise<void> {
  await initWasm();
  const model = createTodos();
  model.load();
  todos = bindTodos(model);
}
```

Next to it is a `package.json` whose `exports` points at that file and which
depends on `nanostores`. With several models, create and bind each of them in
`init()` and export them all.

The glue that wasm-pack emits for `--target web` finds the `.wasm` file with
`new URL("todomvc_model_bg.wasm", import.meta.url)`. Vite resolves that in
dev and in production builds with no configuration. Other bundlers have not
been tried.

### 6. What the front-end team writes

Once, before rendering:

```ts
import { init } from "@todomvc/model";

await init();
```

Then, anywhere:

```tsx
import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function CountBar() {
  const remaining = useStore(todos.stores.remaining);
  const completed = useStore(todos.stores.completed);

  return (
    <div>
      <span>{remaining} left</span>
      {completed > 0 && (
        <button onClick={() => todos.actions.clearCompleted()}>Clear completed</button>
      )}
    </div>
  );
}
```

`todos.stores.*` are plain `ReadableAtom`s. `todos.actions` has the model's
methods and nothing else: the watch, unwatch and free methods of the wasm
class are removed from its type. `todos` is assigned inside `init()`, so read
it inside components and functions, not at the top level of a module.

## How it behaves

- Each watched field gets one Leptos `Effect`, created when `bindTodos` runs,
  whether or not anything subscribes to the store.
- The first value is delivered synchronously, so a store is never
  `undefined`.
- Later values arrive when Leptos re-runs the effect. That is on a later
  microtask, not during the action call, and several writes in the same turn
  produce one update.
- Because of that, do not drive a controlled text input from a store. The
  value comes back a tick late and React moves the caret to the end of the
  field. Keep keystroke state in the view and call an action on submit.
- Values are serialized whole. A `Vec` signal sends the entire array on every
  change. `None` arrives as `null`.
- Stores are read-only. Writes go through actions.
- 64-bit integers are `bigint` as action parameters and return values, and
  `number` in stores.
- The generated `createTodos()` sets up the `any_spawner` executor and a root
  `Owner` before it calls `new()`. When you construct the model somewhere
  else, such as native tests or a Leptos app, that is up to you.
  `examples/todomvc/model/src/runtime.rs` has the headless version.
- The derive writes `plum_gen/todos.ts` whenever the crate is compiled and
  the content has changed. That includes `cargo check` and rust-analyzer.
- Host APIs are your crate's business. The example reaches `localStorage`
  through a `Storage` struct of two async closures, with a wasm
  implementation and a fake for tests. `plum_wasm::js` has a few helpers for
  calling JS functions without web-sys.

## Not there yet

- Struct parameters to actions
- TypeScript types for serde-converted return values
- Awaiting an `async fn` action from JS
- Subscribing lazily, only when a store has listeners
- Releases on crates.io

## Running the example

```sh
npm install
npm run dev
```

The first run builds the wasm package, then serves the app on
http://localhost:5173. `npm run dev:rust` in a second terminal rebuilds the
wasm package when Rust sources change. It needs
[watchexec](https://github.com/watchexec/watchexec).

`npm run check` is what CI runs: formatting, `cargo test`, the wasm and app
builds, eslint, and clippy for native and wasm32. `npm run build` on its own
expects `plum_gen/types.ts` to exist, so run `npm test` first on a fresh
checkout.
