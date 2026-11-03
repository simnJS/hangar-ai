//! What Hangar sends a kit when someone uses its page.

use serde::{Deserialize, Serialize};

use crate::draw::{Mark, Point};
use crate::element::HandlerId;

/// A widget was used: the handler it named, in the view it was drawn from,
/// and what it carries.
///
/// `view` is the `seq` of the [`View`](crate::abi::View) the widget was part
/// of. Hangar may draw a new view between a click and its delivery, so a
/// kit resolves `handler` against the view it came from, not its latest:
/// the SDK keeps the handlers of its last few views for that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiEvent {
    pub view: u32,
    pub handler: HandlerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<EventValue>,
}

/// What an event carries, by widget: see each handler's documentation.
///
/// In JSON, `{ "text": "abc" }`, `{ "bool": true }`, `{ "index": 3 }`,
/// `{ "sort": { "column": 1, "descending": false } }`, `{ "mark": … }`,
/// `{ "point": { "x": 4, "y": 9 } }`, `{ "number": 0.5 }` or
/// `{ "range": { "start": 200, "end": 600 } }`; absent for a press.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventValue {
    /// A text typed; an option, a tab, a menu item or a tree node chosen, by
    /// its id or value.
    Text(String),
    /// A checkbox or a switch toggled: its new state.
    Bool(bool),
    /// A row of a table pressed: its index in the rows the kit gave.
    Index(u32),
    /// A table's header pressed.
    Sort(Sort),
    /// A drag on a canvas finished: what it drew.
    Mark(Mark),
    /// A press on a canvas, in its units.
    Point(Point),
    /// A number: a canvas's scale.
    Number(f64),
    /// The lines of a long log to give next.
    Range(Range),
}

/// Lines of a long log, from `start` to before `end`, as indices in the
/// whole log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start: u32,
    pub end: u32,
}

/// The order of a table's rows: by a column, one way or the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    /// Its index in the table's columns.
    pub column: u32,
    #[serde(default)]
    pub descending: bool,
}

impl Sort {
    pub fn ascending(column: u32) -> Self {
        Sort {
            column,
            descending: false,
        }
    }

    pub fn descending(column: u32) -> Self {
        Sort {
            column,
            descending: true,
        }
    }

    /// What a press on `column`'s header asks for, the table being in
    /// `current`: the same column turns around, another starts ascending.
    pub fn next(current: Option<Sort>, column: u32) -> Sort {
        match current {
            Some(sort) if sort.column == column => Sort {
                column,
                descending: !sort.descending,
            },
            _ => Sort::ascending(column),
        }
    }
}
