// What the app installs. Call init() once from the root config; after that
// `todos` is a set of Nanostores plus the actions that change them, and
// `platformdata` holds named stores for state that has no model of its own.
import type { WritableAtom } from "nanostores";
import initWasm, { createPlatformData, createTodos } from "./pkg/todomvc_model.js";
import { bindPlatformData, type JsonValue } from "./plum_gen/platform-data";
import { bindTodos } from "./plum_gen/todos";

export type { Todo, Filter, FilterOption } from "./plum_gen/todos";
export type { JsonValue };

export let todos: ReturnType<typeof bindTodos>;

let platform: ReturnType<typeof bindPlatformData>;

export const platformdata = {
  /** Creates a store. The name has to be unique on the platform. */
  createStore: (key: string, initial: JsonValue): void =>
    platform.actions.createStore(key, initial),
  /** The store with this name: the same object for the same name, every time. */
  getStore: <T extends JsonValue = JsonValue>(key: string) =>
    platform.stores.getStore(key) as WritableAtom<T>,
};

export async function init(): Promise<void> {
  await initWasm();
  platform = bindPlatformData(createPlatformData());
  todos = bindTodos(createTodos("plum-todomvc"));
  todos.actions.load();
}
