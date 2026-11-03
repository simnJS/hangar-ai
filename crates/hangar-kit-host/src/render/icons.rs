//! The glyphs of the five states of Hangar UI (`.kit-glyph`), each drawn in
//! one colour through iced's colour filter: a mask, as in the HTML library.
//! The icons themselves are hangar-ui's (`render::ui_icon`); these glyphs say
//! a run's state, which the window's agent glyphs do not.

use iced::widget::svg;

use hangar_kit_ui::Status;

macro_rules! icon {
    ($($body:literal),*) => {
        concat!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='black' stroke-width='1.35' stroke-linecap='round' stroke-linejoin='round'>",
            $($body,)*
            "</svg>"
        )
    };
}

// A ring turning, a check, an alert, a ring of dashes, a dot. Shape says the
// state as much as colour does.
const RUNNING: &str = icon!(
    "<circle cx='8' cy='8' r='5.5' stroke-opacity='.35'/>",
    "<path d='M8 2.5a5.5 5.5 0 0 1 5.5 5.5'/>"
);
const SUCCESS: &str = icon!(
    "<circle cx='8' cy='8' r='6'/>",
    "<path d='m5.4 8.2 1.8 1.8 3.5-3.8'/>"
);
const FAILURE: &str = icon!(
    "<path d='M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z'/>",
    "<path d='M8 6.4v3'/>",
    "<circle cx='8' cy='11.5' r='.35' fill='black'/>"
);
const WAITING: &str = icon!("<circle cx='8' cy='8' r='5.5' stroke-dasharray='2.4 2'/>");
const NEUTRAL: &str = icon!("<circle cx='8' cy='8' r='3.2' fill='black' stroke='none'/>");

pub(crate) fn status(status: Status) -> svg::Handle {
    let drawing = match status {
        Status::Running => RUNNING,
        Status::Success => SUCCESS,
        Status::Failure => FAILURE,
        Status::Waiting => WAITING,
        Status::Neutral => NEUTRAL,
    };
    svg::Handle::from_memory(drawing.as_bytes())
}
