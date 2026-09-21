import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function Toolbar() {
  const isEmpty = useStore(todos.stores.isEmpty);
  const allDone = useStore(todos.stores.allDone);
  const saving = useStore(todos.stores.saving);

  return (
    <div className="plum-toolbar">
      <button disabled={isEmpty} onClick={todos.actions.toggleAll}>
        {allDone ? "Mark all active" : "Mark all done"}
      </button>
      <span className="plum-status">{saving ? "Saving…" : ""}</span>
    </div>
  );
}
