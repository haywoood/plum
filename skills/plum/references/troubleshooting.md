# Troubleshooting

## Compile errors

**`the trait bound X: Serialize` / `TS` / `DeserializeOwned is not satisfied`**
The error points at the watched field's type or at the parameter. Add the
missing derive to that type: `Serialize` and `TS` for anything in a watched
value or a return value, `Deserialize` and `TS` for a parameter. Nested
types need them too.

**`the trait bound i32: Get is not satisfied`** on a watched field or a
`#[plum(watch)]` method's return type: it is not a reactive value. Wrap a
plain value in an `RwSignal` or a `Memo`.

**`the trait bound Memo<..>: Set is not satisfied`** (with `Write` and
`IsDisposed` alongside): `#[plum(watch, set)]` sets the field itself, so the field has to be settable (`RwSignal`,
`ArcRwSignal`). For a computed value name a method instead:
`#[plum(watch, set = "set_draft")]`.

**`plum: an async fn cannot be exported`**: put the work in a Leptos
`Action`, dispatch it from a plain method, watch `pending()` and `value()`.
See "Async work" in SKILL.md. Or mark the method `#[plum(skip)]`.

**`plum: a generic method cannot be exported`**, **`&mut parameters`**,
**`an exported parameter needs a plain name`**: mark the method
`#[plum(skip)]`, or give it a signature JS can call.

**`plum: no such method in this block`** on `set = "..."`: the setter has to
be in the same `#[plum_actions]` block, named by its Rust name.

**`cannot find type WasmCart in this scope`**: the `#[plum_actions]` block
is in a different module from the derive, which is where `WasmCart` is
generated. Put them in the same module.

**`duplicate definitions with name __plum_actions`** (and `multiple
applicable items in scope`), only under `cargo test`: two impl blocks of one
model are annotated. The wasm build accepts that, but the binding is
assembled from a single block. Move the exported methods into one
`#[plum_actions]` block; other impl blocks stay unannotated.

**`unexpected cfg condition value: plum`**: the crate has no feature named
`plum`. The name is fixed.

**`unresolved import plum_wasm` / `wasm_bindgen` in a native build**: some
hand-written code uses them outside `#[cfg(feature = "plum")]`. Gate it, the
way `examples/todomvc/model/src/storage.rs` gates its wasm half with
`#[cfg(all(target_arch = "wasm32", feature = "plum"))]`.

**A future is not `Send`** inside `Action::new`: use `Action::new_local` and
`dispatch_local`. Anything that awaits a JS promise is not `Send`.

**wasm-pack: `crate-type must be cdylib`**: add
`crate-type = ["rlib", "cdylib"]` under `[lib]`.

## TypeScript errors

**`Cannot find module './plum_gen/...'`**: the export test has not run. Run
`cargo test __plum_export` (or plain `cargo test`) before `tsc`, and make the
package's build script do so.

**`Cannot find module './pkg/...'`**: `wasm-pack build --target web
--features plum` has not run, or the file name is wrong. It is the crate
name with underscores: crate `my-model` gives `pkg/my_model.js`.

**A store or action is missing from the binding**: check the rules in
SKILL.md. Common causes are a method that is not `pub`, takes `&mut self`,
or returns a signal without `#[plum(watch)]`. Re-run the export test after
changing the model; the binding is only rewritten then.

**An action's parameter or return type is `any` in `pkg/*.d.ts`**: expected.
That file is wasm-bindgen's view. The typed signatures are in
`plum_gen/<model>.ts`, which is what the front-end uses.

## Runtime surprises

**`TypeError: Cannot read properties of undefined` on the exported model**:
it is assigned inside `init()`. Something read it at module top level, or
rendered before `await init()`.

**Effects never run (autosave does nothing)**: depend on
`reactive_graph = { version = "0.2", features = ["effects"] }`. `leptos` with
`default-features = false` does not enable them.

**A value written inside a `#[plum(watch)]` method disappears**: what such a
method creates is disposed when the store loses its listeners. Create
lasting state in `new()` and return the existing handle, or use
`ArcRwSignal`, which is reference counted and not owned by that scope.

**A panic about a disposed signal from a late async task**: `.get()` on a
disposed signal panics; `.set()` is a silent no-op. In a future that can
outlive what it touches, use `try_get()`.

**`use_context` returns `None` between models**: models share context
through the root owner that plum's generated factory sets up. A model
constructed by hand needs `plum_wasm::runtime::init()` called first, and in
native code the `ensure_init()` from SKILL.md, which keeps the root alive.
`Owner::set` alone only stores a weak reference.

**Two microfrontends see different state for the same store name**: each
bundled its own copy of the model package, so there are two wasm instances.
The package has to be one shared module: an import map, or the bundler's
shared/external mechanism.

**Stores with object arguments miss the cache**: arguments are compared by
`JSON.stringify`, so key order matters. Prefer ids and strings as store
arguments.

**A native test sees stale values**: effects and dispatched actions run on
the executor. Call `any_spawner::Executor::poll_local()` (the `pump()` in
SKILL.md) after the change. Reading a `Memo` directly is always current.
