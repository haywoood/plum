import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function NewTodoForm() {
  const text = useStore(todos.stores.inputText);

  return (
    <form
      className="plum-add"
      onSubmit={(e) => {
        e.preventDefault();
        todos.actions.submitNew();
      }}
    >
      <input
        aria-label="New todo"
        value={text}
        placeholder="What needs to be done?"
        onChange={(e) => todos.actions.setInputText(e.target.value)}
      />
      <button type="submit">Add</button>
    </form>
  );
}
