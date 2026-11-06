//! One pane drawn (`TerminalPane.tsx`'s view): its header, the notes its
//! launch left, and its terminal with the search bar floating over it.

use hangar_i18n::{Key, args};
use hangar_theme::{style, tokens};
use hangar_ui::{Icon, button};
use iced::widget::{Column, center, column, container, row, stack, text};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::nodes::NodeStatus;
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
    let away = node_away(ctx, ws);
    if away.is_some() {
        // Greyed while its node is away: what it shows is the last it heard.
        terminal = terminal.dimmed(t.colors.bg, AWAY_OPACITY);
    } else if let Some((behind, opacity)) = dimming(placing.focused, t) {
        terminal = terminal.dimmed(behind, opacity);
    }
    let mut body = stack![terminal].width(Length::Fill).height(Length::Fill);
    if let Some(away) = away {
        body = body.push(away_card(ctx, away));
    }
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

/// How opaque the terminal of a pane whose node is away is.
const AWAY_OPACITY: f32 = 0.4;

/// The node of a remote workspace that is not connected now, its label,
/// and whether it is on its way.
pub struct Away {
    pub node: String,
    pub label: String,
    pub connecting: bool,
}

/// `Some` for a pane of a workspace on a node that is not connected: its
/// terminal is greyed and says so, with a way to reconnect. Its state stays
/// as it was, and its shell runs on in the node.
pub fn node_away(ctx: Ctx<'_>, ws: &Workspace) -> Option<Away> {
    let node = ws.node.as_ref()?;
    if crate::nodes::client(ctx.model, node).is_some() {
        return None;
    }
    let connecting = matches!(ctx.model.live.nodes.status(node), NodeStatus::Connecting(_));
    Some(Away {
        node: node.clone(),
        label: crate::nodes::label(ctx.model, node),
        connecting,
    })
}

fn away_card<'a>(ctx: Ctx<'a>, away: Away) -> Element<'a, Message> {
    let t = ctx.theme;
    let words = if away.connecting {
        ctx.i18n.t_with(
            Key::NodesPaneConnecting,
            &args!("node" => away.label.as_str()),
        )
    } else {
        ctx.i18n
            .t_with(Key::NodesPaneAway, &args!("node" => away.label.as_str()))
    }
    .into_owned();
    // Connecting needs the experiment and the node in the settings.
    let known = crate::nodes::available(ctx.model)
        && ctx
            .model
            .settings()
            .nodes
            .iter()
            .any(|node| node.id == away.node);
    let reconnect = button::secondary(t, ctx.t(Key::NodesPaneReconnect)).on_press_maybe(
        (known && !away.connecting)
            .then(|| Message::Nodes(crate::nodes::Msg::Connect(away.node.clone()))),
    );
    let fg = t.colors.fg;
    let card = container(
        column![
            text(words).style(move |_| text::Style { color: Some(fg) }),
            reconnect,
        ]
        .spacing(tokens::space::MD)
        .align_x(Alignment::Center),
    )
    .padding(tokens::space::LG)
    .style(style::container::window(t));
    center(card).into()
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

    /// A remote workspace whose node is away keeps its panes, greyed, and
    /// says so with a way back; connected, it is drawn as any other.
    #[test]
    fn a_pane_whose_node_is_away_says_so_and_offers_to_reconnect() {
        use crate::nodes::model::NodeInfo;
        use crate::state::types::NodeConfig;
        use crate::workspace_dialog::testing::{Harness, messages};

        let mut h = Harness::new(hangar_i18n::Locale::En, "graphite").edit(|state| {
            state.settings.experiments = vec![crate::settings::experiments::NODES.into()];
            state.settings.nodes = vec![NodeConfig {
                id: "n1".into(),
                label: "GPU box".into(),
                ssh_target: "me@gpu".into(),
                ..NodeConfig::default()
            }];
            state.workspaces[0].node = Some("n1".into());
        });
        let ws = h.model.state().workspaces[0].clone();
        let mut st = State::default();
        st.mount_for_test(&ws);
        let id = ws.id.clone();
        {
            let mut ui = h.show(crate::panes::view_workspace(&st, h.ctx(), &id, false));
            assert!(ui.find("Node GPU box is not connected").is_ok());
            ui.click("Reconnect").expect("a way back");
            let asked = messages(ui).into_iter().any(|message| {
                matches!(message, Message::Nodes(crate::nodes::Msg::Connect(node)) if node == "n1")
            });
            assert!(asked, "Reconnect connects the node");
        }
        h.model.live.nodes.statuses.insert(
            "n1".into(),
            NodeStatus::Connected {
                info: NodeInfo::default(),
                local_port: 21012,
                token: "tok".into(),
            },
        );
        let mut ui = h.show(crate::panes::view_workspace(&st, h.ctx(), &id, false));
        assert!(ui.find("Node GPU box is not connected").is_err());
    }

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
