import { useState } from "react";
import { todos } from "@todomvc/model";

export default function NewTodoForm() {
  const [text, setText] = useState("");

  return (
    <form
      className="plum-add"
      onSubmit={(e) => {
        e.preventDefault();
        // add() ignores blank text and returns -1
        if (todos.actions.add(text) >= 0) setText("");
      }}
    >
      <input
        aria-label="New todo"
        value={text}
        placeholder="What needs to be done?"
        onChange={(e) => setText(e.target.value)}
      />
      <button type="submit">Add</button>
    </form>
  );
}
