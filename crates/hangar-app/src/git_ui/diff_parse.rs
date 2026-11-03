//! A unified diff, as `git diff` prints it, cut into rows the diff view can
//! colour and number (`lib/diff.ts`). Read-only: nothing here writes a patch
//! back.

use std::sync::LazyLock;

use regex::Regex;

/// What a row of the diff is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRowKind {
    /// The file's header: `diff --git`, `index`, `---`, `+++`.
    Meta,
    /// `@@ -3,4 +3,5 @@ fn a()`.
    Hunk,
    Add,
    Del,
    Ctx,
    /// `\ No newline at end of file`.
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: DiffRowKind,
    pub text: String,
    /// Line number on the old side; absent on additions and outside hunks.
    pub old_line: Option<u32>,
    /// Line number on the new side; absent on deletions and outside hunks.
    pub new_line: Option<u32>,
}

impl DiffRow {
    fn bare(kind: DiffRowKind, text: &str) -> DiffRow {
        DiffRow {
            kind,
            text: text.to_string(),
            old_line: None,
            new_line: None,
        }
    }
}

/// Cuts `text` into rows, numbering both sides from each hunk header.
pub fn parse_unified_diff(text: &str) -> Vec<DiffRow> {
    static HUNK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@").unwrap());

    let mut rows = Vec::new();
    let (mut old_line, mut new_line) = (0u32, 0u32);
    // Header lines ("--- a/x", "+++ b/x") look like a deletion and an
    // addition; only a hunk header says the body has started.
    let mut in_hunk = false;

    let mut lines: Vec<&str> = text.split('\n').collect();
    // The newline that ends the last line is not a line of its own.
    if lines.last() == Some(&"") {
        lines.pop();
    }

    for raw in lines {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(hunk) = HUNK.captures(line) {
            old_line = hunk[1].parse().unwrap_or(0);
            new_line = hunk[2].parse().unwrap_or(0);
            in_hunk = true;
            rows.push(DiffRow::bare(DiffRowKind::Hunk, line));
            continue;
        }
        if line.starts_with("diff --git ") {
            in_hunk = false;
        }
        if !in_hunk {
            rows.push(DiffRow::bare(DiffRowKind::Meta, line));
            continue;
        }
        // The first character says what the line is; the rest is the code.
        let mut chars = line.chars();
        let first = chars.next();
        let rest = chars.as_str();
        match first {
            Some('+') => {
                rows.push(DiffRow {
                    kind: DiffRowKind::Add,
                    text: rest.to_string(),
                    old_line: None,
                    new_line: Some(new_line),
                });
                new_line += 1;
            }
            Some('-') => {
                rows.push(DiffRow {
                    kind: DiffRowKind::Del,
                    text: rest.to_string(),
                    old_line: Some(old_line),
                    new_line: None,
                });
                old_line += 1;
            }
            Some('\\') => rows.push(DiffRow::bare(DiffRowKind::Note, line)),
            _ => {
                rows.push(DiffRow {
                    kind: DiffRowKind::Ctx,
                    text: rest.to_string(),
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                });
                old_line += 1;
                new_line += 1;
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::DiffRowKind::{Add, Ctx, Del, Hunk, Meta, Note};
    use super::*;
    use crate::inspector::files_model::split_path;

    fn sample() -> String {
        [
            "diff --git a/src/a.ts b/src/a.ts",
            "index 1111111..2222222 100644",
            "--- a/src/a.ts",
            "+++ b/src/a.ts",
            "@@ -3,4 +3,5 @@ export function a() {",
            " keep",
            "-old",
            "+new",
            "+added",
            " tail",
            "\\ No newline at end of file",
            "",
        ]
        .join("\n")
    }

    #[test]
    fn the_file_header_stays_meta_even_though_it_starts_with_a_minus_and_a_plus() {
        let rows = parse_unified_diff(&sample());
        let kinds: Vec<_> = rows[..4].iter().map(|row| row.kind).collect();
        assert_eq!(kinds, [Meta, Meta, Meta, Meta]);
        assert_eq!(rows[2].text, "--- a/src/a.ts");
    }

    #[test]
    fn both_sides_are_numbered_from_the_hunk_header() {
        let rows: Vec<_> = parse_unified_diff(&sample())
            .into_iter()
            .filter(|row| row.kind != Meta)
            .map(|row| (row.kind, row.text, row.old_line, row.new_line))
            .collect();
        let line = |kind, text: &str, old, new| (kind, text.to_string(), old, new);
        assert_eq!(
            rows,
            vec![
                line(Hunk, "@@ -3,4 +3,5 @@ export function a() {", None, None),
                line(Ctx, "keep", Some(3), Some(3)),
                line(Del, "old", Some(4), None),
                line(Add, "new", None, Some(4)),
                line(Add, "added", None, Some(5)),
                line(Ctx, "tail", Some(5), Some(6)),
                line(Note, "\\ No newline at end of file", None, None),
            ]
        );
    }

    #[test]
    fn it_goes_back_to_headers_at_the_next_file_and_drops_carriage_returns() {
        let two = format!(
            "{}diff --git a/b b/b\r\n--- a/b\r\n+++ b/b\r\n@@ -1 +1 @@\r\n-x\r\n+y\r\n",
            sample()
        );
        let rows = parse_unified_diff(&two);
        let from = rows
            .iter()
            .position(|row| row.text == "diff --git a/b b/b")
            .expect("the second file starts");
        let second = &rows[from..];
        let kinds: Vec<_> = second.iter().map(|row| row.kind).collect();
        assert_eq!(kinds, [Meta, Meta, Meta, Hunk, Del, Add]);
        assert_eq!(second[5].text, "y");
    }

    #[test]
    fn an_empty_diff_is_no_rows() {
        assert!(parse_unified_diff("").is_empty());
    }

    #[test]
    fn a_line_with_nothing_after_the_marker_is_an_empty_line_of_code() {
        // Some tools strip the space of an empty context line.
        let rows = parse_unified_diff("@@ -1,2 +1,2 @@\n\n+x\n");
        assert_eq!(rows[1].kind, Ctx);
        assert_eq!(rows[1].text, "");
        assert_eq!((rows[1].old_line, rows[1].new_line), (Some(1), Some(1)));
        assert_eq!(rows[2].new_line, Some(2));
    }

    #[test]
    fn the_marker_is_one_character_not_one_byte() {
        let rows = parse_unified_diff("@@ -1 +1 @@\n+été\n");
        assert_eq!(rows[1].text, "été");
    }

    #[test]
    fn the_folder_is_separated_from_the_file_name() {
        assert_eq!(
            split_path("src/lib/diff.ts"),
            ("src/lib/".to_string(), "diff.ts".to_string())
        );
        assert_eq!(
            split_path("README.md"),
            (String::new(), "README.md".to_string())
        );
    }
}
