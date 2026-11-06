//! The resolved keymap and the lookups made of it (`indexKeymap`,
//! `findConflicts`, `commandsFor`, `shortcutLabel` of shortcuts.ts).
//!
//! One [`KeymapIndex`] is built whenever `settings.keybindings` changes and
//! shared as an `Arc`: the dispatcher, the terminals' key filter, the
//! palette, every tooltip and menu that shows a command's keys read the same
//! one. Lookups take strokes (chord.rs), so asking whether a key press is a
//! shortcut costs a binary search, and most presses stop there.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use iced::keyboard::key::Physical;
use iced::keyboard::{Key, Modifiers};
use serde_json::{Map, Value};

use hangar_i18n::{Key as Text, Translator, args};
use hangar_ui::shortcut::{Binding, Chord, Platform, format_binding_on, parse_binding_on};

use super::bindings::{Keymap, Overrides};
use super::chord::{Names, Stroke, stroke_of_press};
use crate::commands::CommandId;

/// Each index gets its own number, so that what was built from it (the
/// dispatcher's subscription) can tell an old one from a new one.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// The resolved keymap and its lookups: every command's bindings, defaults
/// filled in, indexed by the sequence of strokes each one is typed with.
#[derive(Debug)]
pub struct KeymapIndex {
    platform: Platform,
    generation: u64,
    /// The `settings.keybindings` it was built from.
    overrides: Overrides,
    keymap: Keymap,
    /// Key names outside chord.rs's table that the bindings use.
    names: Names,
    /// Each sequence bound, with the commands listening for it, in the order
    /// the keymap first names it: the order conflicts are reported in.
    entries: Vec<(Box<[Stroke]>, Vec<CommandId>)>,
    exact: HashMap<Box<[Stroke]>, usize>,
    /// Prefixes of longer sequences: the dispatcher waits on these.
    prefixes: HashSet<Box<[Stroke]>>,
    /// The first stroke of every binding, sorted: a press that is not one of
    /// these is no shortcut, unless a sequence is half typed.
    heads: Vec<Stroke>,
    /// Each command's first binding, formatted for the eye; empty when none.
    labels: Vec<String>,
}

/// What a conflict is (`ConflictKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictKind {
    /// Several commands on the same keystroke: only the first one that is
    /// live ever runs.
    Duplicate,
    /// A sequence whose first chord is already a shortcut of its own, which
    /// makes it unreachable.
    Shadow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The keystroke, written as the first command concerned writes it.
    pub binding: Binding,
    pub commands: Vec<CommandId>,
}

impl Default for KeymapIndex {
    /// The shipped keymap on this platform: what the window answers to before
    /// the settings are in, and for someone who never changed a key.
    fn default() -> Self {
        KeymapIndex::new(&Map::new())
    }
}

impl KeymapIndex {
    /// The index of `settings.keybindings` on this platform.
    pub fn new(keybindings: &Map<String, Value>) -> KeymapIndex {
        KeymapIndex::on(Platform::current(), keybindings)
    }

    pub fn on(platform: Platform, keybindings: &Map<String, Value>) -> KeymapIndex {
        let overrides = Overrides::new(keybindings.clone());
        let keymap = Keymap::resolve(&overrides, platform);
        KeymapIndex::of(platform, overrides, keymap)
    }

    /// The index of a keymap that is not the settings' own: the Shortcuts
    /// settings ask about one they are about to write.
    pub fn of(platform: Platform, overrides: Overrides, keymap: Keymap) -> KeymapIndex {
        let mut names = Names::default();
        let mut entries: Vec<(Box<[Stroke]>, Vec<CommandId>)> = Vec::new();
        let mut exact: HashMap<Box<[Stroke]>, usize> = HashMap::new();
        let mut prefixes = HashSet::new();
        let mut heads = Vec::new();
        let mut labels = Vec::with_capacity(CommandId::ALL.len());

        for (id, bindings) in keymap.iter() {
            labels.push(
                bindings
                    .first()
                    .map(|binding| format_binding_on(platform, binding))
                    .unwrap_or_default(),
            );
            for binding in bindings {
                // A binding that no longer parses (a hand-edited file, a
                // version that spelled keys differently) is skipped rather
                // than taking the window down.
                let Some(chords) = parse_binding_on(platform, binding) else {
                    continue;
                };
                let strokes: Box<[Stroke]> = chords
                    .iter()
                    .map(|chord| Stroke::learn(chord, &mut names))
                    .collect();
                heads.push(strokes[0]);
                for end in 1..strokes.len() {
                    prefixes.insert(Box::from(&strokes[..end]));
                }
                match exact.get(&strokes) {
                    Some(&at) => entries[at].1.push(id),
                    None => {
                        exact.insert(strokes.clone(), entries.len());
                        entries.push((strokes, vec![id]));
                    }
                }
            }
        }
        heads.sort_unstable();
        heads.dedup();

        KeymapIndex {
            platform,
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            overrides,
            keymap,
            names,
            entries,
            exact,
            prefixes,
            heads,
            labels,
        }
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

    /// Whether `keybindings` (the settings' now) is not what this index was
    /// built from.
    pub fn is_stale(&self, keybindings: &Map<String, Value>) -> bool {
        self.overrides.as_map() != keybindings
    }

    pub fn bindings(&self, id: CommandId) -> &[Binding] {
        self.keymap.bindings(id)
    }

    /// The command's first binding, as the settings write it.
    pub fn first_binding(&self, id: CommandId) -> Option<&str> {
        self.keymap.bindings(id).first().map(String::as_str)
    }

    /// The first binding of a command, formatted for a tooltip; empty when it
    /// is unbound (`shortcutLabel`).
    pub fn label(&self, id: CommandId) -> &str {
        &self.labels[id as usize]
    }

    /// `"Close pane (Ctrl+Shift+X)"`: a label followed by the key that runs
    /// it, so a tooltip stays true after a command is rebound
    /// (`useShortcutTitle`).
    pub fn title(&self, i18n: Translator, label: &str, id: CommandId) -> String {
        let keys = self.label(id);
        if keys.is_empty() {
            return label.to_string();
        }
        i18n.t_with(
            Text::KeymapWithKeys,
            &args!("label" => label, "keys" => keys),
        )
        .into_owned()
    }

    /// The stroke of a key press, or `None` for a press that is no chord (a
    /// modifier alone, an AltGr character).
    pub fn stroke_of_press(
        &self,
        key: &Key,
        physical: Physical,
        modifiers: Modifiers,
        text: Option<&str>,
    ) -> Option<Stroke> {
        stroke_of_press(key, physical, modifiers, text, &self.names)
    }

    /// The stroke of a chord, as this index names keys.
    pub fn stroke(&self, chord: &Chord) -> Stroke {
        Stroke::of(chord, &self.names)
    }

    /// The strokes of a written binding, or `None` when it does not parse.
    pub fn strokes(&self, binding: &str) -> Option<Vec<Stroke>> {
        parse_binding_on(self.platform, binding)
            .map(|chords| chords.iter().map(|chord| self.stroke(chord)).collect())
    }

    /// The chord of a stroke, for showing it.
    pub fn chord(&self, stroke: Stroke) -> Chord {
        stroke.chord(&self.names)
    }

    /// Whether some binding starts with this stroke.
    pub fn is_head(&self, stroke: Stroke) -> bool {
        self.heads.binary_search(&stroke).is_ok()
    }

    /// The commands listening for this sequence, in keymap order.
    pub fn owners(&self, sequence: &[Stroke]) -> &[CommandId] {
        self.exact
            .get(sequence)
            .map_or(&[], |&at| self.entries[at].1.as_slice())
    }

    /// Whether this sequence is the start of a longer one.
    pub fn is_prefix(&self, sequence: &[Stroke]) -> bool {
        self.prefixes.contains(sequence)
    }

    /// Commands already answering to a keystroke, `except` aside
    /// (`commandsFor`).
    pub fn commands_for(&self, chords: &[Chord], except: Option<CommandId>) -> Vec<CommandId> {
        let sequence: Vec<Stroke> = chords.iter().map(|chord| self.stroke(chord)).collect();
        self.owners(&sequence)
            .iter()
            .copied()
            .filter(|id| Some(*id) != except)
            .collect()
    }

    /// Duplicates, then sequences shadowed by their own first chord
    /// (`findConflicts`).
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut conflicts = Vec::new();
        for (strokes, commands) in &self.entries {
            if commands.len() > 1 {
                conflicts.push(Conflict {
                    kind: ConflictKind::Duplicate,
                    binding: self.written(commands[0], strokes),
                    commands: commands.clone(),
                });
            }
        }
        for (strokes, commands) in &self.entries {
            if strokes.len() < 2 {
                continue;
            }
            let owners = self.owners(&strokes[..1]);
            if owners.is_empty() {
                continue;
            }
            conflicts.push(Conflict {
                kind: ConflictKind::Shadow,
                binding: self.written(commands[0], strokes),
                commands: owners.iter().chain(commands).copied().collect(),
            });
        }
        conflicts
    }

    /// The written form `id` uses for a sequence (`displayOf`).
    fn written(&self, id: CommandId, strokes: &[Stroke]) -> Binding {
        self.keymap
            .bindings(id)
            .iter()
            .find(|binding| self.strokes(binding).as_deref() == Some(strokes))
            .cloned()
            .unwrap_or_else(|| {
                let chords: Vec<Chord> = strokes.iter().map(|s| self.chord(*s)).collect();
                hangar_ui::shortcut::serialize_chords(&chords)
            })
    }
}

/// `findConflicts` of a keymap that is not the live one.
#[cfg(test)]
pub fn find_conflicts(keymap: &Keymap, platform: Platform) -> Vec<Conflict> {
    KeymapIndex::of(platform, Overrides::default(), keymap.clone()).conflicts()
}

/// `commandsFor` of a keymap that is not the live one.
#[cfg(test)]
pub fn commands_for(
    keymap: &Keymap,
    chords: &[Chord],
    except: Option<CommandId>,
    platform: Platform,
) -> Vec<CommandId> {
    KeymapIndex::of(platform, Overrides::default(), keymap.clone()).commands_for(chords, except)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::bindings::{Keymap, defaults};
    use serde_json::json;

    const OTHER: Platform = Platform::Other;

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => panic!("an object"),
        }
    }

    fn index(value: Value) -> KeymapIndex {
        KeymapIndex::on(OTHER, &map(value))
    }

    fn owners_of(index: &KeymapIndex, binding: &str) -> Vec<CommandId> {
        index.owners(&index.strokes(binding).unwrap()).to_vec()
    }

    mod the_command_table {
        use super::*;

        #[test]
        fn ships_no_two_commands_on_the_same_keys() {
            assert_eq!(index(json!({})).conflicts(), Vec::<Conflict>::new());
        }

        #[test]
        fn ships_no_two_commands_on_the_same_keys_on_a_mac_either() {
            assert_eq!(
                KeymapIndex::on(Platform::Mac, &Map::new()).conflicts(),
                Vec::<Conflict>::new()
            );
        }
    }

    mod index_keymap {
        use super::*;

        #[test]
        fn indexes_each_sequence_and_the_prefixes_the_dispatcher_waits_on() {
            let index = index(json!({}));
            assert_eq!(
                owners_of(&index, "Ctrl+K Ctrl+S"),
                [CommandId::ViewShortcuts]
            );
            assert!(index.is_prefix(&index.strokes("Ctrl+K").unwrap()));
            assert!(owners_of(&index, "Ctrl+K").is_empty());
            // On a Mac the same sequence is Cmd's.
            let mac = KeymapIndex::on(Platform::Mac, &Map::new());
            assert_eq!(owners_of(&mac, "Meta+K Meta+S"), [CommandId::ViewShortcuts]);
        }

        #[test]
        fn skips_a_binding_that_no_longer_parses() {
            let index = index(json!({"pane.split": ["Hyper+A", "F9"]}));
            assert_eq!(owners_of(&index, "F9"), [CommandId::PaneSplit]);
        }

        #[test]
        fn knows_the_first_stroke_of_every_binding_and_nothing_else() {
            let index = index(json!({}));
            assert!(index.is_head(index.strokes("Ctrl+Shift+P").unwrap()[0]));
            assert!(index.is_head(index.strokes("Ctrl+K").unwrap()[0]));
            assert!(!index.is_head(index.strokes("Ctrl+S").unwrap()[0]));
            assert!(!index.is_head(index.strokes("A").unwrap()[0]));
        }
    }

    mod find_conflicts {
        use super::*;

        #[test]
        fn reports_two_commands_on_one_keystroke() {
            let index = index(json!({"pane.split": ["F9"], "pane.close": ["f9"]}));
            assert_eq!(
                index.conflicts(),
                [Conflict {
                    kind: ConflictKind::Duplicate,
                    binding: "F9".into(),
                    commands: vec![CommandId::PaneSplit, CommandId::PaneClose],
                }]
            );
        }

        #[test]
        fn reports_a_sequence_whose_first_step_is_already_a_shortcut() {
            let index = index(json!({"pane.close": ["Mod+K"]}));
            assert_eq!(
                index.conflicts(),
                [Conflict {
                    kind: ConflictKind::Shadow,
                    binding: "Mod+K Mod+S".into(),
                    commands: vec![CommandId::PaneClose, CommandId::ViewShortcuts],
                }]
            );
        }

        #[test]
        fn works_on_a_keymap_about_to_be_written() {
            let keymap = Keymap::resolve(
                &Overrides::new(map(json!({"pane.split": ["F9"], "pane.close": ["F9"]}))),
                OTHER,
            );
            assert_eq!(find_conflicts(&keymap, OTHER).len(), 1);
        }
    }

    mod commands_for {
        use super::*;

        #[test]
        fn names_the_owners_of_a_keystroke_leaving_out_the_one_being_edited() {
            let index = index(json!({"pane.split": ["F9"], "pane.close": ["F9"]}));
            let chords = parse_binding_on(OTHER, "F9").unwrap();
            assert_eq!(
                index.commands_for(&chords, None),
                [CommandId::PaneSplit, CommandId::PaneClose]
            );
            assert_eq!(
                index.commands_for(&chords, Some(CommandId::PaneSplit)),
                [CommandId::PaneClose]
            );
            let f10 = parse_binding_on(OTHER, "F10").unwrap();
            assert!(index.commands_for(&f10, None).is_empty());
            assert_eq!(
                commands_for(index.keymap(), &chords, Some(CommandId::PaneClose), OTHER),
                [CommandId::PaneSplit]
            );
        }
    }

    mod shortcut_label {
        use super::*;

        #[test]
        fn is_the_first_binding_formatted_and_empty_when_unbound() {
            let index = index(json!({"pane.split": ["Ctrl+Shift+Enter", "F9"], "pane.close": []}));
            assert_eq!(index.label(CommandId::PaneClose), "");
            assert_eq!(index.label(CommandId::PaneSplit), "Ctrl+Shift+Enter");
            let mac = KeymapIndex::on(Platform::Mac, &Map::new());
            assert_eq!(mac.label(CommandId::ViewPalette), "⇧⌘P");
        }

        #[test]
        fn makes_a_tooltip_that_names_the_keys() {
            let index = index(json!({"pane.close": []}));
            let en = Translator::new(hangar_i18n::Locale::En);
            assert_eq!(
                index.title(en, "Split", CommandId::PaneSplit),
                "Split (Ctrl+Shift+Enter)"
            );
            assert_eq!(index.title(en, "Close", CommandId::PaneClose), "Close");
        }
    }

    #[test]
    fn says_when_the_settings_moved_on() {
        let index = index(json!({"pane.split": ["F9"]}));
        assert!(!index.is_stale(&map(json!({"pane.split": ["F9"]}))));
        assert!(index.is_stale(&Map::new()));
        assert_ne!(index.generation(), KeymapIndex::default().generation());
    }

    #[test]
    fn the_default_index_is_the_shipped_keymap() {
        let index = KeymapIndex::on(OTHER, &Map::new());
        for id in CommandId::ALL {
            assert_eq!(index.bindings(*id), defaults(*id, OTHER));
        }
    }
}
