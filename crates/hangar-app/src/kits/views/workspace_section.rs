//! The « Kits » section of the workspace dialog: a switch per installed kit
//! (`WorkspaceKitsSection` and `KitEverywhereRow` of `KitActivation.tsx`).
//!
//! One row per kit from `kits_list`. A switch on opens the activation dialog
//! (nothing is stored before the person has read what the kit brings); a
//! switch off applies at once. A kit enabled on all workspaces and not listed
//! by this one shows a locked switch and an « everywhere » tag: Settings › Kits
//! manages it. A kit enabled here but no longer installed shows a « gone » row
//! whose switch removes it. After an accept, if the workspace has Claude Code
//! panes, a notice offers to restart them, as they keep the kits they started
//! with.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_core::kits::KitInfo;
use hangar_i18n::{Key, Translator, args};
use hangar_theme::{fonts, tokens};
use hangar_ui::badge::Tone;
use hangar_ui::{button, form, input, text as typography};

use super::activation_model::{Tag, TagTone};
use super::{msg, notice};
use crate::ctx::Ctx;
use crate::kits::activation::{
    KitState, hooks_to_accept, kit_name, kit_requested, kit_state, panes_to_restart,
    permission_diff,
};
use crate::kits::icon::kit_icon;
use crate::kits::model::{everywhere_kits, workspace_kits};
use crate::kits::{Msg, Restart, State};
use crate::message::Message;
use crate::state::WsId;
use crate::state::types::{Workspace, WorkspaceKit};

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// An installed kit, with a switch of its own.
    Kit(KitRow),
    /// An installed kit enabled on all workspaces: the switch is locked.
    Everywhere(KitRow),
    /// Enabled here, no longer installed.
    Gone(GoneRow),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitRow {
    pub id: String,
    pub name: String,
    /// The error that makes the kit unusable, or its version.
    pub meta: String,
    pub meta_is_error: bool,
    pub on: bool,
    /// Whether the switch can be used.
    pub switch_enabled: bool,
    /// What the switch is for, said to someone who cannot see it.
    pub switch_label: String,
    pub tags: Vec<Tag>,
    /// « Details » or « Review », when the kit is usable and enabled.
    pub open: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoneRow {
    pub id: String,
    pub meta: String,
    pub tag: Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartModel {
    pub title: String,
    pub body: String,
    pub action: String,
    pub later: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionModel {
    pub hint: String,
    pub error: Option<String>,
    pub none: Option<String>,
    pub rows: Vec<Row>,
    /// While the workspace is being created.
    pub pending: Option<String>,
    pub restart: Option<RestartModel>,
    pub restarted: Option<String>,
}

pub struct SectionInput<'a> {
    /// The workspace being edited; `None` while one is being created: there is
    /// no pane to run a setup step in yet, and no running agent to restart.
    pub workspace: Option<&'a Workspace>,
    /// The kits this workspace (or the draft) has enabled.
    pub value: &'a [WorkspaceKit],
    /// `None` until read.
    pub kits: Option<&'a [KitInfo]>,
    pub error: Option<&'a str>,
    pub everywhere: &'a [WorkspaceKit],
    pub restart: Option<&'a Restart>,
    pub restarted: bool,
}

fn tag(label: String, tone: TagTone) -> Tag {
    Tag { label, tone }
}

impl SectionModel {
    pub fn build(t: Translator, input: &SectionInput<'_>) -> SectionModel {
        let SectionInput {
            workspace,
            value,
            kits,
            ..
        } = *input;
        let listed = kits.unwrap_or_default();
        let gone: Vec<&WorkspaceKit> = match kits {
            Some(list) => value
                .iter()
                .filter(|entry| list.iter().all(|kit| kit.id != entry.id))
                .collect(),
            None => Vec::new(),
        };
        let mut rows = Vec::new();
        for kit in listed {
            let own = value.iter().find(|item| item.id == kit.id);
            // Its own entry wins and stays this dialog's; without one, the kit
            // is on by way of Settings › Kits.
            let shared = if own.is_some() {
                None
            } else {
                input.everywhere.iter().find(|item| item.id == kit.id)
            };
            let name = kit_name(kit).to_owned();
            let usable = kit.error.is_none() && kit.manifest.is_some();
            let requested = kit_requested(kit);
            if let Some(shared) = shared {
                let state = if usable {
                    kit_state(Some(shared), requested.as_deref())
                } else {
                    KitState::Broken
                };
                let mut tags = vec![tag(
                    t.t(Key::KitsManageEverywhereNote).into_owned(),
                    TagTone::Plain,
                )];
                if state != KitState::On {
                    tags.push(tag(
                        t.t(state.key()).into_owned(),
                        if state == KitState::Review {
                            TagTone::New
                        } else {
                            TagTone::Plain
                        },
                    ));
                }
                rows.push(Row::Everywhere(KitRow {
                    id: kit.id.clone(),
                    meta: kit
                        .error
                        .clone()
                        .unwrap_or_else(|| t.t(Key::KitsManageEverywhereNoteHint).into_owned()),
                    meta_is_error: kit.error.is_some(),
                    switch_label: t
                        .t_with(
                            Key::KitsManageEverywhereToggleLocked,
                            &args!("name" => name.as_str()),
                        )
                        .into_owned(),
                    name,
                    on: true,
                    switch_enabled: false,
                    tags,
                    open: None,
                }));
                continue;
            }
            let state = if usable {
                kit_state(own, requested.as_deref())
            } else if own.is_some() {
                KitState::Broken
            } else {
                KitState::Off
            };
            let held = state == KitState::Review
                && hooks_to_accept(&permission_diff(
                    requested.as_deref().unwrap_or_default(),
                    own.map_or(&[][..], |entry| &entry.granted[..]),
                ));
            let tone = match state {
                KitState::Review => TagTone::New,
                KitState::On => TagTone::Ok,
                _ => TagTone::Plain,
            };
            rows.push(Row::Kit(KitRow {
                id: kit.id.clone(),
                meta: kit
                    .error
                    .clone()
                    .unwrap_or_else(|| format!("v{}", kit.installed_version)),
                meta_is_error: kit.error.is_some(),
                switch_label: t
                    .t_with(Key::KitsManageToggle, &args!("name" => name.as_str()))
                    .into_owned(),
                name,
                on: own.is_some(),
                switch_enabled: usable || own.is_some(),
                tags: vec![tag(
                    t.t(if held {
                        Key::KitsManageStatusReviewHooks
                    } else {
                        state.key()
                    })
                    .into_owned(),
                    tone,
                )],
                open: (usable && matches!(state, KitState::Review | KitState::On)).then(|| {
                    t.t(if state == KitState::Review {
                        Key::KitsManageReview
                    } else {
                        Key::KitsManageDetails
                    })
                    .into_owned()
                }),
            }));
        }
        rows.extend(gone.iter().map(|entry| {
            Row::Gone(GoneRow {
                id: entry.id.clone(),
                meta: t
                    .t_with(Key::KitsManageBrokenName, &args!("id" => entry.id.as_str()))
                    .into_owned(),
                tag: tag(
                    t.t(Key::KitsManageStatusBroken).into_owned(),
                    TagTone::Plain,
                ),
            })
        }));

        let restart_count = workspace.map_or(0, |ws| panes_to_restart(ws).len());
        SectionModel {
            hint: t.t(Key::KitsManageSectionHint).into_owned(),
            error: input.error.map(|detail| {
                t.t_with(Key::KitsManageErrorPrefix, &args!("detail" => detail))
                    .into_owned()
            }),
            none: (kits.is_some_and(<[KitInfo]>::is_empty) && gone.is_empty())
                .then(|| t.t(Key::KitsManageSectionNone).into_owned()),
            rows,
            pending: (workspace.is_none() && !value.is_empty())
                .then(|| t.t(Key::KitsManagePendingNote).into_owned()),
            restart: input
                .restart
                .filter(|_| workspace.is_some())
                .map(|restart| RestartModel {
                    title: t.t(Key::KitsManageRestartTitle).into_owned(),
                    body: format!(
                        "{} {}",
                        t.t_with(
                            Key::KitsManageRestartBody,
                            &args!("name" => restart.kit_name.as_str())
                        ),
                        t.t(Key::KitsManageRestartResume)
                    ),
                    action: t
                        .t_with(Key::KitsManageRestartAction, &args!("n" => restart_count))
                        .into_owned(),
                    later: t.t(Key::KitsManageRestartLater).into_owned(),
                }),
            restarted: input
                .restarted
                .then(|| t.t(Key::KitsManageRestartDone).into_owned()),
        }
    }

    /// Every sentence the section shows, one per line.
    pub fn text(&self) -> String {
        let mut out = vec![self.hint.clone()];
        out.extend(self.error.clone());
        out.extend(self.none.clone());
        for row in &self.rows {
            match row {
                Row::Kit(kit) | Row::Everywhere(kit) => {
                    out.push(kit.name.clone());
                    out.push(kit.meta.clone());
                    out.push(kit.switch_label.clone());
                    out.extend(kit.tags.iter().map(|tag| tag.label.clone()));
                    out.extend(kit.open.clone());
                }
                Row::Gone(gone) => {
                    out.push(gone.id.clone());
                    out.push(gone.meta.clone());
                    out.push(gone.tag.label.clone());
                }
            }
        }
        out.extend(self.pending.clone());
        if let Some(restart) = &self.restart {
            out.extend([
                restart.title.clone(),
                restart.body.clone(),
                restart.action.clone(),
                restart.later.clone(),
            ]);
        }
        out.extend(self.restarted.clone());
        out.join("\n")
    }
}

/// The section for a workspace (`Some`) or one being created (`None`).
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: Option<&'a WsId>) -> Element<'a, Message> {
    let workspace = ws.and_then(|id| ctx.model.workspace(id));
    let own = match ws {
        Some(_) => workspace_kits(workspace),
        None => st.draft.clone(),
    };
    let everywhere = everywhere_kits(Some(ctx.model.settings()));
    let live = &ctx.model.live.kits;
    let input = SectionInput {
        workspace,
        value: &own,
        kits: live.installed.as_deref(),
        error: live.error.as_deref(),
        everywhere: &everywhere,
        restart: st.restart.as_ref().filter(|r| Some(&r.ws) == ws),
        restarted: ws.is_some() && st.restarted.as_ref() == ws,
    };
    let model = SectionModel::build(ctx.i18n, &input);
    draw(ctx, ws, &model, live.kits())
}

fn draw<'a>(
    ctx: Ctx<'a>,
    ws: Option<&'a WsId>,
    model: &SectionModel,
    installed: &[KitInfo],
) -> Element<'a, Message> {
    let mut parts: Vec<Element<'a, Message>> = vec![
        typography::hint(ctx.theme, model.hint.clone())
            .width(Length::Fill)
            .into(),
    ];
    if let Some(error) = &model.error {
        parts.push(notice(ctx, Tone::Danger, error.clone()));
    }
    if let Some(none) = &model.none {
        parts.push(
            typography::hint(ctx.theme, none.clone())
                .width(Length::Fill)
                .into(),
        );
    }
    let rows: Vec<Element<'a, Message>> = model
        .rows
        .iter()
        .map(|found| row_view(ctx, ws, found, installed))
        .collect();
    if !rows.is_empty() {
        parts.push(form::group(ctx.theme, rows));
    }
    if let Some(pending) = &model.pending {
        parts.push(
            typography::hint(ctx.theme, pending.clone())
                .width(Length::Fill)
                .into(),
        );
    }
    if let (Some(restart), Some(id)) = (&model.restart, ws) {
        parts.push(
            container(
                row![
                    column![
                        typography::ui(ctx.theme, restart.title.clone()).font(fonts::ui_semibold()),
                        typography::hint(ctx.theme, restart.body.clone()),
                    ]
                    .spacing(tokens::space::XS)
                    .width(Length::Fill),
                    button::secondary(ctx.theme, restart.action.clone())
                        .small()
                        .on_press(msg(Msg::RestartPanes(id.clone()))),
                    button::ghost(ctx.theme, restart.later.clone())
                        .small()
                        .on_press(msg(Msg::DismissRestart)),
                ]
                .spacing(tokens::space::MD)
                .align_y(Alignment::Center),
            )
            .padding([tokens::space::MD, tokens::space::XL])
            .style(hangar_theme::style::container::badge(
                ctx.theme,
                Tone::Accent,
            ))
            .into(),
        );
    }
    if let Some(done) = &model.restarted {
        parts.push(
            typography::hint(ctx.theme, done.clone())
                .width(Length::Fill)
                .into(),
        );
    }
    iced::widget::Column::with_children(parts)
        .spacing(tokens::space::LG)
        .width(Length::Fill)
        .into()
}

fn row_view<'a>(
    ctx: Ctx<'a>,
    ws: Option<&'a WsId>,
    found: &Row,
    installed: &[KitInfo],
) -> Element<'a, Message> {
    let line = |content: iced::widget::Row<'a, Message>| -> Element<'a, Message> {
        container(
            content
                .spacing(tokens::space::LG)
                .align_y(Alignment::Center),
        )
        .padding([tokens::space::MD, tokens::space::XL])
        .width(Length::Fill)
        .into()
    };
    match found {
        Row::Kit(kit) | Row::Everywhere(kit) => {
            let mut switch = input::switch(ctx.theme, kit.on);
            if kit.switch_enabled {
                let (ws, id) = (ws.cloned(), kit.id.clone());
                switch = switch.on_toggle(move |on| {
                    msg(Msg::Toggle {
                        ws: ws.clone(),
                        kit: id.clone(),
                        on,
                    })
                });
            }
            let icon: Element<'a, Message> = match installed.iter().find(|k| k.id == kit.id) {
                Some(info) => kit_icon(ctx, info, 16.0),
                None => iced::widget::Space::new().width(16).height(16).into(),
            };
            let meta = if kit.meta_is_error {
                typography::danger(ctx.theme, kit.meta.clone())
            } else {
                typography::hint(ctx.theme, kit.meta.clone())
            };
            let mut content = row![
                switch,
                icon,
                column![
                    text(kit.name.clone())
                        .size(tokens::font_size::BODY)
                        .font(fonts::ui_medium())
                        .wrapping(text::Wrapping::WordOrGlyph),
                    meta.wrapping(text::Wrapping::WordOrGlyph),
                ]
                .width(Length::Fill),
            ];
            for found_tag in &kit.tags {
                content = content.push(super::tag(ctx, found_tag));
            }
            if let Some(label) = &kit.open {
                content = content.push(
                    button::secondary(ctx.theme, label.clone())
                        .small()
                        .on_press(msg(Msg::Open {
                            ws: ws.cloned(),
                            kit: kit.id.clone(),
                        })),
                );
            }
            line(content)
        }
        Row::Gone(gone) => {
            let (ws, id) = (ws.cloned(), gone.id.clone());
            let switch = input::switch(ctx.theme, true).on_toggle(move |_| {
                msg(Msg::RemoveGone {
                    ws: ws.clone(),
                    kit: id.clone(),
                })
            });
            line(row![
                switch,
                column![
                    text(gone.id.clone())
                        .size(tokens::font_size::BODY)
                        .font(fonts::ui_medium()),
                    typography::hint(ctx.theme, gone.meta.clone()),
                ]
                .width(Length::Fill),
                super::tag(ctx, &gone.tag),
            ])
        }
    }
}

#[cfg(test)]
mod tests;
