//! The TodoMVC domain model: plain data + Leptos signals/memos/effects.
//!
//! Feature parity with the official Leptos TodoMVC (leptos-rs/leptos
//! `examples/todomvc`), reworked to be view-agnostic:
//!
//! - add on non-empty (trimmed) text;
//! - edit; saving an empty title removes the todo;
//! - toggle / remove / `clear_completed`;
//! - `toggle_all`: if nothing is left, uncheck everything, else check all;
//! - `remaining` / `completed` / `visible` (filtered) are Leptos memos;
//! - the filter is model state (`All` / `Active` / `Completed`), and so is
//!   everything the view would otherwise keep for itself: the new-todo text,
//!   which row is being edited, and the text of that edit;
//! - persistence: `load()` reads from storage, `save()` writes to storage,
//!   and an autosave `Effect` triggers `save` whenever the list is dirtied.

use leptos::prelude::*;
use reactive_graph::effect::Effect;
use reactive_graph::owner::LocalStorage;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use plum_macro::{plum_actions, PlumModel};

/// One row of the list (plain data; crosses to JS as a plain object).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Todo {
    pub id: i32,
    pub text: String,
    pub done: bool,
}

/// Which slice of the list the view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    All,
    Active,
    Completed,
}

/// A future that resolves to a loaded todo list.
pub type LoadFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<Vec<Todo>, String>>>>;
/// A future that resolves to a completed save.
pub type SaveFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<(), String>>>>;

/// Async storage injected by the host for testability.
#[derive(Clone)]
pub struct Storage {
    pub load: std::sync::Arc<dyn Fn() -> LoadFuture>,
    pub save: std::sync::Arc<dyn Fn(Vec<Todo>) -> SaveFuture>,
}

/// The TodoMVC model. All state is Leptos reactive; all operations are plain
/// methods with direct `&self` access.
#[derive(PlumModel, Clone)]
pub struct Todos {
    #[plum(watch, js = "Todos")]
    list: RwSignal<Vec<Todo>>,
    #[plum(watch)]
    filter: RwSignal<Filter>,
    /// Every filter, in display order.
    #[plum(watch)]
    filters: Memo<Vec<Filter>>,
    next_id: RwSignal<i32>,
    loaded: RwSignal<bool>,
    #[plum(watch)]
    remaining: Memo<i32>,
    #[plum(watch)]
    completed: Memo<i32>,
    #[plum(watch)]
    total: Memo<i32>,
    #[plum(watch)]
    all_done: Memo<bool>,
    #[plum(watch)]
    visible: Memo<Vec<Todo>>,
    #[plum(watch)]
    saving: RwSignal<bool>,
    #[plum(watch)]
    last_error: RwSignal<Option<String>>,
    /// Text of the new-todo field.
    #[plum(watch)]
    input_text: RwSignal<String>,
    /// The todo being edited, if any, and the text of that edit.
    #[plum(watch)]
    editing_id: RwSignal<Option<i32>>,
    #[plum(watch)]
    edit_text: RwSignal<String>,
    dirty: RwSignal<bool>,
    autosave: Effect<LocalStorage>,
    storage: Storage,
}

#[plum_actions]
impl Todos {
    /// Creates the model with the default (target-appropriate) storage backend.
    pub fn new() -> Self {
        Self::with_storage(crate::storage::local_storage())
    }

    /// Creates the model with an explicit storage backend (used by tests).
    pub fn with_storage(storage: Storage) -> Self {
        crate::runtime::ensure_init();

        let list: RwSignal<Vec<Todo>> = RwSignal::new(Vec::new());
        let filter = RwSignal::new(Filter::All);
        let next_id = RwSignal::new(1);
        let loaded = RwSignal::new(false);
        let dirty = RwSignal::new(false);
        let saving = RwSignal::new(false);
        let last_error = RwSignal::new(None);
        let input_text = RwSignal::new(String::new());
        let editing_id = RwSignal::new(None);
        let edit_text = RwSignal::new(String::new());

        let filters = Memo::new(|_| vec![Filter::All, Filter::Active, Filter::Completed]);
        let remaining = Memo::new(move |_| list.get().iter().filter(|t| !t.done).count() as i32);
        let completed = Memo::new(move |_| list.get().iter().filter(|t| t.done).count() as i32);
        let total = Memo::new(move |_| list.get().len() as i32);
        let all_done = Memo::new(move |_| {
            let items = list.get();
            !items.is_empty() && items.iter().all(|t| t.done)
        });
        let visible = Memo::new(move |_| {
            let items = list.get();
            match filter.get() {
                Filter::All => items,
                Filter::Active => items.into_iter().filter(|t| !t.done).collect(),
                Filter::Completed => items.into_iter().filter(|t| t.done).collect(),
            }
        });

        // Autosave: a Leptos effect that watches the list; when data was
        // dirtied after a successful load and no save is in flight, it spawns
        // an async save task.
        let autosave = Effect::new({
            let storage = storage.clone();
            move |_prev: Option<()>| {
                let _ = list.get();
                if loaded.get() && dirty.get() && !saving.get() {
                    dirty.set(false);
                    saving.set(true);
                    let storage = storage.clone();
                    any_spawner::Executor::spawn_local(async move {
                        let todos = list.get();
                        let result = (storage.save)(todos).await;
                        saving.set(false);
                        if let Err(e) = result {
                            last_error.set(Some(e));
                        }
                    });
                }
            }
        });

        Self {
            list,
            filter,
            filters,
            next_id,
            loaded,
            remaining,
            completed,
            total,
            all_done,
            visible,
            saving,
            last_error,
            input_text,
            editing_id,
            edit_text,
            dirty,
            autosave,
            storage,
        }
    }

    pub fn list(&self) -> RwSignal<Vec<Todo>> {
        self.list
    }
    pub fn filter(&self) -> RwSignal<Filter> {
        self.filter
    }
    pub fn filters(&self) -> Memo<Vec<Filter>> {
        self.filters
    }
    pub fn total(&self) -> Memo<i32> {
        self.total
    }
    pub fn all_done(&self) -> Memo<bool> {
        self.all_done
    }
    pub fn remaining(&self) -> Memo<i32> {
        self.remaining
    }
    pub fn completed(&self) -> Memo<i32> {
        self.completed
    }
    /// The list after the active filter is applied (Leptos memo).
    pub fn visible(&self) -> Memo<Vec<Todo>> {
        self.visible
    }
    /// True while a save is in flight.
    pub fn saving(&self) -> RwSignal<bool> {
        self.saving
    }
    /// Message from the last failed load/save, if any.
    pub fn last_error(&self) -> RwSignal<Option<String>> {
        self.last_error
    }
    pub fn input_text(&self) -> RwSignal<String> {
        self.input_text
    }
    pub fn editing_id(&self) -> RwSignal<Option<i32>> {
        self.editing_id
    }
    pub fn edit_text(&self) -> RwSignal<String> {
        self.edit_text
    }

    /// Loads the list from the storage backend.
    pub async fn load(&self) -> Result<Vec<Todo>, String> {
        match (self.storage.load)().await {
            Ok(todos) => {
                let max_id = todos.iter().map(|t| t.id).max().unwrap_or(0);
                self.next_id.set(max_id + 1);
                self.list.set(todos.clone());
                self.loaded.set(true);
                Ok(todos)
            }
            Err(e) => {
                self.last_error.set(Some(e.clone()));
                Err(e)
            }
        }
    }

    /// Saves the list to the storage backend.
    pub async fn save(&self) -> Result<(), String> {
        if self.saving.get() {
            return Ok(());
        }
        self.saving.set(true);
        let result = (self.storage.save)(self.list.get()).await;
        self.saving.set(false);
        match &result {
            Ok(()) => {
                self.last_error.set(None);
            }
            Err(e) => {
                self.last_error.set(Some(e.clone()));
            }
        }
        result
    }

    /// Adds a todo (trimmed; empty text is ignored). Returns the new id, or
    /// `-1` when the text was empty.
    pub fn add(&self, text: &str) -> i32 {
        let text = text.trim();
        if text.is_empty() {
            return -1;
        }
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let mut todos = self.list.get();
        todos.push(Todo {
            id,
            text: text.to_string(),
            done: false,
        });
        self.list.set(todos);
        self.mark_dirty();
        id
    }

    pub fn set_input_text(&self, text: &str) {
        self.input_text.set(text.to_string());
    }

    /// The new-todo form was submitted: add what was typed and clear the
    /// field. Blank text is ignored and left in place.
    pub fn submit_new(&self) {
        if self.add(&self.input_text.get()) >= 0 {
            self.input_text.set(String::new());
        }
    }

    pub fn start_edit(&self, id: i32) {
        let text = self
            .list
            .get()
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.text.clone());
        if let Some(text) = text {
            self.editing_id.set(Some(id));
            self.edit_text.set(text);
        }
    }

    pub fn set_edit_text(&self, text: &str) {
        self.edit_text.set(text.to_string());
    }

    /// A key was pressed in the edit field: Enter commits, Escape cancels.
    pub fn edit_key(&self, key: &str) {
        match key {
            "Enter" => self.commit_edit(),
            "Escape" => self.cancel_edit(),
            _ => {}
        }
    }

    /// Applies the edit in progress, if there is one.
    pub fn commit_edit(&self) {
        if let Some(id) = self.editing_id.get() {
            self.edit(id, &self.edit_text.get());
        }
        self.cancel_edit();
    }

    pub fn cancel_edit(&self) {
        self.editing_id.set(None);
        self.edit_text.set(String::new());
    }

    /// Edits a todo's text (trimmed). An empty result removes the todo —
    /// the official TodoMVC behavior.
    pub fn edit(&self, id: i32, text: &str) {
        let text = text.trim().to_string();
        let mut todos = self.list.get();
        if text.is_empty() {
            todos.retain(|t| t.id != id);
        } else if let Some(t) = todos.iter_mut().find(|t| t.id == id) {
            t.text = text;
        }
        self.list.set(todos);
        self.mark_dirty();
    }

    pub fn toggle(&self, id: i32) {
        let mut todos = self.list.get();
        if let Some(t) = todos.iter_mut().find(|t| t.id == id) {
            t.done = !t.done;
        }
        self.list.set(todos);
        self.mark_dirty();
    }

    pub fn remove(&self, id: i32) {
        let mut todos = self.list.get();
        todos.retain(|t| t.id != id);
        self.list.set(todos);
        self.mark_dirty();
    }

    /// Official TodoMVC semantics: when nothing is left, uncheck everything;
    /// otherwise mark everything done.
    pub fn toggle_all(&self) {
        let mut todos = self.list.get();
        if todos.iter().all(|t| t.done) {
            for t in todos.iter_mut() {
                t.done = false;
            }
        } else {
            for t in todos.iter_mut() {
                t.done = true;
            }
        }
        self.list.set(todos);
        self.mark_dirty();
    }

    pub fn clear_completed(&self) {
        let mut todos = self.list.get();
        todos.retain(|t| !t.done);
        self.list.set(todos);
        self.mark_dirty();
    }

    pub fn set_filter(&self, filter: Filter) {
        self.filter.set(filter);
    }

    /// Stops the autosave effect. In-flight async work completes naturally.
    pub fn dispose(&self) {
        self.autosave.stop();
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

impl Default for Todos {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// In-memory storage fake shared between the closures.
    fn mem_storage(fail_save: bool) -> (Storage, Arc<std::sync::Mutex<Vec<Todo>>>) {
        let mem = Arc::new(std::sync::Mutex::new(Vec::new()));
        let load_mem = mem.clone();
        let save_mem = mem.clone();
        let storage = Storage {
            load: Arc::new(move || {
                let m = load_mem.clone();
                Box::pin(async move { Ok(m.lock().unwrap().clone()) })
            }),
            save: Arc::new(move |todos: Vec<Todo>| {
                let m = save_mem.clone();
                Box::pin(async move {
                    if fail_save {
                        Err("disk full".to_string())
                    } else {
                        *m.lock().unwrap() = todos;
                        Ok(())
                    }
                })
            }),
        };
        (storage, mem)
    }

    fn sample() -> Vec<Todo> {
        vec![
            Todo {
                id: 1,
                text: "a".into(),
                done: false,
            },
            Todo {
                id: 2,
                text: "b".into(),
                done: true,
            },
            Todo {
                id: 3,
                text: "c".into(),
                done: false,
            },
        ]
    }

    #[test]
    fn crud_and_memos() {
        let (storage, _) = mem_storage(false);
        let t = Todos::with_storage(storage);
        futures::executor::block_on(t.load()).unwrap();
        assert!(t.list().get().is_empty());

        let a = t.add("write the adapter");
        let b = t.add("ship it");
        assert_eq!((a, b), (1, 2));
        assert_eq!(t.remaining().get(), 2);
        assert_eq!(t.completed().get(), 0);

        t.toggle(a);
        assert_eq!(t.remaining().get(), 1);
        assert_eq!(t.completed().get(), 1);
        assert_eq!(t.visible().get().len(), 2); // All

        t.set_filter(Filter::Active);
        assert_eq!(t.visible().get().len(), 1);
        t.set_filter(Filter::Completed);
        assert_eq!(t.visible().get().len(), 1);
        t.set_filter(Filter::All);

        t.edit(b, "ship it v2");
        assert_eq!(t.list().get()[1].text, "ship it v2");
        t.edit(b, "   "); // empty edit removes the todo
        assert_eq!(t.list().get().len(), 1);

        t.remove(a);
        assert!(t.list().get().is_empty());
    }

    #[test]
    fn form_and_edit_events() {
        let (storage, _) = mem_storage(false);
        let t = Todos::with_storage(storage);

        t.set_input_text("   ");
        t.submit_new();
        assert!(t.list().get().is_empty());
        assert_eq!(t.input_text().get(), "   ", "blank text stays in the field");

        t.set_input_text("write docs");
        t.submit_new();
        assert_eq!(t.list().get()[0].text, "write docs");
        assert_eq!(t.input_text().get(), "");

        t.start_edit(1);
        assert_eq!(t.edit_text().get(), "write docs");
        t.set_edit_text("changed my mind");
        t.edit_key("Escape");
        assert_eq!(t.list().get()[0].text, "write docs");
        assert_eq!(t.editing_id().get(), None);

        t.start_edit(1);
        t.set_edit_text("write the docs");
        t.edit_key("Enter");
        assert_eq!(t.list().get()[0].text, "write the docs");
        t.commit_edit(); // a blur after Enter has nothing left to apply
        assert_eq!(t.list().get()[0].text, "write the docs");
    }

    #[test]
    fn toggle_all_uses_official_semantics() {
        let (storage, _) = mem_storage(false);
        let t = Todos::with_storage(storage);
        let a = t.add("one");
        let b = t.add("two");

        t.toggle_all(); // nothing done yet -> all done
        assert_eq!(t.remaining().get(), 0);
        t.toggle_all(); // all done -> all unchecked
        assert_eq!(t.remaining().get(), 2);
        t.toggle(a);
        t.toggle(b);
        t.toggle_all(); // all done again -> all unchecked
        assert!(t.list().get().iter().all(|t| !t.done));
    }

    #[test]
    fn clear_completed_keeps_active_only() {
        let (storage, _) = mem_storage(false);
        let t = Todos::with_storage(storage);
        let a = t.add("keep");
        let b = t.add("done one");
        t.toggle(b);
        t.clear_completed();
        assert_eq!(
            t.list().get(),
            vec![Todo {
                id: a,
                text: "keep".into(),
                done: false
            }]
        );
    }

    #[test]
    fn autosave_writes_back_through_the_seam() {
        let (storage, mem) = mem_storage(false);
        let t = Todos::with_storage(storage);
        futures::executor::block_on(t.load()).unwrap();
        assert!(t.list().get().is_empty());

        t.add("persist me");
        crate::runtime::pump(); // effect re-runs, spawns save, save completes
        assert_eq!(
            mem.lock().unwrap().as_slice(),
            [Todo {
                id: 1,
                text: "persist me".into(),
                done: false
            }]
        );
        assert!(t.last_error().get().is_none());
    }

    #[test]
    fn load_restores_persisted_state() {
        let (storage, mem) = mem_storage(false);
        *mem.lock().unwrap() = sample();
        let t = Todos::with_storage(storage);
        futures::executor::block_on(t.load()).unwrap();
        assert_eq!(t.list().get(), sample());
        assert_eq!(t.remaining().get(), 2);
        assert_eq!(t.add("d"), 4, "new ids continue after the loaded ones");
    }

    #[test]
    fn save_failure_surfaces_in_last_error() {
        let (storage, _) = mem_storage(true);
        let t = Todos::with_storage(storage);
        futures::executor::block_on(t.load()).unwrap();
        t.add("boom");
        crate::runtime::pump();
        assert_eq!(t.last_error().get().as_deref(), Some("disk full"));
        // The list is unaffected by the failed save.
        assert_eq!(t.list().get().len(), 1);
    }

    #[test]
    fn dispose_stops_autosave_and_aborts() {
        let (storage, mem) = mem_storage(false);
        let t = Todos::with_storage(storage);
        futures::executor::block_on(t.load()).unwrap();
        t.dispose();
        t.add("after dispose");
        crate::runtime::pump();
        assert!(mem.lock().unwrap().is_empty(), "autosave must be stopped");
        assert_eq!(t.list().get().len(), 1); // state still works in memory
    }

    #[test]
    fn sample_fixture_matches_shape() {
        assert_eq!(sample().len(), 3);
    }
}
