//! How the command palette matches what is typed against what it offers: a
//! port of `src/lib/paletteMatch.ts`, with its tests.
//!
//! Forgiving where people are sloppy (accents and case, so "memoire" finds
//! "Mémoire") and strict about order where it says something: a label that
//! starts with the query beats one with a word starting with it, which beats
//! one merely containing it. Several words find a label holding all of them,
//! in any order. Pure, so the ranking can be tested without a window.
//!
//! ```
//! use hangar_ui::matching::{highlight, match_text};
//!
//! let found = match_text("Afficher la mémoire", "memo").unwrap();
//! let parts = highlight("Afficher la mémoire", &found.ranges);
//! assert_eq!(parts.iter().filter(|part| part.hit).map(|part| part.text).collect::<Vec<_>>(), ["mémo"]);
//! ```

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// The label starts with the query.
pub const SCORE_PREFIX: f32 = 4.0;
/// A word of the label starts with the query.
pub const SCORE_WORD: f32 = 3.0;
/// The query sits somewhere inside the label.
pub const SCORE_SUBSTRING: f32 = 2.0;
/// Every word of the query is in the label, each at the start of a word.
pub const SCORE_WORDS: f32 = 1.5;
/// Every word of the query is in the label, somewhere.
pub const SCORE_TOKENS: f32 = 1.0;
/// Only the secondary text matched: the agent, the group. Nothing to mark.
pub const SCORE_KEYWORD: f32 = 0.5;

/// What a query found in a label.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Higher is better; 0 when there is no query at all.
    pub score: f32,
    /// Byte ranges of the original, unfolded label: what to highlight. They
    /// fall on character boundaries, so slicing the label with them is safe.
    pub ranges: Vec<Range<usize>>,
}

impl Match {
    fn none() -> Match {
        Match {
            score: 0.0,
            ranges: Vec::new(),
        }
    }
}

/// The no-break spaces French labels carry (`lib/typography`), one for one:
/// what is typed has plain spaces, "20 h 01" in a label does not.
fn is_no_break_space(c: char) -> bool {
    matches!(c, '\u{00a0}' | '\u{202f}')
}

/// Accents, case and no-break spaces folded away, the way both sides are
/// compared.
pub fn fold(text: &str) -> String {
    fold_chars(text).collect()
}

fn fold_chars(text: &str) -> impl Iterator<Item = char> + '_ {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .map(|c| if is_no_break_space(c) { ' ' } else { c })
        .flat_map(char::to_lowercase)
}

/// The query as it is compared: folded, trimmed, single-spaced.
pub fn fold_query(query: &str) -> String {
    let folded = fold(query);
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The folded text, plus where each of its characters came from.
///
/// Folding changes lengths ("é" decomposes into two characters before the
/// accent is dropped, and a few capitals lower into two), so an index in the
/// folded text is not an index in the original. Folding one character at a
/// time keeps the way back, which is what lets the palette bold the right
/// letters.
struct Folded {
    chars: Vec<char>,
    /// For each folded character, the byte range of the character it came from.
    origins: Vec<Range<usize>>,
}

impl Folded {
    fn of(text: &str) -> Folded {
        let mut chars = Vec::with_capacity(text.len());
        let mut origins = Vec::with_capacity(text.len());
        for (at, original) in text.char_indices() {
            let end = at + original.len_utf8();
            let mut single = [0u8; 4];
            for folded in fold_chars(original.encode_utf8(&mut single)) {
                chars.push(folded);
                origins.push(at..end);
            }
        }
        Folded { chars, origins }
    }

    /// The range in the original of `length` folded characters from `at`.
    fn span(&self, at: usize, length: usize) -> Range<usize> {
        self.origins[at].start..self.origins[at + length - 1].end
    }

    fn starts_with(&self, needle: &[char]) -> bool {
        self.chars.starts_with(needle)
    }

    /// First index at or after `from` where `needle` occurs.
    fn find(&self, needle: &[char], from: usize) -> Option<usize> {
        if needle.is_empty() || self.chars.len() < needle.len() {
            return None;
        }
        (from..=self.chars.len() - needle.len()).find(|&at| self.chars[at..].starts_with(needle))
    }

    /// First index where `needle` starts a word of the text.
    fn word_start(&self, needle: &[char]) -> Option<usize> {
        let mut from = 0;
        while let Some(at) = self.find(needle, from) {
            if at == 0 || !self.chars[at - 1].is_alphanumeric() {
                return Some(at);
            }
            from = at + 1;
        }
        None
    }
}

/// Sorted, with overlapping or touching ranges merged into one.
fn merge_ranges(ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(sorted.len());
    for range in sorted {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// How well `text` answers `query`, or `None` when it does not. An empty query
/// matches everything with a score of 0.
pub fn match_text(text: &str, query: &str) -> Option<Match> {
    let q = fold_query(query);
    if q.is_empty() {
        return Some(Match::none());
    }
    let needle: Vec<char> = q.chars().collect();
    let folded = Folded::of(text);

    if folded.starts_with(&needle) {
        return Some(Match {
            score: SCORE_PREFIX,
            ranges: vec![folded.span(0, needle.len())],
        });
    }
    if let Some(at) = folded.word_start(&needle) {
        return Some(Match {
            score: SCORE_WORD,
            ranges: vec![folded.span(at, needle.len())],
        });
    }
    if let Some(at) = folded.find(&needle, 0) {
        return Some(Match {
            score: SCORE_SUBSTRING,
            ranges: vec![folded.span(at, needle.len())],
        });
    }

    let tokens: Vec<&str> = q.split(' ').collect();
    if tokens.len() < 2 {
        return None;
    }
    let mut ranges = Vec::with_capacity(tokens.len());
    let mut at_word_start = 0;
    for token in &tokens {
        let token: Vec<char> = token.chars().collect();
        let start = folded.word_start(&token);
        let at = start.or_else(|| folded.find(&token, 0))?;
        if start.is_some() {
            at_word_start += 1;
        }
        ranges.push(folded.span(at, token.len()));
    }
    Some(Match {
        score: if at_word_start == tokens.len() {
            SCORE_WORDS
        } else {
            SCORE_TOKENS
        },
        ranges: merge_ranges(&ranges),
    })
}

/// The label decides; the keywords (the agent a pane runs, the group it is
/// filed under) only rescue an entry the label alone would have missed, and
/// rank it below every label match.
pub fn match_entry(label: &str, keywords: &[String], query: &str) -> Option<Match> {
    if let Some(direct) = match_text(label, query) {
        return Some(direct);
    }
    let all = keywords.join(" ");
    if !all.is_empty() && match_text(&all, query).is_some() {
        return Some(Match {
            score: SCORE_KEYWORD,
            ranges: Vec::new(),
        });
    }
    None
}

/// A run of `highlight`'s output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part<'a> {
    pub text: &'a str,
    pub hit: bool,
}

/// `text` cut into runs, each marked as matched or not, for rendering.
pub fn highlight<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<Part<'a>> {
    let mut parts = Vec::new();
    let mut at = 0;
    for range in merge_ranges(ranges) {
        // A range from a different text could land off a character boundary
        // or past the end; clamp it rather than panic on a stale match.
        let start = floor_boundary(text, range.start.min(text.len()));
        let end = floor_boundary(text, range.end.min(text.len()));
        if start > at {
            parts.push(Part {
                text: &text[at..start],
                hit: false,
            });
        }
        if end > start {
            parts.push(Part {
                text: &text[start..end],
                hit: true,
            });
        }
        at = at.max(end);
    }
    if at < text.len() {
        parts.push(Part {
            text: &text[at..],
            hit: false,
        });
    }
    parts
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// What the palette can search: a heading to be listed under, a label, and
/// the secondary words it is also found by.
pub trait Searchable {
    /// The heading it is listed under. Entries of a group are kept together.
    fn group(&self) -> &str;
    fn label(&self) -> &str;
    fn keywords(&self) -> &[String] {
        &[]
    }
}

/// An entry the query kept, and how well it matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked<'a, T> {
    /// Its place in the slice that was filtered.
    pub index: usize,
    pub entry: &'a T,
    pub matched: Match,
}

/// The entries of one heading.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteGroup<'a, T> {
    pub group: &'a str,
    pub entries: Vec<Ranked<'a, T>>,
}

/// The entries `query` keeps, grouped under their headings.
///
/// With nothing typed, everything in the order it was given. With a query,
/// the best entries of each group come first, and the group holding the best
/// entry overall comes first, so the top row, the one Enter runs, is always
/// the best match there is. Ties keep the order given, which is the order the
/// palette thinks most useful.
pub fn filter_palette<'a, T: Searchable>(
    entries: &'a [T],
    query: &str,
) -> Vec<PaletteGroup<'a, T>> {
    let mut groups: Vec<PaletteGroup<'a, T>> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(matched) = match_entry(entry.label(), entry.keywords(), query) else {
            continue;
        };
        let at = match groups.iter().position(|group| group.group == entry.group()) {
            Some(at) => at,
            None => {
                groups.push(PaletteGroup {
                    group: entry.group(),
                    entries: Vec::new(),
                });
                groups.len() - 1
            }
        };
        groups[at].entries.push(Ranked {
            index,
            entry,
            matched,
        });
    }

    if fold_query(query).is_empty() {
        return groups;
    }

    // `sort_by` is stable, which is what keeps ties in the given order.
    for group in &mut groups {
        group
            .entries
            .sort_by(|a, b| b.matched.score.total_cmp(&a.matched.score));
    }
    let best = |group: &PaletteGroup<'a, T>| group.entries.first().map_or(0.0, |r| r.matched.score);
    groups.sort_by(|a, b| best(b).total_cmp(&best(a)));
    groups
}

/// The other words people reach for when they look for a command, in both
/// languages at once: someone who thinks "paramètres" or "preferences" is
/// looking for "Réglages". The palette also indexes each command's label in
/// the other language, so these only cover what neither label says. Like the
/// agent and group keywords, they rank below every label match.
///
/// Keyed by the command's id (`"view.settings"`), as `COMMAND_SYNONYMS` is in
/// the TypeScript.
pub const COMMAND_SYNONYMS: &[(&str, &[&str])] = &[
    (
        "view.settings",
        &[
            "paramètres",
            "préférences",
            "options",
            "configuration",
            "preferences",
            "config",
        ],
    ),
    (
        "view.shortcuts",
        &["clavier", "touches", "keyboard", "keybindings", "hotkeys"],
    ),
    ("view.terminals", &["grille", "grid"]),
    ("view.board", &["kanban", "tâches", "tasks", "todo"]),
    ("view.boardDock", &["kanban", "tâches", "tasks", "dock"]),
    (
        "view.modelPreview",
        &[
            "modèle", "model", "maillage", "mesh", "glb", "gltf", "fbx", "obj", "viewer",
        ],
    ),
    ("view.closeModels", &["modèle", "model", "3d"]),
    ("view.memory", &["notes", "souvenirs", "knowledge"]),
    (
        "view.stats",
        &[
            "usage",
            "tokens",
            "coût",
            "cost",
            "dépenses",
            "spend",
            "temps",
            "time",
            "dashboard",
        ],
    ),
    (
        "view.mcp",
        &[
            "serveur",
            "server",
            "connecter",
            "connect",
            "intégration",
            "integration",
        ],
    ),
    (
        "workspace.mcpServers",
        &["mémoire", "memory", "processus", "process", "ram", "strict"],
    ),
    (
        "view.broadcast",
        &["diffusion", "envoyer à tous", "send to all", "everyone"],
    ),
    (
        "view.navigator",
        &["barre latérale", "sidebar", "arborescence", "tree"],
    ),
    ("view.inspector", &["détails", "details", "infos", "info"]),
    (
        "pane.split",
        &[
            "nouveau terminal",
            "new terminal",
            "ajouter",
            "add",
            "ouvrir",
            "open",
        ],
    ),
    (
        "pane.splitRight",
        &["vertical", "côte à côte", "side by side"],
    ),
    ("pane.splitDown", &["horizontal"]),
    (
        "pane.close",
        &["supprimer", "remove", "kill", "quitter", "quit"],
    ),
    ("pane.restart", &["redémarrer", "relaunch", "reload"]),
    ("pane.restartAll", &["redémarrer", "relaunch", "reload"]),
    (
        "pane.resetAll",
        &["conversation", "vider", "effacer", "clear", "remise à zéro"],
    ),
    (
        "pane.tidy",
        &[
            "ranger",
            "aligner",
            "grille",
            "disposition",
            "arrange",
            "grid",
            "layout",
        ],
    ),
    (
        "pane.sessions",
        &[
            "reprendre",
            "historique",
            "conversation",
            "resume",
            "history",
            "transcript",
        ],
    ),
    (
        "pane.zoom",
        &["agrandir", "maximiser", "plein écran", "maximize", "focus"],
    ),
    (
        "workspace.new",
        &[
            "projet", "dossier", "créer", "ajouter", "project", "folder", "create", "add",
        ],
    ),
    (
        "workspace.sleep",
        &[
            "hiberner",
            "suspendre",
            "dormir",
            "libérer",
            "mémoire",
            "hibernate",
            "suspend",
            "free",
            "memory",
            "ram",
        ],
    ),
    ("terminal.clear", &["cls", "vider", "nettoyer", "reset"]),
    ("terminal.find", &["chercher", "trouver", "find", "grep"]),
    ("app.zoomIn", &["taille", "plus grand", "size", "larger"]),
    ("app.zoomOut", &["taille", "plus petit", "size", "smaller"]),
    ("app.fullscreen", &["plein écran", "maximiser", "maximize"]),
];

/// The synonyms of a command, or none.
pub fn command_synonyms(id: &str) -> &'static [&'static str] {
    COMMAND_SYNONYMS
        .iter()
        .find(|(command, _)| *command == id)
        .map_or(&[], |(_, words)| *words)
}

#[cfg(test)]
mod tests;
