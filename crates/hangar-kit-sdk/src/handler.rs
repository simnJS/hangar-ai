//! What a widget does when it is used, on the kit's side: a message of the
//! kit's own, or a function that makes one of what the widget carries.

use std::fmt;
use std::rc::Rc;

use hangar_kit_ui::{EventValue, Mark, Point, Range, Sort};

/// A handler as the SDK builds the tree with it. When the view is sent, each
/// one is kept in a table and replaced by its number there; the event that
/// comes back names that number, and the table gives the message.
pub enum Handler<M> {
    /// Pressed: this message, cloned.
    Message(M),
    /// A text typed or chosen.
    Text(Rc<dyn Fn(String) -> M>),
    /// A box ticked or a switch flipped.
    Bool(Rc<dyn Fn(bool) -> M>),
    /// A row of a table, by its index.
    Index(Rc<dyn Fn(usize) -> M>),
    /// A table's order asked for.
    Sort(Rc<dyn Fn(Sort) -> M>),
    /// A mark drawn on a canvas.
    Mark(Rc<dyn Fn(Mark) -> M>),
    /// A point of a canvas pressed.
    Point(Rc<dyn Fn(Point) -> M>),
    /// A number: a canvas's scale.
    Number(Rc<dyn Fn(f64) -> M>),
    /// The lines of a long log to give next.
    Range(Rc<dyn Fn(Range) -> M>),
}

impl<M: Clone> Clone for Handler<M> {
    fn clone(&self) -> Self {
        match self {
            Handler::Message(message) => Handler::Message(message.clone()),
            Handler::Text(f) => Handler::Text(f.clone()),
            Handler::Bool(f) => Handler::Bool(f.clone()),
            Handler::Index(f) => Handler::Index(f.clone()),
            Handler::Sort(f) => Handler::Sort(f.clone()),
            Handler::Mark(f) => Handler::Mark(f.clone()),
            Handler::Point(f) => Handler::Point(f.clone()),
            Handler::Number(f) => Handler::Number(f.clone()),
            Handler::Range(f) => Handler::Range(f.clone()),
        }
    }
}

impl<M> fmt::Debug for Handler<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Handler::Message(_) => "Handler::Message",
            Handler::Text(_) => "Handler::Text",
            Handler::Bool(_) => "Handler::Bool",
            Handler::Index(_) => "Handler::Index",
            Handler::Sort(_) => "Handler::Sort",
            Handler::Mark(_) => "Handler::Mark",
            Handler::Point(_) => "Handler::Point",
            Handler::Number(_) => "Handler::Number",
            Handler::Range(_) => "Handler::Range",
        })
    }
}

impl<M> From<M> for Handler<M> {
    fn from(message: M) -> Self {
        Handler::Message(message)
    }
}

impl<M: Clone> Handler<M> {
    /// The message for what the widget sent; none when the value is not one
    /// this handler takes, which only a widget of another kind would send.
    pub fn resolve(&self, value: Option<EventValue>) -> Option<M> {
        match (self, value) {
            (Handler::Message(message), _) => Some(message.clone()),
            (Handler::Text(f), Some(EventValue::Text(text))) => Some(f(text)),
            (Handler::Bool(f), Some(EventValue::Bool(on))) => Some(f(on)),
            (Handler::Index(f), Some(EventValue::Index(index))) => Some(f(index as usize)),
            (Handler::Sort(f), Some(EventValue::Sort(sort))) => Some(f(sort)),
            (Handler::Mark(f), Some(EventValue::Mark(mark))) => Some(f(mark)),
            (Handler::Point(f), Some(EventValue::Point(point))) => Some(f(point)),
            (Handler::Number(f), Some(EventValue::Number(number))) => Some(f(number)),
            (Handler::Range(f), Some(EventValue::Range(range))) => Some(f(range)),
            _ => None,
        }
    }
}

impl<M: 'static> Handler<M> {
    /// The same handler, its message passed through `f`: a part of a page
    /// written with its own messages, set in a larger one.
    pub fn map<N: 'static>(self, f: Rc<dyn Fn(M) -> N>) -> Handler<N> {
        match self {
            Handler::Message(message) => Handler::Message(f(message)),
            Handler::Text(g) => Handler::Text(Rc::new(move |text| f(g(text)))),
            Handler::Bool(g) => Handler::Bool(Rc::new(move |on| f(g(on)))),
            Handler::Index(g) => Handler::Index(Rc::new(move |index| f(g(index)))),
            Handler::Sort(g) => Handler::Sort(Rc::new(move |sort| f(g(sort)))),
            Handler::Mark(g) => Handler::Mark(Rc::new(move |mark| f(g(mark)))),
            Handler::Point(g) => Handler::Point(Rc::new(move |point| f(g(point)))),
            Handler::Number(g) => Handler::Number(Rc::new(move |number| f(g(number)))),
            Handler::Range(g) => Handler::Range(Rc::new(move |range| f(g(range)))),
        }
    }
}
