import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function ErrorBanner() {
  const lastError = useStore(todos.stores.lastError);

  if (!lastError) return null;

  return (
    <div className="plum-error" role="alert">
      {lastError}
    </div>
  );
}
