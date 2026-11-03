//! P10: Memory view.
//!
//! Ports MemoryView.tsx, memoryGuards.ts, lib/{memory,memoryFormat}.ts. Read
//! inventory-frontend.md §2.2 Memory and crates/hangar-app/PACKAGES.md. Files
//! of this directory: `mod` (the state and its messages), `view`, `guards`
//! (the cap, the title clash, the sort) and `format` (paths, signers, dates).
//!
//! The memory is one store for the whole app, shared by every workspace.
//! Agents write to it over MCP and the window reads the very same file through
//! hangar-core's `memory_*` functions, so both sides always see one truth;
//! `CoreEvent::MemoryChanged` tells this view when the other side wrote.
//!
//! The view is a main-view region: it exists while `ui.view` is `Memory`, as
//! the React component did, so it loads when the view is chosen and forgets
//! everything (the search, the open entries, a half-typed composer) when
//! another view is.

mod format;
mod guards;
mod view;

use std::collections::HashSet;

use iced::widget::{Id, operation, text_editor};
use iced::{Element, Subscription, Task};

use hangar_core::memory::{self, MemoryEntry, MemoryPatch, NewMemory};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Region, View};
use crate::state::Model;

use guards::{MemorySort, find_title_clash, parse_tags};

/// The search field, to give it the keyboard (`focus_entry`, "Clear search").
pub(crate) fn search_id() -> Id {
    Id::new("memory-search")
}

/// The composer's title field, focused when the composer opens.
pub(crate) fn composer_id() -> Id {
    Id::new("memory-composer-title")
}

/// The fields of the composer and of an entry being edited. The content is a
/// text editor's state; the tags stay text until they are saved.
#[derive(Debug, Default)]
pub struct Draft {
    pub title: String,
    pub content: text_editor::Content,
    pub tags: String,
}

impl Draft {
    fn of(entry: &MemoryEntry) -> Draft {
        Draft {
            title: entry.title.clone(),
            content: text_editor::Content::with_text(&entry.content),
            tags: entry.tags.join(", "),
        }
    }

    /// Whether there is something to save: a title and a content that are not
    /// blank.
    pub fn is_complete(&self) -> bool {
        !self.title.trim().is_empty() && !self.content.text().trim().is_empty()
    }
}

/// The entry being edited, with its fields.
#[derive(Debug)]
pub struct Editing {
    pub id: String,
    pub draft: Draft,
}

/// The package's UI state; lives in `App.pkg.memory`.
#[derive(Debug, Default)]
pub struct State {
    /// `None` until the first read answers: a slow read is not an empty
    /// memory.
    pub entries: Option<Vec<MemoryEntry>>,
    /// What a search looks through, one per entry, in the same order.
    pub haystacks: Vec<String>,
    /// A failed read or a refused write. Never cleared by the passing of time:
    /// a successful load or the next action retires it.
    pub error: Option<String>,
    pub query: String,
    pub composing: bool,
    pub draft: Draft,
    /// The composer's title is taken: the next press of the button replaces
    /// that entry.
    pub confirming_replace: bool,
    pub sort: MemorySort,
    /// Entries whose full content is on screen.
    pub open: HashSet<String>,
    pub editing: Option<Editing>,
    /// Delete asks twice, like the destructive actions in the settings do.
    pub armed: Option<String>,
}

/// What the composer would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
}

/// What a press of the composer's button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateStep {
    /// A title or a content is blank: the button is disabled, nothing to do.
    Nothing,
    /// The title is taken and the person has not said to replace it yet: the
    /// button now says "Replace", and the next press writes.
    Asked,
    Write(NewEntry),
}

impl State {
    /// Decides a press of "Create".
    ///
    /// Creating is an upsert: a taken title would overwrite that entry. Ask
    /// inline first, with the entries already loaded: no native dialog.
    pub fn create_step(&mut self) -> CreateStep {
        let title = self.draft.title.trim().to_owned();
        let content = self.draft.content.text().trim().to_owned();
        if title.is_empty() || content.is_empty() {
            return CreateStep::Nothing;
        }
        let clash = self
            .entries
            .as_deref()
            .and_then(|entries| find_title_clash(entries, &title));
        if clash.is_some() && !self.confirming_replace {
            self.confirming_replace = true;
            return CreateStep::Asked;
        }
        self.confirming_replace = false;
        CreateStep::Write(NewEntry {
            title,
            content,
            tags: parse_tags(&self.draft.tags),
        })
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.memory`. Nothing reads the memory from outside this view.
#[derive(Debug, Default)]
pub struct Live;

/// What a write did, for the state to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    Created,
    Updated,
    Deleted,
}

/// The package's messages, routed by the shell as `Message::Memory(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// `memory_load` answered.
    Loaded(Result<Vec<MemoryEntry>, String>),
    /// A create, update or delete answered.
    Wrote(Result<Wrote, String>),

    Query(String),
    /// The cross of the search field, Escape in it, or "Clear search".
    ClearQuery,
    FlipSort,

    /// The "New" button: opens the composer, or closes it.
    ToggleComposer,
    StartComposing,
    CancelCompose,
    DraftTitle(String),
    DraftContent(text_editor::Action),
    DraftTags(String),
    Create,

    Toggle(String),
    StartEdit(String),
    CancelEdit,
    EditTitle(String),
    EditContent(text_editor::Action),
    EditTags(String),
    SaveEdit,
    Arm(String),
    Disarm,
    Delete(String),
}

/// `memory_load` off the UI thread.
fn load(sv: &Services) -> Task<Message> {
    let host = sv.core().host().clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                memory::memory_load(&host).map(|memory| memory.entries)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|loaded| loaded)
        },
        |result| Message::Memory(Msg::Loaded(result)),
    )
}

/// Runs a write off the UI thread. Every write funnels through here, so a
/// refused one (a limit, a missing title) is always shown.
fn write(
    sv: &Services,
    run: impl FnOnce(&hangar_core::Host) -> Result<Wrote, String> + Send + 'static,
) -> Task<Message> {
    let host = sv.core().host().clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || run(&host))
                .await
                .map_err(|error| error.to_string())
                .and_then(|wrote| wrote)
        },
        |result| Message::Memory(Msg::Wrote(result)),
    )
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Loaded(result) => {
            match result {
                Ok(entries) => {
                    st.haystacks = entries.iter().map(guards::haystack).collect();
                    st.entries = Some(entries);
                    // A load that works retires the failure of the last one:
                    // without this, one transient lock leaves its banner over
                    // a healthy list for the rest of the session.
                    st.error = None;
                }
                // `memory_load` fails loudly when memory.json exists but
                // cannot be read; showing it keeps an unreadable memory from
                // looking like an empty one.
                Err(error) => {
                    st.haystacks.clear();
                    st.entries = Some(Vec::new());
                    st.error = Some(error);
                }
            }
            Task::none()
        }
        Msg::Wrote(result) => match result {
            Ok(wrote) => {
                match wrote {
                    Wrote::Created => {
                        st.draft = Draft::default();
                        st.composing = false;
                    }
                    Wrote::Updated => st.editing = None,
                    Wrote::Deleted => {}
                }
                load(sv)
            }
            // A refused write says so where it happened and changes nothing
            // else: the list on screen is still what the core holds.
            Err(error) => {
                st.error = Some(error);
                Task::none()
            }
        },

        Msg::Query(query) => {
            st.query = query;
            Task::none()
        }
        Msg::ClearQuery => {
            st.query.clear();
            operation::focus(search_id())
        }
        Msg::FlipSort => {
            st.sort = st.sort.flipped();
            Task::none()
        }

        Msg::ToggleComposer if st.composing => {
            st.composing = false;
            Task::none()
        }
        Msg::ToggleComposer | Msg::StartComposing => {
            st.composing = true;
            st.error = None;
            operation::focus(composer_id())
        }
        Msg::CancelCompose => {
            st.composing = false;
            st.draft = Draft::default();
            st.confirming_replace = false;
            st.error = None;
            Task::none()
        }
        Msg::DraftTitle(title) => {
            st.draft.title = title;
            st.confirming_replace = false;
            Task::none()
        }
        Msg::DraftContent(action) => {
            st.draft.content.perform(action);
            Task::none()
        }
        Msg::DraftTags(tags) => {
            st.draft.tags = tags;
            Task::none()
        }
        Msg::Create => {
            let CreateStep::Write(entry) = st.create_step() else {
                return Task::none();
            };
            st.error = None;
            let workspace = m
                .active_workspace()
                .map(|ws| ws.cwd.clone())
                .unwrap_or_default();
            write(sv, move |host| {
                memory::memory_create(
                    host,
                    NewMemory {
                        title: entry.title,
                        content: entry.content,
                        tags: entry.tags,
                        workspace,
                        agent: "user".into(),
                    },
                )
                .map(|_| Wrote::Created)
            })
        }

        Msg::Toggle(id) => {
            if !st.open.remove(&id) {
                st.open.insert(id);
            }
            Task::none()
        }
        Msg::StartEdit(id) => {
            let entry = st
                .entries
                .as_deref()
                .and_then(|entries| entries.iter().find(|entry| entry.id == id));
            if let Some(entry) = entry {
                st.editing = Some(Editing {
                    id,
                    draft: Draft::of(entry),
                });
            }
            st.armed = None;
            st.error = None;
            Task::none()
        }
        Msg::CancelEdit => {
            st.editing = None;
            Task::none()
        }
        Msg::EditTitle(title) => {
            if let Some(editing) = &mut st.editing {
                editing.draft.title = title;
            }
            Task::none()
        }
        Msg::EditContent(action) => {
            if let Some(editing) = &mut st.editing {
                editing.draft.content.perform(action);
            }
            Task::none()
        }
        Msg::EditTags(tags) => {
            if let Some(editing) = &mut st.editing {
                editing.draft.tags = tags;
            }
            Task::none()
        }
        Msg::SaveEdit => {
            let Some(editing) = &st.editing else {
                return Task::none();
            };
            let title = editing.draft.title.trim().to_owned();
            let content = editing.draft.content.text().trim().to_owned();
            if title.is_empty() || content.is_empty() {
                return Task::none();
            }
            let id = editing.id.clone();
            let tags = parse_tags(&editing.draft.tags);
            st.error = None;
            write(sv, move |host| {
                memory::memory_update(
                    host,
                    id,
                    MemoryPatch {
                        title: Some(title),
                        content: Some(content),
                        tags: Some(tags),
                    },
                )
                .map(|_| Wrote::Updated)
            })
        }
        Msg::Arm(id) => {
            st.armed = Some(id);
            Task::none()
        }
        Msg::Disarm => {
            st.armed = None;
            Task::none()
        }
        Msg::Delete(id) => {
            st.armed = None;
            st.error = None;
            write(sv, move |host| {
                memory::memory_delete(host, id).map(|()| Wrote::Deleted)
            })
        }
    }
}

pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches region `Main`, which this package
/// owns while the memory view is shown: the search field.
pub fn focus_entry(_st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    (region == Region::Main && m.ui.view == View::Memory).then(|| operation::focus(search_id()))
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
///
/// Choosing the memory view loads it, as mounting the component did; choosing
/// any other forgets what it held, as unmounting it did.
pub fn on_intent(
    st: &mut State,
    _m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        // Choosing the view that is already shown remounts nothing, so what
        // the person typed stays.
        Intent::SetView(View::Memory) => st.entries.is_none().then(|| load(sv)),
        Intent::SetView(_) => {
            *st = State::default();
            None
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it: agents write
/// through the API, and the core says so, so the window reflects their work
/// without polling.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::MemoryChanged if m.ui.view == View::Memory && st.entries.is_some() => {
            Some(load(sv))
        }
        _ => None,
    }
}

/// Region `main` in the memory view.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    view::view(st, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str) -> MemoryEntry {
        MemoryEntry {
            id: title.into(),
            title: title.into(),
            content: "body".into(),
            tags: Vec::new(),
            workspace: String::new(),
            agent: String::new(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn composing(title: &str, content: &str, taken: &[&str]) -> State {
        State {
            entries: Some(taken.iter().map(|title| entry(title)).collect()),
            composing: true,
            draft: Draft {
                title: title.into(),
                content: text_editor::Content::with_text(content),
                tags: "ci, Rust, ci".into(),
            },
            ..State::default()
        }
    }

    #[test]
    fn writes_nothing_while_the_title_or_the_text_is_blank() {
        assert_eq!(
            composing("  ", "text", &[]).create_step(),
            CreateStep::Nothing
        );
        assert_eq!(
            composing("Title", " \n ", &[]).create_step(),
            CreateStep::Nothing
        );
    }

    #[test]
    fn writes_at_once_when_the_title_is_free() {
        let mut st = composing("  Build steps ", " run it ", &["Other"]);
        assert_eq!(
            st.create_step(),
            CreateStep::Write(NewEntry {
                title: "Build steps".into(),
                content: "run it".into(),
                tags: vec!["ci".into(), "Rust".into()],
            })
        );
        assert!(!st.confirming_replace);
    }

    #[test]
    fn asks_before_replacing_a_taken_title_and_writes_on_the_second_press() {
        let mut st = composing("build STEPS", "new text", &["Build steps"]);
        assert_eq!(st.create_step(), CreateStep::Asked);
        assert!(st.confirming_replace);
        assert!(matches!(st.create_step(), CreateStep::Write(_)));
        assert!(!st.confirming_replace);
    }

    #[test]
    fn editing_the_title_takes_the_question_back() {
        let mut st = composing("Build steps", "text", &["Build steps"]);
        assert_eq!(st.create_step(), CreateStep::Asked);
        // The title changes: the message handler lowers the flag, as the TS
        // `onChange` did, and the clash is asked about afresh.
        st.draft.title = "Build steps 2".into();
        st.confirming_replace = false;
        assert!(matches!(st.create_step(), CreateStep::Write(_)));
    }
}
