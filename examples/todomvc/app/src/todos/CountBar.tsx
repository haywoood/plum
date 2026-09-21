import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function CountBar() {
  const remaining = useStore(todos.stores.remaining);
  const hasCompleted = useStore(todos.stores.hasCompleted);

  return (
    <div className="plum-footer">
      <span className="plum-count">{remaining} left</span>
      {hasCompleted && <button onClick={todos.actions.clearCompleted}>Clear completed</button>}
    </div>
  );
}
