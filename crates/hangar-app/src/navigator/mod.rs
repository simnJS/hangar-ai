//! P6: Navigator (`Sidebar.tsx`, `NavigatorModel.ts`, `NavigatorStatus.tsx`,
//! `NavigatorPlan.tsx`, `NavigatorMenu.tsx`, `lib/folders.ts`).
//!
//! The workspaces, filed or not in folders, each with a word on what its
//! agents are doing: a source list in the manner of Xcode's. The workspaces in
//! no folder come first; a folder is a heading over its own, level with them
//! rather than indented, so that a heading never passes for a project. The
//! agents themselves are listed where they work: pane headers, inspector,
//! status bar, palette.
//!
//! It is a tree to the keyboard: one stop, arrows to move, Right and Left to
//! open and fold a folder, Enter to go, F2 to rename, the menu key for the
//! rest. The keys are read while the shell says the navigator has the focus
//! (F6, or a click in it).
//!
//! Gaps with the web navigator, both iced's: buttons take no keyboard focus and
//! a text field reports no blur, so a rename is settled by Enter, Escape, or a
//! press that nothing else took.

pub mod drag;
pub mod editor;
pub mod geometry;
pub mod menu;
pub mod model;
pub mod plan;
pub mod reads;
pub mod skin;
pub mod sleep;
pub mod status;
mod view;

use std::time::Duration;

use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::widget::Id;
use iced::{Element, Event, Point, Subscription, Task, Vector, mouse, window};

use hangar_i18n::{Key as Text, Translator};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Focus, Region};
use crate::state::types::Workspace;
use crate::state::{FolderPatch, Model, PaneId, StoreAction, WorkspacePatch, WsId};

use drag::{Drag, DragItem, DragKind, Hit};
use model::{NavInput, NavRow, RowKind, build_nav_tree, row_key};

/// The note on why « Open with VS Code » did nothing is gone after this.
const NOTE_FOR: Duration = Duration::from_secs(6);

/// The rename field, and the filter field.
fn rename_id() -> Id {
    Id::new("hangar-nav-rename")
}

fn filter_id() -> Id {
    Id::new("hangar-nav-filter")
}

/// The scrollable list, which the keyboard scrolls to the row it is on.
fn list_id() -> Id {
    Id::new("hangar-nav-list")
}

/// A name being typed over a row's (`renaming`, `draft`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    kind: RowKind,
    id: String,
    draft: String,
}

/// The context menu on screen: whose, where, and whether a key opened it
/// (`MenuState`).
#[derive(Debug, Clone, PartialEq)]
pub struct MenuState {
    kind: RowKind,
    id: String,
    /// The pointer's offset from the row's corner.
    at: Vector,
    from_keyboard: bool,
}

/// The package's UI state; lives in `App.pkg.navigator`.
#[derive(Debug)]
pub struct State {
    /// The filter field, as typed.
    query: String,
    /// Keep only the workspaces with an agent that wants you.
    attention_only: bool,
    /// The row the keyboard is on, which is the tree's one stop.
    cursor: Option<String>,
    rename: Option<Rename>,
    menu: Option<MenuState>,
    /// The row under the pointer.
    hover: Option<String>,
    drag: Option<Drag>,
    /// One line on why « Open with VS Code » did nothing, and the id its
    /// expiry names.
    note: Option<(u64, String)>,
    note_serial: u64,
    /// What the system said of the shells of the workspace whose menu is open,
    /// for its « Put to sleep » line.
    probe: Option<sleep::Probe>,
    /// Where the list is scrolled to and how tall it is.
    scroll: (f32, f32),
}

impl Default for State {
    fn default() -> Self {
        State {
            query: String::new(),
            attention_only: false,
            cursor: None,
            rename: None,
            menu: None,
            hover: None,
            drag: None,
            note: None,
            note_serial: 0,
            probe: None,
            scroll: (0.0, 0.0),
        }
        .with_dev_view()
    }
}

impl State {
    /// `HANGAR_DEV_NAV_MENU=<workspace id>` opens that row's menu,
    /// `HANGAR_DEV_NAV_RENAME=<workspace id>` puts it in rename, and
    /// `HANGAR_DEV_NAV_DRAG=<dragged id>:<over id>:<before|after>` holds a drag
    /// of one workspace over another, to see them in a theme and a language
    /// without a pointer (debug builds only).
    fn with_dev_view(mut self) -> State {
        if !cfg!(debug_assertions) {
            return self;
        }
        if let Ok(id) = std::env::var("HANGAR_DEV_NAV_MENU") {
            self.cursor = Some(row_key(RowKind::Ws, &id));
            self.menu = Some(MenuState {
                kind: RowKind::Ws,
                id,
                at: Vector::new(24.0, view::ROW_HEIGHT + 2.0),
                from_keyboard: true,
            });
        }
        if let Ok(id) = std::env::var("HANGAR_DEV_NAV_RENAME") {
            self.rename = Some(Rename {
                kind: RowKind::Ws,
                draft: id.clone(),
                id,
            });
        }
        if let Ok(spec) = std::env::var("HANGAR_DEV_NAV_DRAG") {
            let mut parts = spec.split(':');
            if let (Some(dragged), Some(over), Some(side)) =
                (parts.next(), parts.next(), parts.next())
            {
                let mut drag = Drag::pressed(DragItem {
                    kind: DragKind::Workspace,
                    id: dragged.to_string(),
                });
                drag.moved(
                    &row_key(RowKind::Ws, dragged),
                    Point::new(0.0, 0.0),
                    view::ROW_HEIGHT,
                    None,
                );
                let y = if side == "after" { 20.0 } else { 3.0 };
                drag.moved(
                    &row_key(RowKind::Ws, over),
                    Point::new(10.0, y),
                    view::ROW_HEIGHT,
                    Some(Hit::Workspace(over.to_string())),
                );
                self.drag = Some(drag);
            }
        }
        self
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.navigator`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Navigator(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    // The filter.
    Query(String),
    ClearFilter,
    ToggleAttention,
    /// Down in the filter field: into the list.
    FilterDown,
    /// Escape in the filter field clears the text, then the toggle, then goes
    /// to the list.
    FilterEscape,

    // Rows.
    /// The pointer went down on a row: a click or the start of a drag.
    Press(DragItem),
    /// It came up on this row (`ws:<id>`, `folder:<id>`).
    Release(String),
    DoublePress(String),
    /// While dragging: the pointer moved over `row` (its coordinates), over
    /// what `hit` says.
    RowMove {
        row: String,
        point: Point,
        hit: Hit,
    },
    Hover(Option<String>),
    /// The pointer left the list.
    ListExit,
    /// A release anywhere ends a drag; Escape, a window that lost the
    /// keyboard or a pointer that left the window cancels it.
    DragEnd,
    DragCancel,

    // The context menu.
    OpenMenu {
        kind: RowKind,
        id: String,
        at: Vector,
        from_keyboard: bool,
    },
    CloseMenu,
    Menu(menu::Action),

    // Renaming in place.
    RenameInput(String),
    RenameSubmit,
    RenameCancel,
    /// A press elsewhere.
    RenameBlur,

    // The keyboard.
    EnterTree,
    Key(Key, Modifiers),

    // The rest.
    /// The list was scrolled, or resized.
    Scrolled {
        offset: f32,
        height: f32,
    },
    NewFolder,
    HideNavigator,
    /// A workspace was removed: forget its MCP scope files.
    ClearMcpScope(String),
    EditorDone(Option<String>),
    NoteExpired(u64),
    /// The system said which of `asked` shells, in the workspace whose menu is
    /// open, run something.
    SleepProbed {
        ws: WsId,
        asked: Vec<PaneId>,
        answer: Result<Vec<PaneId>, String>,
    },
}

// ---- What the list is -------------------------------------------------------------------------

/// The workspaces the list is about: the HQ is not a project, it shows apart,
/// above the list.
fn projects(m: &Model) -> Vec<&Workspace> {
    m.state()
        .workspaces
        .iter()
        .filter(|ws| ws.hq != Some(true))
        .collect()
}

/// The rows the keyboard walks, in screen order.
fn rows_of(st: &State, m: &Model) -> Vec<NavRow> {
    let projects = projects(m);
    build_nav_tree(&NavInput {
        workspaces: &projects,
        folders: &m.state().folders,
        query: &st.query,
        attention_only: st.attention_only,
        wants_you: &|pane| reads::wants_you(m, &pane.to_string()),
    })
    .rows
}

/// The row that holds the tab stop: where the keyboard was, else the active
/// workspace, else the first (`tabKey`).
fn tab_key(st: &State, m: &Model, rows: &[NavRow]) -> Option<String> {
    let has = |key: &str| rows.iter().any(|row| row.key == key);
    st.cursor
        .clone()
        .filter(|key| has(key))
        .or_else(|| {
            m.active_workspace()
                .map(|ws| row_key(RowKind::Ws, &ws.id))
                .filter(|key| has(key))
        })
        .or_else(|| rows.first().map(|row| row.key.clone()))
}

fn filtering(st: &State) -> bool {
    !st.query.trim().is_empty() || st.attention_only
}

fn apply(m: &mut Model, action: StoreAction) -> crate::state::StoreOutcome {
    m.apply(action)
}

fn toggle_folder(st: &State, m: &mut Model, id: &str) {
    // Under a filter a folder is held open: folding it would hide the matches.
    if filtering(st) {
        return;
    }
    let Some(folder) = m.state().folders.iter().find(|folder| folder.id == id) else {
        return;
    };
    let collapsed = !folder.collapsed;
    apply(
        m,
        StoreAction::UpdateFolder(
            id.to_string(),
            FolderPatch::new(move |folder| folder.collapsed = collapsed),
        ),
    );
}

fn name_of(m: &Model, kind: RowKind, id: &str) -> Option<String> {
    let state = m.state();
    match kind {
        RowKind::Folder => state
            .folders
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.name.clone()),
        RowKind::Ws => state
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.name.clone()),
    }
}

fn start_rename(st: &mut State, kind: RowKind, id: String, name: String) -> Task<Message> {
    // A name being typed over another is settled first.
    st.cursor = Some(row_key(kind, &id));
    st.rename = Some(Rename {
        kind,
        id,
        draft: name,
    });
    let focus = iced::widget::operation::focus(rename_id());
    // A folder's name is a placeholder until typed over.
    if kind == RowKind::Folder {
        Task::batch([focus, iced::widget::operation::select_all(rename_id())])
    } else {
        focus
    }
}

/// Settles a rename: saves a name that says something, and gives the row the
/// keyboard back when a key ended it (`finishRename`).
fn finish_rename(st: &mut State, m: &mut Model, save: bool) {
    let Some(rename) = st.rename.take() else {
        return;
    };
    let name = rename.draft.trim().to_string();
    if save && !name.is_empty() {
        match rename.kind {
            RowKind::Folder => {
                apply(
                    m,
                    StoreAction::UpdateFolder(
                        rename.id.clone(),
                        FolderPatch::new(move |folder| folder.name = name.clone()),
                    ),
                );
            }
            RowKind::Ws => {
                apply(
                    m,
                    StoreAction::UpdateWorkspace(
                        rename.id.clone(),
                        WorkspacePatch::new(move |ws| ws.name = name.clone()),
                    ),
                );
            }
        }
    }
}

/// A new folder opens straight into its name, which is the first thing it
/// needs. Returns its id, if the store made one.
fn create_folder(st: &mut State, m: &mut Model) -> (Option<String>, Task<Message>) {
    let name = Translator::new(m.locale())
        .t(Text::SidebarFolderDefault)
        .into_owned();
    let outcome = apply(m, StoreAction::AddFolder(name.clone()));
    match outcome.folder {
        Some(id) => {
            let task = start_rename(st, RowKind::Folder, id.clone(), name);
            (Some(id), task)
        }
        None => (None, Task::none()),
    }
}

// ---- Update -----------------------------------------------------------------------------------

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        // The one thing that needs core: the copies of a removed workspace's
        // MCP servers' entries go with it (`lib/mcpScope`).
        Msg::ClearMcpScope(id) => {
            let host = sv.core().host().clone();
            Task::perform(
                async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        hangar_core::mcp_scope::mcp_scope_clear(&host, id)
                    })
                    .await;
                },
                |()| Message::Noop,
            )
        }
        msg => {
            let before = st.menu.clone();
            let task = step(st, m, msg);
            if st.menu == before {
                return task;
            }
            // A menu opened, moved or closed: what was asked of the system for
            // the last one is of no use to this one.
            st.probe = None;
            match &st.menu {
                Some(MenuState {
                    kind: RowKind::Ws,
                    id,
                    ..
                }) => match sleep::request(m, id) {
                    Some(shells) => Task::batch([task, ask_shells(sv, id.clone(), shells)]),
                    None => task,
                },
                _ => task,
            }
        }
    }
}

/// Asks the system which of a workspace's shells run something, as its menu
/// opens (`sleep.check`); the answer comes back as `Msg::SleepProbed`.
fn ask_shells(sv: &Services, ws: WsId, shells: Vec<PaneId>) -> Task<Message> {
    let manager = sv.core().pty().clone();
    let asked = shells.clone();
    Task::perform(
        // A remote workspace's shells are taken as busy: its node is not
        // asked.
        crate::nodes::remote::has_children(manager, shells),
        move |answer| {
            Message::Navigator(Msg::SleepProbed {
                ws: ws.clone(),
                asked: asked.clone(),
                answer,
            })
        },
    )
}

/// Everything else a message does, which touches only the model and the
/// state.
fn step(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    let intent = |intent: Intent| Task::done(Message::Intent(intent));
    match msg {
        Msg::ClearMcpScope(_) => Task::none(),
        Msg::Query(query) => {
            st.query = query;
            Task::none()
        }
        Msg::ClearFilter => {
            st.query.clear();
            st.attention_only = false;
            iced::widget::operation::focus(filter_id())
        }
        Msg::ToggleAttention => {
            st.attention_only = !st.attention_only;
            Task::none()
        }
        Msg::FilterDown => {
            let rows = rows_of(st, m);
            st.cursor = tab_key(st, m, &rows);
            Task::batch([
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::unfocus(),
                ),
                intent(Intent::FocusRegion(Region::Navigator)),
            ])
        }
        Msg::FilterEscape => {
            // First the text, then the toggle; an empty field gives the focus
            // back to the list.
            if !st.query.is_empty() {
                st.query.clear();
                Task::none()
            } else if st.attention_only {
                st.attention_only = false;
                Task::none()
            } else {
                step(st, m, Msg::FilterDown)
            }
        }

        Msg::Press(item) => {
            // A press elsewhere settles a rename (the field has no blur).
            let settle = st.rename.is_some();
            if settle {
                finish_rename(st, m, true);
            }
            st.cursor = Some(row_key(
                match item.kind {
                    DragKind::Workspace => RowKind::Ws,
                    DragKind::Folder => RowKind::Folder,
                },
                &item.id,
            ));
            st.drag = Some(Drag::pressed(item));
            intent(Intent::FocusRegion(Region::Navigator))
        }
        Msg::Release(key) => {
            let Some(drag) = st.drag.take() else {
                return Task::none();
            };
            if drag.active {
                // A drag ends in a drop, never in the click it would be.
                return drop_drag(&drag, m);
            }
            match key.split_once(':') {
                Some(("ws", id)) => intent(Intent::ActivateWorkspace(id.to_string())),
                Some(("folder", id)) => {
                    toggle_folder(st, m, id);
                    Task::none()
                }
                _ => Task::none(),
            }
        }
        Msg::DoublePress(id) => {
            let name = name_of(m, RowKind::Ws, &id).unwrap_or_default();
            // The double click also pressed and released once, which activated
            // the workspace: that is what it did in the web app as well.
            st.drag = None;
            start_rename(st, RowKind::Ws, id, name)
        }
        Msg::RowMove { row, point, hit } => {
            if let Some(drag) = &mut st.drag {
                drag.moved(&row, point, view::ROW_HEIGHT, Some(hit));
            }
            Task::none()
        }
        Msg::Hover(row) => {
            st.hover = row;
            Task::none()
        }
        Msg::ListExit => {
            st.hover = None;
            if let Some(drag) = &mut st.drag {
                drag.left_list();
            }
            Task::none()
        }
        // The global release, which comes whether or not a row took it. A drag
        // drops where the pointer last was in the list; a press that never
        // became one is left for its row's release, which may come before or
        // after this, and is inert meanwhile.
        Msg::DragEnd => match st.drag.take() {
            Some(drag) if drag.active => drop_drag(&drag, m),
            Some(mut drag) => {
                drag.ended = true;
                st.drag = Some(drag);
                Task::none()
            }
            None => Task::none(),
        },
        Msg::DragCancel => {
            st.drag = None;
            Task::none()
        }

        Msg::OpenMenu {
            kind,
            id,
            at,
            from_keyboard,
        } => {
            st.cursor = Some(row_key(kind, &id));
            st.menu = Some(MenuState {
                kind,
                id,
                at,
                from_keyboard,
            });
            Task::none()
        }
        Msg::CloseMenu => {
            st.menu = None;
            Task::none()
        }
        Msg::Menu(action) => {
            st.menu = None;
            run_action(st, m, action)
        }

        Msg::RenameInput(draft) => {
            if let Some(rename) = &mut st.rename {
                rename.draft = draft;
            }
            Task::none()
        }
        Msg::RenameSubmit => {
            finish_rename(st, m, true);
            Task::none()
        }
        Msg::RenameCancel => {
            finish_rename(st, m, false);
            Task::none()
        }
        Msg::RenameBlur => {
            finish_rename(st, m, true);
            Task::none()
        }

        Msg::EnterTree => {
            let rows = rows_of(st, m);
            st.cursor = tab_key(st, m, &rows);
            Task::none()
        }
        Msg::Key(key, modifiers) => on_key(st, m, key, modifiers),

        Msg::Scrolled { offset, height } => {
            st.scroll = (offset, height);
            Task::none()
        }
        Msg::NewFolder => create_folder(st, m).1,
        Msg::HideNavigator => intent(Intent::ToggleNavigator),
        Msg::EditorDone(note) => {
            let Some(note) = note else {
                return Task::none();
            };
            st.note_serial += 1;
            let id = st.note_serial;
            st.note = Some((id, note));
            Task::perform(async { tokio::time::sleep(NOTE_FOR).await }, move |()| {
                Message::Navigator(Msg::NoteExpired(id))
            })
        }
        Msg::NoteExpired(id) => {
            if st.note.as_ref().is_some_and(|(shown, _)| *shown == id) {
                st.note = None;
            }
            Task::none()
        }
        Msg::SleepProbed { ws, asked, answer } => {
            // An answer for a menu that has closed, or moved on, is old.
            let open = matches!(
                &st.menu,
                Some(MenuState { kind: RowKind::Ws, id, .. }) if *id == ws
            );
            if open {
                st.probe = Some(sleep::Probe::answered(ws, &asked, answer));
            }
            Task::none()
        }
    }
}

/// Commits a drag on release: only a release drops (`commit`).
fn drop_drag(drag: &Drag, m: &mut Model) -> Task<Message> {
    if let Some(target) = &drag.target {
        let state = m.state();
        if let Some(action) = drag::commit(&drag.item, target, &state.workspaces, &state.folders) {
            apply(m, action);
        }
    }
    Task::none()
}

fn run_action(st: &mut State, m: &mut Model, action: menu::Action) -> Task<Message> {
    use menu::Action;
    match action {
        Action::Go { ws, pane } => Task::done(Message::Intent(Intent::FocusPane {
            ws,
            pane,
            terminal: true,
        })),
        Action::Rename { kind, id } => {
            let name = name_of(m, kind, &id).unwrap_or_default();
            start_rename(st, kind, id, name)
        }
        Action::OpenEditor(id) => {
            let Some(ws) = m.workspace(&id).cloned() else {
                return Task::none();
            };
            let tr = Translator::new(m.locale());
            Task::perform(
                async move {
                    let outcome = editor::open_in_vscode(&ws).await;
                    editor::editor_notice(&outcome, &ws.name, tr)
                },
                |note| Message::Navigator(Msg::EditorDone(note)),
            )
        }
        Action::Intent(intent) => Task::done(Message::Intent(intent)),
        Action::Run(message) => Task::done(*message),
        Action::MoveTo { ws, folder } => {
            apply(
                m,
                StoreAction::MoveWorkspace {
                    id: ws,
                    folder,
                    before: None,
                },
            );
            Task::none()
        }
        Action::MoveToNewFolder(ws) => {
            let (folder, task) = create_folder(st, m);
            if let Some(folder) = folder {
                apply(
                    m,
                    StoreAction::MoveWorkspace {
                        id: ws,
                        folder: Some(folder),
                        before: None,
                    },
                );
            }
            task
        }
        Action::RemoveFolder(id) => {
            apply(m, StoreAction::RemoveFolder(id));
            Task::none()
        }
        Action::RemoveWorkspace(id) => {
            apply(m, StoreAction::RemoveWorkspace(id.clone()));
            Task::done(Message::Navigator(Msg::ClearMcpScope(id)))
        }
    }
}

/// The tree's keys (`onTreeKeyDown`): Up, Down, Home and End move; Right opens
/// a closed folder and steps into an open one; Left folds an open folder or
/// goes to the parent; Enter and Space toggle a folder or open a workspace;
/// F2 renames; the menu key, or Shift+F10, opens the row's menu under it.
fn on_key(st: &mut State, m: &mut Model, key: Key, modifiers: Modifiers) -> Task<Message> {
    let before = st.cursor.clone();
    let task = walk(st, m, key, modifiers);
    // The row the keyboard moved to is scrolled into view, as a focused element
    // is in a page.
    if st.cursor != before {
        Task::batch([task, reveal_cursor(st, m)])
    } else {
        task
    }
}

/// Scrolls the list just far enough for the row the keyboard is on.
fn reveal_cursor(st: &State, m: &Model) -> Task<Message> {
    let Some(key) = st.cursor.as_deref() else {
        return Task::none();
    };
    let projects = projects(m);
    let tree = build_nav_tree(&NavInput {
        workspaces: &projects,
        folders: &m.state().folders,
        query: &st.query,
        attention_only: st.attention_only,
        wants_you: &|pane| reads::wants_you(m, &pane.to_string()),
    });
    let spans = geometry::spans(&tree);
    let Some(span) = spans.iter().find(|span| span.key == key) else {
        return Task::none();
    };
    match geometry::reveal(span, st.scroll.0, st.scroll.1) {
        Some(y) => iced::widget::operation::scroll_to(
            list_id(),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
        ),
        None => Task::none(),
    }
}

fn walk(st: &mut State, m: &mut Model, key: Key, modifiers: Modifiers) -> Task<Message> {
    if modifiers.alt() || modifiers.control() || modifiers.command() {
        return Task::none();
    }
    let rows = rows_of(st, m);
    let Some(current) = tab_key(st, m, &rows) else {
        return Task::none();
    };
    let Some(at) = rows.iter().position(|row| row.key == current) else {
        return Task::none();
    };
    let row = rows[at].clone();
    let go = |st: &mut State, target: Option<&NavRow>| {
        if let Some(target) = target {
            st.cursor = Some(target.key.clone());
        }
    };
    let by_key = |key: &Option<String>| {
        key.as_ref()
            .and_then(|key| rows.iter().find(|row| &row.key == key))
    };
    let space = matches!(&key, Key::Named(Named::Space))
        || matches!(&key, Key::Character(c) if c.as_str() == " ");
    match key {
        Key::Named(Named::ArrowDown) => go(st, rows.get(at + 1)),
        Key::Named(Named::ArrowUp) => go(st, at.checked_sub(1).and_then(|i| rows.get(i))),
        Key::Named(Named::Home) => go(st, rows.first()),
        Key::Named(Named::End) => go(st, rows.last()),
        Key::Named(Named::ArrowRight) => {
            // A workspace is a leaf; a folder opens, then steps into what it
            // holds.
            if row.kind == RowKind::Folder {
                if !row.expanded {
                    toggle_folder(st, m, &row.id);
                } else if rows
                    .get(at + 1)
                    .is_some_and(|next| next.parent.as_deref() == Some(&row.key))
                {
                    go(st, rows.get(at + 1));
                }
            }
        }
        Key::Named(Named::ArrowLeft) => {
            if row.kind == RowKind::Folder && row.expanded && row.foldable {
                toggle_folder(st, m, &row.id);
            } else {
                go(st, by_key(&row.parent));
            }
        }
        Key::Named(Named::Enter) => {
            return enter(st, m, &row);
        }
        _ if space => {
            return enter(st, m, &row);
        }
        Key::Named(Named::F2) => {
            let name = name_of(m, row.kind, &row.id).unwrap_or_default();
            return start_rename(st, row.kind, row.id, name);
        }
        Key::Named(Named::ContextMenu) => return open_menu_under(st, &row),
        Key::Named(Named::F10) if modifiers.shift() => return open_menu_under(st, &row),
        _ => {}
    }
    Task::none()
}

fn enter(st: &mut State, m: &mut Model, row: &NavRow) -> Task<Message> {
    match row.kind {
        RowKind::Folder => {
            toggle_folder(st, m, &row.id);
            Task::none()
        }
        RowKind::Ws => Task::done(Message::Intent(Intent::ActivateWorkspace(row.id.clone()))),
    }
}

/// The menu key has no pointer: the menu opens under the row, a little in.
fn open_menu_under(st: &mut State, row: &NavRow) -> Task<Message> {
    st.menu = Some(MenuState {
        kind: row.kind,
        id: row.id.clone(),
        at: Vector::new(24.0, view::ROW_HEIGHT + 2.0),
        from_keyboard: true,
    });
    Task::none()
}

// ---- Subscription and hooks -------------------------------------------------------------------

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut subscriptions = Vec::new();

    if st.drag.is_some() {
        subscriptions.push(iced::event::listen_with(
            |event, _status, _window| match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::Navigator(Msg::DragEnd))
                }
                // Only a release drops: a cancelled gesture, a lost window or a
                // pointer that left the window puts everything back.
                Event::Mouse(mouse::Event::CursorLeft)
                | Event::Window(window::Event::Unfocused)
                | Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                }) => Some(Message::Navigator(Msg::DragCancel)),
                _ => None,
            },
        ));
    }

    if st.rename.is_some() {
        // The field reports no blur: a press that nothing else took ends it.
        subscriptions.push(iced::event::listen_with(
            |event, status, _window| match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    if status == iced::event::Status::Ignored =>
                {
                    Some(Message::Navigator(Msg::RenameBlur))
                }
                _ => None,
            },
        ));
    } else if st.menu.is_none() && m.ui.focus == Some(Focus::Region(Region::Navigator)) {
        // The tree's keys, while it has the keyboard and no field wants them.
        subscriptions.push(iced::event::listen_with(
            |event, status, _window| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                    if status == iced::event::Status::Ignored =>
                {
                    Some(Message::Navigator(Msg::Key(key, modifiers)))
                }
                _ => None,
            },
        ));
    }
    Subscription::batch(subscriptions)
}

/// What takes the focus when F6 reaches region `Navigator`: the tree, on its
/// tab stop.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    Some(Task::done(Message::Navigator(Msg::EnterTree)))
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Nothing in the core's events is the navigator's: the plan's windows come
/// from the pane logic, which reads the same reports.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// Region `navigator`, in the main window only.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    view::sidebar(st, ctx)
}

#[cfg(test)]
mod interaction;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
