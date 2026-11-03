//! Every keystroke the app answers to (the defaults of `src/lib/shortcuts.ts`),
//! what the person changed (`settings.keybindings`), and the editing of it
//! that the Shortcuts settings do.
//!
//! Commands are the stable half (code names `pane.split`, never a key) and
//! bindings are the part the person owns. The settings only ever store what
//! differs from the defaults below, so a default that changes in a later
//! version reaches everyone who never touched it.

use serde_json::{Map, Value};

use hangar_ui::shortcut::{Binding, Platform, parse_binding_on, serialize_chords};

use crate::commands::CommandId;

/// The bindings a command ships with on `platform` (`COMMANDS[].defaults`).
///
/// Most are written with `Mod`, which is Ctrl on Windows and Linux and Cmd on
/// a Mac, and that is the whole difference. The others state both: a chord
/// macOS keeps for itself, a key a MacBook does not have, or an action a Mac
/// user expects under another shortcut. The reasons are the comments of
/// shortcuts.ts, kept here next to the keys they explain.
pub fn defaults(id: CommandId, platform: Platform) -> &'static [&'static str] {
    use CommandId as C;
    let mac = platform == Platform::Mac;
    let pick = |others: &'static [&'static str], on_mac: &'static [&'static str]| {
        if mac { on_mac } else { others }
    };
    match id {
        C::PaneSplit => &["Mod+Shift+Enter"],
        // Cmd+D and Cmd+Shift+D are the splits every Mac terminal already
        // uses, and they leave Cmd+Option+arrow free for stepping between
        // workspaces, which is where a Mac puts tab navigation.
        C::PaneSplitRight => pick(&["Ctrl+Alt+Right"], &["Meta+D"]),
        C::PaneSplitDown => pick(&["Ctrl+Alt+Down"], &["Meta+Shift+D"]),
        C::PaneClose => &["Mod+Shift+X"],
        C::PaneRestart => &["Mod+Shift+R"],
        C::PaneRestartAll => &["Mod+Alt+Shift+R"],
        // Unbound: it throws away every conversation of the workspace, which
        // no stray chord should get one step closer to.
        C::PaneResetAll => &[],
        C::PaneTidy => &["Mod+Alt+G"],
        C::PaneSessions => &["Mod+Shift+S"],
        // One key both ways, like tmux's `prefix z`. Cmd+Shift+Z is Redo on
        // the macOS Edit menu, which takes it before the window sees it.
        C::PaneZoom => pick(&["Mod+Shift+Z"], &["Meta+Alt+Z"]),
        // Orchestration's commands are found in the palette and the menus
        // by those who use them, and cost nobody else a chord.
        C::PanePinOrchestrator | C::PaneUnpinOrchestrator => &[],
        // Ctrl on both: Cmd+Tab is the macOS application switcher.
        C::PaneNext => &["Ctrl+Tab"],
        C::PanePrev => &["Ctrl+Shift+Tab"],
        // Option+arrow moves by word in a terminal, so Ctrl joins the chord
        // on a Mac rather than taking it over.
        C::PaneFocusLeft => pick(&["Alt+Left"], &["Ctrl+Alt+Left"]),
        C::PaneFocusRight => pick(&["Alt+Right"], &["Ctrl+Alt+Right"]),
        C::PaneFocusUp => pick(&["Alt+Up"], &["Ctrl+Alt+Up"]),
        C::PaneFocusDown => pick(&["Alt+Down"], &["Ctrl+Alt+Down"]),
        C::PaneFocus1 => &["Mod+1"],
        C::PaneFocus2 => &["Mod+2"],
        C::PaneFocus3 => &["Mod+3"],
        C::PaneFocus4 => &["Mod+4"],
        C::PaneFocus5 => &["Mod+5"],
        C::PaneFocus6 => &["Mod+6"],
        C::PaneFocus7 => &["Mod+7"],
        C::PaneFocus8 => &["Mod+8"],
        C::PaneFocus9 => &["Mod+9"],
        C::PaneSwapLeft => pick(&["Alt+Shift+Left"], &["Ctrl+Alt+Shift+Left"]),
        C::PaneSwapRight => pick(&["Alt+Shift+Right"], &["Ctrl+Alt+Shift+Right"]),
        C::PaneSwapUp => pick(&["Alt+Shift+Up"], &["Ctrl+Alt+Shift+Up"]),
        C::PaneSwapDown => pick(&["Alt+Shift+Down"], &["Ctrl+Alt+Shift+Down"]),
        // A MacBook has no Page keys; Cmd+Option+arrow is what macOS browsers
        // step through tabs with.
        C::WorkspaceNext => pick(&["Ctrl+PageDown"], &["Meta+Alt+Right"]),
        C::WorkspacePrev => pick(&["Ctrl+PageUp"], &["Meta+Alt+Left"]),
        C::WorkspaceNew => &["Mod+Shift+N"],
        // Unbound: visited now and then, found in the palette and the menus.
        C::WorkspaceMcpServers
        | C::WorkspaceNewOrchestrator
        | C::WorkspaceOrchestration
        | C::WorkspaceStopWorkers
        | C::WorkspaceSleep => &[],
        // Option+digit types a character on a Mac, and with
        // `macOptionIsMeta` it is how a Meta digit reaches the shell, so Cmd
        // joins it there.
        C::WorkspaceGo1 => pick(&["Alt+1"], &["Meta+Alt+1"]),
        C::WorkspaceGo2 => pick(&["Alt+2"], &["Meta+Alt+2"]),
        C::WorkspaceGo3 => pick(&["Alt+3"], &["Meta+Alt+3"]),
        C::WorkspaceGo4 => pick(&["Alt+4"], &["Meta+Alt+4"]),
        C::WorkspaceGo5 => pick(&["Alt+5"], &["Meta+Alt+5"]),
        C::WorkspaceGo6 => pick(&["Alt+6"], &["Meta+Alt+6"]),
        C::WorkspaceGo7 => pick(&["Alt+7"], &["Meta+Alt+7"]),
        C::WorkspaceGo8 => pick(&["Alt+8"], &["Meta+Alt+8"]),
        C::WorkspaceGo9 => pick(&["Alt+9"], &["Meta+Alt+9"]),
        C::ViewSettings => &["Mod+,"],
        // A two-step sequence: it reads well, and it is the one place the
        // feature documents itself.
        C::ViewShortcuts => &["Mod+K Mod+S"],
        // Mod+K already opens the sequence above; Mod+Shift+P is the palette
        // everywhere else a developer looks.
        C::ViewPalette => &["Mod+Shift+P"],
        // Mod+Alt letters type nothing on AZERTY's AltGr.
        C::ViewNavigator => &["Mod+Alt+N"],
        C::ViewInspector => &["Mod+Alt+I"],
        C::ViewFocusNavigator => &["Mod+Alt+Shift+N"],
        C::ViewFocusInspector => &["Mod+Alt+Shift+I"],
        // F6, as in browsers and editors: a terminal keeps Tab for the shell,
        // and a function key types nothing, so it leaves a field as well.
        C::ViewNextRegion => &["F6"],
        C::ViewPrevRegion => &["Shift+F6"],
        C::ViewTerminals => &["Mod+Alt+T"],
        C::ViewBoard => &["Mod+Alt+B"],
        C::ViewBoardDock => &["Mod+Alt+Shift+B"],
        C::ViewModelPreview => &["Mod+Alt+P"],
        // Unbound: a stray chord should not close what an agent is showing.
        C::ViewCloseModels => &[],
        // K for knowledge: M is the MCP panel's.
        C::ViewMemory => &["Mod+Alt+K"],
        // U for usage: S is the session picker's.
        C::ViewStats => &["Mod+Alt+U"],
        C::ViewMcp => &["Mod+Alt+M"],
        C::ViewNeedsYou => &[],
        C::ViewBroadcast => &["Mod+Shift+B"],
        C::TerminalClear => &["Mod+Shift+L"],
        // Shift on both: on Windows Ctrl+C stays the interrupt, and on a Mac
        // plain Cmd+C is the Edit menu's.
        C::TerminalCopy => pick(&["Ctrl+Shift+C"], &["Meta+Shift+C"]),
        C::TerminalPaste => pick(&["Ctrl+Shift+V"], &["Meta+Shift+V"]),
        C::TerminalSelectAll => pick(&["Ctrl+Shift+A"], &["Meta+Shift+A"]),
        // Home and End are other keys a MacBook does without.
        C::TerminalScrollTop => pick(&["Ctrl+Home"], &["Meta+Up"]),
        C::TerminalScrollBottom => pick(&["Ctrl+End"], &["Meta+Down"]),
        // Plain Ctrl+F is the shell's forward-char.
        C::TerminalFind => pick(&["Ctrl+Shift+F"], &["Meta+F"]),
        // Unbound: which command is the third is the person's own
        // arrangement, and it changes the moment they save a fourth.
        C::CommandRun1
        | C::CommandRun2
        | C::CommandRun3
        | C::CommandRun4
        | C::CommandRun5
        | C::CommandRun6
        | C::CommandRun7
        | C::CommandRun8
        | C::CommandRun9 => &[],
        // Both spellings of the same key: a keyboard that needs Shift for
        // "+" would otherwise report the shifted chord only.
        C::AppZoomIn => &["Mod+=", "Mod++"],
        C::AppZoomOut => &["Mod+-"],
        C::AppZoomReset => &["Mod+0"],
        C::AppFullscreen => pick(&["F11"], &["Ctrl+Meta+F"]),
        // Push to talk: whatever this is rebound to wants to be comfortable
        // to hold down for a sentence.
        C::VoiceDictate => &["Mod+Alt+V"],
    }
}

/// The comparable form of a binding on `platform`, or `None` when it does
/// not parse (`serializeBinding`).
pub fn serial(platform: Platform, binding: &str) -> Option<String> {
    parse_binding_on(platform, binding).map(|chords| serialize_chords(&chords))
}

/// What `settings.keybindings` holds: by command id, the bindings of the
/// commands the person changed (`Keymap` of shortcuts.ts, as stored).
///
/// An empty list is meaningful: it is how a command is turned off. Ids this
/// version does not know are kept, since they belong to a version that knows
/// more commands, and dropping them would lose that person's bindings. The
/// map is kept as it was read, odd values included, so that writing it back
/// loses nothing either.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides(Map<String, Value>);

impl Overrides {
    pub fn new(map: Map<String, Value>) -> Overrides {
        Overrides(map)
    }

    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }

    pub fn into_map(self) -> Map<String, Value> {
        self.0
    }

    /// The bindings stored for `id`, when what is stored is a list. Entries
    /// that are not strings are skipped, as a hand-edited file may hold them.
    pub fn bindings(&self, id: &str) -> Option<Vec<Binding>> {
        let Value::Array(list) = self.0.get(id)? else {
            return None;
        };
        Some(
            list.iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect(),
        )
    }

    /// Whether the person changed this command's keys (`isCustomized`).
    pub fn is_customized(&self, id: CommandId) -> bool {
        self.0.contains_key(id.as_str())
    }

    fn set(&mut self, id: CommandId, bindings: Vec<Binding>) {
        self.0.insert(
            id.as_str().to_string(),
            Value::Array(bindings.into_iter().map(Value::String).collect()),
        );
    }

    /// How many commands are changed: what « Reset all » would undo.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Every command's bindings, defaults filled in (`resolveKeymap`), in the
/// order of `CommandId::ALL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<Vec<Binding>>,
}

impl Keymap {
    pub fn resolve(overrides: &Overrides, platform: Platform) -> Keymap {
        let bindings = CommandId::ALL
            .iter()
            .map(|id| {
                overrides.bindings(id.as_str()).unwrap_or_else(|| {
                    defaults(*id, platform)
                        .iter()
                        .map(|binding| binding.to_string())
                        .collect()
                })
            })
            .collect();
        Keymap { bindings }
    }

    /// The bindings of `id` (`bindingsOf`).
    pub fn bindings(&self, id: CommandId) -> &[Binding] {
        &self.bindings[id as usize]
    }

    /// Every command with its bindings, in `COMMANDS` order.
    pub fn iter(&self) -> impl Iterator<Item = (CommandId, &[Binding])> {
        CommandId::ALL
            .iter()
            .zip(&self.bindings)
            .map(|(id, bindings)| (*id, bindings.as_slice()))
    }
}

fn same_list(platform: Platform, a: &[&str], b: &[Binding]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| serial(platform, x) == serial(platform, y))
}

/// Drops the overrides that ended up matching the defaults again (`prune`).
fn prune(overrides: Overrides, platform: Platform) -> Overrides {
    let map = overrides
        .0
        .into_iter()
        .filter(|(id, value)| {
            let Ok(command) = id.parse::<CommandId>() else {
                return true;
            };
            let Value::Array(list) = value else {
                return true;
            };
            let list: Vec<Binding> = list
                .iter()
                .map(|value| value.as_str().unwrap_or_default().to_string())
                .collect();
            !same_list(platform, defaults(command, platform), &list)
        })
        .collect();
    Overrides(map)
}

/// Adds a binding to a command, ignoring one it already answers to however
/// it is spelled, or one that does not parse (`withBinding`).
pub fn with_binding(
    overrides: &Overrides,
    keymap: &Keymap,
    id: CommandId,
    binding: &str,
    platform: Platform,
) -> Overrides {
    let Some(wanted) = serial(platform, binding) else {
        return overrides.clone();
    };
    let current = keymap.bindings(id);
    if current
        .iter()
        .any(|entry| serial(platform, entry).as_deref() == Some(wanted.as_str()))
    {
        return overrides.clone();
    }
    let mut next = overrides.clone();
    let mut bindings = current.to_vec();
    bindings.push(binding.to_string());
    next.set(id, bindings);
    prune(next, platform)
}

/// Takes a binding off a command (`withoutBinding`).
pub fn without_binding(
    overrides: &Overrides,
    keymap: &Keymap,
    id: CommandId,
    binding: &str,
    platform: Platform,
) -> Overrides {
    let unwanted = serial(platform, binding);
    let mut next = overrides.clone();
    next.set(
        id,
        keymap
            .bindings(id)
            .iter()
            .filter(|entry| serial(platform, entry) != unwanted)
            .cloned()
            .collect(),
    );
    prune(next, platform)
}

/// Puts a command back on its shipped bindings (`resetCommand`).
pub fn reset_command(overrides: &Overrides, id: CommandId) -> Overrides {
    let mut next = overrides.clone();
    next.0.remove(id.as_str());
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OTHER: Platform = Platform::Other;

    fn overrides(value: Value) -> Overrides {
        match value {
            Value::Object(map) => Overrides::new(map),
            _ => panic!("an object"),
        }
    }

    fn strings(list: &[&str]) -> Vec<Binding> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_ids_index_the_table() {
        for (at, id) in CommandId::ALL.iter().enumerate() {
            assert_eq!(*id as usize, at, "{id}");
        }
    }

    #[test]
    fn ships_only_defaults_that_parse() {
        for platform in [Platform::Other, Platform::Mac] {
            for id in CommandId::ALL {
                for binding in defaults(*id, platform) {
                    assert!(
                        parse_binding_on(platform, binding).is_some(),
                        "{id}: {binding}"
                    );
                }
            }
        }
    }

    mod resolve_keymap {
        use super::*;

        #[test]
        fn fills_in_the_defaults() {
            let keymap = Keymap::resolve(&Overrides::default(), OTHER);
            for id in CommandId::ALL {
                assert_eq!(keymap.bindings(*id), strings(defaults(*id, OTHER)));
            }
        }

        #[test]
        fn lets_an_override_replace_them_and_an_empty_list_turn_a_command_off() {
            let keymap = Keymap::resolve(
                &overrides(json!({"pane.split": ["F9"], "pane.close": []})),
                OTHER,
            );
            assert_eq!(keymap.bindings(CommandId::PaneSplit), ["F9"]);
            assert!(keymap.bindings(CommandId::PaneClose).is_empty());
        }

        #[test]
        fn survives_settings_that_were_never_written() {
            let keymap = Keymap::resolve(&Overrides::default(), OTHER);
            assert_eq!(
                keymap.bindings(CommandId::PaneSplit),
                strings(defaults(CommandId::PaneSplit, OTHER))
            );
        }

        #[test]
        fn reads_past_values_that_are_not_lists_of_strings() {
            let keymap = Keymap::resolve(
                &overrides(json!({"pane.split": "F9", "pane.close": ["F8", 3]})),
                OTHER,
            );
            assert_eq!(
                keymap.bindings(CommandId::PaneSplit),
                strings(defaults(CommandId::PaneSplit, OTHER))
            );
            assert_eq!(keymap.bindings(CommandId::PaneClose), ["F8"]);
        }
    }

    mod editing_bindings {
        use super::*;

        fn split_defaults() -> Vec<Binding> {
            strings(defaults(CommandId::PaneSplit, OTHER))
        }

        fn resolved(overrides: &Overrides) -> Keymap {
            Keymap::resolve(overrides, OTHER)
        }

        #[test]
        fn adds_a_binding_next_to_the_defaults() {
            let none = Overrides::default();
            let next = with_binding(&none, &resolved(&none), CommandId::PaneSplit, "F9", OTHER);
            let mut expected = split_defaults();
            expected.push("F9".into());
            assert_eq!(next, overrides(json!({ "pane.split": expected })));
            assert!(next.is_customized(CommandId::PaneSplit));
        }

        #[test]
        fn ignores_a_binding_the_command_already_answers_to_however_it_is_spelled() {
            let none = Overrides::default();
            let respelled = split_defaults()[0].to_lowercase();
            assert_eq!(
                serial(OTHER, &respelled),
                serial(OTHER, &split_defaults()[0])
            );
            assert_eq!(
                with_binding(
                    &none,
                    &resolved(&none),
                    CommandId::PaneSplit,
                    &respelled,
                    OTHER
                ),
                none
            );
        }

        #[test]
        fn ignores_a_binding_that_does_not_parse() {
            let none = Overrides::default();
            assert_eq!(
                with_binding(
                    &none,
                    &resolved(&none),
                    CommandId::PaneSplit,
                    "Hyper+A",
                    OTHER
                ),
                none
            );
        }

        #[test]
        fn forgets_the_override_once_it_matches_the_defaults_again() {
            let none = Overrides::default();
            let added = with_binding(&none, &resolved(&none), CommandId::PaneSplit, "F9", OTHER);
            let removed =
                without_binding(&added, &resolved(&added), CommandId::PaneSplit, "F9", OTHER);
            assert_eq!(removed, Overrides::default());
        }

        #[test]
        fn keeps_an_emptied_command_as_an_override_since_that_is_how_it_is_turned_off() {
            let keymap = resolved(&Overrides::default());
            let mut next = Overrides::default();
            for binding in split_defaults() {
                next = without_binding(
                    &next,
                    &resolved(&next),
                    CommandId::PaneSplit,
                    &binding,
                    OTHER,
                );
            }
            assert_eq!(next, overrides(json!({"pane.split": []})));
            assert_eq!(keymap.bindings(CommandId::PaneSplit), split_defaults());
        }

        #[test]
        fn keeps_bindings_for_commands_this_version_does_not_know() {
            let current = overrides(json!({"future.command": ["Ctrl+Q"]}));
            let next = with_binding(
                &current,
                &resolved(&current),
                CommandId::PaneSplit,
                "F9",
                OTHER,
            );
            assert_eq!(next.bindings("future.command"), Some(strings(&["Ctrl+Q"])));
        }

        #[test]
        fn puts_a_command_back_on_its_defaults() {
            let next = reset_command(
                &overrides(json!({"pane.split": ["F9"], "pane.close": []})),
                CommandId::PaneSplit,
            );
            assert_eq!(next, overrides(json!({"pane.close": []})));
            assert!(!next.is_customized(CommandId::PaneSplit));
        }
    }
}
