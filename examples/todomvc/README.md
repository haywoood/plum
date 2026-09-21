# todomvc example

TodoMVC with the state in a Leptos crate and the view in React. The model
follows the behaviour of the Leptos `todomvc` example, with the view code
removed.

- `model/` is what the Rust team owns. `src/` is the Leptos crate.
  `index.ts` and `package.json` wrap its wasm build as the `@todomvc/model`
  npm package. `pkg/` and `plum_gen/` appear here after a build.
- `app/` is what the front-end team owns: React, `@nanostores/react`, and a
  dependency on `@todomvc/model`. Its components keep no state and compute
  nothing. They render stores, branch on booleans the model gives them, and
  pass DOM events to actions.

Run it from the repository root:

```sh
npm install
npm run dev
```

## Files to read

| File                            | What it shows                                                                |
| ------------------------------- | ---------------------------------------------------------------------------- |
| `model/src/platform_data.rs`    | A service that keeps named JSON values for code that has no model of its own |
| `model/src/todos.rs`            | The todos model, with `#[derive(PlumModel)]` and `#[plum_actions]`           |
| `model/src/storage.rs`          | `localStorage` behind a seam, so the model also runs natively                |
| `model/src/runtime.rs`          | Executor and owner setup for running the model without a view                |
| `model/index.ts`                | The package entry point: `init()`, `todos` and `platformdata`                |
| `model/plum_gen/todos.ts`       | Generated binding (after a build)                                            |
| `app/src/main.tsx`              | The one `init()` call                                                        |
| `app/src/todos/CountBar.tsx`    | A component that reads two stores and calls one action                       |
| `app/src/todos/NewTodoForm.tsx` | A controlled input on a writable store: `inputText.set(...)`                 |
| `app/src/todos/TodoItem.tsx`    | A store that takes an argument: `isEditing(todo.id)`                         |
