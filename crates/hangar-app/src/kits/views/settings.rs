//! Settings › Kits: the kits installed on this machine, adding one from a
//! folder or a GitHub link, checking and applying updates, removing
//! (`KitsView` and `KitsSettings` of `settings/KitsSettings.tsx`).
//!
//! The page is a model ([`PageModel`], from the kits and what the page
//! remembers) and its drawing. The library sits above the list
//! ([`super::gallery`]).
//!
//! A kit's own icon is read from its folder and drawn by `icon::kit_icon`; a
//! plug stands in when it has none or does not load.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_core::kits::{GitSource, KitInfo, Source};
use hangar_i18n::{Key, Translator, args};
use hangar_theme::{fonts, tokens};
use hangar_ui::badge::Tone;
use hangar_ui::{button, form, input, text as typography};

use super::activation_model::{Loadable, Tag, TagTone};
use super::{gallery, msg, notice, tag};
use crate::ctx::Ctx;
use crate::kits::activation::{
    KitState, SourceKind, hooks_to_accept, kit_name, kit_requested, kit_state,
    only_commands_to_accept, permission_diff, source_line,
};
use crate::kits::icon::kit_icon;
use crate::kits::model::{everywhere_kits, kit_pinned_tag, workspace_kits};
use crate::kits::{Adding, Msg, Page, State, UpdateNote};
use crate::message::Message;
use crate::state::WsId;
use crate::state::types::{Workspace, WorkspaceKit};

/// The text a kit's update note says (`noteText`).
pub fn note_text(t: Translator, note: &UpdateNote) -> String {
    match note {
        UpdateNote::Checking => t.t(Key::KitsManageChecking).into_owned(),
        UpdateNote::None { version } => t
            .t_with(
                Key::KitsManageUpToDate,
                &args!("version" => version.as_str()),
            )
            .into_owned(),
        UpdateNote::Available { current, latest } => t
            .t_with(
                Key::KitsManageUpdateAvailable,
                &args!("current" => current.as_str(), "latest" => latest.as_str()),
            )
            .into_owned(),
        UpdateNote::Updating => t.t(Key::KitsManageUpdating).into_owned(),
        UpdateNote::Done { from, to, .. } => t
            .t_with(
                Key::KitsManageUpdated,
                &args!("from" => from.as_str(), "to" => to.as_str()),
            )
            .into_owned(),
        UpdateNote::Folder => t.t(Key::KitsManageFolderNoUpdate).into_owned(),
        UpdateNote::Reloaded { version } => t
            .t_with(
                Key::KitsManageReloaded,
                &args!("version" => version.as_str()),
            )
            .into_owned(),
        UpdateNote::Error(detail) => t
            .t_with(
                Key::KitsManageErrorPrefix,
                &args!("detail" => detail.as_str()),
            )
            .into_owned(),
    }
}

/// What the main button of a kit's row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Primary {
    /// A linked folder is read again.
    Reload {
        label: String,
        enabled: bool,
    },
    /// A git kit moves to the version found.
    Update {
        label: String,
        enabled: bool,
    },
    Check {
        label: String,
        enabled: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actions {
    /// The removal waits for its second click.
    Confirm {
        ask: String,
        wording: String,
        cancel: String,
        confirm: String,
    },
    Normal {
        primary: Primary,
        remove: String,
    },
}

/// The switch « On all workspaces » of a kit and what is said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EverywhereModel {
    pub label: String,
    pub on: bool,
    pub switch_enabled: bool,
    pub switch_label: String,
    pub tags: Vec<Tag>,
    /// The « Review » button of a grant an update left to accept again.
    pub review: Option<String>,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowModel {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub source: String,
    /// « This kit cannot be used » and why.
    pub error: Option<(String, String)>,
    pub note: Option<String>,
    /// Said under a finished update that asked for more.
    pub note_extra: Option<String>,
    pub available: Option<String>,
    pub everywhere: EverywhereModel,
    pub actions: Actions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow {
    pub id: WsId,
    pub name: String,
    pub enabled: String,
    pub manage: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartNotice {
    pub title: String,
    pub body: String,
    pub dismiss: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddModel {
    pub title: String,
    pub folder_button: String,
    pub folder_enabled: bool,
    pub folder_hint: String,
    pub folder_error: Option<String>,
    pub git_label: String,
    pub git_button: String,
    pub git_enabled: bool,
    pub git_hint: String,
    pub git_error: Option<String>,
    pub flash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageModel {
    pub intro: String,
    pub error: Option<String>,
    pub restart: Option<RestartNotice>,
    pub installed_title: String,
    pub installed: Loadable<Vec<RowModel>>,
    pub by_workspace: Option<(String, String, Vec<WorkspaceRow>)>,
    pub add: AddModel,
}

pub struct PageInput<'a> {
    /// `None` until hangar-core has answered.
    pub kits: Option<&'a [KitInfo]>,
    pub error: Option<&'a str>,
    pub page: &'a Page,
    /// The workspaces a kit can be enabled on, HQ excluded.
    pub workspaces: &'a [Workspace],
    /// The kits enabled on all workspaces, as stored.
    pub everywhere: &'a [WorkspaceKit],
    /// Whether a workspace is active, to open a pane for a setup step in.
    pub has_active_workspace: bool,
}

/// The example the hint gives.
const GIT_EXAMPLE: &str = "https://github.com/owner/repo/tree/main/my-kit";
/// What the field shows while it is empty: an address, not a sentence.
pub const GIT_PLACEHOLDER: &str = "https://github.com/owner/repo";

impl PageModel {
    pub fn build(t: Translator, input: &PageInput<'_>) -> PageModel {
        let page = input.page;
        let installed = match input.kits {
            None => Loadable::Loading(t.t(Key::KitsManageLoading).into_owned()),
            Some([]) => Loadable::Empty(t.t(Key::KitsManageEmpty).into_owned()),
            Some(kits) => {
                Loadable::Ready(kits.iter().map(|kit| Self::row(t, kit, input)).collect())
            }
        };
        let workspaces: Vec<WorkspaceRow> = input
            .workspaces
            .iter()
            .map(|ws| WorkspaceRow {
                id: ws.id.clone(),
                name: ws.name.clone(),
                enabled: t
                    .tn(
                        Key::KitsManageEnabledCount,
                        workspace_kits(Some(ws)).len(),
                        &[],
                    )
                    .into_owned(),
                manage: t.t(Key::KitsManageManage).into_owned(),
            })
            .collect();
        PageModel {
            intro: t.t(Key::KitsManageIntro).into_owned(),
            error: input.error.map(|detail| {
                t.t_with(Key::KitsManageErrorPrefix, &args!("detail" => detail))
                    .into_owned()
            }),
            restart: page.restart_for.as_ref().map(|name| RestartNotice {
                title: t.t(Key::KitsManageEverywhereRestartTitle).into_owned(),
                body: format!(
                    "{} {}",
                    t.t_with(
                        Key::KitsManageEverywhereRestartBody,
                        &args!("name" => name.as_str())
                    ),
                    t.t(Key::KitsManageRestartResume)
                ),
                dismiss: t.t(Key::KitsManageEverywhereDismiss).into_owned(),
            }),
            installed_title: t.t(Key::KitsManageInstalled).into_owned(),
            installed,
            by_workspace: (!workspaces.is_empty()).then(|| {
                (
                    t.t(Key::KitsManageByWorkspace).into_owned(),
                    t.t(Key::KitsManageByWorkspaceHint).into_owned(),
                    workspaces,
                )
            }),
            add: AddModel {
                title: t.t(Key::KitsManageAdd).into_owned(),
                folder_button: t
                    .t(if page.adding == Some(Adding::Folder) {
                        Key::KitsManageAdding
                    } else {
                        Key::KitsManageAddFolder
                    })
                    .into_owned(),
                folder_enabled: page.adding.is_none(),
                folder_hint: t.t(Key::KitsManageAddFolderHint).into_owned(),
                folder_error: page.folder_error.clone(),
                git_label: t.t(Key::KitsManageGitLabel).into_owned(),
                git_button: t
                    .t(if page.adding == Some(Adding::Git) {
                        Key::KitsManageAdding
                    } else {
                        Key::KitsManageAddGit
                    })
                    .into_owned(),
                git_enabled: page.adding.is_none() && !page.git_url.trim().is_empty(),
                git_hint: t
                    .t_with(Key::KitsManageGitHint, &args!("example" => GIT_EXAMPLE))
                    .into_owned(),
                git_error: page.git_error.clone(),
                flash: page.flash.clone(),
            },
        }
    }

    fn row(t: Translator, kit: &KitInfo, input: &PageInput<'_>) -> RowModel {
        let page = input.page;
        let note = page.notes.get(&kit.id);
        let pinned = kit_pinned_tag(&kit.source);
        // The tag is said in words below, rather than by git's full name for it.
        let shown_source = match (&kit.source, pinned) {
            (Source::Git(git), Some(_)) => Source::Git(GitSource {
                git_ref: None,
                ..git.clone()
            }),
            (source, _) => source.clone(),
        };
        let (source_kind, source_text) = source_line(&shown_source);
        let is_folder = matches!(kit.source, Source::Folder { .. });
        let working = matches!(note, Some(UpdateNote::Checking | UpdateNote::Updating));
        let available = matches!(note, Some(UpdateNote::Available { .. }))
            || (note.is_none() && kit.update.as_ref().is_some_and(|u| u.available));
        let latest = match note {
            Some(UpdateNote::Available { latest, .. }) => Some(latest.clone()),
            _ => kit.update.as_ref().map(|u| u.latest.clone()),
        };
        let manifest = kit.manifest.as_ref();
        let everywhere = input.everywhere.iter().find(|item| item.id == kit.id);
        let usable = kit.error.is_none() && manifest.is_some();
        let requested = kit_requested(kit);
        let state = if usable {
            kit_state(everywhere, requested.as_deref())
        } else if everywhere.is_some() {
            KitState::Broken
        } else {
            KitState::Off
        };
        let diff = permission_diff(
            requested.as_deref().unwrap_or_default(),
            everywhere.map_or(&[][..], |entry| &entry.granted[..]),
        );
        let held_for_hooks = state == KitState::Review && hooks_to_accept(&diff);
        let name = kit_name(kit).to_owned();

        let mut source = match source_kind {
            SourceKind::Folder => t
                .t_with(
                    Key::KitsManageSourceFolder,
                    &args!("path" => source_text.as_str()),
                )
                .into_owned(),
            SourceKind::Git => t
                .t_with(
                    Key::KitsManageSourceGit,
                    &args!("url" => source_text.as_str()),
                )
                .into_owned(),
        };
        if is_folder {
            source.push_str(&format!(" · {}", t.t(Key::KitsManageLinked)));
        }
        if let Some(tag_name) = pinned {
            source.push_str(&format!(
                " · {}",
                t.t_with(Key::KitsManagePinned, &args!("tag" => tag_name))
            ));
        }

        let mut every_tags = Vec::new();
        let mut review = None;
        if everywhere.is_some() {
            match state {
                KitState::Review => {
                    every_tags.push(Tag {
                        label: t
                            .t(if held_for_hooks {
                                Key::KitsManageStatusReviewHooks
                            } else {
                                Key::KitsManageStatusReview
                            })
                            .into_owned(),
                        tone: TagTone::New,
                    });
                    review = Some(t.t(Key::KitsManageReview).into_owned());
                }
                KitState::On => every_tags.push(Tag {
                    label: t.t(Key::KitsManageStatusOn).into_owned(),
                    tone: TagTone::Ok,
                }),
                KitState::Broken => every_tags.push(Tag {
                    label: t.t(Key::KitsManageStatusBroken).into_owned(),
                    tone: TagTone::Plain,
                }),
                KitState::Off => {}
            }
        }
        let mut hint = t
            .t(if state != KitState::Review {
                Key::KitsManageEverywhereHint
            } else if only_commands_to_accept(&diff) {
                Key::KitsTabEverywhereReviewCommands
            } else {
                Key::KitsManageEverywhereReview
            })
            .into_owned();
        if everywhere.is_none() && usable && !input.has_active_workspace {
            hint.push('\n');
            hint.push_str(&t.t(Key::KitsManageEverywhereNeedsWs));
        }

        let actions = if page.removing.as_deref() == Some(kit.id.as_str()) {
            let ask = t
                .t_with(Key::KitsManageRemoveAsk, &args!("name" => name.as_str()))
                .into_owned();
            Actions::Confirm {
                ask,
                wording: t
                    .t(if is_folder {
                        Key::KitsManageRemoveFolder
                    } else {
                        Key::KitsManageRemoveGit
                    })
                    .into_owned(),
                cancel: t.t(Key::KitsManageCancel).into_owned(),
                confirm: t.t(Key::KitsManageRemoveConfirm).into_owned(),
            }
        } else {
            let primary = if is_folder {
                Primary::Reload {
                    label: t.t(Key::KitsManageReload).into_owned(),
                    enabled: !working && !(kit.error.is_some() && manifest.is_none()),
                }
            } else if available {
                Primary::Update {
                    label: t
                        .t(if working {
                            Key::KitsManageUpdating
                        } else {
                            Key::KitsManageUpdate
                        })
                        .into_owned(),
                    enabled: !working,
                }
            } else {
                Primary::Check {
                    label: t
                        .t(if matches!(note, Some(UpdateNote::Checking)) {
                            Key::KitsManageChecking
                        } else {
                            Key::KitsManageCheckUpdate
                        })
                        .into_owned(),
                    enabled: !working,
                }
            };
            Actions::Normal {
                primary,
                remove: t.t(Key::KitsManageRemove).into_owned(),
            }
        };

        RowModel {
            id: kit.id.clone(),
            name: match manifest {
                Some(manifest) => manifest.name.clone(),
                None => t
                    .t_with(Key::KitsManageBrokenName, &args!("id" => kit.id.as_str()))
                    .into_owned(),
            },
            version: manifest.map(|_| {
                t.t_with(
                    Key::KitsManageVersion,
                    &args!("version" => kit.installed_version.as_str()),
                )
                .into_owned()
            }),
            description: manifest
                .map(|manifest| manifest.description.clone())
                .filter(|text| !text.is_empty()),
            source,
            error: kit
                .error
                .as_ref()
                .map(|detail| (t.t(Key::KitsManageBroken).into_owned(), detail.clone())),
            note: note
                .filter(|note| !matches!(note, UpdateNote::Available { .. }))
                .map(|note| note_text(t, note)),
            note_extra: matches!(note, Some(UpdateNote::Done { new_permissions, .. }) if *new_permissions > 0)
                .then(|| t.t(Key::KitsManageUpdatedPerms).into_owned()),
            available: latest.filter(|_| available).map(|latest| {
                t.t_with(
                    Key::KitsManageUpdateAvailable,
                    &args!("current" => kit.installed_version.as_str(), "latest" => latest.as_str()),
                )
                .into_owned()
            }),
            everywhere: EverywhereModel {
                label: t.t(Key::KitsManageEverywhere).into_owned(),
                on: everywhere.is_some(),
                switch_enabled: usable || everywhere.is_some(),
                switch_label: t
                    .t_with(Key::KitsManageEverywhereToggle, &args!("name" => name.as_str()))
                    .into_owned(),
                tags: every_tags,
                review,
                hint,
            },
            actions,
        }
    }

    /// Every sentence the page shows, one per line, for the tests to look
    /// through the way the web app's looked through its markup.
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut out = vec![self.intro.clone()];
        out.extend(self.error.clone());
        if let Some(restart) = &self.restart {
            out.extend([
                restart.title.clone(),
                restart.body.clone(),
                restart.dismiss.clone(),
            ]);
        }
        out.push(self.installed_title.clone());
        match &self.installed {
            Loadable::Loading(text) | Loadable::Empty(text) => out.push(text.clone()),
            Loadable::Ready(rows) => {
                for row in rows {
                    out.push(row.name.clone());
                    out.extend(row.version.clone());
                    out.extend(row.description.clone());
                    out.push(row.source.clone());
                    if let Some((title, detail)) = &row.error {
                        out.extend([title.clone(), detail.clone()]);
                    }
                    out.extend(row.note.clone());
                    out.extend(row.note_extra.clone());
                    out.extend(row.available.clone());
                    out.push(row.everywhere.label.clone());
                    out.push(row.everywhere.switch_label.clone());
                    out.extend(row.everywhere.tags.iter().map(|tag| tag.label.clone()));
                    out.extend(row.everywhere.review.clone());
                    out.push(row.everywhere.hint.clone());
                    match &row.actions {
                        Actions::Confirm {
                            ask,
                            wording,
                            cancel,
                            confirm,
                        } => out.extend([
                            ask.clone(),
                            wording.clone(),
                            cancel.clone(),
                            confirm.clone(),
                        ]),
                        Actions::Normal { primary, remove } => {
                            out.push(match primary {
                                Primary::Reload { label, .. }
                                | Primary::Update { label, .. }
                                | Primary::Check { label, .. } => label.clone(),
                            });
                            out.push(remove.clone());
                        }
                    }
                }
            }
        }
        if let Some((title, hint, rows)) = &self.by_workspace {
            out.extend([title.clone(), hint.clone()]);
            for row in rows {
                out.extend([row.name.clone(), row.enabled.clone(), row.manage.clone()]);
            }
        }
        let add = &self.add;
        out.extend([
            add.title.clone(),
            add.folder_button.clone(),
            add.folder_hint.clone(),
        ]);
        out.extend(add.folder_error.clone());
        out.extend([
            add.git_label.clone(),
            add.git_button.clone(),
            add.git_hint.clone(),
        ]);
        out.extend(add.git_error.clone());
        out.extend(add.flash.clone());
        out.join("\n")
    }
}

/// Settings › Kits.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let state = ctx.model.state();
    let workspaces: Vec<Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| ws.hq != Some(true))
        .cloned()
        .collect();
    let everywhere = everywhere_kits(Some(&state.settings));
    let live = &ctx.model.live.kits;
    let input = PageInput {
        kits: live.installed.as_deref(),
        error: live.error.as_deref(),
        page: &st.page,
        workspaces: &workspaces,
        everywhere: &everywhere,
        has_active_workspace: ctx.model.active_workspace().is_some(),
    };
    let model = PageModel::build(ctx.i18n, &input);
    draw(ctx, st, &model, live.kits())
}

fn draw<'a>(
    ctx: Ctx<'a>,
    st: &'a State,
    model: &PageModel,
    installed: &[KitInfo],
) -> Element<'a, Message> {
    let mut sections: Vec<Element<'a, Message>> = Vec::new();
    let mut top: Vec<Element<'a, Message>> = vec![
        typography::label(ctx.theme, model.intro.clone())
            .width(Length::Fill)
            .into(),
    ];
    if let Some(error) = &model.error {
        top.push(notice(ctx, Tone::Danger, error.clone()));
    }
    if let Some(restart) = &model.restart {
        top.push(
            container(
                row![
                    column![
                        typography::ui(ctx.theme, restart.title.clone()).font(fonts::ui_semibold()),
                        typography::hint(ctx.theme, restart.body.clone()),
                    ]
                    .spacing(tokens::space::XS)
                    .width(Length::Fill),
                    button::ghost(ctx.theme, restart.dismiss.clone())
                        .small()
                        .on_press(msg(Msg::DismissEverywhereRestart)),
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
    sections.push(
        iced::widget::Column::with_children(top)
            .spacing(tokens::space::LG)
            .into(),
    );

    sections.push(gallery::view(
        ctx,
        &st.page,
        ctx.model.live.kits.installed.as_deref(),
    ));

    let rows: Vec<Element<'a, Message>> = match &model.installed {
        Loadable::Loading(words) | Loadable::Empty(words) => vec![
            container(typography::hint(ctx.theme, words.clone()))
                .padding([tokens::space::LG, tokens::space::XL])
                .into(),
        ],
        Loadable::Ready(rows) => rows
            .iter()
            .map(|one| row_view(ctx, one, installed))
            .collect(),
    };
    sections.push(
        form::section(ctx.theme, model.installed_title.clone())
            .group(rows)
            .into(),
    );

    if let Some((title, hint, rows)) = &model.by_workspace {
        let rows = rows
            .iter()
            .map(|one| {
                container(
                    row![
                        column![
                            typography::body(ctx.theme, one.name.clone()),
                            typography::hint(ctx.theme, one.enabled.clone()),
                        ]
                        .width(Length::Fill),
                        button::secondary(ctx.theme, one.manage.clone())
                            .on_press(msg(Msg::ManageWorkspace(one.id.clone()))),
                    ]
                    .spacing(tokens::space::LG)
                    .align_y(Alignment::Center),
                )
                .padding([tokens::space::MD, tokens::space::XL])
                .into()
            })
            .collect();
        sections.push(
            form::section(ctx.theme, title.clone())
                .hint(hint.clone())
                .group(rows)
                .into(),
        );
    }

    sections.push(add_view(ctx, st, &model.add));
    form::page(sections)
}

fn row_view<'a>(ctx: Ctx<'a>, one: &RowModel, installed: &[KitInfo]) -> Element<'a, Message> {
    let kit = installed.iter().find(|kit| kit.id == one.id);
    let icon: Element<'a, Message> = match kit {
        Some(kit) => kit_icon(ctx, kit, 20.0),
        None => iced::widget::Space::new().width(20).height(20).into(),
    };
    let mut title = row![typography::body(ctx.theme, one.name.clone()).font(fonts::ui_medium()),]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center);
    if let Some(version) = &one.version {
        title = title.push(typography::mono(ctx.theme, version.clone()));
    }
    let mut text_column = column![title.wrap()].spacing(tokens::space::XXS);
    if let Some(description) = &one.description {
        text_column = text_column.push(typography::hint(ctx.theme, description.clone()));
    }
    text_column = text_column.push(
        typography::subtle(ctx.theme, one.source.clone())
            .font(fonts::mono())
            .wrapping(text::Wrapping::WordOrGlyph),
    );

    let mut parts: Vec<Element<'a, Message>> = vec![
        row![icon, text_column.width(Length::Fill)]
            .spacing(tokens::space::LG)
            .align_y(Alignment::Start)
            .into(),
    ];
    if let Some((title, detail)) = &one.error {
        parts.push(notice(ctx, Tone::Danger, format!("{title}\n{detail}")));
    }
    for words in [&one.note, &one.note_extra, &one.available]
        .into_iter()
        .flatten()
    {
        parts.push(
            typography::hint(ctx.theme, words.clone())
                .width(Length::Fill)
                .into(),
        );
    }

    let every = &one.everywhere;
    let mut switch = input::switch(ctx.theme, every.on).label(every.label.clone());
    if every.switch_enabled {
        let id = one.id.clone();
        switch = switch.on_toggle(move |on| {
            msg(Msg::ToggleEverywhere {
                kit: id.clone(),
                on,
            })
        });
    }
    let mut every_line = row![switch]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center);
    for found in &every.tags {
        every_line = every_line.push(tag(ctx, found));
    }
    if let Some(label) = &every.review {
        every_line = every_line.push(
            button::secondary(ctx.theme, label.clone())
                .small()
                .on_press(msg(Msg::ReviewEverywhere(one.id.clone()))),
        );
    }
    parts.push(every_line.wrap().into());
    parts.push(
        typography::hint(ctx.theme, every.hint.clone())
            .width(Length::Fill)
            .into(),
    );

    match &one.actions {
        Actions::Confirm {
            ask,
            wording,
            cancel,
            confirm,
        } => {
            parts.push(
                column![
                    typography::ui(ctx.theme, ask.clone()).font(fonts::ui_semibold()),
                    typography::hint(ctx.theme, wording.clone()),
                ]
                .spacing(tokens::space::XXS)
                .into(),
            );
            parts.push(
                row![
                    button::ghost(ctx.theme, cancel.clone()).on_press(msg(Msg::AskRemove(None))),
                    button::danger(ctx.theme, confirm.clone())
                        .on_press(msg(Msg::Remove(one.id.clone()))),
                ]
                .spacing(tokens::space::MD)
                .into(),
            );
        }
        Actions::Normal { primary, remove } => {
            let id = one.id.clone();
            let main: Element<'a, Message> = match primary {
                Primary::Reload { label, enabled } => button::secondary(ctx.theme, label.clone())
                    .on_press_maybe(enabled.then(|| msg(Msg::Update(id))))
                    .into(),
                Primary::Update { label, enabled } => button::primary(ctx.theme, label.clone())
                    .on_press_maybe(enabled.then(|| msg(Msg::Update(id))))
                    .into(),
                Primary::Check { label, enabled } => button::secondary(ctx.theme, label.clone())
                    .on_press_maybe(enabled.then(|| msg(Msg::Check(id))))
                    .into(),
            };
            parts.push(
                row![
                    main,
                    button::danger(ctx.theme, remove.clone())
                        .on_press(msg(Msg::AskRemove(Some(one.id.clone())))),
                ]
                .spacing(tokens::space::MD)
                .into(),
            );
        }
    }
    container(iced::widget::Column::with_children(parts).spacing(tokens::space::MD))
        .padding([tokens::space::LG, tokens::space::XL])
        .width(Length::Fill)
        .into()
}

fn add_view<'a>(
    ctx: Ctx<'a>,
    st: &'a State,
    add: &super::settings::AddModel,
) -> Element<'a, Message> {
    let mut folder: Vec<Element<'a, Message>> = vec![
        row![
            button::secondary(ctx.theme, add.folder_button.clone())
                .on_press_maybe(add.folder_enabled.then(|| msg(Msg::AddFolder))),
            typography::hint(ctx.theme, add.folder_hint.clone()).width(Length::Fill),
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center)
        .into(),
    ];
    if let Some(error) = &add.folder_error {
        folder.push(notice(ctx, Tone::Danger, error.clone()));
    }

    let mut git: Vec<Element<'a, Message>> = vec![
        typography::body(ctx.theme, add.git_label.clone()).into(),
        row![
            crate::keymap::guard(
                ctx,
                input::text_field(ctx.theme, &st.page.git_url)
                    .placeholder(GIT_PLACEHOLDER)
                    .on_input(|url| msg(Msg::GitUrl(url)))
                    .on_submit(msg(Msg::AddGit)),
            ),
            button::secondary(ctx.theme, add.git_button.clone())
                .on_press_maybe(add.git_enabled.then(|| msg(Msg::AddGit))),
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center)
        .into(),
        typography::hint(ctx.theme, add.git_hint.clone())
            .width(Length::Fill)
            .into(),
    ];
    if let Some(error) = &add.git_error {
        git.push(notice(ctx, Tone::Danger, error.clone()));
    }

    let mut rows: Vec<Element<'a, Message>> = vec![
        container(iced::widget::Column::with_children(folder).spacing(tokens::space::MD))
            .padding([tokens::space::LG, tokens::space::XL])
            .into(),
        container(iced::widget::Column::with_children(git).spacing(tokens::space::MD))
            .padding([tokens::space::LG, tokens::space::XL])
            .into(),
    ];
    if let Some(flash) = &add.flash {
        rows.push(
            container(typography::hint(ctx.theme, flash.clone()))
                .padding([tokens::space::MD, tokens::space::XL])
                .into(),
        );
    }
    form::section(ctx.theme, add.title.clone())
        .group(rows)
        .into()
}

#[cfg(test)]
mod tests;
