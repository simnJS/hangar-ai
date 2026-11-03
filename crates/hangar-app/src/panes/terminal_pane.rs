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
    let terminal = pane_terminal(&rt.link, &pane.id, rt.look.padding, filter);
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
