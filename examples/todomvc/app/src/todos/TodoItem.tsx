import { useStore } from "@nanostores/react";
import { todos, type Todo } from "@todomvc/model";
import EditTodoInput from "./EditTodoInput";

export default function TodoItem({ todo }: { todo: Todo }) {
  const editing = useStore(todos.stores.isEditing(todo.id));

  return (
    <li className={todo.done ? "is-done" : ""}>
      {editing ? (
        <EditTodoInput />
      ) : (
        <label className="plum-label">
          <input
            type="checkbox"
            aria-label={`Toggle ${todo.text}`}
            checked={todo.done}
            onChange={() => todos.actions.toggle(todo.id)}
          />
          <span onDoubleClick={() => todos.actions.startEdit(todo.id)}>{todo.text}</span>
        </label>
      )}
      <button
        className="plum-delete"
        aria-label={`Delete ${todo.text}`}
        onClick={() => todos.actions.remove(todo.id)}
      >
        ×
      </button>
    </li>
  );
}
