//! « Connecter les agents au tableau »: registers Hangar's MCP server in the
//! agents' own configuration, and writes the playbook that tells them how to
//! use the board (`McpPanel.tsx`; `install.rs` and `instructions.rs` of
//! hangar-core do the writing).
//!
//! The panel opens on the active workspace's folder. It lists the tools it
//! knows how to configure, ticks the ones that are installed and not wired up
//! yet (or whose entry is broken, since reinstalling is the repair), and
//! reports what each write did.

use std::collections::HashSet;

use iced::widget::{column, container, row, rule, space, text};
use iced::{Alignment, Element, Length, Padding, Task};

use hangar_core::{install, instructions};
use hangar_i18n::{Key, args};
use hangar_theme::{fonts, style, tokens};
use hangar_ui::badge::{self, Tone};
use hangar_ui::{Icon, button, dialog, icon, input, text as typography};

use super::{Msg, State};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::Services;
use crate::state::Model;

/// A tool that can be configured, as `mcp_targets` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub path: String,
    /// The tool looks installed on this machine.
    pub detected: bool,
    /// Our server is already registered in its config.
    pub configured: bool,
    /// Registered, but pointing at an executable that is no longer there.
    pub stale: bool,
    /// The executable the config names now, shown next to a stale entry.
    pub command: Option<String>,
}

impl Target {
    /// Installed but not wired up, or wired up to nothing: what the panel
    /// ticks by itself.
    fn wants_install(&self) -> bool {
        self.detected && (!self.configured || self.stale)
    }
}

impl From<install::TargetStatus> for Target {
    fn from(status: install::TargetStatus) -> Self {
        Target {
            id: status.id,
            label: status.label,
            path: status.path,
            detected: status.detected,
            configured: status.configured,
            stale: status.stale,
            command: status.command,
        }
    }
}

/// What one write did: a target's, or a playbook file's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

/// The two lines that register the server by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manual {
    pub claude: String,
    pub codex: String,
}

#[derive(Debug, Clone)]
pub enum InstallMsg {
    /// `mcp_targets` answered for the first time: the ones to configure are
    /// ticked.
    Found(Vec<Target>),
    /// `mcp_targets` answered after an install: only the list changes.
    Refreshed(Vec<Target>),
    Manual(Option<Manual>),
    PlaybookPresent(bool),
    Toggle(String),
    Playbook(bool),
    Install,
    Installed {
        reports: Vec<Report>,
        targets: Vec<Target>,
        playbook: bool,
    },
    Copy(String),
}

/// The panel while it is open.
#[derive(Debug)]
pub struct Install {
    cwd: String,
    targets: Vec<Target>,
    selected: HashSet<String>,
    reports: Option<Vec<Report>>,
    manual: Option<Manual>,
    busy: bool,
    write_playbook: bool,
    playbook_present: bool,
}

/// Opens the panel on `cwd` and asks for what it shows.
pub fn open(st: &mut State, m: &Model) -> Task<Message> {
    let cwd = m
        .active_workspace()
        .map(|ws| ws.cwd.clone())
        .unwrap_or_default();
    st.install = Some(Install {
        cwd: cwd.clone(),
        targets: Vec::new(),
        selected: HashSet::new(),
        reports: None,
        manual: None,
        busy: false,
        write_playbook: true,
        playbook_present: false,
    });
    let ask = |work: fn(String) -> InstallMsg, cwd: String| {
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || work(cwd))
                    .await
                    .unwrap_or_else(|_| InstallMsg::Found(Vec::new()))
            },
            |message| Message::Mcp(Msg::Install(message)),
        )
    };
    Task::batch([
        ask(
            |cwd| {
                InstallMsg::Found(
                    install::mcp_targets(cwd)
                        .into_iter()
                        .map(Target::from)
                        .collect(),
                )
            },
            cwd.clone(),
        ),
        ask(
            |cwd| {
                InstallMsg::Manual(install::mcp_manual_commands(cwd).ok().and_then(|commands| {
                    Some(Manual {
                        claude: commands.get("claude")?.as_str()?.to_owned(),
                        codex: commands.get("codex")?.as_str()?.to_owned(),
                    })
                }))
            },
            cwd.clone(),
        ),
        ask(
            |cwd| InstallMsg::PlaybookPresent(instructions::agent_instructions_status(cwd)),
            cwd,
        ),
    ])
}

/// The id a playbook file is reported under: its name, not its path.
fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

pub fn update(st: &mut State, _m: &mut Model, _sv: &Services, msg: InstallMsg) -> Task<Message> {
    let Some(panel) = st.install.as_mut() else {
        return Task::none();
    };
    match msg {
        InstallMsg::Found(targets) => {
            // Pre-tick the tools that are installed but not wired up yet, and
            // the ones whose entry is broken, since reinstalling is the
            // repair.
            panel.selected = targets
                .iter()
                .filter(|target| target.wants_install())
                .map(|target| target.id.clone())
                .collect();
            panel.targets = targets;
        }
        InstallMsg::Refreshed(targets) => panel.targets = targets,
        InstallMsg::Manual(manual) => panel.manual = manual,
        InstallMsg::PlaybookPresent(present) => panel.playbook_present = present,
        InstallMsg::Toggle(id) => {
            if !panel.selected.remove(&id) {
                panel.selected.insert(id);
            }
        }
        InstallMsg::Playbook(on) => panel.write_playbook = on,
        InstallMsg::Install => {
            if panel.busy || panel.selected.is_empty() {
                return Task::none();
            }
            panel.busy = true;
            // In the order the list shows them, as the TS spread a set.
            let ids: Vec<String> = panel
                .targets
                .iter()
                .filter(|target| panel.selected.contains(&target.id))
                .map(|target| target.id.clone())
                .collect();
            let (cwd, playbook) = (panel.cwd.clone(), panel.write_playbook);
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || run(ids, cwd, playbook))
                        .await
                        .unwrap_or_else(|error| InstallMsg::Installed {
                            reports: vec![Report {
                                id: String::new(),
                                ok: false,
                                message: error.to_string(),
                            }],
                            targets: Vec::new(),
                            playbook,
                        })
                },
                |message| Message::Mcp(Msg::Install(message)),
            );
        }
        InstallMsg::Installed {
            reports,
            targets,
            playbook,
        } => {
            panel.busy = false;
            if playbook {
                panel.playbook_present = true;
            }
            panel.reports = Some(reports);
            // A list that could not be read again leaves the one on screen.
            if !targets.is_empty() {
                panel.targets = targets;
            }
        }
        InstallMsg::Copy(text) => return crate::services::clipboard::write(text),
    }
    Task::none()
}

/// The install, then the playbook if it is ticked, then the list again.
fn run(ids: Vec<String>, cwd: String, playbook: bool) -> InstallMsg {
    let mut reports: Vec<Report> = install::mcp_install(ids, cwd.clone())
        .into_iter()
        .map(|report| Report {
            id: report.id,
            ok: report.ok,
            message: report.message,
        })
        .collect();
    if playbook {
        reports.extend(
            instructions::write_agent_instructions(cwd.clone())
                .into_iter()
                .map(|written| Report {
                    id: file_name(&written.path),
                    ok: written.ok,
                    message: written.message,
                }),
        );
    }
    let targets = install::mcp_targets(cwd)
        .into_iter()
        .map(Target::from)
        .collect();
    InstallMsg::Installed {
        reports,
        targets,
        playbook,
    }
}

fn msg(message: InstallMsg) -> Message {
    Message::Mcp(Msg::Install(message))
}

/// `Overlay::McpInstall`: connect agents to the board.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(panel) = st.install.as_ref() else {
        return space().into();
    };

    let mut body = column![typography::label(t, ctx.t(Key::McpIntro))].spacing(tokens::space::XL);

    let mut targets = column![].spacing(0);
    for (at, target) in panel.targets.iter().enumerate() {
        if at > 0 {
            targets = targets.push(rule::horizontal(1).style(style::rule::hairline(t)));
        }
        targets = targets.push(target_row(ctx, panel, target));
    }
    if !panel.targets.is_empty() {
        body = body.push(well(ctx, targets.into()));
    }

    body = body.push(well(ctx, playbook_row(ctx, panel)));

    if let Some(reports) = &panel.reports {
        body = body.push(reports_view(ctx, reports));
    }
    if let Some(manual) = &panel.manual {
        body = body.push(manual_view(ctx, manual));
    }

    let install = button::primary(
        t,
        if panel.busy {
            ctx.t(Key::McpInstalling)
        } else {
            ctx.i18n
                .t_with(Key::McpInstall, &args!("n" => panel.selected.len()))
        },
    )
    .on_press_maybe((!panel.busy && !panel.selected.is_empty()).then(|| msg(InstallMsg::Install)));
    let close =
        button::ghost(t, ctx.t(Key::McpClose)).on_press(Message::Intent(Intent::CloseOverlay));

    let shown = dialog::dialog(t, ctx.t(Key::McpTitle), body)
        .subtitle(panel.cwd.as_str())
        .width(tokens::size::DIALOG_LIST_WIDTH)
        .body_max_height(480.0)
        .close_tip(ctx.t(Key::McpClose))
        .on_close(Message::Intent(Intent::CloseOverlay))
        .action(close)
        .action(install);
    dialog::over(
        t,
        space().width(Length::Fill).height(Length::Fill),
        Some(shown),
    )
    .into()
}

/// A box the targets and the playbook sit in (`.targets`).
fn well<'a>(ctx: Ctx<'a>, content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .style(style::container::well(ctx.theme))
        .into()
}

/// A tick box, what it is, the badges that say its state, and a line under.
fn choice<'a>(
    ctx: Ctx<'a>,
    label: String,
    checked: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
    badges: Vec<Element<'a, Message>>,
    lines: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut head = row![input::checkbox(t, label, checked).on_toggle(on_toggle)]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center);
    for badge in badges {
        head = head.push(badge);
    }
    let mut stack = column![head].spacing(2);
    for line in lines {
        stack = stack.push(container(line).padding(Padding {
            left: 24.0,
            ..Padding::ZERO
        }));
    }
    container(stack)
        .padding(Padding::from([tokens::space::LG, tokens::space::XL]))
        .width(Length::Fill)
        .into()
}

fn target_row<'a>(ctx: Ctx<'a>, panel: &'a Install, target: &'a Target) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut badges = Vec::new();
    if target.stale {
        badges.push(badge::badge(t, ctx.t(Key::McpStale), Tone::Danger));
    }
    if target.configured && !target.stale {
        badges.push(badge::badge(t, ctx.t(Key::McpConfigured), Tone::Success));
    }
    if !target.detected && !target.configured {
        badges.push(badge::badge(t, ctx.t(Key::McpNotDetected), Tone::Neutral));
    }
    let mut lines = vec![
        typography::mono(t, target.path.as_str())
            .size(tokens::font_size::CAPTION)
            .style(style::text::subtle(t))
            .into(),
    ];
    if target.stale {
        let hint = match &target.command {
            Some(command) => ctx
                .i18n
                .t_with(Key::McpStaleHint, &args!("command" => command.as_str())),
            None => ctx.t(Key::McpStaleHintNoCommand),
        };
        lines.push(typography::danger(t, hint).into());
    }
    let id = target.id.clone();
    choice(
        ctx,
        target.label.clone(),
        panel.selected.contains(&target.id),
        move |_| msg(InstallMsg::Toggle(id.clone())),
        badges,
        lines,
    )
}

fn playbook_row<'a>(ctx: Ctx<'a>, panel: &'a Install) -> Element<'a, Message> {
    let t = ctx.theme;
    let badges = if panel.playbook_present {
        vec![badge::badge(t, ctx.t(Key::McpAlreadyThere), Tone::Success)]
    } else {
        Vec::new()
    };
    choice(
        ctx,
        ctx.t(Key::McpPlaybook).into_owned(),
        panel.write_playbook,
        |on| msg(InstallMsg::Playbook(on)),
        badges,
        vec![
            typography::mono(t, ctx.t(Key::McpPlaybookHint))
                .size(tokens::font_size::CAPTION)
                .style(style::text::subtle(t))
                .into(),
        ],
    )
}

/// What each write did, and what to do about it.
fn reports_view<'a>(ctx: Ctx<'a>, reports: &'a [Report]) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut list = column![].spacing(tokens::space::SM);
    for report in reports {
        let (glyph, tint) = if report.ok {
            (Icon::Check, t.colors.success)
        } else {
            (Icon::Alert, t.colors.danger)
        };
        list = list.push(
            row![
                icon(glyph, tokens::size::ICON).color(tint),
                text(report.id.as_str())
                    .size(tokens::font_size::UI)
                    .font(fonts::mono())
                    .color(tint),
                typography::muted(t, report.message.as_str()).width(Length::Fill),
            ]
            .spacing(tokens::space::MD)
            .align_y(Alignment::Start),
        );
    }
    list = list.push(typography::hint(t, ctx.t(Key::McpRestartHint)));
    container(list)
        .padding(tokens::space::XL)
        .width(Length::Fill)
        .style(style::container::well(t))
        .into()
}

/// The two commands that do by hand what the button does, each to copy: iced
/// cannot select text, so a button puts the line on the clipboard.
fn manual_view<'a>(ctx: Ctx<'a>, manual: &'a Manual) -> Element<'a, Message> {
    let t = ctx.theme;
    let snippet = |line: &'a str| -> Element<'a, Message> {
        container(
            row![
                text(line)
                    .size(tokens::font_size::UI)
                    .font(fonts::mono())
                    .width(Length::Fill),
                button::icon_button(t, Icon::Copy)
                    .small()
                    .tip(ctx.t(Key::PreviewCopy))
                    .on_press(msg(InstallMsg::Copy(line.to_owned()))),
            ]
            .spacing(tokens::space::MD)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([tokens::space::MD, tokens::space::XL]))
        .width(Length::Fill)
        .style(style::container::well(t))
        .into()
    };
    column![
        row![
            typography::body(t, ctx.t(Key::McpManual)).font(fonts::ui_semibold()),
            typography::hint(t, ctx.t(Key::McpManualHint)),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
        snippet(&manual.claude),
        snippet(&manual.codex),
    ]
    .spacing(tokens::space::MD)
    .into()
}
