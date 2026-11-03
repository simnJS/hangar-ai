//! The search bar floating over one terminal (`TerminalSearch.tsx`,
//! inventory §4/1.11).
//!
//! The search itself is hangar-term's (`Terminal::search`): plain or regex,
//! case or not, matches highlighted in the theme's colours, the count capped
//! at 1,000. It runs upwards: what anyone looks for in a terminal is nearly
//! always the latest occurrence, at the bottom, so typing lands there and
//! Enter steps back through older output.

use hangar_i18n::{Key, args};
use hangar_term::{MAX_MATCHES, SearchDirection, SearchOptions, SearchStatus, Terminal};
use hangar_theme::{style, tokens};
use hangar_ui::{Icon, button, keys};
use iced::keyboard::{Key as KeyCode, key::Named};
use iced::widget::{container, row, rule, text, text_input};
use iced::{Alignment, Element, Length, Padding, Task, widget};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes::Msg;
use crate::state::PaneId;

/// An open search bar.
#[derive(Debug, Clone)]
pub struct Search {
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    /// The query is a regex that does not compile, which is every regex
    /// halfway through being typed.
    pub invalid: bool,
    /// The field has the keyboard. A click in the terminal gives it back to
    /// the terminal with the bar left open, as in the web app.
    pub typing: bool,
    pub input: widget::Id,
}

#[derive(Debug, Clone)]
pub enum SearchMsg {
    Input(String),
    /// Enter steps up, towards older output; Shift+Enter down.
    Step(SearchDirection),
    ToggleCase,
    ToggleRegex,
    Close,
    /// A press landed on the bar: the field has the keyboard again.
    Focused,
}

impl Search {
    fn new(pane: &PaneId, seed: String) -> Search {
        Search {
            query: seed,
            case_sensitive: false,
            regex: false,
            invalid: false,
            typing: true,
            input: widget::Id::from(format!("pane-search-{pane}")),
        }
    }

    fn options(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.case_sensitive,
            regex: self.regex,
        }
    }

    /// Runs the search for what the bar holds now.
    fn run(&mut self, terminal: &mut Terminal) {
        self.invalid = false;
        if self.query.is_empty() {
            terminal.clear_search();
            return;
        }
        if terminal.search(&self.query, self.options()).is_err() {
            self.invalid = true;
            // The highlights of the previous pattern would point at
            // something the field no longer says.
            terminal.clear_search();
        }
    }
}

/// `terminal.find`: opens the bar, or refocuses and selects its field if it
/// is open already. The first opening seeds from a single-line selection,
/// else from the last query (`registerFinder`).
pub fn open(
    slot: &mut Option<Search>,
    last_query: &str,
    pane: &PaneId,
    terminal: &mut Terminal,
) -> Task<Message> {
    let search = match slot {
        Some(search) => {
            search.typing = true;
            search
        }
        None => {
            let selected = terminal
                .selection_text()
                .filter(|s| !s.is_empty() && !s.contains('\n'));
            let mut search = Search::new(pane, selected.unwrap_or_else(|| last_query.to_owned()));
            search.run(terminal);
            slot.insert(search)
        }
    };
    let id = search.input.clone();
    Task::batch([
        widget::operation::focus(id.clone()),
        widget::operation::select_all(id),
    ])
}

/// What a message of the bar does. Returns the query when the bar closed, to
/// keep it for the next opening.
pub fn update(
    slot: &mut Option<Search>,
    terminal: &mut Terminal,
    msg: SearchMsg,
) -> (Task<Message>, Option<String>) {
    let Some(search) = slot else {
        return (Task::none(), None);
    };
    match msg {
        SearchMsg::Input(query) => {
            search.query = query;
            search.typing = true;
            search.run(terminal);
        }
        SearchMsg::Step(direction) => {
            search.typing = true;
            if !search.query.is_empty() && !search.invalid {
                terminal.search_step(direction);
            }
        }
        SearchMsg::ToggleCase => {
            search.case_sensitive = !search.case_sensitive;
            search.run(terminal);
            return (widget::operation::focus(search.input.clone()), None);
        }
        SearchMsg::ToggleRegex => {
            search.regex = !search.regex;
            search.run(terminal);
            return (widget::operation::focus(search.input.clone()), None);
        }
        SearchMsg::Focused => search.typing = true,
        SearchMsg::Close => {
            let query = search.query.clone();
            terminal.clear_search();
            *slot = None;
            return (Task::none(), Some(query));
        }
    }
    (Task::none(), None)
}

/// The counter: `i/N`, `N` when no match is the current one, `1000+` past
/// the cap; or why there is nothing to count.
pub fn status_text(ctx: &Ctx<'_>, search: &Search, status: SearchStatus) -> (String, bool) {
    if search.invalid {
        return (ctx.t(Key::SearchInvalid).into_owned(), true);
    }
    if search.query.is_empty() {
        return (String::new(), false);
    }
    if status.count == 0 {
        return (ctx.t(Key::SearchNone).into_owned(), true);
    }
    let total = if status.truncated || status.count >= MAX_MATCHES {
        format!("{MAX_MATCHES}+")
    } else {
        status.count.to_string()
    };
    let text = match status.current {
        Some(index) => format!("{}/{total}", index + 1),
        None => total,
    };
    (text, false)
}

fn wrap(pane: &PaneId, msg: SearchMsg) -> Message {
    Message::Panes(Msg::Search(pane.clone(), msg))
}

/// The bar, top right of the terminal (`.term-search`).
pub fn view<'a>(
    ctx: Ctx<'a>,
    pane: &'a PaneId,
    search: &'a Search,
    status: SearchStatus,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let (count, missed) = status_text(&ctx, search, status);
    let label = ctx.t(Key::SearchLabel).into_owned();

    let input = text_input(&ctx.t(Key::SearchPlaceholder), &search.query)
        .id(search.input.clone())
        .on_input(move |query| wrap(pane, SearchMsg::Input(query)))
        .size(tokens::font_size::UI)
        .padding(Padding::from([tokens::space::XXS, tokens::space::MD]))
        .width(180)
        .style(move |theme, status| {
            if search.invalid {
                style::text_input::field_invalid(t)(theme, status)
            } else {
                style::text_input::field(t)(theme, status)
            }
        });

    // Enter, Shift+Enter, Escape, Alt+C and Alt+R, before the field sees
    // them; only while the field has the keyboard, or Enter typed into the
    // terminal would step the search. The keymap goes first, as the TS's
    // shortcuts listened before any field: Mod+Shift+Enter splits the pane
    // and types nothing here, and a person who bound Alt+C to a command gets
    // the command. What the field owns (characters, Enter, Escape, copy and
    // paste) the keymap leaves to it.
    let input: Element<'a, Message> = if search.typing {
        crate::keymap::guard(
            ctx,
            keys::on_key(input, move |key, modifiers| match key {
                KeyCode::Named(Named::Enter) => Some(wrap(
                    pane,
                    SearchMsg::Step(if modifiers.shift() {
                        SearchDirection::Forward
                    } else {
                        SearchDirection::Backward
                    }),
                )),
                KeyCode::Named(Named::Escape) => Some(wrap(pane, SearchMsg::Close)),
                KeyCode::Character(c) if modifiers.alt() && c.eq_ignore_ascii_case("c") => {
                    Some(wrap(pane, SearchMsg::ToggleCase))
                }
                KeyCode::Character(c) if modifiers.alt() && c.eq_ignore_ascii_case("r") => {
                    Some(wrap(pane, SearchMsg::ToggleRegex))
                }
                _ => None,
            }),
        )
    } else {
        input.into()
    };

    let counter = container(
        text(count)
            .size(tokens::font_size::LABEL)
            .font(hangar_theme::fonts::mono())
            .style(move |theme| {
                if missed {
                    style::text::danger(t)(theme)
                } else {
                    style::text::muted(t)(theme)
                }
            }),
    )
    .padding(Padding::from([0.0, tokens::space::SM]))
    .center_x(44);

    let step = |icon, direction, key: Key, keys: &str| {
        let words = ctx.i18n.t_with(key, &args!("keys" => keys)).into_owned();
        button::icon_button(t, icon)
            .small()
            .icon_size(12.0)
            .tip(words)
            .on_press_maybe(
                (!search.query.is_empty() && !search.invalid)
                    .then(|| wrap(pane, SearchMsg::Step(direction))),
            )
    };
    let option = |label: &'static str, on: bool, msg: SearchMsg, key: Key, keys: &str| {
        let words = ctx.i18n.t_with(key, &args!("keys" => keys)).into_owned();
        hangar_ui::tip::bottom(
            t,
            iced::widget::button(
                text(label)
                    .size(tokens::font_size::LABEL)
                    .font(hangar_theme::fonts::mono()),
            )
            .padding(Padding::from([tokens::space::XXS, tokens::space::XS]))
            .height(tokens::size::CONTROL_HEIGHT_SM)
            .style(style::button::toggle(t, on))
            .on_press(wrap(pane, msg)),
            words,
        )
    };

    let bar = row![
        input,
        counter,
        option(
            "Aa",
            search.case_sensitive,
            SearchMsg::ToggleCase,
            Key::SearchCase,
            &shortcut("Alt+C")
        ),
        option(
            ".*",
            search.regex,
            SearchMsg::ToggleRegex,
            Key::SearchRegex,
            &shortcut("Alt+R")
        ),
        rule::vertical(tokens::BORDER_WIDTH).style(style::rule::strong(t)),
        step(
            Icon::ChevronUp,
            SearchDirection::Backward,
            Key::SearchUp,
            &shortcut("Enter")
        ),
        step(
            Icon::ChevronDown,
            SearchDirection::Forward,
            Key::SearchDown,
            &shortcut("Shift+Enter")
        ),
        button::icon_button(t, Icon::Close)
            .small()
            .icon_size(12.0)
            .tip(
                ctx.i18n
                    .t_with(Key::SearchClose, &args!("keys" => shortcut("Escape")))
                    .into_owned()
            )
            .on_press(wrap(pane, SearchMsg::Close)),
    ]
    .spacing(1)
    .height(tokens::size::CONTROL_HEIGHT_SM)
    .align_y(Alignment::Center);

    let _ = label;
    let card = container(bar)
        .padding(3)
        .style(style::container::popover(t));
    let watched = super::press::on_press(card, wrap(pane, SearchMsg::Focused));
    container(watched)
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding(Padding {
            top: tokens::space::MD,
            right: 14.0,
            bottom: 0.0,
            left: 14.0,
        })
        .into()
}

/// A binding written as the platform shows it (`formatBinding`).
fn shortcut(binding: &str) -> String {
    hangar_ui::shortcut::format_binding(binding)
}

#[cfg(test)]
mod tests {
    use iced::keyboard::Modifiers;
    use iced::keyboard::key::Named;

    use super::*;
    use crate::app::Packages;
    use crate::navigator::testing::{Drive, ctx, model_of, pane, simulate, workspace};
    use crate::shell::View;
    use crate::state::types::{AgentId, AppState};

    /// What the bar sends when Enter is pressed in its field with
    /// `modifiers`, over a workspace on screen, where splitting a pane
    /// (Mod+Shift+Enter) is a live shortcut.
    fn enter_in_the_field(modifiers: Modifiers) -> Vec<Message> {
        let mut model = model_of(AppState {
            workspaces: vec![workspace(
                "w",
                "hangar",
                None,
                vec![pane("p1", "Ava", AgentId::Claude)],
            )],
            active_workspace_id: Some("w".into()),
            ..AppState::default()
        });
        model.ui.view = View::Terminals;
        let pkg = Packages::default();
        let id: PaneId = "p1".into();
        let search = Search::new(&id, "needle".into());
        let mut ui = simulate(view(
            ctx(&model, &pkg),
            &id,
            &search,
            SearchStatus::default(),
        ));
        ui.click("needle").expect("the field is shown");
        ui.key(Named::Enter, modifiers);
        ui.into_messages().collect()
    }

    fn steps(messages: &[Message]) -> Vec<SearchDirection> {
        messages
            .iter()
            .filter_map(|message| match message {
                Message::Panes(Msg::Search(_, SearchMsg::Step(direction))) => Some(*direction),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn enter_steps_back_and_shift_enter_forward() {
        assert_eq!(
            steps(&enter_in_the_field(Modifiers::empty())),
            [SearchDirection::Backward]
        );
        assert_eq!(
            steps(&enter_in_the_field(Modifiers::SHIFT)),
            [SearchDirection::Forward]
        );
    }

    #[test]
    fn a_shortcut_typed_in_the_field_is_the_keymaps_and_does_not_step() {
        let messages = enter_in_the_field(Modifiers::COMMAND | Modifiers::SHIFT);
        assert!(steps(&messages).is_empty(), "{messages:?}");
        // Spent by the guard; the keymap's subscription runs the split.
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::Noop)),
            "{messages:?}"
        );
    }
}
