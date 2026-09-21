import { useState } from "react";
import { todos, type Todo } from "@todomvc/model";

export default function TodoItem({ todo }: { todo: Todo }) {
  // null while not editing
  const [draft, setDraft] = useState<string | null>(null);

  const commit = () => {
    if (draft !== null) todos.actions.edit(todo.id, draft);
    setDraft(null);
  };

  return (
    <li className={todo.done ? "is-done" : ""}>
      {draft !== null ? (
        <input
          className="plum-edit"
          aria-label="Edit todo"
          value={draft}
          autoFocus
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") setDraft(null);
          }}
        />
      ) : (
        <label className="plum-label">
          <input
            type="checkbox"
            aria-label={`Toggle ${todo.text}`}
            checked={todo.done}
            onChange={() => todos.actions.toggle(todo.id)}
          />
          <span onDoubleClick={() => setDraft(todo.text)}>{todo.text}</span>
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
