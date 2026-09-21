// What the app installs. Call init() once from the root config; after that
// `todos` is a set of Nanostores plus the actions that change them.
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
