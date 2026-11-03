//! Searching the screen and the history.
//!
//! The patterns are alacritty_terminal's [`RegexSearch`], which runs a lazy
//! DFA straight over the grid and follows lines that were soft-wrapped. What
//! this module adds is what a search bar needs on top: every match in one
//! list, so there is a count to show and a current match to step from, and
//! the highlighting of what is on screen.
//!
//! The list is computed when the query changes and when the user steps. While
//! output keeps arriving the lines move under it, so it is recomputed again,
//! but not more often than [`REFRESH`], since a full scan of a long history is
//! the one expensive thing the terminal does.

use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use crate::terminal::Terminal;

/// The most matches kept. The bar only shows a count, and a pattern that
/// matches every cell of a long history would otherwise build a list of
/// millions.
pub const MAX_MATCHES: usize = 1000;

/// The shortest time between two recomputations of the match list while
/// output is arriving.
const REFRESH: Duration = Duration::from_millis(250);

/// How the query is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// Match upper and lower case as different. Off, the search ignores case.
    pub case_sensitive: bool,
    /// Read the query as a regular expression. Off, it is plain text.
    pub regex: bool,
}

/// Which way to step through the matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    /// Towards older output: up the screen.
    Backward,
    /// Towards newer output: down the screen.
    Forward,
}

/// Where the search stands, for the bar's counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchStatus {
    /// The match the search is on, counted from 0 at the top of the history.
    pub current: Option<usize>,
    /// How many matches there are, up to [`MAX_MATCHES`].
    pub count: usize,
    /// Whether there are more than `count`.
    pub truncated: bool,
}

/// The query is not a valid regular expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchError(pub String);

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SearchError {}

pub(crate) struct Search {
    pub(crate) query: String,
    pub(crate) options: SearchOptions,
    regex: RegexSearch,
    pub(crate) matches: Vec<Match>,
    pub(crate) current: Option<usize>,
    truncated: bool,
    /// Output arrived since the list was computed.
    stale: bool,
    computed_at: Instant,
}

impl Search {
    /// When the list is next due for a refresh, if it is out of date.
    pub(crate) fn refresh_due(&self) -> Option<Instant> {
        self.stale.then(|| self.computed_at + REFRESH)
    }

    pub(crate) fn mark_stale(&mut self) {
        self.stale = true;
    }
}

/// The regex source for a query.
///
/// Case is always stated explicitly: alacritty_terminal's own default is
/// "smart case", which would make the option a toggle that sometimes does
/// nothing.
fn pattern(query: &str, options: SearchOptions) -> String {
    let flag = if options.case_sensitive { "(?-i)" } else { "(?i)" };
    if options.regex {
        format!("{flag}{query}")
    } else {
        let mut out = String::from(flag);
        for c in query.chars() {
            if "\\.+*?()|[]{}^$#&-~".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }
}

impl Terminal {
    /// Starts a search, highlights the matches and moves to the one nearest
    /// the bottom of what is on screen, which is the occurrence a person
    /// looking for something in a terminal almost always means.
    ///
    /// An empty query clears the search. A regex that does not compile
    /// returns the reason and leaves the previous search in place, so a
    /// pattern halfway through being typed does not flash the highlights off.
    pub fn search(&mut self, query: &str, options: SearchOptions) -> Result<SearchStatus, SearchError> {
        if query.is_empty() {
            self.clear_search();
            return Ok(SearchStatus::default());
        }
        let regex = RegexSearch::new(&pattern(query, options))
            .map_err(|error| SearchError(error.to_string()))?;
        self.search = Some(Search {
            query: query.to_owned(),
            options,
            regex,
            matches: Vec::new(),
            current: None,
            truncated: false,
            stale: false,
            computed_at: Instant::now(),
        });
        self.recompute_matches();

        // The match nearest the bottom of the screen, looking upward; failing
        // that, the last one in the history.
        let bottom = self.viewport_bottom();
        let search = self.search.as_mut().expect("search was just set");
        let at_or_above = search.matches.iter().rposition(|m| *m.start() <= bottom);
        search.current = at_or_above.or(search.matches.len().checked_sub(1));
        self.reveal_current_match();
        self.bump_all_rows();
        Ok(self.search_status())
    }

    /// Steps to the next or previous match, wrapping at the ends.
    pub fn search_step(&mut self, direction: SearchDirection) -> SearchStatus {
        if self.search.is_none() {
            return SearchStatus::default();
        }
        // The list may be out of date after output; the step is the moment
        // the person wants the right answer, so it is refreshed here.
        self.recompute_matches();
        let origin = self.viewport_origin_for_step();
        let search = self.search.as_mut().expect("search is set");
        let count = search.matches.len();
        if count == 0 {
            return self.search_status();
        }
        let next = match (search.current, direction) {
            (Some(i), SearchDirection::Backward) => (i + count - 1) % count,
            (Some(i), SearchDirection::Forward) => (i + 1) % count,
            // No current match: start from where the screen is.
            (None, SearchDirection::Backward) => search
                .matches
                .iter()
                .rposition(|m| *m.start() < origin)
                .unwrap_or(count - 1),
            (None, SearchDirection::Forward) => search
                .matches
                .iter()
                .position(|m| *m.start() > origin)
                .unwrap_or(0),
        };
        search.current = Some(next);
        self.reveal_current_match();
        self.bump_all_rows();
        self.search_status()
    }

    /// Ends the search and its highlights.
    pub fn clear_search(&mut self) {
        if self.search.take().is_some() {
            self.bump_all_rows();
        }
    }

    /// Whether a search is active.
    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    pub fn search_status(&self) -> SearchStatus {
        match &self.search {
            Some(search) => SearchStatus {
                current: search.current,
                count: search.matches.len(),
                truncated: search.truncated,
            },
            None => SearchStatus::default(),
        }
    }

    /// The text of the match the search is on.
    pub fn current_match_text(&self) -> Option<String> {
        let search = self.search.as_ref()?;
        let m = search.matches.get(search.current?)?;
        Some(self.term.bounds_to_string(*m.start(), *m.end()))
    }

    /// The matches that have a cell on screen, as grid ranges, and whether
    /// each is the current one.
    pub(crate) fn visible_matches(&self) -> impl Iterator<Item = (&Match, bool)> {
        let top = Line(-(self.term.grid().display_offset() as i32));
        let bottom = Line(top.0 + self.rows as i32 - 1);
        let current = self.search.as_ref().and_then(|s| s.current);
        self.search.iter().flat_map(move |search| {
            search
                .matches
                .iter()
                .enumerate()
                .filter(move |(_, m)| m.end().line >= top && m.start().line <= bottom)
                .map(move |(i, m)| (m, Some(i) == current))
        })
    }

    /// Recomputes the match list if it is out of date and due.
    pub(crate) fn refresh_search(&mut self, now: Instant) {
        let due = self.search.as_ref().and_then(|s| s.refresh_due());
        if due.is_some_and(|at| now >= at) {
            self.recompute_matches();
            self.bump_all_rows();
        }
    }

    fn recompute_matches(&mut self) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let previous = search
            .current
            .and_then(|i| search.matches.get(i))
            .map(|m| *m.start());
        let term = &self.term;
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        let mut matches = Vec::new();
        let mut truncated = false;
        for m in RegexIter::new(start, end, Direction::Right, term, &mut search.regex) {
            if matches.len() >= MAX_MATCHES {
                truncated = true;
                break;
            }
            matches.push(m);
        }
        // Keep the current match where it is when the list is recomputed: the
        // one that starts at the same cell, or failing that the nearest after.
        search.current = match previous {
            Some(p) => matches.iter().position(|m| *m.start() >= p).or(matches.len().checked_sub(1)),
            None => search.current.filter(|&i| i < matches.len()),
        };
        search.matches = matches;
        search.truncated = truncated;
        search.stale = false;
        search.computed_at = Instant::now();
    }

    /// The bottom-right of what is on screen.
    fn viewport_bottom(&self) -> Point {
        let offset = self.term.grid().display_offset() as i32;
        Point::new(Line(self.rows as i32 - 1 - offset), self.term.last_column())
    }

    fn viewport_origin_for_step(&self) -> Point {
        let offset = self.term.grid().display_offset() as i32;
        Point::new(Line(-offset), Column(0))
    }

    /// Scrolls so the current match is visible, with a few lines of context
    /// above it when it has to move.
    fn reveal_current_match(&mut self) {
        let Some(search) = &self.search else { return };
        let Some(m) = search.current.and_then(|i| search.matches.get(i)) else {
            return;
        };
        let line = m.start().line;
        let offset = self.term.grid().display_offset() as i32;
        let top = -offset;
        let bottom = top + self.rows as i32 - 1;
        if line.0 >= top && line.0 <= bottom {
            return;
        }
        // Centre it: the line that is `rows / 2` below the top of the screen.
        let target = (-line.0 + self.rows as i32 / 2).max(0) as usize;
        let target = target.min(self.term.history_size());
        self.set_display_offset(target);
    }

    /// Clamps a point to the grid, as used for a search origin.
    #[allow(dead_code)]
    pub(crate) fn clamp_point(&self, point: Point) -> Point {
        point.grid_clamp(&self.term, Boundary::Grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn terminal_with(text: &str, columns: u16, rows: u16) -> Terminal {
        let mut term = Terminal::new(Config::default(), columns, rows);
        term.feed(text.as_bytes());
        term
    }

    fn lines(n: usize) -> String {
        (0..n).map(|i| format!("line {i} needle {}\r\n", i % 3)).collect()
    }

    #[test]
    fn plain_text_is_not_a_regex() {
        let mut term = terminal_with("a.c abc a.c\r\n", 40, 5);
        let status = term.search("a.c", SearchOptions::default()).unwrap();
        assert_eq!(status.count, 2);
    }

    #[test]
    fn regex_mode_reads_the_pattern() {
        let mut term = terminal_with("a.c abc a-c\r\n", 40, 5);
        let options = SearchOptions { regex: true, ..SearchOptions::default() };
        assert_eq!(term.search("a.c", options).unwrap().count, 3);
        assert_eq!(term.search("a\\.c", options).unwrap().count, 1);
    }

    #[test]
    fn case_is_ignored_unless_asked() {
        let mut term = terminal_with("Foo foo FOO\r\n", 40, 5);
        assert_eq!(term.search("foo", SearchOptions::default()).unwrap().count, 3);
        let sensitive = SearchOptions { case_sensitive: true, ..SearchOptions::default() };
        assert_eq!(term.search("foo", sensitive).unwrap().count, 1);
        // Upper case in the query does not switch the mode on by itself.
        assert_eq!(term.search("Foo", SearchOptions::default()).unwrap().count, 3);
    }

    #[test]
    fn a_broken_regex_is_reported_and_keeps_the_last_search() {
        let mut term = terminal_with("foo\r\n", 40, 5);
        let options = SearchOptions { regex: true, ..SearchOptions::default() };
        term.search("fo+", options).unwrap();
        assert!(term.search("fo(", options).is_err());
        assert_eq!(term.search_status().count, 1);
    }

    #[test]
    fn search_starts_on_the_match_nearest_the_bottom_and_steps_up_then_wraps() {
        let mut term = terminal_with(&lines(10), 40, 5);
        let status = term.search("needle", SearchOptions::default()).unwrap();
        assert_eq!(status.count, 10);
        assert_eq!(status.current, Some(9));
        let status = term.search_step(SearchDirection::Backward);
        assert_eq!(status.current, Some(8));
        let status = term.search_step(SearchDirection::Forward);
        assert_eq!(status.current, Some(9));
        // Forward from the last wraps to the first.
        let status = term.search_step(SearchDirection::Forward);
        assert_eq!(status.current, Some(0));
        let status = term.search_step(SearchDirection::Backward);
        assert_eq!(status.current, Some(9));
    }

    #[test]
    fn stepping_to_a_match_in_the_history_scrolls_to_it() {
        let mut term = terminal_with(&lines(40), 40, 5);
        term.search("line 3 ", SearchOptions::default()).unwrap();
        // The match is far above the five rows on screen.
        assert!(term.display_offset() > 0);
        assert_eq!(term.current_match_text().as_deref(), Some("line 3 "));
        let (first, is_current) = term.visible_matches().next().unwrap();
        assert!(is_current);
        assert_eq!(first.start().line, first.end().line);
    }

    #[test]
    fn matches_across_a_soft_wrap_are_found() {
        // Ten columns: "0123456789abcdef" wraps after the ninth cell.
        let mut term = terminal_with("0123456789abcdef\r\n", 10, 5);
        assert_eq!(term.search("89ab", SearchOptions::default()).unwrap().count, 1);
    }

    #[test]
    fn clearing_the_search_removes_it() {
        let mut term = terminal_with("foo\r\n", 40, 5);
        term.search("foo", SearchOptions::default()).unwrap();
        assert!(term.is_searching());
        term.clear_search();
        assert!(!term.is_searching());
        assert_eq!(term.search_status(), SearchStatus::default());
        assert_eq!(term.search("", SearchOptions::default()).unwrap(), SearchStatus::default());
    }

    #[test]
    fn the_count_is_capped() {
        let text = "x".repeat(40).repeat(100) + "\r\n";
        let mut term = terminal_with(&text, 40, 5);
        let status = term.search("x", SearchOptions::default()).unwrap();
        assert_eq!(status.count, MAX_MATCHES);
        assert!(status.truncated);
    }
}
