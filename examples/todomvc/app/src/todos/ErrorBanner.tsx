import { useStore } from "@nanostores/react";
import { todos } from "@todomvc/model";

export default function ErrorBanner() {
  const hasError = useStore(todos.stores.hasError);
  const lastError = useStore(todos.stores.lastError);

  if (!hasError) return null;

  return (
    <div className="plum-error" role="alert">
      {lastError}
    </div>
  );
}
