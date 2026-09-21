import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";
import TodoItem from "./TodoItem";

export default function TodoList() {
  const visible = useStore(todos.stores.visible);
  const hasVisible = useStore(todos.stores.hasVisible);

  if (!hasVisible) {
    return <p className="plum-empty">Nothing to show.</p>;
  }

  return (
    <ul className="plum-list">
      {visible.map((todo) => (
        <TodoItem key={todo.id} todo={todo} />
      ))}
    </ul>
  );
}
