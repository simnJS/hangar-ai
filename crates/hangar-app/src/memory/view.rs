//! The memory view (`MemoryView.tsx`): a bar with the count, the search and
//! the "New" button, then the facts one per row, what each says on the left
//! and where it came from on the right.

use std::time::{SystemTime, UNIX_EPOCH};

use iced::keyboard::{Key, key::Named};
use iced::widget::{
    button, column, container, row, rule, scrollable, space, text, text_editor::Content,
};
use iced::{Alignment, Element, Length, Padding};

use hangar_core::memory::MemoryEntry;
use hangar_i18n::{Key as K, args};
use hangar_theme::{Theme, fonts, style, tokens};
use hangar_ui::badge::Tone;
use hangar_ui::{Icon, button as btn, empty, icon, input, keys, text as typography, tip};

use super::format::{ShortDay, compact_path, full_date, short_day, signer_name, was_updated};
use super::guards::{
    MEMORY_CAP, MemorySort, find_title_clash, matches_haystack, near_cap, search_terms,
    sort_entries,
};
use super::{Draft, Msg, State, composer_id, search_id};
use crate::ctx::Ctx;
use crate::message::Message;

/// The width under which a row's origin folds under its text, and the hint
/// loses its words: the stylesheet's `@container memory (max-width: 760px)`.
const NARROW: f32 = 760.0;
/// The width of the column that says where a fact came from.
const ORIGIN_WIDTH: f32 = 200.0;
/// How long a path may be in that column.
const PATH_CHARS: usize = 30;
/// A collapsed entry shows two lines of its text (12 px on a 1.55 line).
const CLAMP_HEIGHT: f32 = 2.0 * 12.0 * 1.55 + 1.0;

fn msg(msg: Msg) -> Message {
    Message::Memory(msg)
}

/// A label without arguments, as the widgets that keep a `&str` for the whole
/// frame want it. The dictionary lends such a label as a `'static` borrow
/// (hangar-i18n, `t`), so nothing is allocated; the owned case, which only a
/// computed key naming nothing could reach, is leaked rather than shown wrong.
fn label(ctx: Ctx<'_>, key: K) -> &'static str {
    match ctx.t(key) {
        std::borrow::Cow::Borrowed(text) => text,
        std::borrow::Cow::Owned(text) => Box::leak(text.into_boxed_str()),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

pub(super) fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let loaded = st.entries.is_some();
    let all: &[MemoryEntry] = st.entries.as_deref().unwrap_or(&[]);
    let terms = search_terms(&st.query);
    let visible = sort_entries(
        all.iter()
            .zip(&st.haystacks)
            .filter(|(_, haystack)| matches_haystack(haystack, &terms))
            .map(|(entry, _)| entry),
        st.sort,
    );
    let narrow = ctx.metrics.body_width < NARROW;

    let mut page = column![bar(st, ctx, &visible, all.len(), narrow)];
    page = page.push(rule::horizontal(1).style(style::rule::sep(t)));

    if let Some(error) = &st.error {
        page = page.push(banner(
            t,
            Icon::Alert,
            text(error.as_str()).size(tokens::font_size::UI).into(),
        ));
    }
    if loaded && near_cap(all.len()) {
        page = page.push(cap_note(st, ctx, all.len()));
    }

    let mut list = column![].width(Length::Fill);
    if st.composing {
        list = list.push(composer(st, ctx, all));
    }
    if !loaded {
        list = list.push(skeleton(t));
    }
    // Never under an error: a memory that could not be read is not one that
    // has nothing in it, and saying "empty" would invite rewriting facts that
    // are still on disk.
    if loaded && all.is_empty() && st.error.is_none() && !st.composing {
        list = list.push(
            container(
                empty::empty(t, Icon::Bookmark, ctx.t(K::MemoryEmpty))
                    .description(ctx.t(K::MemoryEmptyBody))
                    .action(
                        btn::primary(t, ctx.t(K::MemoryEmptyAction))
                            .icon(Icon::Plus)
                            .on_press(msg(Msg::StartComposing)),
                    ),
            )
            .height(360.0),
        );
    }
    if !all.is_empty() && visible.is_empty() {
        list = list.push(no_results(st, ctx));
    }
    for entry in &visible {
        list = list.push(entry_row(st, ctx, entry, narrow));
    }

    let body = scrollable(container(list).padding(Padding {
        top: 6.0,
        right: tokens::space::XXL,
        bottom: 28.0,
        left: tokens::space::XXL,
    }))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(style::scrollable::default(t));

    container(page.push(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::window(t))
        .into()
}

/// The heading, the search, the hint that agents write here, and "New".
fn bar<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    visible: &[&MemoryEntry],
    total: usize,
    narrow: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let heading = if st.entries.is_none() {
        ctx.t(K::MemoryTitle)
    } else if !search_terms(&st.query).is_empty() {
        ctx.i18n.tn(K::MemoryResults, visible.len(), &[])
    } else {
        ctx.i18n.tn(K::MemoryEntries, total, &[])
    };

    let search = input::search_field(t, &st.query, label(ctx, K::MemorySearch))
        .on_input(|query| msg(Msg::Query(query)))
        .on_clear(msg(Msg::ClearQuery))
        .id(search_id())
        .width(Length::Fill);
    let search = crate::keymap::guard(ctx, search);
    // Escape empties the field first, the way a search field does.
    let escapes = !st.query.is_empty();
    let search = keys::on_key(search, move |key, _| match key {
        Key::Named(Named::Escape) if escapes => Some(msg(Msg::ClearQuery)),
        _ => None,
    });
    let search = container(search).width(320.0).max_width(320.0);

    // One line, cut where it runs out of room: the button after it keeps its
    // width, and the whole sentence is in the tip.
    let mut hint = row![icon(Icon::Plug, tokens::size::ICON).color(t.colors.fg_subtle)]
        .spacing(tokens::space::SM)
        .align_y(Alignment::Center);
    if !narrow {
        hint = hint.push(
            typography::subtle(t, ctx.t(K::MemoryHint))
                .size(tokens::font_size::CAPTION_LG)
                .wrapping(text::Wrapping::None),
        );
    }
    let hint = container(tip::bottom(t, hint, ctx.t(K::MemoryHint)))
        .width(Length::Fill)
        .align_x(Alignment::End)
        .clip(true);

    let new = btn::secondary(t, ctx.t(K::MemoryNew))
        .icon(Icon::Plus)
        .small()
        .on_press(msg(Msg::ToggleComposer));

    container(
        row![
            typography::body(t, heading).font(fonts::ui_semibold()),
            search,
            hint,
            new,
        ]
        .spacing(tokens::space::LG)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 0.0,
        right: tokens::space::XL,
        bottom: 0.0,
        left: tokens::space::XXL,
    })
    .height(tokens::size::TOOLBAR_HEIGHT)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .style(style::container::pane_head(t, false))
    .into()
}

/// A line that says something is wrong, over the list.
fn banner<'a>(t: &'a Theme, glyph: Icon, content: Element<'a, Message>) -> Element<'a, Message> {
    container(
        row![
            icon(glyph, tokens::size::ICON).color(t.colors.danger),
            container(content).width(Length::Fill),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([tokens::space::MD, tokens::space::XXL]))
    .width(Length::Fill)
    .style(style::container::badge(t, Tone::Danger))
    .into()
}

/// "The memory holds 912 of 1000", with the toggle that sorts the oldest
/// first, to find what to delete.
fn cap_note<'a>(st: &'a State, ctx: Ctx<'a>, count: usize) -> Element<'a, Message> {
    let t = ctx.theme;
    let words = ctx
        .i18n
        .t_with(K::MemoryNearCap, &args!("n" => count, "max" => MEMORY_CAP));
    let toggle = btn::ghost(
        t,
        match st.sort {
            MemorySort::Oldest => ctx.t(K::MemorySortRecent),
            MemorySort::Recent => ctx.t(K::MemorySortOldest),
        },
    )
    .small()
    .on_press(msg(Msg::FlipSort));
    container(
        row![
            icon(Icon::Alert, tokens::size::ICON).color(t.colors.warning),
            typography::subtle(t, words)
                .size(tokens::font_size::UI)
                .width(Length::Fill),
            toggle,
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([tokens::space::SM, tokens::space::XXL]))
    .width(Length::Fill)
    .into()
}

/// A multi-line field of `rows` lines over a text editor's state.
fn editor<'a>(
    ctx: Ctx<'a>,
    content: &'a Content,
    rows: u16,
    placeholder: &'static str,
    on_action: fn(iced::widget::text_editor::Action) -> Msg,
) -> Element<'a, Message> {
    let field = iced::widget::text_editor(content)
        .placeholder(placeholder)
        .on_action(move |action| msg(on_action(action)))
        .size(tokens::font_size::BODY)
        .padding(Padding::from([tokens::space::MD, tokens::space::LG]))
        .height(f32::from(rows) * tokens::font_size::BODY * tokens::line_height::BODY + 20.0)
        .style(style::text_editor::field(ctx.theme));
    crate::keymap::guard(ctx, field)
}

/// The new-fact form: a title, the text, the tags, and what creating means.
fn composer<'a>(st: &'a State, ctx: Ctx<'a>, all: &'a [MemoryEntry]) -> Element<'a, Message> {
    let t = ctx.theme;
    let draft: &Draft = &st.draft;
    let clash = find_title_clash(all, &draft.title);
    let replacing = st.confirming_replace && clash.is_some();

    let mut form = column![
        crate::keymap::guard(
            ctx,
            input::text_field(t, &draft.title)
                .placeholder(label(ctx, K::MemoryTitlePlaceholder))
                .on_input(|title| msg(Msg::DraftTitle(title)))
                .on_submit(msg(Msg::Create))
                .id(composer_id()),
        ),
        editor(
            ctx,
            &draft.content,
            5,
            label(ctx, K::MemoryContentPlaceholder),
            Msg::DraftContent
        ),
        crate::keymap::guard(
            ctx,
            input::text_field(t, &draft.tags)
                .placeholder(label(ctx, K::MemoryTagsPlaceholder))
                .on_input(|tags| msg(Msg::DraftTags(tags)))
                .mono(),
        ),
    ]
    .spacing(tokens::space::MD);

    if let (true, Some(clash)) = (replacing, clash) {
        form = form.push(banner(
            t,
            Icon::Alert,
            text(
                ctx.i18n
                    .t_with(K::MemoryReplaceAsk, &args!("title" => clash.title.as_str())),
            )
            .size(tokens::font_size::UI)
            .into(),
        ));
    }

    let create = btn::primary(
        t,
        if replacing {
            ctx.t(K::MemoryReplaceConfirm)
        } else {
            ctx.t(K::MemoryCreate)
        },
    )
    .on_press_maybe(draft.is_complete().then(|| msg(Msg::Create)));
    form = form.push(
        row![
            typography::subtle(t, ctx.t(K::MemoryUpsertHint)).width(Length::Fill),
            btn::ghost(t, ctx.t(K::CreateCancel)).on_press(msg(Msg::CancelCompose)),
            create,
        ]
        .spacing(tokens::space::SM)
        .align_y(Alignment::Center),
    );

    container(
        container(form)
            .padding(tokens::space::XL)
            .style(style::container::card(t)),
    )
    .padding(Padding {
        top: tokens::space::LG,
        bottom: tokens::space::SM,
        ..Padding::ZERO
    })
    .into()
}

/// Four ghost rows while the first read is out.
fn skeleton<'a>(t: &'a Theme) -> Element<'a, Message> {
    let bar = |width: f32, height: f32| {
        container(space::horizontal())
            .width(width)
            .height(height)
            .style(style::container::chip(t))
    };
    let mut rows = column![];
    for _ in 0..4 {
        rows = rows.push(
            container(
                row![
                    column![bar(180.0, 10.0), bar(320.0, 8.0), bar(220.0, 8.0)]
                        .spacing(tokens::space::MD)
                        .width(Length::Fill),
                    bar(110.0, 8.0),
                ]
                .spacing(24),
            )
            .padding(Padding::from([tokens::space::XL, tokens::space::XS])),
        );
    }
    rows.into()
}

/// A search that found nothing, and the way out of it.
fn no_results<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    container(
        column![
            typography::muted(
                t,
                ctx.i18n
                    .t_with(K::MemoryNoResults, &args!("q" => st.query.trim()))
            ),
            btn::ghost(t, ctx.t(K::MemoryClearSearch)).on_press(msg(Msg::ClearQuery)),
        ]
        .spacing(tokens::space::MD)
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .padding(Padding::from([48.0, tokens::space::XL]))
    .into()
}

/// One fact: what it says, its tags and actions on the left, who wrote it,
/// when and from where on the right.
fn entry_row<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    entry: &'a MemoryEntry,
    narrow: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let open = st.open.contains(&entry.id);
    let editing = st.editing.as_ref().filter(|editing| editing.id == entry.id);

    let main: Element<'a, Message> = match editing {
        Some(editing) => editing_form(ctx, &editing.draft),
        None => reading(st, ctx, entry, open),
    };
    let origin = origin(ctx, entry, narrow);

    let line: Element<'a, Message> = if narrow {
        column![main, origin].spacing(tokens::space::SM).into()
    } else {
        row![
            container(main).width(Length::Fill),
            container(origin).width(ORIGIN_WIDTH),
        ]
        .spacing(24)
        .into()
    };
    column![
        container(line)
            .padding(Padding::from([tokens::space::XL, tokens::space::XS]))
            .width(Length::Fill),
        rule::horizontal(1).style(style::rule::hairline(t)),
    ]
    .into()
}

/// An entry as it is read: the title that opens it, its text, its tags and,
/// open, the actions.
fn reading<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    entry: &'a MemoryEntry,
    open: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let toggle = tip::over(
        t,
        button(
            row![
                icon(
                    if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    10.0
                )
                .color(t.colors.fg_subtle),
                text(entry.title.as_str())
                    .size(tokens::font_size::UI_LG)
                    .font(fonts::ui_semibold())
                    .style(style::text::primary(t)),
            ]
            .spacing(tokens::space::SM)
            .align_y(Alignment::Center),
        )
        .padding(0)
        .style(style::button::plain(t))
        .on_press(msg(Msg::Toggle(entry.id.clone()))),
        if open {
            ctx.t(K::MemoryCollapse)
        } else {
            ctx.t(K::MemoryExpand)
        },
    );

    let body = typography::muted(t, entry.content.as_str()).line_height(tokens::line_height::BODY);
    let body: Element<'a, Message> = if open {
        body.into()
    } else {
        // Collapsed entries keep the list scannable; the whole text is one
        // click away. iced has no line clamp, so the text is cut at two lines.
        container(body).max_height(CLAMP_HEIGHT).clip(true).into()
    };

    let mut main = column![toggle, body].spacing(5);
    if !entry.tags.is_empty() {
        let chips = entry.tags.iter().map(|tag| {
            container(
                text(tag.as_str())
                    .size(tokens::font_size::LABEL)
                    .font(fonts::mono()),
            )
            .padding(Padding::from([1.0, tokens::space::SM]))
            .style(style::container::chip(t))
            .into()
        });
        main = main.push(row(chips).spacing(tokens::space::XS).wrap());
    }
    if open {
        main = main.push(actions(st, ctx, entry));
    }
    main.into()
}

/// The row under an open entry: when it was written, edit, delete (twice).
fn actions<'a>(st: &'a State, ctx: Ctx<'a>, entry: &'a MemoryEntry) -> Element<'a, Message> {
    let t = ctx.theme;
    let locale = ctx.i18n.locale();
    let created = ctx.i18n.t_with(
        K::MemoryCreatedAt,
        &args!("date" => full_date(entry.created_at, locale)),
    );
    let mut line = row![
        typography::subtle(t, created).width(Length::Fill),
        btn::ghost(t, ctx.t(K::MemoryEdit))
            .icon(Icon::Pencil)
            .small()
            .on_press(msg(Msg::StartEdit(entry.id.clone()))),
    ]
    .spacing(tokens::space::SM)
    .align_y(Alignment::Center);
    if st.armed.as_deref() == Some(entry.id.as_str()) {
        line = line
            .push(
                btn::ghost(t, ctx.t(K::CreateCancel))
                    .small()
                    .on_press(msg(Msg::Disarm)),
            )
            .push(
                btn::danger(t, ctx.t(K::MemoryDeleteConfirm))
                    .small()
                    .on_press(msg(Msg::Delete(entry.id.clone()))),
            );
    } else {
        line = line.push(
            btn::ghost(t, ctx.t(K::MemoryDelete))
                .icon(Icon::Trash)
                .small()
                .on_press(msg(Msg::Arm(entry.id.clone()))),
        );
    }
    container(line)
        .max_width(620.0)
        .padding(Padding {
            top: 6.0,
            ..Padding::ZERO
        })
        .into()
}

/// An entry being edited: its three fields, save and cancel.
fn editing_form<'a>(ctx: Ctx<'a>, draft: &'a Draft) -> Element<'a, Message> {
    let t = ctx.theme;
    let save = btn::primary(t, ctx.t(K::MemorySave))
        .on_press_maybe(draft.is_complete().then(|| msg(Msg::SaveEdit)));
    column![
        crate::keymap::guard(
            ctx,
            input::text_field(t, &draft.title)
                .placeholder(label(ctx, K::MemoryTitlePlaceholder))
                .on_input(|title| msg(Msg::EditTitle(title))),
        ),
        editor(
            ctx,
            &draft.content,
            8,
            label(ctx, K::MemoryContentPlaceholder),
            Msg::EditContent
        ),
        crate::keymap::guard(
            ctx,
            input::text_field(t, &draft.tags)
                .placeholder(label(ctx, K::MemoryTagsPlaceholder))
                .on_input(|tags| msg(Msg::EditTags(tags)))
                .mono(),
        ),
        row![
            typography::subtle(t, ctx.t(K::MemoryTagsHint)).width(Length::Fill),
            btn::ghost(t, ctx.t(K::CreateCancel)).on_press(msg(Msg::CancelEdit)),
            save,
        ]
        .spacing(tokens::space::SM)
        .align_y(Alignment::Center),
    ]
    .spacing(tokens::space::MD)
    .into()
}

/// "Written by Ava", "Updated Oct 21", the folder: the right column.
fn origin<'a>(ctx: Ctx<'a>, entry: &'a MemoryEntry, narrow: bool) -> Element<'a, Message> {
    let t = ctx.theme;
    let locale = ctx.i18n.locale();
    let updated = was_updated(entry.created_at, entry.updated_at);
    let at = if updated {
        entry.updated_at
    } else {
        entry.created_at
    };
    let dated = match short_day(at, now_ms(), locale) {
        ShortDay::Today => ctx.t(if updated {
            K::MemoryUpdatedToday
        } else {
            K::MemoryCreatedToday
        }),
        ShortDay::On(day) => ctx.i18n.t_with(
            if updated {
                K::MemoryUpdatedAt
            } else {
                K::MemoryCreatedAt
            },
            &args!("date" => day),
        ),
    };
    let full = ctx.i18n.t_with(
        if updated {
            K::MemoryUpdatedAt
        } else {
            K::MemoryCreatedAt
        },
        &args!("date" => full_date(at, locale)),
    );

    let signer: Element<'a, Message> = match signer_name(&entry.agent) {
        None => typography::subtle(t, ctx.t(K::MemoryByYou)).into(),
        Some(name) => row![
            typography::subtle(t, ctx.t(K::MemoryWrittenBy)),
            typography::subtle(t, name)
                .font(fonts::ui_medium())
                .style(style::text::muted(t)),
        ]
        .spacing(tokens::space::XS)
        .into(),
    };
    let signer = tip::over(
        t,
        signer,
        ctx.i18n
            .t_with(K::MemoryByAgent, &args!("agent" => entry.agent.as_str())),
    );
    let date = tip::over(t, typography::subtle(t, dated), full);

    let mut lines = column![signer, date].spacing(1);
    if !entry.workspace.is_empty() {
        lines = lines.push(tip::over(
            t,
            typography::subtle(t, compact_path(&entry.workspace, PATH_CHARS))
                .font(fonts::mono())
                .size(tokens::font_size::LABEL),
            ctx.i18n.t_with(
                K::MemoryFromWorkspace,
                &args!("path" => entry.workspace.as_str()),
            ),
        ));
    }
    lines
        .align_x(if narrow {
            Alignment::Start
        } else {
            Alignment::End
        })
        .width(Length::Fill)
        .into()
}
