//! Settings › Nodes: each node with its status and what it said about
//! itself, Connect / Disconnect / Remove, and the form that adds one.

use std::borrow::Cow;

use hangar_i18n::{Key, args};
use hangar_theme::tokens;
use hangar_ui::{badge, button as ui_button, form, input, text as typography};
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::types::NodeConfig;

use super::model::{Failure, NodeStatus, Step, TargetProblem, target_problem};
use super::{Msg, State};

fn press(msg: Msg) -> Message {
    Message::Nodes(msg)
}

/// A dictionary string that takes no argument: always borrowed from the
/// dictionary, which a field's placeholder needs to be.
fn static_text(text: Cow<'static, str>) -> &'static str {
    match text {
        Cow::Borrowed(text) => text,
        Cow::Owned(_) => "",
    }
}

pub fn step_key(step: Step) -> Key {
    match step {
        Step::Probe => Key::NodesStepProbe,
        Step::Install => Key::NodesStepInstall,
        Step::Start => Key::NodesStepStart,
        Step::Config => Key::NodesStepConfig,
        Step::Tunnel => Key::NodesStepTunnel,
        Step::Handshake => Key::NodesStepHandshake,
    }
}

/// The failure in the person's language; the details stay as the tools said
/// them.
pub fn failure_text(ctx: Ctx<'_>, failure: &Failure) -> String {
    let i18n = ctx.i18n;
    match failure {
        Failure::Ssh { step, detail } => i18n.t_with(
            Key::NodesErrorSsh,
            &args!("step" => i18n.t(step_key(*step)).into_owned(), "detail" => detail.as_str()),
        ),
        Failure::NoBinary { arch, file, dir } => i18n.t_with(
            Key::NodesErrorNoBinary,
            &args!("arch" => arch.as_str(), "file" => file.as_str(), "dir" => dir.as_str()),
        ),
        Failure::UnsupportedArch(arch) => i18n.t_with(
            Key::NodesErrorUnsupportedArch,
            &args!("arch" => arch.as_str()),
        ),
        Failure::NodeJson(detail) => {
            i18n.t_with(Key::NodesErrorNodeJson, &args!("detail" => detail.as_str()))
        }
        Failure::Tunnel(detail) => {
            i18n.t_with(Key::NodesErrorTunnel, &args!("detail" => detail.as_str()))
        }
        Failure::Api(detail) => {
            i18n.t_with(Key::NodesErrorApi, &args!("detail" => detail.as_str()))
        }
        Failure::Local(detail) => {
            i18n.t_with(Key::NodesErrorLocal, &args!("detail" => detail.as_str()))
        }
    }
    .into_owned()
}

pub fn settings_page<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let nodes = &ctx.model.settings().nodes;
    let mut body = column![].spacing(tokens::space::XL);
    if nodes.is_empty() {
        body = body.push(typography::hint(t, ctx.t(Key::NodesSettingsEmpty)));
    } else {
        let rows: Vec<Element<'a, Message>> =
            nodes.iter().map(|node| node_row(ctx, node)).collect();
        body = body.push(form::group(t, rows));
    }
    body = body.push(add_form(st, ctx));
    form::section(t, ctx.t(Key::SettingsCatNodes))
        .hint(ctx.t(Key::NodesSettingsIntro))
        .bare(body)
        .into()
}

fn node_row<'a>(ctx: Ctx<'a>, node: &'a NodeConfig) -> Element<'a, Message> {
    let t = ctx.theme;
    let status = ctx.model.live.nodes.status(&node.id);
    let name = if node.label.trim().is_empty() {
        node.ssh_target.as_str()
    } else {
        node.label.as_str()
    };

    let (status_label, tone) = match &status {
        NodeStatus::Disconnected => (Key::NodesStatusDisconnected, badge::Tone::Neutral),
        NodeStatus::Connecting(_) => (Key::NodesStatusConnecting, badge::Tone::Accent),
        NodeStatus::Connected { .. } => (Key::NodesStatusConnected, badge::Tone::Success),
        NodeStatus::Unreachable { .. } => (Key::NodesStatusUnreachable, badge::Tone::Danger),
    };
    let head = row![
        typography::body(t, name).font(hangar_theme::fonts::ui_semibold()),
        badge::badge(t, ctx.t(status_label), tone),
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);

    let mut lines = column![head].spacing(tokens::space::SM);
    let mut target = node.ssh_target.clone();
    if let Some(config) = node.ssh_config.as_deref() {
        target = format!("{target} · -F {config}");
    }
    lines = lines.push(typography::hint(t, target));

    match &status {
        NodeStatus::Disconnected => {}
        NodeStatus::Connecting(step) => {
            lines = lines.push(typography::hint(t, ctx.t(step_key(*step))));
        }
        NodeStatus::Connected { info, .. } => {
            lines = lines.push(typography::hint(
                t,
                ctx.i18n
                    .t_with(
                        Key::NodesInfo,
                        &args!(
                            "version" => info.version.as_str(),
                            "hostname" => info.hostname.as_str(),
                            "target" => info.target.as_str()
                        ),
                    )
                    .into_owned(),
            ));
            let present = |found: bool, yes: Key, no: Key| {
                if found {
                    badge::badge(t, ctx.t(yes), badge::Tone::Success)
                } else {
                    badge::badge(t, ctx.t(no), badge::Tone::Warning)
                }
            };
            lines = lines.push(
                row![
                    present(info.claude, Key::NodesClaudeFound, Key::NodesClaudeMissing),
                    present(info.git, Key::NodesGitFound, Key::NodesGitMissing),
                ]
                .spacing(tokens::space::MD),
            );
        }
        NodeStatus::Unreachable { failure, retry_in } => {
            let mut text = failure_text(ctx, failure);
            if let Some(seconds) = retry_in {
                text.push(' ');
                text.push_str(&ctx.i18n.t_with(
                    Key::NodesErrorRetrying,
                    &args!("seconds" => *seconds as i64),
                ));
            }
            lines = lines.push(typography::danger(t, text));
        }
    }

    let idle = matches!(
        status,
        NodeStatus::Disconnected | NodeStatus::Unreachable { retry_in: None, .. }
    );
    let toggle = if idle {
        ui_button::primary(t, ctx.t(Key::NodesConnect))
            .small()
            .on_press(press(Msg::Connect(node.id.clone())))
    } else {
        ui_button::secondary(t, ctx.t(Key::NodesDisconnect))
            .small()
            .on_press(press(Msg::Disconnect(node.id.clone())))
    };
    lines = lines.push(
        row![
            toggle,
            ui_button::ghost(t, ctx.t(Key::NodesRemove))
                .small()
                .on_press(press(Msg::Remove(node.id.clone()))),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
    );

    container(lines)
        .padding(Padding::from([tokens::space::XL, tokens::space::XL]))
        .width(Length::Fill)
        .into()
}

fn add_form<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let form_state = &st.form;
    let problem = form_state
        .touched
        .then(|| target_problem(&form_state.target))
        .flatten();
    let label = input::text_field(t, &form_state.label)
        .placeholder(static_text(ctx.t(Key::NodesAddLabelPlaceholder)))
        .on_input(|value| press(Msg::AddLabel(value)))
        .on_submit(press(Msg::AddSubmit));
    let target = input::text_field(t, &form_state.target)
        .placeholder(static_text(ctx.t(Key::NodesAddTargetPlaceholder)))
        .on_input(|value| press(Msg::AddTarget(value)))
        .on_submit(press(Msg::AddSubmit))
        .error(problem.map(|problem| {
            ctx.t(match problem {
                TargetProblem::Empty => Key::NodesAddTargetEmpty,
                TargetProblem::Invalid => Key::NodesAddTargetInvalid,
            })
            .into_owned()
        }));
    let config = input::text_field(t, &form_state.config)
        .placeholder(static_text(ctx.t(Key::NodesAddConfigPlaceholder)))
        .on_input(|value| press(Msg::AddConfig(value)))
        .on_submit(press(Msg::AddSubmit));
    let field = |key: Key, input: Element<'a, Message>| -> Element<'a, Message> {
        column![typography::hint(t, ctx.t(key)), input]
            .spacing(tokens::space::SM)
            .into()
    };
    form::group(
        t,
        vec![
            container(
                column![
                    typography::body(t, ctx.t(Key::NodesAddTitle))
                        .font(hangar_theme::fonts::ui_semibold()),
                    field(Key::NodesAddLabel, label.into()),
                    field(Key::NodesAddTarget, target.into()),
                    field(Key::NodesAddConfig, config.into()),
                    row![
                        ui_button::primary(t, ctx.t(Key::NodesAddSubmit))
                            .on_press(press(Msg::AddSubmit)),
                        typography::hint(t, ctx.t(Key::NodesAddHint)),
                    ]
                    .spacing(tokens::space::MD)
                    .align_y(Alignment::Center),
                ]
                .spacing(tokens::space::MD),
            )
            .padding(tokens::space::XL)
            .width(Length::Fill)
            .into(),
        ],
    )
}
