//! The TodoMVC model: state, CRUD and everything computed from the state.
//!
//! The behaviour is that of the official Leptos TodoMVC, with the view
//! removed:
//!
//! - add on non-empty trimmed text; an edit to empty text removes the todo;
//!   `toggle_all` unchecks everything when nothing is left and checks all
//!   otherwise;
//! - everything the view shows is computed here (`remaining`, `visible`, the
//!   filter bar, and every yes/no question the view has), so that it never
//!   compares, counts or formats anything itself;
//! - what the view would otherwise keep for itself is state here too: the
//!   new-todo text, which row is being edited and the text of that edit;
//! - persistence: loading and saving are Leptos `Action`s that `load()` and
//!   `save()` dispatch, an autosave `Effect` dispatches a save whenever the
//!   list is dirtied, and `saving` / `last_error` are derived from them.

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

/// One button of the filter bar, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FilterOption {
    pub filter: Filter,
    pub label: String,
    pub selected: bool,
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

#[derive(PlumModel)]
pub struct Todos {
    #[plum(watch, js = "Todos")]
    list: RwSignal<Vec<Todo>>,
    next_id: RwSignal<i32>,
    #[plum(watch)]
    filter: RwSignal<Filter>,
    /// Every filter, in display order, with the current one selected.
    #[plum(watch)]
    filters: Memo<Vec<FilterOption>>,
    #[plum(watch)]
    remaining: Memo<i32>,
    #[plum(watch)]
    completed: Memo<i32>,
    /// There are no todos at all.
    #[plum(watch)]
    is_empty: Memo<bool>,
    #[plum(watch)]
    has_completed: Memo<bool>,
    #[plum(watch)]
    all_done: Memo<bool>,
    #[plum(watch)]
    visible: Memo<Vec<Todo>>,
    /// The current filter lets at least one todo through.
    #[plum(watch)]
    has_visible: Memo<bool>,
    /// True while a save is in flight.
    #[plum(watch)]
    saving: Memo<bool>,
    /// What went wrong in the last save, or else in the load.
    #[plum(watch)]
    last_error: Memo<Option<String>>,
    #[plum(watch)]
    has_error: Memo<bool>,
    /// Text of the new-todo field. The view writes it with `inputText.set`.
    #[plum(watch, set)]
    input_text: RwSignal<String>,
    /// The todo being edited, if any. Views ask `is_editing(id)`.
    editing_id: RwSignal<Option<i32>>,
    /// The text of that edit, written by the view the same way.
    #[plum(watch, set)]
    edit_text: RwSignal<String>,
    dirty: RwSignal<bool>,
    autosave: Effect<LocalStorage>,
    load_action: Action<(), Result<(), String>>,
    save_action: Action<Vec<Todo>, Result<(), String>>,
}

#[plum_actions]
impl Todos {
    /// Creates the model on the default storage backend for the target,
    /// keeping the list under `storage_key`.
    pub fn new(storage_key: &str) -> Self {
        Self::with_storage(crate::storage::local_storage(storage_key))
    }

    /// Creates the model with an explicit storage backend (used by tests).
    pub fn with_storage(storage: Storage) -> Self {
        crate::runtime::ensure_init();

        let list: RwSignal<Vec<Todo>> = RwSignal::new(Vec::new());
        let next_id = RwSignal::new(1);
        let filter = RwSignal::new(Filter::All);
        let input_text = RwSignal::new(String::new());
        let editing_id = RwSignal::new(None);
        let edit_text = RwSignal::new(String::new());

        let filters = Memo::new(move |_| {
            let current = filter.get();
            [
                (Filter::All, "All"),
                (Filter::Active, "Active"),
                (Filter::Completed, "Completed"),
            ]
            .into_iter()
            .map(|(option, label)| FilterOption {
                filter: option,
                label: label.to_string(),
                selected: option == current,
            })
            .collect()
        });
        let remaining = Memo::new(move |_| list.get().iter().filter(|t| !t.done).count() as i32);
        let completed = Memo::new(move |_| list.get().iter().filter(|t| t.done).count() as i32);
        let is_empty = Memo::new(move |_| list.get().is_empty());
        let has_completed = Memo::new(move |_| completed.get() > 0);
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
        let has_visible = Memo::new(move |_| !visible.get().is_empty());

        let loaded = RwSignal::new(false);
        let dirty = RwSignal::new(false);

        // `new_local` because the wasm storage futures are not `Send`.
        let load_action = Action::new_local({
            let storage = storage.clone();
            move |_: &()| {
                let loading = (storage.load)();
                async move {
                    let todos = loading.await?;
                    let max_id = todos.iter().map(|t| t.id).max().unwrap_or(0);
                    next_id.set(max_id + 1);
                    list.set(todos);
                    loaded.set(true);
                    Ok(())
                }
            }
        });
        let save_action = Action::new_local(move |todos: &Vec<Todo>| (storage.save)(todos.clone()));

        let saving = save_action.pending();
        let (loaded_value, saved_value) = (load_action.value(), save_action.value());
        let last_error = Memo::new(move |_| {
            let failure = |value: Option<Result<(), String>>| value.and_then(Result::err);
            failure(saved_value.get()).or_else(|| failure(loaded_value.get()))
        });
        let has_error = Memo::new(move |_| last_error.get().is_some());

        // Autosave: when the list was dirtied after a successful load and no
        // save is in flight, dispatch one. It runs again when `saving` clears.
        let autosave = Effect::new(move |_prev: Option<()>| {
            let todos = list.get();
            if loaded.get() && dirty.get() && !saving.get() {
                dirty.set(false);
                save_action.dispatch_local(todos);
            }
        });

        Self {
            list,
            next_id,
            filter,
            filters,
            remaining,
            completed,
            is_empty,
            has_completed,
            all_done,
            visible,
            has_visible,
            saving,
            last_error,
            has_error,
            input_text,
            editing_id,
            edit_text,
            dirty,
            autosave,
            load_action,
            save_action,
        }
    }

    /// Applies a change to the list and marks it for saving.
    fn update_list(&self, change: impl FnOnce(&mut Vec<Todo>)) {
        self.list.update(change);
        self.dirty.set(true);
    }

    /// Starts loading the list from storage. `todos` changes when it arrives.
    pub fn load(&self) {
        self.load_action.dispatch_local(());
    }

    /// Starts a save, unless one is running. `saving` and `last_error` report
    /// how it goes.
    pub fn save(&self) {
        if !self.saving.get() {
            self.save_action.dispatch_local(self.list.get());
        }
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
        self.update_list(|todos| {
            todos.push(Todo {
                id,
                text: text.to_string(),
                done: false,
            })
        });
        id
    }

    /// The new-todo form was submitted: add what was typed and clear the
    /// field. Blank text is ignored and left in place.
    pub fn submit_new(&self) {
        if self.add(&self.input_text.get()) >= 0 {
            self.input_text.set(String::new());
        }
    }

    /// Whether the todo with this id is the one being edited. A store per id:
    /// a row subscribes to its own answer and to nothing else.
    #[plum(watch)]
    pub fn is_editing(&self, id: i32) -> Memo<bool> {
        let editing_id = self.editing_id;
        Memo::new(move |_| editing_id.get() == Some(id))
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
        self.update_list(|todos| {
            if text.is_empty() {
                todos.retain(|t| t.id != id);
            } else if let Some(t) = todos.iter_mut().find(|t| t.id == id) {
                t.text = text;
            }
        });
    }

    pub fn toggle(&self, id: i32) {
        self.update_list(|todos| {
            if let Some(t) = todos.iter_mut().find(|t| t.id == id) {
                t.done = !t.done;
            }
        });
    }

    pub fn remove(&self, id: i32) {
        self.update_list(|todos| todos.retain(|t| t.id != id));
    }

    /// Official TodoMVC semantics: when nothing is left, uncheck everything;
    /// otherwise mark everything done.
    pub fn toggle_all(&self) {
        self.update_list(|todos| {
            let done = !todos.iter().all(|t| t.done);
            for t in todos.iter_mut() {
                t.done = done;
            }
        });
    }

    pub fn clear_completed(&self) {
        self.update_list(|todos| todos.retain(|t| !t.done));
    }

    pub fn set_filter(&self, filter: Filter) {
        self.filter.set(filter);
    }

    /// Stops the autosave effect. In-flight async work completes naturally.
    pub fn dispose(&self) {
        self.autosave.stop();
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
        t.load();
        crate::runtime::pump();
        assert!(t.list.get().is_empty());

        let a = t.add("write the adapter");
        let b = t.add("ship it");
        assert_eq!((a, b), (1, 2));
        assert_eq!(t.remaining.get(), 2);
        assert_eq!(t.completed.get(), 0);

        t.toggle(a);
        assert_eq!(t.remaining.get(), 1);
        assert_eq!(t.completed.get(), 1);
        assert_eq!(t.visible.get().len(), 2); // All

        t.set_filter(Filter::Active);
        assert_eq!(t.visible.get().len(), 1);
        let selected: Vec<_> = t.filters.get().into_iter().filter(|o| o.selected).collect();
        assert_eq!((selected.len(), selected[0].label.as_str()), (1, "Active"));
        t.set_filter(Filter::Completed);
        assert_eq!(t.visible.get().len(), 1);
        t.set_filter(Filter::All);

        t.edit(b, "ship it v2");
        assert_eq!(t.list.get()[1].text, "ship it v2");
        t.edit(b, "   "); // empty edit removes the todo
        assert_eq!(t.list.get().len(), 1);

        t.remove(a);
        assert!(t.list.get().is_empty());
        assert!(t.is_empty.get() && !t.has_visible.get() && !t.has_completed.get());
    }

    #[test]
    fn form_and_edit_events() {
        let (storage, _) = mem_storage(false);
        let t = Todos::with_storage(storage);

        t.input_text.set("   ".into());
        t.submit_new();
        assert!(t.list.get().is_empty());
        assert_eq!(t.input_text.get(), "   ", "blank text stays in the field");

        t.input_text.set("write docs".into());
        t.submit_new();
        assert_eq!(t.list.get()[0].text, "write docs");
        assert_eq!(t.input_text.get(), "");

        t.start_edit(1);
        assert!(t.is_editing(1).get() && !t.is_editing(2).get());
        assert_eq!(t.edit_text.get(), "write docs");
        t.edit_text.set("changed my mind".into());
        t.edit_key("Escape");
        assert_eq!(t.list.get()[0].text, "write docs");
        assert_eq!(t.editing_id.get(), None);
        assert!(!t.is_editing(1).get());

        t.start_edit(1);
        t.edit_text.set("write the docs".into());
        t.edit_key("Enter");
        assert_eq!(t.list.get()[0].text, "write the docs");
        t.commit_edit(); // a blur after Enter has nothing left to apply
        assert_eq!(t.list.get()[0].text, "write the docs");
    }

    #[test]
    fn toggle_all_uses_official_semantics() {
        let (storage, _) = mem_storage(false);
        let t = Todos::with_storage(storage);
        let a = t.add("one");
        let b = t.add("two");

        t.toggle_all(); // nothing done yet -> all done
        assert_eq!(t.remaining.get(), 0);
        t.toggle_all(); // all done -> all unchecked
        assert_eq!(t.remaining.get(), 2);
        t.toggle(a);
        t.toggle(b);
        t.toggle_all(); // all done again -> all unchecked
        assert!(t.list.get().iter().all(|t| !t.done));
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
            t.list.get(),
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
        t.load();
        crate::runtime::pump();
        assert!(t.list.get().is_empty());

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
        assert!(t.last_error.get().is_none());
    }

    #[test]
    fn load_restores_persisted_state() {
        let (storage, mem) = mem_storage(false);
        *mem.lock().unwrap() = sample();
        let t = Todos::with_storage(storage);
        t.load();
        crate::runtime::pump();
        assert_eq!(t.list.get(), sample());
        assert_eq!(t.remaining.get(), 2);
        assert_eq!(t.add("d"), 4, "new ids continue after the loaded ones");
    }

    #[test]
    fn save_failure_surfaces_in_last_error() {
        let (storage, _) = mem_storage(true);
        let t = Todos::with_storage(storage);
        t.load();
        crate::runtime::pump();
        t.add("boom");
        crate::runtime::pump();
        assert_eq!(t.last_error.get().as_deref(), Some("disk full"));
        // The list is unaffected by the failed save.
        assert_eq!(t.list.get().len(), 1);
    }

    #[test]
    fn dispose_stops_autosave_and_aborts() {
        let (storage, mem) = mem_storage(false);
        let t = Todos::with_storage(storage);
        t.load();
        crate::runtime::pump();
        t.dispose();
        t.add("after dispose");
        crate::runtime::pump();
        assert!(mem.lock().unwrap().is_empty(), "autosave must be stopped");
        assert_eq!(t.list.get().len(), 1); // state still works in memory
    }

    #[test]
    fn sample_fixture_matches_shape() {
        assert_eq!(sample().len(), 3);
    }
}
