import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function FilterBar() {
  const filters = useStore(todos.stores.filters);

  return (
    <nav className="plum-filters">
      {filters.map((option) => (
        <button
          key={option.filter}
          className={option.selected ? "is-on" : undefined}
          onClick={() => todos.actions.setFilter(option.filter)}
        >
          {option.label}
        </button>
      ))}
    </nav>
  );
}
