//! One pane drawn (`TerminalPane.tsx`'s view): its header, the notes its
//! launch left, and its terminal with the search bar floating over it.

use hangar_i18n::Key;
use hangar_theme::{style, tokens};
use hangar_ui::{Icon, button};
use iced::widget::{Column, column, container, row, stack, text};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes::controller::{PaneRt, shell_of};
use crate::panes::header::{self, Head};
use crate::panes::term_widget::pane_terminal;
use crate::panes::{Msg, Picker, State, look, search};
use crate::state::types::{Pane, Workspace};

/// How opaque a terminal is when its pane is not the focused one
/// (`.pane:not(.pane--focused) .pane__term { opacity: 0.94 }`): the panes you
/// are not typing into step back a little, only a little, as a theme's dim
/// output is often near 3:1 already. The TS faded the change in over a
/// transition (`--dur`, 140 ms); here it is at once, since a frame of the
/// old look costs nothing and the fade would keep the window redrawing.
pub const UNFOCUSED_OPACITY: f32 = 0.94;

/// What a pane's terminal is drawn over, and how opaque it is, for a pane
/// that is `focused` or not. The pane's own background (`var(--bg)`) is what
/// shows through.
pub fn dimming(focused: bool, theme: &hangar_theme::Theme) -> Option<(iced::Color, f32)> {
    (!focused).then_some((theme.colors.bg, UNFOCUSED_OPACITY))
}

/// Where the pane stands in its grid.
pub struct Placing {
    pub focused: bool,
    pub zoomed: bool,
    pub can_close: bool,
    pub can_zoom: bool,
    /// The pane's width on screen, for the header's breakpoints.
    pub width: f32,
    /// Its place in the workspace's reading order, from 0: the first nine
    /// are reached with `pane.focus1`…`9`.
    pub index: usize,
}

pub fn view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    rt: &'a PaneRt,
    placing: Placing,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let picker = st
        .picker
        .as_ref()
        .filter(|(id, _)| id == &pane.id)
        .map(|(_, picker)| *picker);
    let head = header::view(
        ctx,
        Head {
            ws,
            pane,
            rt,
            focused: placing.focused,
            hovered: st.hovered.as_deref() == Some(pane.id.as_str()),
            zoomed: placing.zoomed,
            can_close: placing.can_close,
            can_zoom: placing.can_zoom,
            width: placing.width,
            index: placing.index,
            shell: shell_of(ctx.model, ws, pane),
            picker: picker.filter(|p| matches!(p, Picker::Agent | Picker::Shell)),
        },
    );

    let mut pane_column: Column<'a, Message> = column![head];
    for note in rt.notes.iter().filter(|note| !rt.dismissed.contains(*note)) {
        pane_column = pane_column.push(note_row(ctx, &pane.id, note));
    }

    // Shortcuts reach the app before the program: the keymap says which.
    let filter = Box::new(crate::keymap::terminal_filter(ctx));
    let mut terminal = pane_terminal(&rt.link, &pane.id, rt.look.padding, filter)
        .scale(crate::panes::scale_of(st, ctx.window, ctx.metrics.scale));
    if let Some((behind, opacity)) = dimming(placing.focused, t) {
        terminal = terminal.dimmed(behind, opacity);
    }
    let mut body = stack![terminal].width(Length::Fill).height(Length::Fill);
    if let Some(open) = &rt.search {
        let status = rt.link.lock().search_status();
        body = body.push(search::view(ctx, &pane.id, open, status));
    }
    let background = look::theme_of(ctx.model.settings(), ws).terminal.background;
    pane_column = pane_column.push(
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(background)),
                ..container::Style::default()
            }),
    );
    container(pane_column)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::window(t))
        .into()
}

/// A note of the launch: a kit left out, an account that is gone. In the
/// pane's bar rather than its terminal, which Claude Code clears as it
/// starts; each one dismissible.
fn note_row<'a>(ctx: Ctx<'a>, pane: &'a str, note: &'a str) -> Element<'a, Message> {
    let t = ctx.theme;
    let warning = t.colors.warning;
    container(
        row![
            text(note)
                .size(tokens::font_size::CAPTION)
                .width(Length::Fill)
                .style(move |_| text::Style {
                    color: Some(warning)
                }),
            button::icon_button(t, Icon::Close)
                .side(18.0)
                .icon_size(10.0)
                .tip(ctx.t(Key::PaneNoteDismiss).into_owned())
                .on_press(Message::Panes(Msg::DismissNote(
                    pane.to_owned(),
                    note.to_owned()
                ))),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([tokens::space::XXS, tokens::space::MD]))
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(warning.scale_alpha(0.1))),
        border: Border {
            color: warning.scale_alpha(0.25),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_panes_that_are_not_focused_step_back() {
        let theme = hangar_theme::default_theme();
        assert_eq!(dimming(true, theme), None);
        let (behind, opacity) = dimming(false, theme).expect("an unfocused pane");
        // The pane's background shows through, at 0.94.
        assert_eq!(behind, theme.colors.bg);
        assert_eq!(opacity, 0.94);
    }
}
