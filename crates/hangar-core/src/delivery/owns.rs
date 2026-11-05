//! What a task `owns`, read the way the board reads it.
//!
//! The board decides whether two tasks may run side by side by reading each
//! one's `owns` (`board::owns_overlap`); the delivery decides which changed
//! files fall outside it. Both must read a pattern the same way, or a
//! delivery is refused for files the board let the task claim: braces
//! (`src/{lib,components}`), `[…]` classes, case, and a folder owning what is
//! under it.
//!
//! The board (`board::owns_overlap`) reads patterns with the functions of this
//! module (`expand_braces`, `owned_segments`, `glob_matches`): there is one
//! copy of the rules. `tests::agrees_with_the_board` keeps the delivery's
//! `covers` and the board's `owns_overlap` answering one table the same way.

/// Past this many alternatives a brace pattern is read no further: a pattern
/// that large is a mistake, and the ones read already decide most overlaps.
const MAX_ALTERNATIVES: usize = 64;

/// Segments past this are not read: nobody owns a path that deep, and each
/// `**` makes the comparison more expensive.
const MAX_SEGMENTS: usize = 32;

/// `src/{lib,components}/x` as the two patterns it stands for.
pub(crate) fn expand_braces(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut depth = 0;
    let mut open = 0;
    for (at, &c) in chars.iter().enumerate() {
        match c {
            '{' => {
                if depth == 0 {
                    open = at;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth > 0 {
                    continue;
                }
                let alternatives = split_alternatives(&chars[open + 1..at]);
                // `{x}` alone is not a choice: its braces stay as they are.
                if alternatives.len() < 2 {
                    continue;
                }
                let head: String = chars[..open].iter().collect();
                let tail: String = chars[at + 1..].iter().collect();
                return alternatives
                    .iter()
                    .flat_map(|alternative| expand_braces(&format!("{head}{alternative}{tail}")))
                    .take(MAX_ALTERNATIVES)
                    .collect();
            }
            _ => {}
        }
    }
    vec![pattern.to_string()]
}

/// The commas of one brace group that are not inside a nested one.
fn split_alternatives(inner: &[char]) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut depth = 0;
    for &c in inner {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(String::new());
                continue;
            }
            _ => {}
        }
        parts.last_mut().unwrap().push(c);
    }
    parts
}

/// A pattern as the rule compares it: relative, in forward slashes, without
/// `./`, and in lower case — the filesystems that ignore case would put both
/// spellings on one file, and erring towards an overlap only costs a refusal
/// that says why. None for a pattern that names nothing.
///
/// A folder owns what is under it, so it ends in an implicit `**`. A name with
/// an extension reads as a file and does not: `src/app.ts` is not where
/// `**/*.test.ts` lives. A folder whose name has a dot in it is written with a
/// trailing slash, or as `name/**`.
pub(crate) fn owned_segments(pattern: &str) -> Option<Vec<String>> {
    let normalized = pattern.trim().replace('\\', "/").to_lowercase();
    let mut segments: Vec<String> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .take(MAX_SEGMENTS)
        .map(str::to_string)
        .collect();
    let last = segments.last()?;
    let file = !normalized.ends_with('/') && last.char_indices().any(|(at, c)| c == '.' && at > 0);
    if !file && last != "**" {
        segments.push("**".to_string());
    }
    Some(segments)
}

/// One name against one segment pattern: `*`, `?` and `[…]` (`[!…]` or
/// `[^…]` negated, with ranges). None of them crosses a separator.
pub(crate) fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    glob_from(&pattern, &name)
}

fn glob_from(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => {
            let rest = &pattern[1..];
            (0..=name.len()).any(|skipped| glob_from(rest, &name[skipped..]))
        }
        Some('?') => !name.is_empty() && glob_from(&pattern[1..], &name[1..]),
        Some('[') => match (char_class(pattern), name.first()) {
            (Some((length, matches)), Some(&c)) => {
                matches(c) && glob_from(&pattern[length..], &name[1..])
            }
            (Some(_), None) => false,
            // No closing bracket: a plain `[`.
            (None, first) => first == Some(&'[') && glob_from(&pattern[1..], &name[1..]),
        },
        Some(c) => name.first() == Some(c) && glob_from(&pattern[1..], &name[1..]),
    }
}

/// The class `pattern` opens with, as its length and what it accepts.
#[allow(clippy::type_complexity)]
fn char_class(pattern: &[char]) -> Option<(usize, Box<dyn Fn(char) -> bool>)> {
    let mut at = 1;
    let negated = matches!(pattern.get(at), Some('!') | Some('^'));
    if negated {
        at += 1;
    }
    let start = at;
    // A `]` first in the class is one of its characters.
    let close = (start..pattern.len()).find(|&i| pattern[i] == ']' && i > start)?;
    let members: Vec<char> = pattern[start..close].to_vec();
    let accepts = move |c: char| {
        let mut i = 0;
        let mut found = false;
        while i < members.len() {
            if i + 2 < members.len() && members[i + 1] == '-' {
                found |= members[i] <= c && c <= members[i + 2];
                i += 3;
            } else {
                found |= members[i] == c;
                i += 1;
            }
        }
        found != negated
    };
    Some((close + 1, Box::new(accepts)))
}

/// Whether `owns` names any file at all. A task whose patterns name nothing
/// (empty, blank, `.`) states no files, as far as the board is concerned.
pub(crate) fn scoped(owns: &[String]) -> bool {
    owns.iter().any(|pattern| owned_segments(pattern).is_some())
}

/// Whether `owns` — paths and globs relative to the workspace folder, which
/// sits at `prefix` in the repository (`""`, or `app/`) — covers `path`,
/// relative to the repository. A path owns everything under it, and so does
/// a glob matching a folder: `src/lib` and `src/*` both own `src/lib/a.ts`.
/// `*`, `?` and `[…]` stay within a name, `**` crosses folders.
pub(super) fn covers(path: &str, owns: &[String], prefix: &str) -> bool {
    let path = path.replace('\\', "/").to_lowercase();
    let path: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let prefix = prefix.replace('\\', "/").to_lowercase();
    let prefix: Vec<&str> = prefix.split('/').filter(|s| !s.is_empty()).collect();
    owns.iter().any(|pattern| {
        expand_braces(pattern)
            .iter()
            .filter_map(|each| owned_segments(each))
            .any(|segments| {
                let wanted: Vec<&str> = prefix
                    .iter()
                    .copied()
                    .chain(segments.iter().map(String::as_str))
                    .collect();
                matches_path(&wanted, &path)
            })
    })
}

/// `pattern` against `path`, segment by segment, `**` standing for any number
/// of segments, none included. Filled bottom-up: no pattern can make it
/// backtrack.
fn matches_path(pattern: &[&str], path: &[&str]) -> bool {
    let width = path.len() + 1;
    // reach[i * width + j]: pattern[i..] matches path[j..].
    let mut reach = vec![false; (pattern.len() + 1) * width];
    reach[pattern.len() * width + path.len()] = true;
    for i in (0..pattern.len()).rev() {
        for j in (0..=path.len()).rev() {
            reach[i * width + j] = if pattern[i] == "**" {
                reach[(i + 1) * width + j] || (j < path.len() && reach[i * width + j + 1])
            } else {
                j < path.len()
                    && reach[(i + 1) * width + j + 1]
                    && glob_matches(pattern[i], path[j])
            };
        }
    }
    reach[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owns(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(|p| p.to_string()).collect()
    }

    /// (patterns, path, prefix, covered). Each path names a file with an
    /// extension, so the board reads it as a file and `owns_overlap` against
    /// it is the same question as `covers`.
    const TABLE: &[(&[&str], &str, &str, bool)] = &[
        (&["src/lib/a.ts"], "src/lib/a.ts", "", true),
        (&["src/lib"], "src/lib/a.ts", "", true),
        (&["./src/lib/"], "src/lib/a.ts", "", true),
        (&["src/*"], "src/lib/a.ts", "", true),
        (&["src/**/*.ts"], "src/lib/deep/a.ts", "", true),
        (&["src/**/*.ts"], "src/a.ts", "", true),
        (&["src/lib/*.test.ts"], "src/lib/a.test.ts", "", true),
        (&["src/lib/*.test.ts"], "src/lib/a.ts", "", false),
        (&["src\\lib"], "src/lib/a.ts", "", true),
        (&["src/lib/b.ts", "docs"], "src/lib/a.ts", "", false),
        (&["src/lib"], "src/library/a.ts", "", false),
        (&["src/lib/*.ts"], "src/lib/a.tsx", "", false),
        // Braces, as the orchestrator briefs write them.
        (
            &["crates/app/src/{orchestration,board}/"],
            "crates/app/src/board/x.rs",
            "",
            true,
        ),
        (
            &["crates/app/src/{orchestration,board}/"],
            "crates/app/src/orchestration/w/y.rs",
            "",
            true,
        ),
        (
            &["crates/app/src/{orchestration,board}/"],
            "crates/app/src/term/x.rs",
            "",
            false,
        ),
        (&["src/{a,b/{c,d}}/x.rs"], "src/b/d/x.rs", "", true),
        (&["src/{a,b/{c,d}}/x.rs"], "src/b/e/x.rs", "", false),
        (&["src/{only}/x.rs"], "src/only/x.rs", "", false),
        // Classes.
        (&["src/file[0-9].ts"], "src/file7.ts", "", true),
        (&["src/file[0-9].ts"], "src/filex.ts", "", false),
        (&["src/file[!0-9].ts"], "src/filex.ts", "", true),
        (&["src/file[^0-9].ts"], "src/file7.ts", "", false),
        // Case never decides.
        (&["SRC/Lib"], "src/lib/a.ts", "", true),
        (&["src/lib/*.TS"], "src/lib/A.ts", "", true),
        // A folder with a dot in its name is written with a slash.
        (&["vendor.d/"], "vendor.d/x.c", "", true),
        // The workspace folder inside the repository.
        (&["src"], "app/src/a.ts", "app/", true),
        (&["src"], "src/a.ts", "app/", false),
        (&["src/{a,b}"], "app/src/b/x.ts", "app/", true),
        (&["src"], "app/other/a.ts", "app/", false),
    ];

    #[test]
    fn what_a_task_owns_covers_its_paths_folders_globs_braces_and_classes() {
        for (patterns, path, prefix, expected) in TABLE {
            assert_eq!(
                covers(path, &owns(patterns), prefix),
                *expected,
                "{patterns:?} against {path} (workspace at {prefix:?})"
            );
        }
    }

    #[test]
    fn agrees_with_the_board() {
        for (patterns, path, prefix, expected) in TABLE {
            if !prefix.is_empty() {
                continue;
            }
            let overlap = crate::board::owns_overlap(&owns(patterns), &owns(&[path]));
            assert_eq!(
                !overlap.is_empty(),
                *expected,
                "the board reads {patterns:?} against {path} differently"
            );
        }
    }

    #[test]
    fn a_pattern_that_names_nothing_states_no_files() {
        assert!(!scoped(&[]));
        assert!(!scoped(&owns(&["", "  ", ".", "./"])));
        assert!(scoped(&owns(&[".", "src"])));
        assert!(!covers("a.ts", &owns(&[".", ""]), ""));
    }

    #[test]
    fn many_double_stars_never_make_the_match_expensive() {
        let pattern = owns(&["**/**/**/**/**/**/**/**/**/**/**/**/zzz.ts"]);
        let path = "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/a.ts";
        assert!(!covers(path, &pattern, ""));
    }
}
