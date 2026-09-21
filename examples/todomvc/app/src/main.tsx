import { createRoot } from "react-dom/client";
import { init } from "@todomvc/model";
import TodoMVC from "./todos/TodoMVC";

await init();

createRoot(document.getElementById("root")!).render(<TodoMVC />);
