//! Links in the grid: `OSC 8` hyperlinks and URLs spotted in the text.
//!
//! A hyperlink is whatever the program marked, and carries its own target. A
//! detected link is a URL found in the characters. Either is a link only when
//! it points at the web (`http`, `https`; [`is_web_link`]): that is all a
//! click opens, as xterm.js's web-links addon was all the old app had, and a
//! link underlined under the pointer that a click then ignores promises what
//! it does not do. Both are looked up under
//! the pointer when it moves, never for the whole screen: a screenful of
//! output is rarely hovered over, and scanning it all on every frame is the
//! kind of cost a terminal has to avoid.
//!
//! A URL can run over the end of a row, since the emulator wraps long lines,
//! so the search works on the *logical* line: the row under the pointer and
//! every row joined to it by a soft wrap.

use std::ops::Range;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

use crate::event::{Link, LinkSource};

/// A run of cells of one row, columns inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub line: Line,
    pub first: usize,
    pub last: usize,
}

/// A link and the cells it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkHit {
    pub link: Link,
    pub spans: Vec<Span>,
}

/// How many rows a logical line may span. A program that prints a megabyte on
/// one line would otherwise make a hover scan all of it.
const MAX_ROWS: i32 = 24;

/// The link under `point`, if any.
pub(crate) fn link_at<T>(term: &Term<T>, point: Point) -> Option<LinkHit> {
    let grid = term.grid();
    let columns = term.columns();
    if columns == 0 || point.line < term.topmost_line() || point.line > term.bottommost_line() {
        return None;
    }

    // The logical line: up while the row above wraps into this one, down while
    // this one wraps into the next.
    let mut first = point.line;
    while first > term.topmost_line()
        && point.line.0 - first.0 < MAX_ROWS
        && grid[Line(first.0 - 1)][Column(columns - 1)]
            .flags
            .contains(Flags::WRAPLINE)
    {
        first = Line(first.0 - 1);
    }
    let mut last = point.line;
    while last < term.bottommost_line()
        && last.0 - point.line.0 < MAX_ROWS
        && grid[last][Column(columns - 1)]
            .flags
            .contains(Flags::WRAPLINE)
    {
        last = Line(last.0 + 1);
    }

    // The characters, with the cell each one sits in. The spacer after a wide
    // character is not a character of its own.
    let mut chars = Vec::new();
    let mut cells: Vec<Point> = Vec::new();
    let mut wide = Vec::new();
    let mut line = first;
    while line <= last {
        for column in 0..columns {
            let cell = &grid[line][Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            chars.push(if cell.c == '\0' || cell.c == '\t' {
                ' '
            } else {
                cell.c
            });
            cells.push(Point::new(line, Column(column)));
            wide.push(cell.flags.contains(Flags::WIDE_CHAR));
        }
        line = Line(line.0 + 1);
    }

    // The pointer may be on the spacer of a wide character: that is the
    // character before it.
    let target = cells
        .iter()
        .rposition(|p| p.line == point.line && p.column <= point.column)
        .filter(|&i| {
            let p = cells[i];
            p.column == point.column || (wide[i] && p.column.0 + 1 == point.column.0)
        })?;

    let (range, link) =
        hyperlink_range(term, &cells, target).or_else(|| url_range(&chars, target))?;
    Some(LinkHit {
        link,
        spans: spans_of(&cells[range.clone()], &wide[range]),
    })
}

/// The cells around `target` carrying the same `OSC 8` hyperlink.
fn hyperlink_range<T>(
    term: &Term<T>,
    cells: &[Point],
    target: usize,
) -> Option<(Range<usize>, Link)> {
    let grid = term.grid();
    let at = |i: usize| grid[cells[i]].hyperlink();
    let hyperlink = at(target)?;
    // A hyperlink elsewhere than the web is not a link; the text it marks
    // may still hold a web address of its own.
    if !is_web_link(hyperlink.uri()) {
        return None;
    }
    let mut start = target;
    while start > 0 && at(start - 1).as_ref() == Some(&hyperlink) {
        start -= 1;
    }
    let mut end = target + 1;
    while end < cells.len() && at(end).as_ref() == Some(&hyperlink) {
        end += 1;
    }
    Some((
        start..end,
        Link {
            uri: hyperlink.uri().to_owned(),
            source: LinkSource::Hyperlink,
        },
    ))
}

fn url_range(chars: &[char], target: usize) -> Option<(Range<usize>, Link)> {
    let range = find_urls(chars).into_iter().find(|r| r.contains(&target))?;
    let uri: String = chars[range.clone()].iter().collect();
    Some((
        range,
        Link {
            uri,
            source: LinkSource::Detected,
        },
    ))
}

fn spans_of(cells: &[Point], wide: &[bool]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (point, &is_wide) in cells.iter().zip(wide) {
        let last = point.column.0 + is_wide as usize;
        match spans.last_mut() {
            Some(span) if span.line == point.line => span.last = last,
            _ => spans.push(Span {
                line: point.line,
                first: point.column.0,
                last,
            }),
        }
    }
    spans
}

/// The schemes taken for links: the web's alone. A `file:`, `ssh:` or `git:`
/// URL a program prints would hand the system's handler for that scheme
/// whatever was printed, on one click, so none of them opens, and a link
/// that opens nothing is worse than one that is not underlined. `foo://`
/// followed by text is often a path or a pattern in output anyway.
const SCHEMES: &[&str] = &["http", "https"];

/// Whether `uri` is a link a click opens: an `http` or `https` address with
/// something after the `//`. What the terminal reports as a link
/// ([`Event::OpenLink`](crate::Event::OpenLink), the underline under the
/// pointer) always is one; an app that opens links can check again with this.
pub fn is_web_link(uri: &str) -> bool {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return false;
    };
    SCHEMES.iter().any(|web| scheme.eq_ignore_ascii_case(web)) && !rest.is_empty()
}

/// The ranges of `chars` that are URLs.
pub(crate) fn find_urls(chars: &[char]) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut i = 0;
    while i + 3 <= chars.len() {
        if chars[i] == ':' && chars[i + 1] == '/' && chars[i + 2] == '/' {
            let mut start = i;
            while start > 0
                && (chars[start - 1].is_ascii_alphanumeric()
                    || matches!(chars[start - 1], '+' | '-' | '.'))
            {
                start -= 1;
            }
            let scheme: String = chars[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            if SCHEMES.contains(&scheme.as_str()) {
                let mut end = i + 3;
                while end < chars.len() && is_url_char(chars[end]) {
                    end += 1;
                }
                let end = trim_end(chars, i + 3, end);
                if end > i + 3 {
                    found.push(start..end);
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

fn is_url_char(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_graphic()
            && !matches!(
                c,
                '"' | '\'' | '<' | '>' | '`' | '\\' | '^' | '{' | '}' | '|'
            );
    }
    // Non-ASCII is allowed (internationalised addresses), except what only
    // ever delimits a URL in a terminal: the ellipsis that truncates long
    // ones, other punctuation, and the box-drawing characters of the frames
    // programs draw around their output.
    let code = c as u32;
    !c.is_whitespace()
        && !matches!(code, 0x2000..=0x206f | 0x2500..=0x259f | 0x3000..=0x3003 | 0xff00..=0xff0f)
}

/// Where a URL really ends: trailing punctuation belongs to the sentence, and
/// a closing bracket belongs to it unless the URL opened one.
fn trim_end(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        let c = chars[end - 1];
        let drop = match c {
            '.' | ',' | ';' | ':' | '!' | '?' | '*' => true,
            ')' => balance(&chars[start..end], '(', ')') < 0,
            ']' => balance(&chars[start..end], '[', ']') < 0,
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    end
}

fn balance(chars: &[char], open: char, close: char) -> i32 {
    chars.iter().fold(0, |n, &c| {
        if c == open {
            n + 1
        } else if c == close {
            n - 1
        } else {
            n
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        find_urls(&chars)
            .into_iter()
            .map(|r| chars[r].iter().collect())
            .collect()
    }

    #[test]
    fn plain_urls() {
        assert_eq!(
            urls("see https://hangar.ai/docs now"),
            ["https://hangar.ai/docs"]
        );
        assert_eq!(
            urls("a http://localhost:3000/x?y=1#z b"),
            ["http://localhost:3000/x?y=1#z"]
        );
    }

    #[test]
    fn trailing_punctuation_is_the_sentences() {
        assert_eq!(urls("Go to https://hangar.ai."), ["https://hangar.ai"]);
        assert_eq!(urls("(see https://hangar.ai/a)"), ["https://hangar.ai/a"]);
        assert_eq!(urls("https://hangar.ai/a, then"), ["https://hangar.ai/a"]);
    }

    #[test]
    fn brackets_the_url_opened_stay() {
        assert_eq!(
            urls("https://en.wikipedia.org/wiki/Rust_(programming_language)"),
            ["https://en.wikipedia.org/wiki/Rust_(programming_language)"]
        );
    }

    #[test]
    fn quotes_and_frames_end_a_url() {
        assert_eq!(urls("'https://hangar.ai/x'"), ["https://hangar.ai/x"]);
        assert_eq!(urls("\"https://hangar.ai/x\""), ["https://hangar.ai/x"]);
        assert_eq!(urls("│https://hangar.ai/x│"), ["https://hangar.ai/x"]);
        assert_eq!(
            urls("https://hangar.ai/very/long…"),
            ["https://hangar.ai/very/long"]
        );
        assert_eq!(urls("<https://hangar.ai/x>"), ["https://hangar.ai/x"]);
    }

    #[test]
    fn several_urls_and_unknown_schemes() {
        assert_eq!(
            urls("https://a.io/1 and https://b.io/2"),
            ["https://a.io/1", "https://b.io/2"]
        );
        assert!(urls("foo://bar and https://").is_empty());
        assert!(urls("not a url: just text").is_empty());
    }

    #[test]
    fn schemes_are_case_insensitive_and_only_the_web_s_are_links() {
        assert_eq!(urls("HTTPS://HANGAR.AI/X"), ["HTTPS://HANGAR.AI/X"]);
        // Nothing opens these, so nothing underlines them.
        for other in [
            "file:///home/me/a.txt",
            "ssh://git@github.com/x/y",
            "git://github.com/x/y",
            "sftp://host/x",
            "ftp://example.com/x",
            "wss://example.com/socket",
        ] {
            assert!(urls(&format!("see {other} now")).is_empty(), "{other}");
        }
        assert_eq!(
            urls("file:///a and https://hangar.ai/b"),
            ["https://hangar.ai/b"]
        );
    }

    #[test]
    fn only_web_links_open() {
        for uri in [
            "https://example.com/page?q=1",
            "http://localhost:5173",
            "HTTPS://EXAMPLE.COM",
        ] {
            assert!(is_web_link(uri), "{uri}");
        }
        for uri in [
            "file:///etc/passwd",
            "file://host/share",
            "ssh://git@github.com/x/y",
            "git://github.com/x/y",
            "ftp://example.com",
            "smb://server/share",
            "javascript:alert(1)",
            "mailto:someone@example.com",
            "https://",
            "https:example.com",
            " https://example.com",
            "/home/me/file.txt",
            "",
        ] {
            assert!(!is_web_link(uri), "{uri}");
        }
    }
}
