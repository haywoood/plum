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
- `skills/plum`: a skill that teaches a coding agent to do the integration.

## With an agent

`skills/plum` is a skill for coding agents: the integration steps below, what
belongs in the model and what in the view, and what each compile error
means. Copy the directory to where your agent looks for skills (for Claude
Code, `.claude/skills/` in your repository) and ask it to expose a crate
with plum. Moving state out of components into a model is most of the work,
and it is work an agent does well.

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
    #[plum(watch, set)]
    draft: RwSignal<String>, // JS can write it: stores.draft.set("...")
    #[plum(watch)]
    remaining: Memo<i32>,
    #[plum(watch)]
    by_day: Memo<HashMap<String, Vec<Todo>>>,
    #[plum(watch)]
    saving: Memo<bool>, // save.pending()
    save: Action<Vec<Todo>, Result<(), String>>,
    next_id: RwSignal<i32>, // not watched, never leaves Rust
}

#[plum_actions]
impl Todos {
    pub fn new(storage_key: &str) -> Self { /* create the signals and memos */ }

    pub fn add(&self, text: &str) -> i32 { /* ... */ }
    pub fn set_filter(&self, filter: Filter) { self.filter.set(filter); }
    pub fn import(&self, todos: Vec<Todo>) -> Result<(), String> { /* ... */ }
    pub fn save(&self) { self.save.dispatch_local(self.list.get()); }

    // a store that takes an argument
    #[plum(watch)]
    pub fn is_editing(&self, id: i32) -> Memo<bool> {
        let editing_id = self.editing_id;
        Memo::new(move |_| editing_id.get() == Some(id))
    }

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
- A store is writable when it says how: `#[plum(watch, set)]` on a field sets
  the field itself, and `#[plum(watch, set = "method")]`, on a field or a
  method, calls that method of the model, which can validate or normalise.
  JS gets a `WritableAtom`, and `store.set(value)` has gone through Rust and
  come back by the time it returns. A method named as a setter is not listed
  as an action.
- A method marked `#[plum(watch)]` becomes a store too. It returns the signal
  or memo to watch. If it takes parameters, so does the store:
  `stores.isEditing(id)`. The same arguments always give the same store
  object, so it can be called during render. The method runs once per
  subscription, and what it creates is disposed when the store loses its
  listeners.
- Every other `pub fn` that takes `&self` becomes an action under its
  camelCase name, or the one given with `#[plum(js = "...")]`. Left out are
  methods marked `#[plum(skip)]`, unmarked methods that return a signal or
  memo, `&mut self` methods and associated functions.
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
- Actions are synchronous. Async work goes in a Leptos `Action` that a plain
  method dispatches. Its `pending()`, `value()`, `input()` and `version()`
  implement `Get`, so they can be watched like any other field: `pending` is
  already `true` when the dispatching call returns, and the result arrives in
  a store. The example loads and saves this way. A `pub async fn` in the
  block is a compile error that says so. On wasm, futures that touch JS are
  not `Send`, so use `Action::new_local` and `dispatch_local`.

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

A store that takes arguments is called where it is used:

```tsx
export default function TodoItem({ todo }: { todo: Todo }) {
  const editing = useStore(todos.stores.isEditing(todo.id));
  // ...
}
```

Each row listens to its own answer, so starting an edit re-renders that row
and not the list.

`todos.stores.*` are plain `ReadableAtom`s. `todos.actions` has the model's
methods and nothing else, with the types they have in Rust:
`setFilter(filter: Filter): void`. The actions are plain functions, so one
that takes no arguments can be passed as a handler as it is. `todos` is
assigned inside `init()`, so read it inside components and functions, not at
the top level of a module.

## How it behaves

- A store is subscribed in Rust, with one Leptos `Effect`, while it has
  listeners. Nanostores drops the subscription a second after the last one
  leaves. A store nobody uses costs nothing.
- The first value is delivered synchronously when a store is listened to or
  read, so it is never `undefined`.
- Arguments of a store are compared as data: two calls give the same store
  when `JSON.stringify` of their arguments is equal.
- When an action returns, every store it affected already holds the new
  value, including stores of other models on the same page. Leptos runs
  effects on a later tick, so the generated action wrapper flushes every
  bridge before it returns. A store can therefore back a
  controlled text input, as `inputText` does in the example.
- Changes that do not come from a JS call, such as an async load finishing,
  arrive when Leptos runs the effect. A value is never delivered twice.
- Values are serialized whole. A `Vec` signal sends the entire array on every
  change. `None` arrives as `null`.
- A store is read-only unless it is marked `set`. Every write, through an
  action or through `store.set`, happens in Rust.
- 64-bit integers are JS numbers, in stores and in actions.
- The generated `createTodos()` sets up the `any_spawner` executor and a root
  `Owner` before it calls `new()`. The root is kept alive, so
  `provide_context` and `use_context` work between models. When you construct the model somewhere
  else, such as native tests or a Leptos app, that is up to you.
  `examples/todomvc/model/src/runtime.rs` has the headless version.
- The macros do not write files. `plum_gen/todos.ts` is written by the
  generated test, and only when its content has changed.
- Host APIs are your crate's business. The example reaches `localStorage`
  through a `Storage` struct of two async closures, with a wasm
  implementation and a fake for tests. `plum_wasm::js` has a few helpers for
  calling JS functions without web-sys.

## State shared across the platform

Not part of plum, but something it makes possible, and the example does it.
`examples/todomvc/model/src/platform_data.rs` is a service that keeps named
values for whoever is on the page: "platform, store this and make it
accessible".

```ts
platformdata.createStore("cart/state", { items: 0 });
const cart = useStore(platformdata.getStore<Cart>("cart/state"));
platformdata.getStore<Cart>("cart/state").set({ items: 1 });
```

- The data is ad hoc. Teams create the stores they need, and the service
  neither knows nor checks what is in them. The type parameter of `getStore`
  is the caller's claim, not a check.
- `getStore(name)` is a store that takes an argument, so the same name gives
  the same store to everyone, and it can be subscribed to before the store
  has been created. Creating a name twice throws.
- Values are JSON, on purpose. The service could hold JS values as they
  are and skip the serialization, but then platform state could contain
  functions and class instances, and would stop being something you can
  print, diff or persist.
- It is for state that has no model of its own. A model written in Rust,
  like the todos, keeps its state in its own signals: it is in the same wasm
  instance anyway, and typed.

"The same name gives the same store" only holds if every microfrontend uses
one instance of the package, so it has to be a shared module (an import map
or the bundler's equivalent), not bundled into each of them.

## Not there yet

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
