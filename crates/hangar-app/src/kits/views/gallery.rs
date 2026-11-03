//! Settings › Kits › Library: `KitsGalleryView` of `settings/KitsGallery.tsx`.
//!
//! The public kits of the `hangar-ai` organization, from a static file
//! hangar-core reads. Installing only brings the kit onto the machine; enabling
//! it stays in the activation, where its permissions are shown, verified kit or
//! not, and the page says so.

use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length};

use hangar_core::kits::KitInfo;
use hangar_i18n::{Key, Locale, Translator, args};
use hangar_theme::{fonts, tokens};
use hangar_ui::badge::Tone;
use hangar_ui::{button, form, input, text as typography};

use super::activation_model::{Loadable, Tag, TagTone};
use super::{msg, note, notice, tag};
use crate::ctx::Ctx;
use crate::kits::gallery::{
    Gallery, GalleryKit, GalleryStatus, Origin, filter_gallery, gallery_status,
};
use crate::kits::{GalleryFailure, Msg};
use crate::message::Message;

/// What a card offers to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardAction {
    None,
    Install {
        label: String,
        enabled: bool,
    },
    /// Moves the installed kit to the version the catalog now names.
    Update {
        label: String,
        enabled: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// `repo · tag`.
    pub source: String,
    pub action: CardAction,
    pub verified: Tag,
    /// Said beside « Community kit ».
    pub community_note: Option<String>,
    pub tags: Vec<String>,
    pub asks: Option<String>,
    /// Where the kit stands on this machine.
    pub status: Vec<Tag>,
}

/// A line the library says about itself, above its kits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The copy kept, because the catalog could not be read.
    Cached(String),
    Unavailable(String),
    InstallFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryModel {
    pub title: String,
    pub hint: String,
    /// Borrowed from the dictionary: the search field keeps it for the frame.
    pub filter_placeholder: &'static str,
    pub filter_label: String,
    pub refresh: String,
    pub refresh_enabled: bool,
    pub notices: Vec<Notice>,
    pub body: Loadable<Vec<Card>>,
}

pub struct GalleryInput<'a> {
    /// `None` until hangar-core has answered.
    pub gallery: Option<&'a Gallery>,
    /// The list could not be asked for at all.
    pub error: Option<&'a str>,
    pub refreshing: bool,
    /// The kits on this machine; `None` until they are read.
    pub installed: Option<&'a [KitInfo]>,
    /// The library kit being installed, or updated.
    pub busy: Option<&'a str>,
    /// The last install that failed.
    pub failure: Option<&'a GalleryFailure>,
    pub query: &'a str,
}

/// A label of the dictionary as the dictionary holds it: a string without
/// placeholders is never copied, so this is always the string itself. (Were it
/// ever not, the field would show no placeholder, which loses a hint and
/// nothing else.)
pub fn borrowed(label: std::borrow::Cow<'static, str>) -> &'static str {
    match label {
        std::borrow::Cow::Borrowed(label) => label,
        std::borrow::Cow::Owned(_) => "",
    }
}

/// `fetchedAt` as a date, the way the language writes one. In UTC: the date is
/// said to the day, and the zone of the machine is not known to the app.
pub fn format_date(ms: u64, locale: Locale) -> String {
    let days = (ms / 86_400_000) as i64;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    match locale {
        Locale::En => format!("{month}/{day}/{year}"),
        Locale::Fr => format!("{day:02}/{month:02}/{year}"),
    }
}

impl GalleryModel {
    pub fn build(t: Translator, input: &GalleryInput<'_>) -> GalleryModel {
        let gallery = input.gallery;
        let shown: Vec<&GalleryKit> = gallery
            .map(|found| filter_gallery(&found.kits, input.query))
            .unwrap_or_default();

        let mut notices = Vec::new();
        if let Some(found) = gallery.filter(|found| found.origin == Origin::Cached) {
            let date = found
                .fetched_at
                .map(|ms| format_date(ms, t.locale()))
                .unwrap_or_default();
            notices.push(Notice::Cached(
                t.t_with(
                    Key::KitsManageGalleryCached,
                    &args!("date" => date.as_str()),
                )
                .into_owned(),
            ));
        }
        if gallery.is_some_and(|found| found.origin == Origin::Unavailable) || input.error.is_some()
        {
            notices.push(Notice::Unavailable(
                t.t(Key::KitsManageGalleryUnavailable).into_owned(),
            ));
        }
        if let Some(failure) = input.failure {
            notices.push(Notice::InstallFailed(
                t.t_with(
                    Key::KitsManageGalleryInstallFailed,
                    &args!("name" => failure.name.as_str(), "detail" => failure.detail.as_str()),
                )
                .into_owned(),
            ));
        }

        let body = match gallery {
            None if input.error.is_none() => {
                Loadable::Loading(t.t(Key::KitsManageGalleryLoading).into_owned())
            }
            Some(found) if found.origin != Origin::Unavailable && found.kits.is_empty() => {
                Loadable::Empty(t.t(Key::KitsManageGalleryEmpty).into_owned())
            }
            Some(found) if !found.kits.is_empty() && shown.is_empty() => {
                Loadable::Empty(t.t(Key::KitsManageGalleryNoMatch).into_owned())
            }
            _ => Loadable::Ready(
                shown
                    .iter()
                    .map(|entry| Self::card(t, entry, input))
                    .collect(),
            ),
        };

        GalleryModel {
            title: t.t(Key::KitsManageGalleryTitle).into_owned(),
            hint: t.t(Key::KitsManageGalleryHint).into_owned(),
            filter_placeholder: borrowed(t.t(Key::KitsManageGalleryFilterPlaceholder)),
            filter_label: t.t(Key::KitsManageGalleryFilter).into_owned(),
            refresh: t
                .t(if input.refreshing {
                    Key::KitsManageGalleryRefreshing
                } else {
                    Key::KitsManageGalleryRefresh
                })
                .into_owned(),
            refresh_enabled: !input.refreshing,
            notices,
            body,
        }
    }

    fn card(t: Translator, entry: &GalleryKit, input: &GalleryInput<'_>) -> Card {
        let installed = input.installed.unwrap_or_default();
        let status = gallery_status(entry, installed);
        let working = input.busy == Some(entry.id.as_str());
        let version = |version: &str| format!("v{version}");
        let pinned = |tag: &str| Tag {
            label: t
                .t_with(Key::KitsManageGalleryPinned, &args!("tag" => tag))
                .into_owned(),
            tone: TagTone::Plain,
        };
        let (action, status_tags) = match &status {
            GalleryStatus::New => (
                CardAction::Install {
                    label: t
                        .t(if working {
                            Key::KitsManageGalleryInstalling
                        } else {
                            Key::KitsManageGalleryInstall
                        })
                        .into_owned(),
                    enabled: !working && input.busy.is_none() && input.installed.is_some(),
                },
                Vec::new(),
            ),
            GalleryStatus::Update {
                version: have,
                latest,
                pinned: tag,
            } => (
                CardAction::Update {
                    label: if working {
                        t.t(Key::KitsManageUpdating).into_owned()
                    } else {
                        t.t_with(
                            Key::KitsManageGalleryUpdate,
                            &args!("latest" => latest.as_str()),
                        )
                        .into_owned()
                    },
                    enabled: input.busy.is_none(),
                },
                vec![
                    Tag {
                        label: t
                            .t_with(
                                Key::KitsManageGalleryUpdateAvailable,
                                &args!("version" => have.as_str(), "latest" => latest.as_str()),
                            )
                            .into_owned(),
                        tone: TagTone::New,
                    },
                    pinned(tag),
                ],
            ),
            GalleryStatus::Installed {
                version: have,
                pinned: tag,
            } => {
                let mut tags = vec![Tag {
                    label: t
                        .t_with(
                            Key::KitsManageGalleryInstalled,
                            &args!("version" => have.as_str()),
                        )
                        .into_owned(),
                    tone: TagTone::Ok,
                }];
                tags.extend(tag.as_deref().map(pinned));
                (CardAction::None, tags)
            }
            GalleryStatus::Linked { version: have } => (
                CardAction::None,
                vec![Tag {
                    label: format!("{} ({})", t.t(Key::KitsManageLinked), version(have)),
                    tone: TagTone::Plain,
                }],
            ),
        };
        Card {
            id: entry.id.clone(),
            name: entry.name.clone(),
            version: t
                .t_with(
                    Key::KitsManageVersion,
                    &args!("version" => entry.version.as_str()),
                )
                .into_owned(),
            description: entry.description.clone(),
            source: format!("{} · {}", entry.repo, entry.tag),
            action,
            verified: if entry.verified {
                Tag {
                    label: t.t(Key::KitsManageGalleryVerified).into_owned(),
                    tone: TagTone::Ok,
                }
            } else {
                Tag {
                    label: t.t(Key::KitsManageGalleryCommunity).into_owned(),
                    tone: TagTone::Plain,
                }
            },
            community_note: (!entry.verified)
                .then(|| t.t(Key::KitsManageGalleryCommunityNote).into_owned()),
            tags: entry.tags.clone(),
            asks: (!entry.permissions_summary.is_empty()).then(|| {
                t.t_with(
                    Key::KitsManageGalleryAsks,
                    &args!("summary" => entry.permissions_summary.as_str()),
                )
                .into_owned()
            }),
            status: status_tags,
        }
    }

    /// Every sentence the library shows, one per line.
    pub fn text(&self) -> String {
        let mut out = vec![
            self.title.clone(),
            self.hint.clone(),
            self.filter_placeholder.to_owned(),
            self.refresh.clone(),
        ];
        for found in &self.notices {
            out.push(match found {
                Notice::Cached(text) | Notice::Unavailable(text) | Notice::InstallFailed(text) => {
                    text.clone()
                }
            });
        }
        match &self.body {
            Loadable::Loading(text) | Loadable::Empty(text) => out.push(text.clone()),
            Loadable::Ready(cards) => {
                for card in cards {
                    out.extend([
                        card.name.clone(),
                        card.version.clone(),
                        card.description.clone(),
                        card.source.clone(),
                        card.verified.label.clone(),
                    ]);
                    out.extend(card.community_note.clone());
                    out.extend(card.tags.clone());
                    out.extend(card.asks.clone());
                    out.extend(card.status.iter().map(|tag| tag.label.clone()));
                    match &card.action {
                        CardAction::None => {}
                        CardAction::Install { label, .. } | CardAction::Update { label, .. } => {
                            out.push(label.clone())
                        }
                    }
                }
            }
        }
        out.join("\n")
    }
}

/// The library, drawn.
pub fn view<'a>(
    ctx: Ctx<'a>,
    page: &'a crate::kits::Page,
    installed: Option<&'a [KitInfo]>,
) -> Element<'a, Message> {
    let input = GalleryInput {
        gallery: page.gallery.as_ref(),
        error: page.gallery_error.as_deref(),
        refreshing: page.gallery_refreshing,
        installed,
        busy: page.gallery_busy.as_deref(),
        failure: page.gallery_failure.as_ref(),
        query: &page.gallery_query,
    };
    let model = GalleryModel::build(ctx.i18n, &input);

    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    rows.push(
        container(
            row![
                input::search_field(ctx.theme, &page.gallery_query, model.filter_placeholder)
                    .on_input(|query| msg(Msg::GalleryQuery(query)))
                    .on_clear(msg(Msg::GalleryQuery(String::new())))
                    .clear_tip(ctx.t(Key::KitsManageGalleryFilter)),
                button::secondary(ctx.theme, model.refresh.clone())
                    .on_press_maybe(model.refresh_enabled.then(|| msg(Msg::GalleryRefresh))),
            ]
            .spacing(tokens::space::LG)
            .align_y(Alignment::Center),
        )
        .padding([tokens::space::MD, tokens::space::XL])
        .into(),
    );
    for found in &model.notices {
        let element = match found {
            Notice::Cached(words) => note(ctx, words.clone()),
            Notice::Unavailable(words) => notice(ctx, Tone::Neutral, words.clone()),
            Notice::InstallFailed(words) => notice(ctx, Tone::Danger, words.clone()),
        };
        rows.push(
            container(element)
                .padding([tokens::space::MD, tokens::space::XL])
                .into(),
        );
    }
    match &model.body {
        Loadable::Loading(words) | Loadable::Empty(words) => rows.push(
            container(typography::hint(ctx.theme, words.clone()))
                .padding([tokens::space::LG, tokens::space::XL])
                .into(),
        ),
        Loadable::Ready(cards) => rows.extend(cards.iter().map(|card| card_view(ctx, card))),
    }

    form::section(ctx.theme, model.title.clone())
        .hint(model.hint.clone())
        .group(rows)
        .into()
}

fn card_view<'a>(ctx: Ctx<'a>, card: &Card) -> Element<'a, Message> {
    let action: Element<'a, Message> = match &card.action {
        CardAction::None => iced::widget::Space::new().into(),
        CardAction::Install { label, enabled } => button::primary(ctx.theme, label.clone())
            .on_press_maybe(enabled.then(|| msg(Msg::GalleryInstall(card.id.clone()))))
            .into(),
        CardAction::Update { label, enabled } => button::primary(ctx.theme, label.clone())
            .on_press_maybe(enabled.then(|| msg(Msg::GalleryUpdate(card.id.clone()))))
            .into(),
    };
    let mut parts: Vec<Element<'a, Message>> = vec![
        row![
            column![
                row![
                    typography::body(ctx.theme, card.name.clone()).font(fonts::ui_medium()),
                    typography::mono(ctx.theme, card.version.clone()),
                ]
                .spacing(tokens::space::MD)
                .align_y(Alignment::Center),
                typography::hint(ctx.theme, card.description.clone()),
                typography::subtle(ctx.theme, card.source.clone()).font(fonts::mono()),
            ]
            .spacing(tokens::space::XXS)
            .width(Length::Fill),
            action,
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Start)
        .into(),
    ];
    let mut trust = row![tag(ctx, &card.verified)]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center);
    if let Some(words) = &card.community_note {
        trust = trust.push(typography::hint(ctx.theme, words.clone()));
    }
    parts.push(trust.wrap().into());
    if !card.tags.is_empty() {
        parts.push(
            iced::widget::Row::with_children(
                card.tags
                    .iter()
                    .map(|word| hangar_ui::badge::tag(ctx.theme, word.clone())),
            )
            .spacing(tokens::space::SM)
            .wrap()
            .into(),
        );
    }
    if let Some(words) = &card.asks {
        parts.push(note(ctx, words.clone()));
    }
    if !card.status.is_empty() {
        parts.push(
            iced::widget::Row::with_children(card.status.iter().map(|one| tag(ctx, one)))
                .spacing(tokens::space::SM)
                .wrap()
                .into(),
        );
    }
    container(iced::widget::Column::with_children(parts).spacing(tokens::space::MD))
        .padding([tokens::space::LG, tokens::space::XL])
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests;
