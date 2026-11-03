//! Hangar's icons, the drawings of src/components/Icon.tsx that kit.css
//! carries as masks, and the glyphs of the five states. Each is drawn in one
//! colour, the text's around it, through iced's colour filter: a mask, as in
//! the HTML library.

use iced::widget::svg;

use hangar_kit_ui::{Icon, Status};

macro_rules! icon {
    ($($body:literal),*) => {
        concat!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='black' stroke-width='1.35' stroke-linecap='round' stroke-linejoin='round'>",
            $($body,)*
            "</svg>"
        )
    };
}

const ALERT: &str = icon!(
    "<path d='M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z'/>",
    "<path d='M8 6.4v3'/>",
    "<circle cx='8' cy='11.5' r='.35' fill='black'/>"
);
const CHECK: &str = icon!("<path d='m3.5 8.4 2.9 2.9 6.1-6.6'/>");
const CHEVRON_DOWN: &str = icon!("<path d='M3.5 6 8 10.5 12.5 6'/>");
const CLOSE: &str = icon!("<path d='m4.5 4.5 7 7m0-7-7 7'/>");
const COPY: &str = icon!(
    "<rect x='5.4' y='5.4' width='8.2' height='8.2' rx='1.6'/>",
    "<path d='M10.6 5.4V3.9a1.5 1.5 0 0 0-1.5-1.5H3.9a1.5 1.5 0 0 0-1.5 1.5v5.2a1.5 1.5 0 0 0 1.5 1.5h1.5'/>"
);
const INFO: &str = icon!(
    "<circle cx='8' cy='8' r='6'/>",
    "<path d='M8 11V7.33M8 5h.01'/>"
);
const LOCK: &str = icon!(
    "<rect x='2.67' y='7.33' width='10.67' height='7.33' rx='1.33'/>",
    "<path d='M5.33 7.33V4.67a2.67 2.67 0 0 1 5.34 0v2.66'/>"
);
const MORE: &str = icon!(
    "<circle cx='3.6' cy='8' r='.9' fill='black' stroke='none'/>",
    "<circle cx='8' cy='8' r='.9' fill='black' stroke='none'/>",
    "<circle cx='12.4' cy='8' r='.9' fill='black' stroke='none'/>"
);
const PLUS: &str = icon!("<path d='M8 3.25v9.5M3.25 8h9.5'/>");
const RESTART: &str = icon!(
    "<path d='M13 8.2A5 5 0 1 1 11.3 4.4'/>",
    "<path d='M11.6 1.9v2.8H8.8'/>"
);
const SEARCH: &str = icon!(
    "<circle cx='7' cy='7' r='4.4'/>",
    "<path d='m10.3 10.3 3.3 3.3'/>"
);
const UPDOWN: &str = icon!("<path d='M5 6.2 8 3.2l3 3M5 9.8l3 3 3-3'/>");

// The states' glyphs: a ring turning, a check, an alert, a ring of dashes, a
// dot. Shape says the state as much as colour does.
const RUNNING: &str = icon!(
    "<circle cx='8' cy='8' r='5.5' stroke-opacity='.35'/>",
    "<path d='M8 2.5a5.5 5.5 0 0 1 5.5 5.5'/>"
);
const SUCCESS: &str = icon!(
    "<circle cx='8' cy='8' r='6'/>",
    "<path d='m5.4 8.2 1.8 1.8 3.5-3.8'/>"
);
const FAILURE: &str = ALERT;
const WAITING: &str = icon!("<circle cx='8' cy='8' r='5.5' stroke-dasharray='2.4 2'/>");
const NEUTRAL: &str = icon!("<circle cx='8' cy='8' r='3.2' fill='black' stroke='none'/>");

pub(crate) fn icon(icon: Icon) -> Option<svg::Handle> {
    let drawing = match icon {
        Icon::Alert => ALERT,
        Icon::Check => CHECK,
        Icon::ChevronDown => CHEVRON_DOWN,
        Icon::Close => CLOSE,
        Icon::Copy => COPY,
        Icon::Info => INFO,
        Icon::Lock => LOCK,
        Icon::More => MORE,
        Icon::Plus => PLUS,
        Icon::Restart => RESTART,
        Icon::Search => SEARCH,
        Icon::Updown => UPDOWN,
        Icon::Unknown => return None,
    };
    Some(svg::Handle::from_memory(drawing.as_bytes()))
}

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
