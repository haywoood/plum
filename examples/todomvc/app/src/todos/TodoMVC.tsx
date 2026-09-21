import Header from "./Header";
import NewTodoForm from "./NewTodoForm";
import Toolbar from "./Toolbar";
import TodoList from "./TodoList";
import FilterBar from "./FilterBar";
import CountBar from "./CountBar";
import ErrorBanner from "./ErrorBanner";
import "./styles.css";

export default function TodoMVC() {
  return (
    <main className="plum">
      <Header />
      <NewTodoForm />
      <Toolbar />
      <TodoList />
      <FilterBar />
      <CountBar />
      <ErrorBanner />
    </main>
  );
}
