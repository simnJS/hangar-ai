//! What the palette lists (`CommandPalette.tsx`, its `entries`): the panes
//! that need the person, the panes of the workspace on screen, every command
//! live right now, the other workspaces and the project manager, each with
//! the words it is found by in both languages.
//!
//! Also the small readings of the model the other menus of this package
//! share: panes in reading order, workspaces in navigator order, a pane's
//! activity.

use iced::window;

use hangar_i18n::{Arg, Key, Locale, Translator, args};
use hangar_theme::AgentState;
use hangar_ui::glyph::GlyphState as Glyph;
use hangar_ui::icon::Icon;
use hangar_ui::matching::command_synonyms;
use hangar_ui::palette::{Detail, Item};

use crate::commands::{self, CommandId, Section};
use crate::keymap::KeymapIndex;
use crate::navigator::reads;
use crate::shell::WindowKind;
use crate::state::types::{AgentId, Pane, Workspace};
use crate::state::{AgentActivity, Model, PaneId, WsId, folders, layout};

/// What running an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Command(CommandId),
    Pane {
        ws: WsId,
        pane: PaneId,
    },
    Workspace(WsId),
    /// « Open the project manager », the navigator's button.
    Manager,
    /// A tab of one of the workspace's kits (`extraEntries` of App.tsx).
    KitTab {
        ws: WsId,
        kit: String,
        tab: String,
    },
    /// The workspace shown as the chat view (`on`) or as its panes, while
    /// the `chat-view` experiment is on (`chat_view`).
    ChatView {
        ws: WsId,
        on: bool,
    },
}

/// The order groups are listed in with nothing typed. Pane commands share
/// the heading of the pane list and workspace commands that of the
/// workspace list, rather than repeating a heading with a different meaning.
const SECTION_ORDER: [Section; 7] = [
    Section::Panes,
    Section::Navigation,
    Section::View,
    Section::Terminal,
    Section::Commands,
    Section::App,
    Section::Workspaces,
];

/// `pane.focus1` to `pane.focus9`, by the place of the pane they focus.
pub const PANE_FOCUS: [CommandId; 9] = [
    CommandId::PaneFocus1,
    CommandId::PaneFocus2,
    CommandId::PaneFocus3,
    CommandId::PaneFocus4,
    CommandId::PaneFocus5,
    CommandId::PaneFocus6,
    CommandId::PaneFocus7,
    CommandId::PaneFocus8,
    CommandId::PaneFocus9,
];

/// `workspace.go1` to `workspace.go9`, by the place of the workspace.
pub const WORKSPACE_GO: [CommandId; 9] = [
    CommandId::WorkspaceGo1,
    CommandId::WorkspaceGo2,
    CommandId::WorkspaceGo3,
    CommandId::WorkspaceGo4,
    CommandId::WorkspaceGo5,
    CommandId::WorkspaceGo6,
    CommandId::WorkspaceGo7,
    CommandId::WorkspaceGo8,
    CommandId::WorkspaceGo9,
];

/// `command.run1` to `command.run9`, by the place of the saved command.
pub const COMMAND_RUN: [CommandId; 9] = [
    CommandId::CommandRun1,
    CommandId::CommandRun2,
    CommandId::CommandRun3,
    CommandId::CommandRun4,
    CommandId::CommandRun5,
    CommandId::CommandRun6,
    CommandId::CommandRun7,
    CommandId::CommandRun8,
    CommandId::CommandRun9,
];

/// Commands that leave the keyboard on what they opened: a dialog, a field,
/// a confirmation. The terminal must not take it back after them.
pub fn owns_focus(id: CommandId) -> bool {
    use CommandId as C;
    matches!(
        id,
        C::ViewSettings
            | C::ViewShortcuts
            | C::ViewMcp
            | C::WorkspaceMcpServers
            | C::ViewBroadcast
            | C::WorkspaceNew
            | C::PaneSessions
            | C::PaneResetAll
            | C::TerminalFind
            | C::ViewFocusNavigator
            | C::ViewFocusInspector
            | C::WorkspaceOrchestration
            | C::WorkspaceStopWorkers
            | C::ViewNeedsYou
    )
}

/// A command's icon (`ICONS`), else its section's (`SECTION_ICONS`).
fn icon_of(id: CommandId) -> Icon {
    use CommandId as C;
    match id {
        C::PaneSplit | C::PaneSplitRight => Icon::SplitRight,
        C::PaneSplitDown => Icon::SplitDown,
        C::PaneClose | C::ViewCloseModels => Icon::Close,
        C::PaneRestart | C::PaneRestartAll => Icon::Restart,
        C::PaneResetAll => Icon::Reset,
        C::PaneTidy => Icon::Tidy,
        C::PaneSessions => Icon::Clock,
        C::PaneZoom | C::AppFullscreen => Icon::Expand,
        C::WorkspaceNew => Icon::FolderPlus,
        C::ViewSettings => Icon::Gear,
        C::ViewShortcuts => Icon::Keyboard,
        C::ViewTerminals => Icon::Terminal,
        C::ViewBoard | C::ViewBoardDock => Icon::Board,
        C::ViewMemory => Icon::Bookmark,
        C::ViewStats => Icon::Chart,
        C::ViewMcp | C::WorkspaceMcpServers => Icon::Plug,
        C::ViewBroadcast => Icon::Broadcast,
        C::ViewNavigator | C::ViewFocusNavigator => Icon::SidebarLeft,
        C::ViewInspector | C::ViewFocusInspector => Icon::SidebarRight,
        C::TerminalFind => Icon::Search,
        C::PanePinOrchestrator | C::PaneUnpinOrchestrator | C::WorkspaceNewOrchestrator => {
            Icon::Orchestrator
        }
        C::WorkspaceOrchestration => Icon::Sliders,
        C::WorkspaceStopWorkers => Icon::Stop,
        C::WorkspaceSleep => Icon::Moon,
        C::ViewNeedsYou => Icon::Flag,
        _ => match id.section() {
            Section::Panes => Icon::SplitRight,
            Section::Navigation => Icon::ArrowRight,
            Section::Workspaces => Icon::Folder,
            Section::View | Section::App => Icon::Window,
            Section::Terminal => Icon::Terminal,
            Section::Commands => Icon::Bolt,
            Section::Voice => Icon::Mic,
        },
    }
}

/// What the toolbar calls the same commands (`ALIASES`): someone who saw
/// « Réinitialiser les agents » on a button types that, not the sentence of
/// the shortcut list, and still finds it.
fn aliases(id: CommandId) -> &'static [Key] {
    use CommandId as C;
    match id {
        C::PaneSplit => &[Key::ToolbarNewTerminal],
        C::PaneRestartAll => &[Key::TopbarRestartAll],
        C::PaneResetAll => &[Key::TopbarResetAll],
        C::PaneTidy => &[Key::TopbarTidy],
        C::ViewBoardDock => &[Key::BoardDock],
        C::ViewInspector => &[Key::ToolbarInspector],
        C::ViewBroadcast => &[Key::BroadcastSend],
        C::PanePinOrchestrator => &[Key::OrchPin],
        C::WorkspaceOrchestration => &[Key::OrchTitle],
        C::WorkspaceSleep => &[Key::SleepAction],
        C::ViewNeedsYou => &[Key::NeedsYouTitle],
        _ => &[],
    }
}

fn state_word(activity: AgentActivity) -> Key {
    match activity {
        AgentActivity::Working => Key::PaneStateWorking,
        AgentActivity::Waiting => Key::PaneStateWaiting,
        AgentActivity::Yours => Key::PaneStateYours,
        AgentActivity::Idle => Key::PaneStateIdle,
    }
}

fn agent_state(activity: AgentActivity) -> AgentState {
    activity.into()
}

fn glyph(activity: AgentActivity) -> Glyph {
    agent_state(activity).into()
}

/// What a pane's agent is doing (`getPaneActivity`), idle for one that
/// reports none.
pub fn activity(m: &Model, pane: &PaneId) -> AgentActivity {
    reads::activity(m, pane)
}

/// The panes of a workspace as they sit on screen, which is what Ctrl+N
/// counts in (`readingOrder`).
pub fn reading_order(ws: &Workspace) -> Vec<&Pane> {
    layout::reading_order(&ws.panes, ws.tree.as_ref())
}

/// The workspace a pane belongs to.
pub fn workspace_of<'a>(m: &'a Model, pane: &str) -> Option<&'a Workspace> {
    m.state()
        .workspaces
        .iter()
        .find(|ws| ws.panes.iter().any(|entry| entry.id == pane))
}

/// The workspaces a window offers, and the one it shows: the navigator's in
/// the main window, its own in a workspace's window.
pub fn window_workspaces(m: &Model, window: window::Id) -> (Vec<&Workspace>, Option<&Workspace>) {
    match m.live.windows.get(&window) {
        Some(WindowKind::Workspace(id)) => {
            let ws = m.workspace(id);
            (ws.into_iter().collect(), ws)
        }
        // The main window's active workspace is none for it while that
        // workspace is shown in a window of its own (`activeWorkspace` was
        // null there): its entry is then listed with the others.
        _ => (
            folders::sidebar_order(&m.state().workspaces, &m.state().folders),
            m.window_workspace(window),
        ),
    }
}

/// The name a pane's agent goes by (`agentLabel`).
pub fn agent_label(i18n: Translator, agent: AgentId) -> String {
    match agent {
        AgentId::Shell => i18n.t(Key::PaneShell).into_owned(),
        other => other.info().label.to_string(),
    }
}

/// A command's label (`labelKey` with `labelParams`).
pub fn command_label(locale: Locale, id: CommandId) -> String {
    match id.label_n() {
        Some(n) => hangar_i18n::t_with(locale, id.label_key(), &args!("n" => n)).into_owned(),
        None => hangar_i18n::t(locale, id.label_key()).into_owned(),
    }
}

/// The same words in the languages not shown, as hidden keywords: someone
/// who reads the app in French may still type "settings", and the other way
/// round.
fn elsewhere(locale: Locale, key: Key, params: &[(&str, Arg)]) -> Vec<String> {
    Locale::ALL
        .iter()
        .filter(|other| **other != locale)
        .map(|other| hangar_i18n::t_with(*other, key, params).into_owned())
        .collect()
}

fn command_elsewhere(locale: Locale, id: CommandId) -> Vec<String> {
    Locale::ALL
        .iter()
        .filter(|other| **other != locale)
        .map(|other| command_label(*other, id))
        .collect()
}

/// "Attend ta réponse" reads "attend ta réponse" after a "·".
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Every entry of the palette opened in `window`, in build order.
pub fn items(
    m: &Model,
    window: window::Id,
    i18n: Translator,
    index: &KeymapIndex,
) -> Vec<Item<Target>> {
    let locale = i18n.locale();
    let (workspaces, active) = window_workspaces(m, window);
    let active_id = active.map(|ws| ws.id.as_str());
    let mut list: Vec<Item<Target>> = Vec::new();
    let panes_group = i18n.t(Key::KeymapSectionPanes).into_owned();
    let workspaces_group = i18n.t(Key::KeymapSectionWorkspaces).into_owned();

    let pane_entry = |ws: &Workspace, pane: &Pane, activity: AgentActivity, group: &str| {
        let agent = agent_label(i18n, pane.agent);
        let word = i18n.t(state_word(activity)).into_owned();
        let name = pane.name.as_str();
        let mut keywords = vec![
            pane.name.clone(),
            agent.clone(),
            word.clone(),
            ws.name.clone(),
        ];
        keywords.extend(elsewhere(locale, Key::PaletteGoTo, &args!("name" => name)));
        keywords.extend(elsewhere(locale, state_word(activity), &[]));
        let mut detail = Detail::new();
        if Some(ws.id.as_str()) != active_id {
            detail = detail.plain(format!("{} · ", ws.name));
        }
        detail = detail
            .plain(format!("{agent} · "))
            .state(agent_state(activity), lower_first(&word));
        let at = if Some(ws.id.as_str()) == active_id {
            reading_order(ws)
                .iter()
                .position(|entry| entry.id == pane.id)
        } else {
            None
        };
        let keys = at
            .and_then(|at| PANE_FOCUS.get(at))
            .and_then(|id| index.first_binding(*id))
            .unwrap_or("");
        Item::new(
            Target::Pane {
                ws: ws.id.clone(),
                pane: pane.id.clone(),
            },
            group,
            i18n.t_with(Key::PaletteGoTo, &args!("name" => name)),
        )
        .keywords(keywords)
        .state(glyph(activity))
        .detail(detail)
        .keys(keys)
    };

    // What needs the person, wherever it is: the active workspace first,
    // then the others in navigator order.
    let mut suggested: Vec<&str> = Vec::new();
    let near: Vec<&Workspace> = match active {
        Some(active) => std::iter::once(active)
            .chain(workspaces.iter().copied().filter(|ws| ws.id != active.id))
            .collect(),
        None => workspaces.clone(),
    };
    let suggestions = i18n.t(Key::PaletteSuggestions).into_owned();
    for wanted in [AgentActivity::Waiting, AgentActivity::Yours] {
        for ws in &near {
            for pane in reading_order(ws) {
                if activity(m, &pane.id) != wanted {
                    continue;
                }
                suggested.push(pane.id.as_str());
                list.push(pane_entry(ws, pane, wanted, &suggestions));
            }
        }
    }

    if let Some(active) = active {
        for pane in reading_order(active) {
            if suggested.contains(&pane.id.as_str()) {
                continue;
            }
            list.push(pane_entry(
                active,
                pane,
                activity(m, &pane.id),
                &panes_group,
            ));
        }
    }

    // What a key would run now, but the palette's own key.
    let live: Vec<CommandId> = commands::available(m, window)
        .into_iter()
        .filter(|id| *id != CommandId::ViewPalette && id.in_palette())
        .collect();
    let saved = active
        .map(|ws| crate::saved_commands::merged(ws, m.settings()))
        .unwrap_or_default();

    for section in SECTION_ORDER {
        let group = i18n.t(section.label_key()).into_owned();

        // The other workspaces lead their group, before the commands about
        // them.
        if section == Section::Workspaces {
            for (at, ws) in workspaces.iter().enumerate() {
                if Some(ws.id.as_str()) == active_id {
                    continue;
                }
                list.push(workspace_entry(m, i18n, index, ws, at, &group));
            }
            let mut keywords = elsewhere(locale, Key::ManagerOpen, &[]);
            keywords.push(i18n.t(Key::ManagerHqName).into_owned());
            keywords.extend(elsewhere(locale, Key::ManagerHqName, &[]));
            keywords.extend(["manager".to_string(), "HQ".to_string()]);
            list.push(
                Item::new(Target::Manager, group.as_str(), i18n.t(Key::ManagerOpen))
                    .keywords(keywords)
                    .icon(Icon::Orchestrator),
            );
        }

        for id in CommandId::ALL.iter().copied() {
            if id.section() != section || !live.contains(&id) {
                continue;
            }
            // A saved command is listed by what it runs, not by its slot.
            let saved_command = COMMAND_RUN
                .iter()
                .position(|slot| *slot == id)
                .and_then(|at| saved.get(at));
            let label = match saved_command {
                Some(command) => {
                    let named = command.label.trim();
                    let named = if named.is_empty() {
                        command.command.as_str()
                    } else {
                        named
                    };
                    i18n.t_with(Key::PaletteRunSaved, &args!("label" => named))
                        .into_owned()
                }
                None => command_label(locale, id),
            };
            let mut keywords = match saved_command {
                Some(command) => vec![command.command.clone()],
                None => command_elsewhere(locale, id),
            };
            for alias in aliases(id) {
                keywords.push(i18n.t(*alias).into_owned());
                keywords.extend(elsewhere(locale, *alias, &[]));
            }
            keywords.extend(
                command_synonyms(id.as_str())
                    .iter()
                    .map(|word| word.to_string()),
            );
            keywords.push(group.clone());
            keywords.extend(elsewhere(locale, section.label_key(), &[]));
            let heading = match section {
                Section::Panes => panes_group.as_str(),
                Section::Workspaces => workspaces_group.as_str(),
                _ => group.as_str(),
            };
            list.push(
                Item::new(Target::Command(id), heading, label)
                    .keywords(keywords)
                    .icon(icon_of(id))
                    .keys(index.first_binding(id).unwrap_or("")),
            );
        }

        // The chat view's switch, the toolbar's « Chat / Panes », after the
        // view commands: what it says is what it would do.
        if section == Section::View
            && let Some(ws) = active
            && crate::chat_view::available(m)
        {
            let on = !crate::chat_view::shown(m, ws);
            let label = if on {
                Key::ChatViewShowChat
            } else {
                Key::ChatViewShowPanes
            };
            let mut keywords = vec![
                i18n.t(Key::ChatViewChat).into_owned(),
                i18n.t(Key::ChatViewPanes).into_owned(),
                "chat".to_string(),
                group.clone(),
            ];
            keywords.extend(elsewhere(locale, label, &[]));
            keywords.extend(elsewhere(locale, Key::ChatViewChat, &[]));
            keywords.extend(elsewhere(locale, Key::ChatViewPanes, &[]));
            list.push(
                Item::new(
                    Target::ChatView {
                        ws: ws.id.clone(),
                        on,
                    },
                    group.as_str(),
                    i18n.t(label),
                )
                .keywords(keywords)
                .icon(if on { Icon::Window } else { Icon::Terminal }),
            );
        }

        // The tabs of the active workspace's kits come after the view
        // commands (`extraEntries`).
        if section == Section::View
            && let Some(ws) = active
        {
            let group = i18n.t(Key::KitsTabPaletteGroup).into_owned();
            for choice in crate::kit_tabs::choices(m, ws) {
                let title = choice.tab.title.as_str();
                let mut keywords = vec![
                    choice.tab.title.clone(),
                    choice.kit_name.clone(),
                    choice.kit_id.clone(),
                    "kit".to_string(),
                    "kits".to_string(),
                    group.clone(),
                ];
                keywords.extend(elsewhere(
                    locale,
                    Key::KitsTabPaletteOpen,
                    &args!("title" => title),
                ));
                list.push(
                    Item::new(
                        Target::KitTab {
                            ws: ws.id.clone(),
                            kit: choice.kit_id.clone(),
                            tab: choice.tab.id.clone(),
                        },
                        group.as_str(),
                        i18n.t_with(Key::KitsTabPaletteOpen, &args!("title" => title)),
                    )
                    .keywords(keywords)
                    .icon(Icon::Window)
                    .detail(Detail::new().plain(choice.kit_name.clone())),
                );
            }
        }
    }
    list
}

/// Another workspace: its busiest state, or where it is, or its size.
fn workspace_entry(
    m: &Model,
    i18n: Translator,
    index: &KeymapIndex,
    ws: &Workspace,
    at: usize,
    group: &str,
) -> Item<Target> {
    let locale = i18n.locale();
    let (mut working, mut waiting, mut yours) = (0u64, 0u64, 0u64);
    for pane in &ws.panes {
        match activity(m, &pane.id) {
            AgentActivity::Working => working += 1,
            AgentActivity::Waiting => waiting += 1,
            AgentActivity::Yours => yours += 1,
            AgentActivity::Idle => {}
        }
    }
    let busiest = if waiting > 0 {
        Some((
            AgentActivity::Waiting,
            i18n.tn(Key::SidebarActivityWaiting, waiting, &[]),
        ))
    } else if yours > 0 {
        Some((
            AgentActivity::Yours,
            i18n.tn(Key::SidebarActivityYours, yours, &[]),
        ))
    } else if working > 0 {
        Some((
            AgentActivity::Working,
            i18n.tn(Key::SidebarActivityWorking, working, &[]),
        ))
    } else {
        None
    };
    let detail = match busiest {
        Some((kind, words)) => Detail::new().state(agent_state(kind), words),
        None if ws.detached == Some(true) => Detail::new().plain(i18n.t(Key::PaletteInWindow)),
        None => Detail::new().plain(i18n.tn(Key::StatusPanes, ws.panes.len(), &[])),
    };
    let name = ws.name.as_str();
    let mut keywords = vec![ws.cwd.clone()];
    keywords.extend(elsewhere(locale, Key::PaletteOpen, &args!("name" => name)));
    let keys = WORKSPACE_GO
        .get(at)
        .and_then(|id| index.first_binding(*id))
        .unwrap_or("");
    Item::new(
        Target::Workspace(ws.id.clone()),
        group,
        i18n.t_with(Key::PaletteOpen, &args!("name" => name)),
    )
    .keywords(keywords)
    .icon(Icon::Folder)
    .detail(detail)
    .keys(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigator::testing::{detach, model_of};
    use crate::state::AppState;
    use crate::state::types::{SplitDir, SplitNode, WorkspaceFolder};

    fn pane(id: &str) -> Pane {
        Pane {
            id: id.into(),
            name: id.to_uppercase(),
            ..Pane::default()
        }
    }

    fn leaf(id: &str) -> Box<SplitNode> {
        Box::new(SplitNode::Pane { id: id.into() })
    }

    #[test]
    fn panes_are_read_in_the_order_the_tree_shows_them() {
        let ws = Workspace {
            panes: vec![pane("a"), pane("b"), pane("c")],
            tree: Some(SplitNode::Split {
                dir: SplitDir::Row,
                ratio: 0.5,
                a: leaf("c"),
                b: Box::new(SplitNode::Split {
                    dir: SplitDir::Col,
                    ratio: 0.5,
                    a: leaf("a"),
                    b: leaf("b"),
                }),
            }),
            ..Workspace::default()
        };
        let order: Vec<&str> = reading_order(&ws).iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, ["c", "a", "b"]);
    }

    #[test]
    fn a_tree_that_lost_a_pane_keeps_what_it_holds_then_the_rest() {
        let ws = Workspace {
            panes: vec![pane("a"), pane("b"), pane("c")],
            tree: Some(SplitNode::Split {
                dir: SplitDir::Row,
                ratio: 0.5,
                a: leaf("b"),
                b: Box::new(SplitNode::Split {
                    dir: SplitDir::Col,
                    ratio: 0.5,
                    a: leaf("gone"),
                    b: leaf("b"),
                }),
            }),
            ..Workspace::default()
        };
        let order: Vec<&str> = reading_order(&ws).iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, ["b", "a", "c"]);
        let no_tree = Workspace {
            panes: vec![pane("x"), pane("y")],
            ..Workspace::default()
        };
        let order: Vec<&str> = reading_order(&no_tree)
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(order, ["x", "y"]);
    }

    fn filed(id: &str, folder: Option<&str>) -> Workspace {
        Workspace {
            id: id.into(),
            name: id.into(),
            folder_id: folder.map(str::to_string),
            ..Workspace::default()
        }
    }

    /// Four workspaces, listed by the navigator as "loose, lost, in-a, in-b".
    fn model() -> Model {
        let folder = |id: &str| WorkspaceFolder {
            id: id.into(),
            ..WorkspaceFolder::default()
        };
        model_of(AppState {
            workspaces: vec![
                filed("in-b", Some("b")),
                filed("loose", None),
                filed("in-a", Some("a")),
                filed("lost", Some("deleted")),
            ],
            folders: vec![folder("a"), folder("b")],
            active_workspace_id: Some("loose".into()),
            ..AppState::default()
        })
    }

    fn ids(list: &[&Workspace]) -> Vec<String> {
        list.iter().map(|ws| ws.id.clone()).collect()
    }

    #[test]
    fn the_main_window_lists_loose_workspaces_then_each_folder() {
        let m = model();
        let main = m.main_window().unwrap();
        let (listed, active) = window_workspaces(&m, main);
        assert_eq!(ids(&listed), ["loose", "lost", "in-a", "in-b"]);
        assert_eq!(active.map(|ws| ws.id.as_str()), Some("loose"));
    }

    #[test]
    fn the_main_window_has_no_active_workspace_while_it_is_in_a_window_of_its_own() {
        let mut m = model();
        let own = detach(&mut m, "loose");
        let main = m.main_window().unwrap();
        // Still listed, to be opened, but none is in front of this window.
        let (listed, active) = window_workspaces(&m, main);
        assert!(ids(&listed).contains(&"loose".to_string()));
        assert!(active.is_none());
        // And the workspace window offers its own workspace alone.
        let (offered, in_front) = window_workspaces(&m, own);
        assert_eq!(ids(&offered), ["loose"]);
        assert_eq!(in_front.map(|ws| ws.id.as_str()), Some("loose"));
    }

    #[test]
    fn a_palette_opened_in_a_workspace_window_gives_the_keyboard_back_to_that_workspaces_pane() {
        let mut m = model_of(AppState {
            workspaces: vec![
                Workspace {
                    panes: vec![pane("a1")],
                    ..filed("main", None)
                },
                Workspace {
                    panes: vec![pane("b1")],
                    ..filed("own", None)
                },
            ],
            active_workspace_id: Some("main".into()),
            ..AppState::default()
        });
        let own = detach(&mut m, "own");
        let main = m.main_window().unwrap();
        m.ui.focus_by_workspace.insert("main".into(), "a1".into());
        m.ui.focus_by_workspace.insert("own".into(), "b1".into());
        assert_eq!(super::super::focused_pane(&m, main).as_deref(), Some("a1"));
        assert_eq!(super::super::focused_pane(&m, own).as_deref(), Some("b1"));
    }

    #[test]
    fn a_state_word_after_a_dot_starts_in_lower_case() {
        assert_eq!(lower_first("Attend ta réponse"), "attend ta réponse");
        assert_eq!(lower_first("Écrit"), "écrit");
        assert_eq!(lower_first(""), "");
    }

    #[test]
    fn commands_are_found_by_their_label_in_the_other_language() {
        let words = command_elsewhere(Locale::Fr, CommandId::ViewSettings);
        assert_eq!(words, [command_label(Locale::En, CommandId::ViewSettings)]);
        assert_ne!(
            command_label(Locale::Fr, CommandId::PaneFocus3),
            command_label(Locale::Fr, CommandId::PaneFocus4)
        );
    }
}
