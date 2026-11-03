//! The shortcut model the editor works on: the shipped defaults of every
//! command, the keymap resolved over the person's overrides, and the edits and
//! conflicts the editor shows (the `shortcuts.ts` half of
//! `ShortcutsSettings.tsx`'s needs).
//!
//! This is the seam to P7's keymap (`crate::keymap`). P7 builds the
//! dispatcher's index and the same defaults in parallel and had published
//! nothing by the time the settings landed, so the editor carries the part it
//! needs, under the names of `shortcuts.ts` (`resolve`, `bindings_of`,
//! `with_binding`, `without_binding`, `reset_command`, `find_conflicts`,
//! `commands_for`, `shortcut_label`). When P7's lands, this file becomes
//! re-exports of it and the table below goes; nothing outside this file names
//! the table. The chord algebra (`parse_binding`, `serialize_chords`…) is
//! hangar-ui's, which P7 uses as well.
//!
//! Settings only ever store what differs from the defaults, so a default that
//! changes in a later version reaches everyone who never touched it.

use std::collections::HashMap;

use hangar_ui::shortcut::{
    Binding, Chord, Platform, format_binding_on, parse_binding_on, serialize_chords,
};
use serde_json::{Map, Value};

use crate::commands::CommandId;

/// What the settings store: only the commands the person re-bound
/// (`settings.keybindings`). An empty list is meaningful, it is how a command
/// is turned off, so it is kept as it is.
pub type Overrides = Map<String, Value>;

/// Every command's bindings, defaults filled in (`Keymap` of `shortcuts.ts`).
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    platform: Platform,
    bindings: HashMap<CommandId, Vec<Binding>>,
}

/// The platform's own choice, for the handful of commands where one binding
/// cannot serve both.
///
/// Most defaults do not need this: they are written with `Mod`, which is Ctrl
/// on Windows and Cmd on a Mac, and that is the whole difference. What this is
/// for is the keys where the two platforms disagree on more than a modifier: a
/// chord macOS reserves for itself, a key a MacBook does not have, or an
/// action a Mac user expects under a different shortcut entirely.
fn for_platform(platform: Platform, windows: &[&str], mac: &[&str]) -> Vec<Binding> {
    let chosen = match platform {
        Platform::Mac => mac,
        Platform::Other => windows,
    };
    chosen.iter().map(|binding| binding.to_string()).collect()
}

/// What `id` answers to when the person never touched it (`defaults` of
/// `COMMANDS`).
///
/// Defaults lean on Mod+Shift for pane management and plain Alt for moving
/// around, which keeps every Ctrl+letter the shell needs (Ctrl+C, Ctrl+D,
/// Ctrl+R) reaching the terminal untouched.
pub fn default_bindings_on(platform: Platform, id: CommandId) -> Vec<Binding> {
    let one = |binding: &str| vec![binding.to_string()];
    let both = |windows: &[&str], mac: &[&str]| for_platform(platform, windows, mac);
    // The three digit families: focus a pane, run a saved command, go to a
    // workspace. Saved commands are unbound out of the box: which command is
    // the third one is the person's own arrangement, and it changes the moment
    // they save a fourth, so shipping nine chords for it would spend keys the
    // shell wants on a list that may well be empty.
    if let Some(n) = id.label_n() {
        return match id.as_str().split('.').next() {
            Some("pane") => one(&format!("Mod+{n}")),
            // Option+digit types a character on a Mac, and with
            // `macOptionIsMeta` on it is also how you send a Meta digit to the
            // shell, so Cmd joins it there.
            Some("workspace") => both(&[&format!("Alt+{n}")], &[&format!("Meta+Alt+{n}")]),
            _ => Vec::new(),
        };
    }
    match id.as_str() {
        "pane.split" => one("Mod+Shift+Enter"),
        // Cmd+D and Cmd+Shift+D are the splits every Mac terminal already
        // uses, and they leave Cmd+Option+arrow free for stepping between
        // workspaces, which is where a Mac puts tab navigation.
        "pane.splitRight" => both(&["Ctrl+Alt+Right"], &["Meta+D"]),
        "pane.splitDown" => both(&["Ctrl+Alt+Down"], &["Meta+Shift+D"]),
        "pane.close" => one("Mod+Shift+X"),
        "pane.restart" => one("Mod+Shift+R"),
        "pane.restartAll" => one("Mod+Alt+Shift+R"),
        "pane.tidy" => one("Mod+Alt+G"),
        "pane.sessions" => one("Mod+Shift+S"),
        // One key both ways, like tmux's `prefix z`. Cmd+Shift+Z is Redo on
        // the macOS Edit menu, which takes it before the window ever sees it.
        "pane.zoom" => both(&["Mod+Shift+Z"], &["Meta+Alt+Z"]),
        "pane.next" => one("Ctrl+Tab"),
        "pane.prev" => one("Ctrl+Shift+Tab"),
        "pane.focusLeft" => both(&["Alt+Left"], &["Ctrl+Alt+Left"]),
        "pane.focusRight" => both(&["Alt+Right"], &["Ctrl+Alt+Right"]),
        "pane.focusUp" => both(&["Alt+Up"], &["Ctrl+Alt+Up"]),
        "pane.focusDown" => both(&["Alt+Down"], &["Ctrl+Alt+Down"]),
        "pane.swapLeft" => both(&["Alt+Shift+Left"], &["Ctrl+Alt+Shift+Left"]),
        "pane.swapRight" => both(&["Alt+Shift+Right"], &["Ctrl+Alt+Shift+Right"]),
        "pane.swapUp" => both(&["Alt+Shift+Up"], &["Ctrl+Alt+Shift+Up"]),
        "pane.swapDown" => both(&["Alt+Shift+Down"], &["Ctrl+Alt+Shift+Down"]),
        "workspace.next" => both(&["Ctrl+PageDown"], &["Meta+Alt+Right"]),
        "workspace.prev" => both(&["Ctrl+PageUp"], &["Meta+Alt+Left"]),
        "workspace.new" => one("Mod+Shift+N"),
        "view.settings" => one("Mod+,"),
        "view.shortcuts" => one("Mod+K Mod+S"),
        "view.palette" => one("Mod+Shift+P"),
        "view.navigator" => one("Mod+Alt+N"),
        "view.inspector" => one("Mod+Alt+I"),
        "view.focusNavigator" => one("Mod+Alt+Shift+N"),
        "view.focusInspector" => one("Mod+Alt+Shift+I"),
        "view.nextRegion" => one("F6"),
        "view.prevRegion" => one("Shift+F6"),
        "view.terminals" => one("Mod+Alt+T"),
        "view.board" => one("Mod+Alt+B"),
        "view.boardDock" => one("Mod+Alt+Shift+B"),
        "view.modelPreview" => one("Mod+Alt+P"),
        "view.memory" => one("Mod+Alt+K"),
        "view.stats" => one("Mod+Alt+U"),
        "view.mcp" => one("Mod+Alt+M"),
        "view.broadcast" => one("Mod+Shift+B"),
        "terminal.clear" => one("Mod+Shift+L"),
        "terminal.copy" => both(&["Ctrl+Shift+C"], &["Meta+Shift+C"]),
        "terminal.paste" => both(&["Ctrl+Shift+V"], &["Meta+Shift+V"]),
        "terminal.selectAll" => both(&["Ctrl+Shift+A"], &["Meta+Shift+A"]),
        "terminal.scrollTop" => both(&["Ctrl+Home"], &["Meta+Up"]),
        "terminal.scrollBottom" => both(&["Ctrl+End"], &["Meta+Down"]),
        "terminal.find" => both(&["Ctrl+Shift+F"], &["Meta+F"]),
        "app.zoomIn" => vec!["Mod+=".to_string(), "Mod++".to_string()],
        "app.zoomOut" => one("Mod+-"),
        "app.zoomReset" => one("Mod+0"),
        "app.fullscreen" => both(&["F11"], &["Ctrl+Meta+F"]),
        // The one Mod+Alt letter neither platform had already taken. Push to
        // talk: held it records, released it transcribes, so whatever this is
        // rebound to wants to be something comfortable to hold down for a
        // sentence.
        "voice.dictate" => one("Mod+Alt+V"),
        // Unbound by default. Orchestration's commands are found in the
        // palette, the pane's menu and the navigator's, by those who use
        // them, and cost nobody else a chord; "reset all agents" throws away
        // every conversation of the workspace, which is not something a stray
        // chord should get one step closer to.
        _ => Vec::new(),
    }
}

/// Every command's bindings: the person's where they re-bound it, the
/// defaults elsewhere (`resolveKeymap`). A value that is not a list reads as
/// "not re-bound", and anything in a list that is not text is left out.
pub fn resolve_on(platform: Platform, overrides: &Overrides) -> Keymap {
    let bindings = CommandId::ALL
        .iter()
        .map(|id| {
            let custom = overrides.get(id.as_str()).and_then(Value::as_array);
            let list = match custom {
                Some(list) => list
                    .iter()
                    .filter_map(|binding| binding.as_str().map(str::to_string))
                    .collect(),
                None => default_bindings_on(platform, *id),
            };
            (*id, list)
        })
        .collect();
    Keymap { platform, bindings }
}

pub fn resolve(overrides: &Overrides) -> Keymap {
    resolve_on(Platform::current(), overrides)
}

pub fn bindings_of(keymap: &Keymap, id: CommandId) -> &[Binding] {
    keymap.bindings.get(&id).map_or(&[], Vec::as_slice)
}

/// The comparable form of a written binding, or `None` when it does not parse.
fn serial_of(platform: Platform, binding: &str) -> Option<String> {
    parse_binding_on(platform, binding).map(|chords| serialize_chords(&chords))
}

fn same_list(platform: Platform, a: &[Binding], b: &[Binding]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| serial_of(platform, a) == serial_of(platform, b))
}

/// Drops overrides that ended up matching the default again.
fn prune(platform: Platform, overrides: Overrides) -> Overrides {
    overrides
        .into_iter()
        .filter(|(id, bindings)| {
            // Ids this version does not know are kept: they belong to a
            // version that knows more commands than this one, and dropping
            // them would lose that person's bindings.
            let Ok(command) = id.parse::<CommandId>() else {
                return true;
            };
            let Some(list) = bindings.as_array() else {
                return true;
            };
            let list: Vec<Binding> = list
                .iter()
                .filter_map(|binding| binding.as_str().map(str::to_string))
                .collect();
            !same_list(platform, &default_bindings_on(platform, command), &list)
        })
        .collect()
}

fn to_value(bindings: &[Binding]) -> Value {
    Value::Array(bindings.iter().cloned().map(Value::String).collect())
}

/// Adds a binding to a command, ignoring one it already answers to or one
/// that does not parse.
pub fn with_binding(
    overrides: &Overrides,
    keymap: &Keymap,
    id: CommandId,
    binding: &str,
) -> Overrides {
    let platform = keymap.platform;
    let Some(serial) = serial_of(platform, binding) else {
        return overrides.clone();
    };
    let current = bindings_of(keymap, id);
    if current
        .iter()
        .any(|entry| serial_of(platform, entry).as_deref() == Some(serial.as_str()))
    {
        return overrides.clone();
    }
    let mut next = overrides.clone();
    let mut list = current.to_vec();
    list.push(binding.to_string());
    next.insert(id.as_str().to_string(), to_value(&list));
    prune(platform, next)
}

/// Takes a binding off a command. A command left with none stays an override:
/// that is how it is turned off.
pub fn without_binding(
    overrides: &Overrides,
    keymap: &Keymap,
    id: CommandId,
    binding: &str,
) -> Overrides {
    let platform = keymap.platform;
    let serial = serial_of(platform, binding);
    let list: Vec<Binding> = bindings_of(keymap, id)
        .iter()
        .filter(|entry| serial_of(platform, entry) != serial)
        .cloned()
        .collect();
    let mut next = overrides.clone();
    next.insert(id.as_str().to_string(), to_value(&list));
    prune(platform, next)
}

/// Puts a command back on its shipped bindings.
pub fn reset_command(overrides: &Overrides, id: CommandId) -> Overrides {
    let mut next = overrides.clone();
    next.remove(id.as_str());
    next
}

pub fn is_customized(overrides: &Overrides, id: CommandId) -> bool {
    overrides.contains_key(id.as_str())
}

/// Serialised sequence → the commands listening for it, in keymap order
/// (`exact` of `indexKeymap`). The dispatcher's prefixes are P7's.
struct Index {
    order: Vec<String>,
    exact: HashMap<String, Vec<CommandId>>,
}

fn index(keymap: &Keymap) -> Index {
    let mut order = Vec::new();
    let mut exact: HashMap<String, Vec<CommandId>> = HashMap::new();
    for id in CommandId::ALL {
        for binding in bindings_of(keymap, *id) {
            // A binding that no longer parses (hand-edited state, a version
            // that spelled keys differently) is skipped rather than crashing
            // the app.
            let Some(chords) = parse_binding_on(keymap.platform, binding) else {
                continue;
            };
            let serial = serialize_chords(&chords);
            match exact.get_mut(&serial) {
                Some(listeners) => listeners.push(*id),
                None => {
                    order.push(serial.clone());
                    exact.insert(serial, vec![*id]);
                }
            }
        }
    }
    Index { order, exact }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Several commands on the same keystroke: only the first one ever runs.
    Duplicate,
    /// A sequence whose first chord is already a shortcut of its own.
    Shadow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub binding: Binding,
    pub commands: Vec<CommandId>,
}

pub fn find_conflicts(keymap: &Keymap) -> Vec<Conflict> {
    let Index { order, exact } = index(keymap);
    let mut conflicts = Vec::new();

    for serial in &order {
        let commands = &exact[serial];
        if commands.len() > 1 {
            conflicts.push(Conflict {
                kind: ConflictKind::Duplicate,
                binding: display_of(keymap, commands[0], serial),
                commands: commands.clone(),
            });
        }
    }

    // A sequence is unreachable when its opening chord already fires
    // something.
    for serial in &order {
        let Some((head, _)) = serial.split_once(' ') else {
            continue;
        };
        let Some(owners) = exact.get(head) else {
            continue;
        };
        let commands = &exact[serial];
        conflicts.push(Conflict {
            kind: ConflictKind::Shadow,
            binding: display_of(keymap, commands[0], serial),
            commands: owners.iter().chain(commands).copied().collect(),
        });
    }

    conflicts
}

/// The written form a command uses for a given serialised sequence.
fn display_of(keymap: &Keymap, id: CommandId, serial: &str) -> Binding {
    bindings_of(keymap, id)
        .iter()
        .find(|binding| serial_of(keymap.platform, binding).as_deref() == Some(serial))
        .cloned()
        .unwrap_or_else(|| serial.to_string())
}

/// Commands already answering to a keystroke, `except` aside.
pub fn commands_for(
    keymap: &Keymap,
    chords: &[Chord],
    except: Option<CommandId>,
) -> Vec<CommandId> {
    let serial = serialize_chords(chords);
    index(keymap)
        .exact
        .remove(&serial)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| Some(*id) != except)
        .collect()
}

/// First binding of a command, formatted for a tooltip. Empty when unbound.
pub fn shortcut_label(keymap: &Keymap, id: CommandId) -> String {
    bindings_of(keymap, id)
        .first()
        .map_or_else(String::new, |binding| {
            format_binding_on(keymap.platform, binding)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_ui::shortcut::{parse_binding, serialize_binding};
    use serde_json::json;

    fn overrides(value: Value) -> Overrides {
        value.as_object().cloned().unwrap_or_default()
    }

    /// How `Mod+…` serialises on the platform the tests run on.
    fn modifier() -> &'static str {
        if Platform::current() == Platform::Mac {
            "meta"
        } else {
            "ctrl"
        }
    }

    fn defaults(id: CommandId) -> Vec<Binding> {
        default_bindings_on(Platform::current(), id)
    }

    #[test]
    fn the_table_has_a_default_for_every_command_that_has_one_and_each_parses() {
        for platform in [Platform::Other, Platform::Mac] {
            for id in CommandId::ALL {
                for binding in default_bindings_on(platform, *id) {
                    assert!(
                        parse_binding_on(platform, &binding).is_some(),
                        "{id}: {binding}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_shipped_defaults_hold_the_commands_of_the_web_app() {
        // 88 commands; 19 ship unbound (nine saved commands, the orchestration
        // ones, "reset all agents", the preview's close, the needs-you panel).
        let bound = CommandId::ALL
            .iter()
            .filter(|id| !defaults(**id).is_empty())
            .count();
        assert_eq!(bound, 69);
        assert_eq!(defaults(CommandId::PaneFocus3), ["Mod+3"]);
        assert!(defaults(CommandId::CommandRun3).is_empty());
        assert!(defaults(CommandId::PaneResetAll).is_empty());
        assert_eq!(defaults(CommandId::AppZoomIn), ["Mod+=", "Mod++"]);
        assert_eq!(
            default_bindings_on(Platform::Other, CommandId::WorkspaceGo2),
            ["Alt+2"]
        );
        assert_eq!(
            default_bindings_on(Platform::Mac, CommandId::WorkspaceGo2),
            ["Meta+Alt+2"]
        );
    }

    #[test]
    fn no_two_commands_ship_on_the_same_keys() {
        assert_eq!(find_conflicts(&resolve(&Overrides::new())), Vec::new());
    }

    #[test]
    fn nor_on_a_mac() {
        let mac = resolve_on(Platform::Mac, &Overrides::new());
        assert_eq!(find_conflicts(&mac), Vec::new());
    }

    #[test]
    fn resolving_fills_in_the_defaults() {
        let keymap = resolve(&Overrides::new());
        for id in CommandId::ALL {
            assert_eq!(bindings_of(&keymap, *id), defaults(*id), "{id}");
        }
    }

    #[test]
    fn an_override_replaces_them_and_an_empty_list_turns_a_command_off() {
        let keymap = resolve(&overrides(json!({"pane.split": ["F9"], "pane.close": []})));
        assert_eq!(bindings_of(&keymap, CommandId::PaneSplit), ["F9"]);
        assert!(bindings_of(&keymap, CommandId::PaneClose).is_empty());
    }

    #[test]
    fn something_that_is_not_a_list_leaves_the_defaults() {
        let keymap = resolve(&overrides(json!({"pane.split": "F9"})));
        assert_eq!(
            bindings_of(&keymap, CommandId::PaneSplit),
            defaults(CommandId::PaneSplit)
        );
    }

    #[test]
    fn a_binding_is_added_next_to_the_defaults() {
        let none = Overrides::new();
        let next = with_binding(&none, &resolve(&none), CommandId::PaneSplit, "F9");
        let mut expected = defaults(CommandId::PaneSplit);
        expected.push("F9".into());
        assert_eq!(next, overrides(json!({"pane.split": expected})));
        assert!(is_customized(&next, CommandId::PaneSplit));
    }

    #[test]
    fn a_binding_the_command_already_answers_to_is_ignored_however_it_is_spelled() {
        let none = Overrides::new();
        let respelled = defaults(CommandId::PaneSplit)[0].to_lowercase();
        assert_eq!(
            serialize_binding(&respelled),
            serialize_binding(&defaults(CommandId::PaneSplit)[0])
        );
        assert_eq!(
            with_binding(&none, &resolve(&none), CommandId::PaneSplit, &respelled),
            none
        );
    }

    #[test]
    fn a_binding_that_does_not_parse_is_ignored() {
        let none = Overrides::new();
        assert_eq!(
            with_binding(&none, &resolve(&none), CommandId::PaneSplit, "Hyper+A"),
            none
        );
    }

    #[test]
    fn the_override_is_forgotten_once_it_matches_the_defaults_again() {
        let none = Overrides::new();
        let added = with_binding(&none, &resolve(&none), CommandId::PaneSplit, "F9");
        let removed = without_binding(&added, &resolve(&added), CommandId::PaneSplit, "F9");
        assert_eq!(removed, none);
    }

    #[test]
    fn an_emptied_command_stays_an_override_since_that_is_how_it_is_turned_off() {
        let mut current = Overrides::new();
        for binding in defaults(CommandId::PaneSplit) {
            current = without_binding(&current, &resolve(&current), CommandId::PaneSplit, &binding);
        }
        assert_eq!(current, overrides(json!({"pane.split": []})));
        assert_eq!(
            bindings_of(&resolve(&Overrides::new()), CommandId::PaneSplit),
            defaults(CommandId::PaneSplit)
        );
    }

    #[test]
    fn bindings_of_commands_this_version_does_not_know_are_kept() {
        let known = overrides(json!({"future.command": ["Ctrl+Q"]}));
        let next = with_binding(&known, &resolve(&known), CommandId::PaneSplit, "F9");
        assert_eq!(next["future.command"], json!(["Ctrl+Q"]));
    }

    #[test]
    fn a_command_goes_back_to_its_defaults() {
        let next = reset_command(
            &overrides(json!({"pane.split": ["F9"], "pane.close": []})),
            CommandId::PaneSplit,
        );
        assert_eq!(next, overrides(json!({"pane.close": []})));
        assert!(!is_customized(&next, CommandId::PaneSplit));
    }

    #[test]
    fn two_commands_on_one_keystroke_are_a_conflict() {
        let keymap = resolve(&overrides(
            json!({"pane.split": ["F9"], "pane.close": ["f9"]}),
        ));
        assert_eq!(
            find_conflicts(&keymap),
            [Conflict {
                kind: ConflictKind::Duplicate,
                binding: "F9".into(),
                commands: vec![CommandId::PaneSplit, CommandId::PaneClose],
            }]
        );
    }

    #[test]
    fn a_sequence_whose_first_step_is_already_a_shortcut_is_a_conflict() {
        let keymap = resolve(&overrides(json!({"pane.close": ["Mod+K"]})));
        assert_eq!(
            find_conflicts(&keymap),
            [Conflict {
                kind: ConflictKind::Shadow,
                binding: "Mod+K Mod+S".into(),
                commands: vec![CommandId::PaneClose, CommandId::ViewShortcuts],
            }]
        );
    }

    #[test]
    fn the_owners_of_a_keystroke_are_named_leaving_out_the_one_being_edited() {
        let keymap = resolve(&overrides(
            json!({"pane.split": ["F9"], "pane.close": ["F9"]}),
        ));
        let chords = parse_binding("F9").unwrap();
        assert_eq!(
            commands_for(&keymap, &chords, None),
            [CommandId::PaneSplit, CommandId::PaneClose]
        );
        assert_eq!(
            commands_for(&keymap, &chords, Some(CommandId::PaneSplit)),
            [CommandId::PaneClose]
        );
        assert!(commands_for(&keymap, &parse_binding("F10").unwrap(), None).is_empty());
    }

    #[test]
    fn a_binding_that_no_longer_parses_is_skipped_by_the_index() {
        let keymap = resolve(&overrides(json!({"pane.split": ["Hyper+A", "F9"]})));
        assert_eq!(
            commands_for(&keymap, &parse_binding("F9").unwrap(), None),
            [CommandId::PaneSplit]
        );
    }

    #[test]
    fn a_sequence_is_indexed_whole() {
        let keymap = resolve(&Overrides::new());
        let sequence = format!("{m}+K {m}+S", m = modifier());
        let chords = parse_binding(&sequence).unwrap();
        assert_eq!(
            commands_for(&keymap, &chords, None),
            [CommandId::ViewShortcuts]
        );
        assert!(commands_for(&keymap, &chords[..1], None).is_empty());
    }

    #[test]
    fn the_label_is_the_first_binding_formatted_and_empty_when_unbound() {
        let keymap = resolve(&overrides(
            json!({"pane.split": ["Ctrl+Shift+Enter", "F9"], "pane.close": []}),
        ));
        assert_eq!(shortcut_label(&keymap, CommandId::PaneClose), "");
        if Platform::current() != Platform::Mac {
            assert_eq!(
                shortcut_label(&keymap, CommandId::PaneSplit),
                "Ctrl+Shift+Enter"
            );
        }
    }
}
