//! The pure rules behind the memory view's safeguards, kept free of iced and
//! of the core so a test can run them alone (`components/memoryGuards.ts`),
//! plus the two small rules of the composer and the search field that
//! `MemoryView.tsx` kept inline (`parseTags`, `matches`).

use hangar_core::memory::MemoryEntry;

/// Mirrors `MAX_ENTRIES` in hangar-core's memory.rs: past it, `memory_create`
/// refuses.
pub const MEMORY_CAP: usize = 1000;
/// From here the view tells the person the memory is nearly full.
pub const MEMORY_WARN_AT: usize = 900;

/// True when the memory is close enough to its cap to deserve a line.
pub fn near_cap(count: usize) -> bool {
    count >= MEMORY_WARN_AT
}

/// The same identity memory.rs upserts on: titles that differ only by case or
/// edge whitespace are one fact (`title_key`).
pub fn title_key(title: &str) -> String {
    title.trim().to_lowercase()
}

/// The entry a new title would replace, if any. Creating from the composer is
/// an upsert in the core, so a taken title silently overwrites that entry's
/// content: the view asks first, using the entries it already loaded.
pub fn find_title_clash<'a>(entries: &'a [MemoryEntry], title: &str) -> Option<&'a MemoryEntry> {
    let key = title_key(title);
    if key.is_empty() {
        return None;
    }
    entries.iter().find(|entry| title_key(&entry.title) == key)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MemorySort {
    #[default]
    Recent,
    Oldest,
}

impl MemorySort {
    /// The other order: what the near-cap note's toggle switches to.
    pub fn flipped(self) -> MemorySort {
        match self {
            MemorySort::Recent => MemorySort::Oldest,
            MemorySort::Oldest => MemorySort::Recent,
        }
    }
}

/// Newest update first, or oldest update first to find what to clean up. The
/// sort is stable, as the TS one was, so entries updated at the same instant
/// keep the order the core gave them.
pub fn sort_entries<'a>(
    entries: impl IntoIterator<Item = &'a MemoryEntry>,
    sort: MemorySort,
) -> Vec<&'a MemoryEntry> {
    let mut sorted: Vec<&MemoryEntry> = entries.into_iter().collect();
    match sort {
        MemorySort::Oldest => sorted.sort_by_key(|entry| entry.updated_at),
        MemorySort::Recent => sorted.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at)),
    }
    sorted
}

/// Free text in, tag list out; duplicates (ignoring case) and blanks dropped.
pub fn parse_tags(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut tags = Vec::new();
    for part in raw.split(',') {
        let tag = part.trim();
        if tag.is_empty() || !seen.insert(tag.to_lowercase()) {
            continue;
        }
        tags.push(tag.to_owned());
    }
    tags
}

/// The words of a search, lowercased: every one has to appear.
pub fn search_terms(query: &str) -> Vec<String> {
    query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// What a search looks through for one entry: the title, the content and the
/// tags, lowercased. Built once when the entries load (`State::haystacks`), so
/// typing in the search field does not lowercase the whole memory per key.
pub fn haystack(entry: &MemoryEntry) -> String {
    format!(
        "{}\n{}\n{}",
        entry.title,
        entry.content,
        entry.tags.join(" ")
    )
    .to_lowercase()
}

/// Same rule as the MCP search: every term has to appear somewhere in the
/// haystack of the entry.
pub fn matches_haystack(haystack: &str, terms: &[String]) -> bool {
    terms.iter().all(|term| haystack.contains(term.as_str()))
}

/// `matches_haystack` of an entry's own haystack, for the tests and the
/// callers with no index at hand.
#[cfg(test)]
pub fn matches(entry: &MemoryEntry, terms: &[String]) -> bool {
    matches_haystack(&haystack(entry), terms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, title: &str, updated_at: u64) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            title: title.into(),
            content: "body".into(),
            tags: Vec::new(),
            workspace: String::new(),
            agent: String::new(),
            created_at: 1,
            updated_at,
        }
    }

    fn ids(entries: &[&MemoryEntry]) -> Vec<String> {
        entries.iter().map(|entry| entry.id.clone()).collect()
    }

    #[test]
    fn finds_the_entry_a_title_would_replace_ignoring_case_and_edge_whitespace() {
        let entries = [
            entry("a", "Build steps", 1),
            entry("b", "ONNX cannot ship", 1),
        ];
        assert_eq!(find_title_clash(&entries, "build steps").unwrap().id, "a");
        assert_eq!(
            find_title_clash(&entries, "  ONNX Cannot Ship  ")
                .unwrap()
                .id,
            "b"
        );
    }

    #[test]
    fn finds_nothing_for_a_free_title_or_a_blank_one() {
        let entries = [
            entry("a", "Build steps", 1),
            entry("b", "ONNX cannot ship", 1),
        ];
        assert!(find_title_clash(&entries, "Something else").is_none());
        assert!(find_title_clash(&entries, "   ").is_none());
        assert!(find_title_clash(&[], "Build steps").is_none());
    }

    #[test]
    fn does_not_match_a_title_that_only_contains_another() {
        let entries = [entry("a", "Build steps", 1)];
        assert!(find_title_clash(&entries, "Build").is_none());
    }

    #[test]
    fn uses_the_same_key_as_the_core_upsert() {
        assert_eq!(title_key("  bUiLd StEpS "), "build steps");
    }

    #[test]
    fn near_cap_starts_at_900_entries() {
        assert!(!near_cap(899));
        assert!(near_cap(900));
        assert!(near_cap(1000));
    }

    #[test]
    fn puts_the_oldest_update_first_for_a_clean_up() {
        let entries = [
            entry("mid", "m", 5),
            entry("old", "o", 1),
            entry("new", "n", 9),
        ];
        assert_eq!(
            ids(&sort_entries(&entries, MemorySort::Oldest)),
            ["old", "mid", "new"]
        );
    }

    #[test]
    fn puts_the_latest_update_first_by_default_and_leaves_the_input_alone() {
        let entries = [
            entry("mid", "m", 5),
            entry("old", "o", 1),
            entry("new", "n", 9),
        ];
        assert_eq!(
            ids(&sort_entries(&entries, MemorySort::default())),
            ["new", "mid", "old"]
        );
        assert_eq!(
            entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["mid", "old", "new"]
        );
    }

    #[test]
    fn tags_are_split_trimmed_and_deduplicated_ignoring_case() {
        assert_eq!(
            parse_tags(" build, Rust ,rust,, ci ,BUILD"),
            ["build", "Rust", "ci"]
        );
        assert!(parse_tags(" , ,").is_empty());
    }

    #[test]
    fn a_search_needs_every_term_somewhere() {
        let mut found = entry("a", "Build steps", 1);
        found.content = "Run cargo test before the push".into();
        found.tags = vec!["ci".into()];
        assert!(matches(&found, &search_terms("")));
        assert!(matches(&found, &search_terms("  BUILD cargo ")));
        assert!(matches(&found, &search_terms("ci push")));
        assert!(!matches(&found, &search_terms("build pnpm")));
    }
}
