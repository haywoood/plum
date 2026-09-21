import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function CountBar() {
  const remaining = useStore(todos.stores.remaining);
  const completed = useStore(todos.stores.completed);

  return (
    <div className="plum-footer">
      <span className="plum-count">{remaining} left</span>
      {completed > 0 && <button onClick={todos.actions.clearCompleted}>Clear completed</button>}
    </div>
  );
}
