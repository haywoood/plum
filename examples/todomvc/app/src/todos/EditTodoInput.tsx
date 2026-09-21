import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function EditTodoInput() {
  const text = useStore(todos.stores.editText);

  return (
    <input
      className="plum-edit"
      aria-label="Edit todo"
      value={text}
      autoFocus
      onChange={(e) => todos.stores.editText.set(e.target.value)}
      onKeyDown={(e) => todos.actions.editKey(e.key)}
      onBlur={todos.actions.commitEdit}
    />
  );
}
