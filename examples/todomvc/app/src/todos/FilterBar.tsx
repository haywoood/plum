import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function FilterBar() {
  const filter = useStore(todos.stores.filter);
  const filters = useStore(todos.stores.filters);

  return (
    <nav className="plum-filters">
      {filters.map((f) => (
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
