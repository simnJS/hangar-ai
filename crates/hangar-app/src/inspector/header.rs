//! The head of the column (`InspectorHeader`): the pane's monogram with its
//! state on a badge, its name and the word for its state, the key that
//! focuses it, the close cross, and under them what runs in it.

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{AgentState, fonts, style};
use hangar_ui::Icon;
use hangar_ui::button::icon_button;
use hangar_ui::glyph::{self, GlyphState};
use hangar_ui::keycap::binding;
use hangar_ui::tail_text::tail_text;
use iced::widget::{column, container, row, stack, text};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use super::Msg;
use super::sections::{Scene, ink, state_word};
use super::sources;
use crate::commands::CommandId;
use crate::message::Message;

/// What the head says besides the pane's name.
pub(super) struct Head<'a> {
    pub state: AgentState,
    pub agent_label: &'a str,
    pub is_shell: bool,
    /// The agent's own version, from the mod's hello.
    pub version: Option<&'a str>,
    /// The model, in the words people say ("Sonnet 4.5").
    pub model: Option<String>,
    /// A shell pane's shell, named in place of an agent's version and model.
    pub shell: Option<String>,
    /// 1-based place in reading order, for the "focus pane N" key.
    pub index: usize,
}

/// The first letter of the name, upper-cased, or a question mark.
fn monogram(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|first| first.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// The width of the avatar and its badge's overhang.
const AVATAR: f32 = 30.0;
const OVERHANG: f32 = 4.0;

fn avatar<'a>(s: &Scene<'a>, state: AgentState) -> Element<'a, Message> {
    let t = s.theme();
    let c = &t.colors;
    let (raised, edge, chrome) = (c.raised, c.line_strong, c.chrome);
    let face = container(
        text(monogram(&s.pane.name))
            .size(font_size::BODY)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t)),
    )
    .center(AVATAR)
    .style(move |_: &iced::Theme| container::Style {
        background: Some(Background::Color(raised)),
        border: Border {
            color: edge,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..container::Style::default()
    });
    // The state, pinned to the monogram's corner on a ring of the column's
    // own colour, so it reads as a badge on any theme.
    let badge = container(glyph::state(t, GlyphState::from(state), 8.0))
        .center(14.0)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(chrome)),
            border: Border {
                radius: radius::FULL.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    stack![
        container(face).padding(Padding {
            right: OVERHANG,
            bottom: OVERHANG,
            ..Padding::ZERO
        }),
        container(badge)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom),
    ]
    .width(AVATAR + OVERHANG)
    .height(AVATAR + OVERHANG)
    .into()
}

pub(super) fn view<'a>(s: &Scene<'a>, head: Head<'a>) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;

    let title = row![
        container(
            tail_text(s.pane.name.clone())
                .cut_end()
                .size(font_size::TITLE)
                .font(fonts::ui_semibold())
                .color(c.fg)
                .width(Length::Shrink)
        )
        .max_width(150.0),
        tail_text(state_word(tr, head.state))
            .cut_end()
            .size(font_size::CAPTION_LG)
            .font(fonts::ui_medium())
            .color(c.state(head.state).text)
            .width(Length::Shrink),
    ]
    .spacing(space::MD)
    .align_y(Alignment::End);

    // The shortcut that focuses this pane, when it is one of the first nine.
    let focus_command: Option<CommandId> = (1..=9)
        .contains(&head.index)
        .then(|| format!("pane.focus{}", head.index).parse().ok())
        .flatten();
    let mut tools = row![].spacing(4).align_y(Alignment::Center);
    if let Some(keys) = focus_command.and_then(|command| sources::shortcut(s.ctx, command)) {
        let hint = tr
            .t_with(Key::CmdPaneFocusN, &args!("n" => head.index as i64))
            .into_owned();
        tools = tools.push(hangar_ui::tip::bottom(t, binding(t, keys), hint));
    }
    let mut close = icon_button(t, Icon::Close)
        .side(20.0)
        .icon_size(12.0)
        .tip(tr.t(Key::InspectorClose).into_owned())
        .on_press(Message::Inspector(Msg::Close(
            s.ws.id.clone(),
            Some(s.pane.id.clone()),
        )));
    if let Some(keys) = sources::shortcut(s.ctx, CommandId::ViewInspector) {
        close = close.keys(keys);
    }
    tools = tools.push(close);

    // What runs in it: the agent (and, when there is room, its version) and
    // the model, or the shell.
    let sub: Element<'a, Message> = if head.is_shell {
        let mut line = row![ink(
            head.agent_label.to_string(),
            font_size::CAPTION_LG,
            c.fg_muted
        )];
        if let Some(shell) = head.shell {
            line = line.push(ink(
                format!(" · {shell}"),
                font_size::CAPTION_LG,
                c.fg_muted,
            ));
        }
        line.into()
    } else {
        let mut line = row![ink(
            head.agent_label.to_string(),
            font_size::CAPTION_LG,
            c.fg_muted
        )];
        // A narrow column keeps the essentials: the version goes first.
        if let Some(version) = head.version.filter(|_| s.width > 280.0) {
            line = line.push(ink(
                format!(" {version}"),
                font_size::CAPTION_LG,
                c.fg_subtle,
            ));
        }
        if let Some(model) = head.model {
            line = line.push(ink(
                format!(" · {model}"),
                font_size::CAPTION_LG,
                c.fg_muted,
            ));
        }
        line.into()
    };

    container(
        row![
            avatar(s, head.state),
            column![title, container(sub).clip(true).width(Length::Fill)]
                .spacing(1)
                .width(Length::Fill),
            container(tools).align_y(iced::alignment::Vertical::Top),
        ]
        .spacing(space::LG)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: Scene::PAD,
    })
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::monogram;

    #[test]
    fn the_monogram_is_the_first_letter_upper_cased() {
        assert_eq!(monogram("ava"), "A");
        assert_eq!(monogram("  ben"), "B");
        assert_eq!(monogram("émile"), "É");
        assert_eq!(monogram(""), "?");
        assert_eq!(monogram("   "), "?");
    }
}
