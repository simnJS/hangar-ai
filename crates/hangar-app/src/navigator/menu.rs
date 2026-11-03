//! The navigator's context menu (`Sidebar.tsx`'s `menuEntries`): right-click,
//! the « … » of a row, or the menu key on a row with the keyboard.
//!
//! The list itself, with its highlight, arrows, Escape and outside press, is
//! hangar-ui's `menu`; what is here is which lines a workspace or a folder has.

use hangar_i18n::Key;
use hangar_ui::menu::{self, Entry};
use hangar_ui::{Icon, glyph::GlyphState};

use super::Msg;
use super::model::RowKind;
use super::reads;
use super::status;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::shell::MenuEntry;
use crate::state::types::Workspace;
use crate::state::{AgentActivity, PaneId, WsId};

/// What a line of the menu does once picked. The menu closes first.
#[derive(Debug, Clone)]
pub enum Action {
    /// Brings an agent's pane forward and gives it the keyboard.
    Go {
        ws: WsId,
        pane: PaneId,
    },
    Rename {
        kind: RowKind,
        id: String,
    },
    OpenEditor(WsId),
    Intent(Intent),
    /// A line another package made (the Claude account choice), run as it
    /// asked.
    Run(Box<Message>),
    /// Files a workspace in a folder, or with the loose ones.
    MoveTo {
        ws: WsId,
        folder: Option<String>,
    },
    /// « New folder… »: made, the workspace filed in it, its name asked for.
    MoveToNewFolder(WsId),
    RemoveFolder(String),
    RemoveWorkspace(WsId),
}

fn pick(action: Action) -> Message {
    Message::Navigator(Msg::Menu(action))
}

fn glyph_of(state: AgentActivity) -> GlyphState {
    reads::activity_glyph(state)
}

/// The agents of these workspaces that are doing something, to go to from the
/// keyboard: what a click on the row's summary does with the pointer.
fn agent_entries(ctx: Ctx<'_>, workspaces: &[&Workspace]) -> Vec<Entry<Message>> {
    let found = status::busy_agents(ctx, workspaces);
    if found.is_empty() {
        return Vec::new();
    }
    let tr = ctx.i18n;
    let mut entries = vec![menu::heading(tr.t(Key::SidebarGoTo).into_owned())];
    for (state, ws, pane) in found {
        entries.push(
            menu::item(pane.name.clone())
                .glyph(glyph_of(state))
                .on_select(pick(Action::Go {
                    ws: ws.id.clone(),
                    pane: pane.id.clone(),
                }))
                .into(),
        );
    }
    entries.push(menu::separator());
    entries
}

/// An entry another package made, as the menu draws it.
fn adopt(entry: MenuEntry) -> Entry<Message> {
    match entry {
        MenuEntry::Heading(label) => menu::heading(label),
        MenuEntry::Separator => menu::separator(),
        MenuEntry::Item {
            label,
            icon,
            danger,
            checked,
            disabled,
            on_select,
        } => {
            let mut item = menu::item(label).on_select(pick(Action::Run(Box::new(on_select))));
            if let Some(icon) = icon.and_then(Icon::from_name) {
                item = item.icon(icon);
            }
            if danger {
                item = item.danger();
            }
            if let Some(checked) = checked {
                item = item.checked(checked);
            }
            match disabled {
                Some(reason) if reason.is_empty() => item = item.disabled(),
                Some(reason) => item = item.disabled_because(reason),
                None => {}
            }
            item.into()
        }
    }
}

/// The lines of a folder's menu.
pub fn folder_entries(ctx: Ctx<'_>, id: &str, filtering: bool) -> Vec<Entry<Message>> {
    let tr = ctx.i18n;
    let state = ctx.model.state();
    let Some(folder) = state.folders.iter().find(|folder| folder.id == id) else {
        return Vec::new();
    };
    // Open, its workspaces' own rows and menus say it.
    let folded = folder.collapsed && !filtering;
    let mut entries = Vec::new();
    if folded {
        let filed: Vec<&Workspace> = state
            .workspaces
            .iter()
            .filter(|ws| ws.folder_id.as_deref() == Some(id))
            .collect();
        entries.extend(agent_entries(ctx, &filed));
    }
    entries.push(
        menu::item(tr.t(Key::SidebarMenuRename).into_owned())
            .icon(Icon::Pencil)
            .on_select(pick(Action::Rename {
                kind: RowKind::Folder,
                id: id.to_string(),
            }))
            .into(),
    );
    entries.push(menu::separator());
    entries.push(
        menu::item(tr.t(Key::SidebarMenuRemoveFolder).into_owned())
            .icon(Icon::Trash)
            .danger()
            .on_select(pick(Action::RemoveFolder(id.to_string())))
            .into(),
    );
    entries
}

/// The lines of a workspace's menu.
pub fn workspace_entries(ctx: Ctx<'_>, id: &str) -> Vec<Entry<Message>> {
    let tr = ctx.i18n;
    let state = ctx.model.state();
    let Some(ws) = state.workspaces.iter().find(|ws| ws.id == id) else {
        return Vec::new();
    };
    let filed_in = ws
        .folder_id
        .as_deref()
        .filter(|id| state.folders.iter().any(|folder| folder.id == *id));

    let mut entries = agent_entries(ctx, &[ws]);
    let intent = |intent| pick(Action::Intent(intent));
    entries.push(
        menu::item(tr.t(Key::SidebarMenuRename).into_owned())
            .icon(Icon::Pencil)
            .on_select(pick(Action::Rename {
                kind: RowKind::Ws,
                id: id.to_string(),
            }))
            .into(),
    );
    entries.push(
        menu::item(tr.t(Key::EditorOpen).into_owned())
            .icon(Icon::Folder)
            .on_select(pick(Action::OpenEditor(ws.id.clone())))
            .into(),
    );
    entries.push(
        menu::item(tr.t(Key::WorktreesAction).into_owned())
            .icon(Icon::Branch)
            .on_select(intent(Intent::OpenWorktrees(ws.id.clone())))
            .into(),
    );
    entries.push(
        menu::item(tr.t(Key::McpScopeMenu).into_owned())
            .icon(Icon::Plug)
            .on_select(intent(Intent::OpenMcpServers(ws.id.clone())))
            .into(),
    );
    entries.push(
        menu::item(tr.t(Key::KitsManageMenu).into_owned())
            .icon(Icon::Plug)
            .on_select(intent(Intent::OpenKitsDialog(ws.id.clone())))
            .into(),
    );
    entries.push(
        menu::item(tr.t(Key::OrchSettingsMenu).into_owned())
            .icon(Icon::Orchestrator)
            .on_select(intent(Intent::OpenOrchestration(ws.id.clone())))
            .into(),
    );

    // The Claude account this workspace runs on: the same choice as the
    // toolbar's switcher, which is the accounts package's.
    let accounts = crate::accounts::menu_items(ctx, &ws.id, None);
    if !accounts.is_empty() {
        entries.push(menu::separator());
        if !matches!(accounts.first(), Some(MenuEntry::Heading(_))) {
            entries.push(menu::heading(tr.t(Key::AccountsMenuTitle).into_owned()));
        }
        entries.extend(accounts.into_iter().map(adopt));
    }

    entries.push(menu::separator());
    // Dragging does the same, but a menu is the one way to find out folders
    // exist.
    entries.push(menu::heading(tr.t(Key::SidebarMoveTo).into_owned()));
    for folder in &state.folders {
        let here = filed_in == Some(folder.id.as_str());
        let item = menu::item(folder.name.clone())
            .icon(Icon::Folder)
            .checked(here)
            .on_select(pick(Action::MoveTo {
                ws: ws.id.clone(),
                folder: Some(folder.id.clone()),
            }));
        entries.push(if here {
            item.disabled().into()
        } else {
            item.into()
        });
    }
    if filed_in.is_some() {
        entries.push(
            menu::item(tr.t(Key::SidebarNoFolder).into_owned())
                .on_select(pick(Action::MoveTo {
                    ws: ws.id.clone(),
                    folder: None,
                }))
                .into(),
        );
    }
    entries.push(
        menu::item(tr.t(Key::SidebarNewFolder).into_owned())
            .icon(Icon::FolderPlus)
            .on_select(pick(Action::MoveToNewFolder(ws.id.clone())))
            .into(),
    );
    entries.push(menu::separator());

    // Next to deleting it: the two ways to stop what it runs, for a while or
    // for good.
    //
    // TODO(P4): the sleep verdict (`sleep.verdict`, `sleepRefusal`, and the
    // shells probed as the menu opens) is P4's hibernation; until it lands the
    // line is always live and the intent's handler refuses with a notice.
    entries.push(
        menu::item(tr.t(Key::SleepAction).into_owned())
            .icon(Icon::Moon)
            .on_select(intent(Intent::Sleep(ws.id.clone())))
            .into(),
    );
    entries.push(
        menu::item(tr.t(Key::SidebarRemove).into_owned())
            .icon(Icon::Trash)
            .danger()
            .on_select(pick(Action::RemoveWorkspace(ws.id.clone())))
            .into(),
    );
    entries
}
