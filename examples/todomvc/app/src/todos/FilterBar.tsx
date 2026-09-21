import { useStore } from "@nanostores/react";
import { todos, type Filter } from "@todomvc/model";

const FILTERS: Filter[] = ["all", "active", "completed"];

export default function FilterBar() {
  const filter = useStore(todos.stores.filter);

  return (
    <nav className="plum-filters">
      {FILTERS.map((f) => (
        <button
          key={f}
          className={filter === f ? "is-on" : undefined}
          onClick={() => todos.actions.setFilter(f)}
        >
          {f.charAt(0).toUpperCase() + f.slice(1)}
        </button>
      ))}
    </nav>
  );
}
