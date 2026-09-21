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
serde = { version = "1", features = ["derive"] }
ts-rs = "12"
plum-macro = { git = "https://github.com/haywoood/plum" }

# wasm build only
plum-wasm = { git = "https://github.com/haywoood/plum", optional = true }
wasm-bindgen = { version = "0.2", optional = true }

[features]
plum = ["dep:plum-wasm", "dep:wasm-bindgen"]
```

Everything the macros generate for wasm is behind `#[cfg(feature = "plum")]`,
so `cargo test` and your Leptos app build the crate with no wasm
dependencies. `plum-macro` is not optional because the attributes have to
resolve either way. The feature has to be named `plum`. It is a feature and
not a target check because your own Leptos app builds this crate for wasm32
too, and should not get the exports.

### 3. Annotate the model

```rust
use std::collections::HashMap;

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

#[derive(PlumModel)]
pub struct Todos {
    #[plum(watch, js = "Todos")]
    list: RwSignal<Vec<Todo>>,
    #[plum(watch)]
    filter: RwSignal<Filter>,
    #[plum(watch)]
    remaining: Memo<i32>,
    #[plum(watch)]
    by_day: Memo<HashMap<String, Vec<Todo>>>,
    next_id: RwSignal<i32>, // not watched, never leaves Rust
}

#[plum_actions]
impl Todos {
    pub fn new(storage_key: &str) -> Self { /* create the signals and memos */ }

    pub fn add(&self, text: &str) -> i32 { /* ... */ }
    pub fn set_filter(&self, filter: Filter) { self.filter.set(filter); }
    pub fn import(&self, todos: Vec<Todo>) -> Result<(), String> { /* ... */ }
    pub async fn load(&self) -> Result<(), String> { /* ... */ }

    #[plum(skip)]
    pub fn debug_dump(&self) -> String { /* stays in Rust */ }
}
```

The full version is `examples/todomvc/model/src/todos.rs`.

The macros work with names and leave types to the compiler, so there is
little they require:

- The derive and the `#[plum_actions]` block are in the same module, and
  there is one such block per model.
- A watched field implements Leptos' `Get`: `RwSignal`, `Memo`, `Signal`,
  their `Arc` versions, or anything of your own. It can be private, and no
  accessor method is needed.
- The value of a watched field is `Serialize` and `TS`. Parameters are
  `Deserialize` and `TS`, return values `Serialize` and `TS`. That covers
  numbers, `bool`, strings, your structs and enums, `Vec`, `Option`, tuples
  and maps. A missing derive is reported on the field or parameter.

What is exposed:

- Every watched field becomes a store, named after the field in camelCase or
  after the `js` override.
- Every `pub fn` that takes `&self` becomes an action under its camelCase
  name, or the one given with `#[plum(js = "...")]`. Left out are methods
  marked `#[plum(skip)]`, methods that return a signal or memo, `&mut self`
  methods and associated functions.
- `pub fn new(..)` becomes `createTodos(..)` with the same parameters. It can
  return `Self` or `Result<Self, E>`. Without a `new`, write the factory
  yourself around `WasmTodos::new_with(model)`.
- A `dispose(&self)` on the model is called by the generated `dispose()`. It
  is optional.

How actions behave:

- Arguments are deserialized into the declared types. An argument of the
  wrong shape throws a JS `Error` that names the action and the parameter.
- A returned `Err` is thrown as a JS `Error`. `Ok(value)` returns the value.
- `None` is `null` in both directions, and `undefined` is accepted for an
  `Option` parameter.
- A call to an `async fn` action returns nothing, immediately. The future is
  spawned on the Leptos executor and its result is dropped. Report progress
  and failure through signals, as the example does with `saving` and
  `last_error`.

### 4. Build

```sh
cargo test __plum_export                       # writes plum_gen/todos.ts
wasm-pack build --target web --features plum   # writes pkg/
```

All output lands in the crate directory.

- `plum_gen/todos.ts` is written by a test that the derive adds. A proc macro
  only sees tokens, so the TypeScript is produced where real types exist: the
  test asks ts-rs to name every watched value, parameter and return type and
  to find the structs and enums inside them. The file declares those data
  types, `TodosStores`, `TodosActions`, and `bindTodos(model)`, which returns
  `{ stores, actions, dispose }`. Plain `cargo test` writes it as well.
- `pkg/` is wasm-pack's output: the `.wasm` file, its JS glue, and a `.d.ts`
  with the `WasmTodos` class and `createTodos()`.

Both directories are build output. The example gitignores them.

### 5. Wrap it as a package

plum stops at `bindTodos`. Loading the wasm file, constructing models and
whatever has to happen at startup are specific to your crate, so you write
the entry point. This is the example's, in full:

```ts
import initWasm, { createTodos } from "./pkg/todomvc_model.js";
import { bindTodos } from "./plum_gen/todos";

export type { Todo, Filter } from "./plum_gen/todos";

export let todos: ReturnType<typeof bindTodos>;

export async function init(): Promise<void> {
  await initWasm();
  todos = bindTodos(createTodos("plum-todomvc"));
  todos.actions.load();
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
      {completed > 0 && <button onClick={todos.actions.clearCompleted}>Clear completed</button>}
    </div>
  );
}
```

`todos.stores.*` are plain `ReadableAtom`s. `todos.actions` has the model's
methods and nothing else, with the types they have in Rust:
`setFilter(filter: Filter): void`. The actions are plain functions, so one
that takes no arguments can be passed as a handler as it is. `todos` is
assigned inside `init()`, so read it inside components and functions, not at
the top level of a module.

## How it behaves

- Each watched field gets one Leptos `Effect`, created when `bindTodos` runs,
  whether or not anything subscribes to the store.
- The first value is delivered synchronously, so a store is never
  `undefined`.
- When an action returns, every store it affected already holds the new
  value. Leptos runs effects on a later tick, so the generated action wrapper
  flushes the bridge before it returns. A store can therefore back a
  controlled text input, as `inputText` does in the example.
- Changes that do not come from a JS call, such as an async load finishing,
  arrive when Leptos runs the effect. A value is never delivered twice.
- Values are serialized whole. A `Vec` signal sends the entire array on every
  change. `None` arrives as `null`.
- Stores are read-only. Writes go through actions.
- 64-bit integers are JS numbers, in stores and in actions.
- The generated `createTodos()` sets up the `any_spawner` executor and a root
  `Owner` before it calls `new()`. When you construct the model somewhere
  else, such as native tests or a Leptos app, that is up to you.
  `examples/todomvc/model/src/runtime.rs` has the headless version.
- The macros do not write files. `plum_gen/todos.ts` is written by the
  generated test, and only when its content has changed.
- Host APIs are your crate's business. The example reaches `localStorage`
  through a `Storage` struct of two async closures, with a wasm
  implementation and a fake for tests. `plum_wasm::js` has a few helpers for
  calling JS functions without web-sys.

## Not there yet

- Awaiting an `async fn` action from JS
- Subscribing lazily, only when a store has listeners
- Sending less than the whole value when a large `Vec` changes
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
builds, eslint, and clippy for native and wasm32.
