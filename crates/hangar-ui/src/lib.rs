//! The widgets every part of the window is built from, styled by the active
//! theme, so a view never picks a colour of its own.
//!
//! See docs/native/PLAN.md, "hangar-ui". `cargo run -p hangar-ui --example
//! gallery` shows every widget below on one window, with a picker over the 33
//! themes; its sources (`examples/gallery/pages.rs`) are the shortest way to
//! see how a view uses each of them.
//!
//! # How a view uses them
//!
//! Every constructor takes the theme first: the [`hangar_theme::Theme`] the
//! app is showing (a `&'static` one from `hangar_theme::themes()`), and from
//! it the widget draws every colour, size and radius. Controls are stateless,
//! as iced's are: the value is the view's and a change comes back as a
//! message. A few keep a little state in structs the view owns: the command
//! palette ([`palette::Palette`]), the shortcut recorder
//! ([`shortcut::Recorder`]), the number field ([`input::NumberField`]) and the
//! toasts ([`toast::Toasts`]).
//!
//! ```ignore
//! use hangar_ui::{button, input, text, Icon};
//!
//! fn view(&self) -> Element<Msg> {
//!     let t = self.theme;
//!     column![
//!         text::title(t, "Rename"),
//!         input::text_field(t, &self.name).label("Name").on_input(Msg::Name),
//!         row![
//!             button::ghost(t, "Cancel").on_press(Msg::Cancel),
//!             button::primary(t, "Save").icon(Icon::Check).on_press(Msg::Save),
//!         ],
//!     ]
//! }
//! ```
//!
//! # The widgets
//!
//! | Module | What is in it |
//! | --- | --- |
//! | [`icon`](mod@icon) | `icon(Icon::X, size)`: the 75 icons of `Icon.tsx`, in the colour of the text around them |
//! | [`text`] | `body`, `label`, `muted`, `hint`, `title`, `heading`, `mono`: text by role |
//! | [`button`] | `primary`, `secondary`, `ghost`, `danger`, `icon_button` (with tooltip), `toggle_button`, `tab_button`, `link` |
//! | [`input`] | `text_field` (label, hint, error), `search_field`, `number_input`, `textarea`, `select`, `checkbox`, `switch`, `slider`, `segmented` |
//! | [`shortcut_field`] | the Shortcuts settings' control: a command's keys, and the field that records another |
//! | [`shortcut`] | chords, bindings and the recorder's state machine (`src/lib/keys.ts`) |
//! | [`keycap`] | `keycap`, `binding`: shortcuts drawn as caps |
//! | [`layer`] | the overlay every floating thing is made of: `modal`, `palette`, `dropdown`, `context` |
//! | [`menu`] | `item`, `heading`, `separator`, `dropdown`, `context_menu`: drop-down and context menus |
//! | [`dialog`] | `dialog(t, title, body)` and `over(t, page, dialog)`: modal dialogs |
//! | [`palette`] | `Palette<T>`: the command palette, generic over what it lists |
//! | [`matching`] | the palette's matching and ranking (`src/lib/paletteMatch.ts`) |
//! | [`toast`] | `Toasts`, `Toast`, `over`: notices that go by themselves |
//! | [`tip`] | `bottom`, `over`, `right`, `keys`: tooltips |
//! | [`section`] | a collapsible block with a title row (`InspectorSection`) |
//! | [`panel`] | `header`, `panel`: a bar with a title and actions |
//! | [`split`] | `split`: two regions with a draggable divider |
//! | [`tabs`] | `strip`: document tabs with close crosses |
//! | [`list`] | `row`: a list row with selection and hover |
//! | [`badge`] | `badge`, `count`, `tag` |
//! | [`logo`] | `logo`: the app mark, in the theme's accent and foreground |
//! | [`glyph`] | `state`: an agent's state; `gauge`: the context ring |
//! | [`empty`] | `empty`: what a list says when it has nothing in it |
//! | [`form`] | `row`, `group`, `section`, `block`, `page`: the settings form, reset button included |
//! | [`keys`] | `on_key`, `on_keyboard`: keys for a subtree, spent before the widget inside sees them |
//! | [`menu_nav`] | where an arrow key lands in a list (`src/lib/menuNav.ts`) |
//!
//! A widget that floats is a [`layer::Layer`] around the widget it belongs
//! to, open while it is given something to show, and it only ever asks to be
//! closed: dismissing it is the view's to do. That is how an overlay sits in
//! an Elm-style app, and why none of them keeps its own open flag.
//!
//! # Gaps
//!
//! - iced 0.14 has no accessibility tree and its buttons take no keyboard
//!   focus: there is no focus ring to draw and no screen reader to speak to.
//!   Menus, the palette, dialogs, the divider and the shortcut recorder have
//!   full keyboard use of their own.
//! - A web page's one-sided hairlines, outlines, backdrop blur and letter
//!   spacing have no counterpart in iced; see `hangar_theme::style` for how
//!   each is approximated.
//! - Menus have no submenus: the web app had none.

pub mod badge;
pub mod button;
pub mod dialog;
pub mod empty;
pub mod form;
pub mod glyph;
pub mod icon;
pub mod input;
pub mod keycap;
pub mod keys;
pub mod layer;
pub mod list;
pub mod logo;
pub mod matching;
pub mod menu;
pub mod menu_nav;
pub mod palette;
pub mod panel;
pub mod section;
pub mod shortcut;
pub mod shortcut_field;
pub mod split;
pub mod tabs;
pub mod text;
pub mod tip;
pub mod toast;

pub use icon::{Icon, icon};

#[cfg(test)]
mod testing;
