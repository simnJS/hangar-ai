//! P13: Settings.
//!
//! The settings sheet (`SettingsPage.tsx`) with every row of the old schema
//! (`schema.tsx`), the shortcut editor, the Mobile, Voice and Discord cards,
//! the Experimental page, and the Discord presence the app publishes
//! (`lib/{experiments,discord,remote}.ts`). Inventory §2.3/2–3 is the map.
//!
//! # How it is put together
//!
//! - `schema` describes the settings as data, rebuilt for every frame from the
//!   persisted settings; `search` filters it; `rows` draws a category from
//!   hangar-ui's form widgets; `change` says what a row edits and turns an
//!   edit into a store action. Every write goes through `Model::apply`.
//! - Blocks that are more than a row have a module each: `previews` (theme
//!   cards, fonts, the terminal, the projects folder), `shortcuts` (the
//!   editor), `mobile` + `mobile_page` + `mobile_model`, `voice_card`,
//!   `discord_card`. Kits and Accounts are P14's and P8's pages, embedded.
//! - The cards load when their category is shown (`sync`), not at start, and
//!   forget what they loaded when it is left.
//!
//! # What the sheet asks of the shell
//!
//! The sheet steers its own Escape (first the search, then the sheet, and
//! never while a shortcut is recording), so its layer does not close on
//! Escape. The shell's own Escape rule (`App::key`, a TODO of P0b) must leave
//! the top overlay alone when it is the settings and the key was spent, or the
//! sheet closes with the search still full.

pub mod discord;
pub mod experiments;

mod change;
mod discord_card;
mod format;
mod mobile;
mod mobile_model;
mod mobile_page;
mod previews;
mod rows;
mod schema;
mod search;
mod shortcuts;
mod voice_card;

use std::path::PathBuf;
use std::time::Duration;

use hangar_i18n::{Key, Translator, args};
use hangar_ui::menu_nav::{Step, arrow_step};
use hangar_ui::{Icon, button, icon, input, keys, layer, text as typography};
use iced::keyboard::{Key as KeyboardKey, key::Named};
use iced::widget::{self, button as plain_button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding, Subscription, Task};

use hangar_theme::style;
use hangar_theme::tokens::{font_size, space};

use self::change::{Change, Scope, Target};
use self::discord::{Publisher, now_ms};
use self::schema::{Act, Category, Context};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services, dialogs};
use crate::shell::{Overlay, Region, SettingsCategory};
use crate::state::Model;

/// The package's UI state; lives in `App.pkg.settings`.
///
/// `discord.rs` and `experiments.rs` of §8.1 live in this directory
/// (`settings::discord`, `settings::experiments`), since the crate root is P0's.
#[derive(Debug)]
pub struct State {
    /// The search box.
    query: String,
    /// The two-step action whose second click is awaited, by row id.
    armed: Option<String>,
    /// The category list was the last thing used, so Up and Down walk it.
    nav_active: bool,
    /// The scrolling page, to take back to its top when the category changes.
    scroll: widget::Id,
    /// A settings sheet is on the overlay stack.
    open: bool,
    /// The category the person picked (or a shortcut asked for). The search
    /// can put another on screen meanwhile, and clearing it comes back to
    /// this one.
    chosen: SettingsCategory,
    /// The category on screen, whose cards were loaded: the chosen one, or
    /// the first the search left. The overlay stack names it, for whoever
    /// watches the stack (the kit library is fetched while the stack holds
    /// `Overlay::Settings(SettingsCategory::Kits)`).
    shown: Option<SettingsCategory>,
    shortcuts: shortcuts::State,
    mobile: mobile::State,
    voice: voice_card::State,
    discord: discord_card::State,
    /// What core was last told of the Discord presence.
    publisher: Publisher,
    /// The app's launch, which the presence counts from. Unix milliseconds.
    launched_at: i64,
    /// A debug build was asked to open the sheet at start (`HANGAR_DEV_SETTINGS`).
    dev_opened: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            query: String::new(),
            armed: None,
            nav_active: false,
            scroll: widget::Id::unique(),
            open: false,
            chosen: SettingsCategory::default(),
            shown: None,
            shortcuts: shortcuts::State::default(),
            mobile: mobile::State::default(),
            voice: voice_card::State::default(),
            discord: discord_card::State::default(),
            publisher: Publisher::default(),
            launched_at: now_ms(),
            dev_opened: false,
        }
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.settings`. Nothing of the settings is read by others: they read
/// the persisted settings themselves.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Settings(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    Query(String),
    ClearQuery,
    /// Escape: the search first, then the sheet.
    Escape,
    /// A category of the list.
    Pick(SettingsCategory),
    /// Up or Down in the list.
    Step(Step),
    Change(Change),
    /// A button that does something other than set a field.
    Act(Act),
    Arm(String),
    Disarm,
    BrowseProjects,
    ProjectsPicked(Option<PathBuf>),
    Shortcuts(shortcuts::Msg),
    Mobile(mobile::Msg),
    Voice(voice_card::Msg),
    Discord(discord_card::Msg),
    /// Time to compare the Discord presence with what core has.
    Publish,
    /// A debug build opens the sheet on a category by itself.
    DevOpen(SettingsCategory),
}

/// The version of the app (`getVersion()`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often the presence is compared with what core has. An update every few
/// seconds is all Discord accepts, so there is no use in looking more often.
const PUBLISH_EVERY: Duration = Duration::from_secs(2);

// ---- The sheet's place on the overlay stack ----------------------------------------------------------

/// The category of the topmost settings sheet, if one is open.
fn open_category(m: &Model) -> Option<SettingsCategory> {
    m.ui.overlays
        .iter()
        .rev()
        .find_map(|overlay| match overlay {
            Overlay::Settings(category) => Some(*category),
            _ => None,
        })
}

/// Shows another category in the sheet that is open: the overlay carries it.
fn go_to(m: &mut Model, category: SettingsCategory) {
    if let Some(Overlay::Settings(shown)) =
        m.ui.overlays
            .iter_mut()
            .rev()
            .find(|overlay| matches!(overlay, Overlay::Settings(_)))
    {
        *shown = category;
    }
}

/// Asking for a category while the sheet is already up moves to it, and does
/// not stack a second sheet on the first.
fn collapse_sheets(m: &mut Model) {
    let sheets =
        m.ui.overlays
            .iter()
            .filter(|overlay| matches!(overlay, Overlay::Settings(_)))
            .count();
    for _ in 1..sheets {
        if let Some(at) =
            m.ui.overlays
                .iter()
                .position(|overlay| matches!(overlay, Overlay::Settings(_)))
        {
            m.ui.overlays.remove(at);
        }
    }
}

// ---- The schema, from the model ----------------------------------------------------------------------

fn categories(m: &Model) -> Vec<Category> {
    build_for(m, Translator::new(m.locale()))
}

fn build_for(m: &Model, t: Translator) -> Vec<Category> {
    schema::build_categories(&Context {
        t,
        settings: m.settings(),
        shells: &m.ui.shells,
        workspace: m.active_workspace(),
        version: VERSION,
        experiments: experiments::EXPERIMENTS,
        platform: experiments::Platform::current(),
        keymap: &m.live.keymap.index,
    })
}

/// Puts what is on screen in the overlay stack, so that the stack always
/// names the category being drawn and not the one the search took the page
/// away from. Answers it; `None` when the search left nothing, and the stack
/// stays as it was.
fn follow_search(st: &State, m: &mut Model) -> Option<SettingsCategory> {
    let shown = shown_category(st, m, st.chosen)?;
    go_to(m, shown);
    Some(shown)
}

/// The category the sheet shows once the search has had its say.
fn shown_category(st: &State, m: &Model, wanted: SettingsCategory) -> Option<SettingsCategory> {
    if st.query.trim().is_empty() {
        return Some(wanted);
    }
    let visible = search::filter(categories(m), &st.query);
    search::active(&visible, wanted).map(|category| category.id)
}

// ---- Writing -----------------------------------------------------------------------------------------

fn apply_change(m: &mut Model, change: Change) -> Task<Message> {
    let action = {
        let scope = Scope {
            workspace: m.active_workspace(),
            theme_id: &m.settings().theme_id,
        };
        change::action(scope, &change)
    };
    if let Some(action) = action {
        m.apply(action);
    }
    Task::none()
}

// ---- Loading what a category shows -------------------------------------------------------------------

/// Brings the page in step with the model after anything that may have moved
/// it: the sheet opened or closed, another category shown, the search
/// changed. A card loads when its category appears and is forgotten when it
/// goes.
fn sync(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    let Some(current) = open_category(m) else {
        if !st.open {
            return Task::none();
        }
        // Closed: whatever it held goes with it, so the next sheet is a fresh
        // one, as the TS page was when it mounted.
        let leaving = leave(st);
        let kept = (
            st.launched_at,
            std::mem::take(&mut st.publisher),
            st.dev_opened,
        );
        *st = State {
            launched_at: kept.0,
            publisher: kept.1,
            dev_opened: kept.2,
            ..State::default()
        };
        return leaving;
    };
    if !st.open {
        st.open = true;
        st.chosen = current;
    }
    collapse_sheets(m);

    let shown = follow_search(st, m);
    if st.shown == shown {
        return Task::none();
    }
    let mut tasks = vec![leave(st)];
    st.shown = shown;
    tasks.push(enter(st, m, sv));
    // A new category starts at its top, not wherever the last one was scrolled.
    tasks.push(widget::operation::snap_to(
        st.scroll.clone(),
        widget::scrollable::RelativeOffset::START,
    ));
    Task::batch(tasks)
}

/// The category that was shown is no longer: its cards forget what they
/// loaded, and a recording in the shortcut editor ends.
fn leave(st: &mut State) -> Task<Message> {
    match st.shown.take() {
        Some(SettingsCategory::Shortcuts) => st.shortcuts.leave(),
        Some(SettingsCategory::Mobile) => {
            st.mobile = mobile::State::default();
            Task::none()
        }
        Some(SettingsCategory::Voice) => {
            st.voice = voice_card::State::default();
            Task::none()
        }
        Some(SettingsCategory::Discord) => {
            st.discord = discord_card::State::default();
            Task::none()
        }
        _ => Task::none(),
    }
}

fn enter(st: &mut State, m: &Model, sv: &Services) -> Task<Message> {
    match st.shown {
        Some(SettingsCategory::Mobile) => st.mobile.enter(sv),
        Some(SettingsCategory::Voice) => st.voice.enter(sv),
        Some(SettingsCategory::Discord) => {
            st.discord.enter(sv, m.settings().discord_presence);
            Task::none()
        }
        _ => Task::none(),
    }
}

// ---- The Discord presence ----------------------------------------------------------------------------

/// Publishes the presence to core when it differs from what core has
/// (`useDiscordPresence`). The window that owns the store speaks for the whole
/// app, which has one presence; a second publisher would clear it.
fn publish(st: &mut State, m: &Model, sv: &Services) {
    if !m.live.hydrated {
        return;
    }
    let workspace = m.active_workspace();
    let focused = workspace.and_then(|ws| m.ui.focus_by_workspace.get(&ws.id));
    let wanted = discord::build_presence(&discord::PresenceInput {
        settings: m.settings(),
        workspace,
        focused_pane: focused.map(String::as_str),
        t: Translator::new(m.locale()),
        started_at: st.launched_at,
    });
    if let Some(send) = st.publisher.changed(wanted) {
        hangar_core::discord::discord_presence_set(sv.core().core().discord(), send);
    }
}

// ---- The package surface -----------------------------------------------------------------------------

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let task = match msg {
        Msg::Query(query) => {
            st.query = query;
            st.armed = None;
            st.nav_active = false;
            Task::none()
        }
        Msg::ClearQuery => {
            st.query.clear();
            Task::none()
        }
        // Escape first empties the search, then closes: a filtered page would
        // otherwise disappear with no way back to what the query was hiding.
        Msg::Escape if st.query.is_empty() => Task::done(Message::Intent(Intent::CloseOverlay)),
        Msg::Escape => {
            st.query.clear();
            Task::none()
        }
        Msg::Pick(category) => {
            st.chosen = category;
            st.armed = None;
            st.nav_active = true;
            Task::none()
        }
        Msg::Step(step) => {
            let visible = search::filter(categories(m), &st.query);
            let active = search::active(&visible, st.chosen).map(|category| category.id);
            if let Some(next) = search::step_category(&visible, active, step) {
                st.chosen = next;
            }
            Task::none()
        }
        Msg::Change(change) => apply_change(m, change),
        Msg::Act(act) => {
            st.armed = None;
            match act {
                Act::ResetSettings => apply_change(m, Change::Reset),
                Act::UnloadVoice => {
                    voice_card::unload(sv);
                    Task::none()
                }
            }
        }
        Msg::Arm(id) => {
            st.armed = Some(id);
            Task::none()
        }
        Msg::Disarm => {
            st.armed = None;
            Task::none()
        }
        Msg::BrowseProjects => {
            let start = previews::browse_start(m.settings().projects_folder.as_deref());
            let title = Translator::new(m.locale())
                .t(Key::SettingsProjectsFolderDialog)
                .into_owned();
            Task::perform(dialogs::pick_folder(title, start), |picked| {
                Message::Settings(Msg::ProjectsPicked(picked))
            })
        }
        Msg::ProjectsPicked(picked) => match picked {
            Some(path) => apply_change(
                m,
                Change::text(Target::ProjectsFolder, path.to_string_lossy()),
            ),
            None => Task::none(),
        },
        Msg::Shortcuts(msg) => shortcuts::update(&mut st.shortcuts, m, msg),
        Msg::Mobile(msg) => mobile::update(&mut st.mobile, m, sv, msg),
        Msg::Voice(msg) => voice_card::update(&mut st.voice, m, sv, msg),
        Msg::Discord(msg) => {
            discord_card::update(&mut st.discord, sv, m.settings().discord_presence, msg)
        }
        Msg::Publish => {
            publish(st, m, sv);
            Task::none()
        }
        Msg::DevOpen(category) => {
            st.dev_opened = true;
            Task::done(Message::Intent(Intent::OpenSettings(Some(category))))
        }
    };
    Task::batch([task, sync(st, m, sv)])
}

/// The category a debug build is asked to open at start, by the id the TS
/// used (`HANGAR_DEV_SETTINGS=voice`): the way to see a surface, since screen
/// capture is refused on the development machines.
fn dev_category() -> Option<SettingsCategory> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let id = std::env::var("HANGAR_DEV_SETTINGS").ok()?;
    schema::ORDER
        .iter()
        .copied()
        .find(|category| category.id() == id.trim())
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut all = Vec::new();

    if m.live.hydrated
        && (m.settings().discord_presence || st.publisher.is_showing() || !st.publisher.has_sent())
    {
        all.push(iced::time::every(PUBLISH_EVERY).map(|_| Message::Settings(Msg::Publish)));
    }

    if st.open {
        match st.shown {
            Some(SettingsCategory::Mobile) => all.push(mobile::subscription(&st.mobile)),
            Some(SettingsCategory::Discord) => {
                all.push(discord_card::subscription(m.settings().discord_presence))
            }
            _ => {}
        }
    }

    if !st.dev_opened && dev_category().is_some() {
        all.push(Subscription::run(dev_open));
    }
    Subscription::batch(all)
}

/// One message, a moment after the start: open the sheet where
/// `HANGAR_DEV_SETTINGS` says.
fn dev_open() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::StreamExt;
    iced::futures::stream::once(async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        dev_category().map(|category| Message::Settings(Msg::DevOpen(category)))
    })
    .filter_map(|message| async move { message })
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: the sheet opening (or
/// moving to another category), and closing, are seen here.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        // Asking for a category (a shortcut aims at one) moves the open sheet
        // to it; the shell has pushed the overlay that names it.
        Intent::OpenOverlay(Overlay::Settings(category)) => {
            st.chosen = *category;
            Some(sync(st, m, sv))
        }
        Intent::OpenSettings(category) => {
            st.chosen = category.unwrap_or_default();
            Some(sync(st, m, sv))
        }
        Intent::CloseOverlay => Some(sync(st, m, sv)),
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::RemoteChanged if st.shown == Some(SettingsCategory::Mobile) => {
            Some(st.mobile.remote_changed(sv))
        }
        CoreEvent::VoiceDownload(progress) if st.shown == Some(SettingsCategory::Voice) => st
            .voice
            .download_event(progress.clone())
            .then(|| voice_card::read_models(sv)),
        _ => None,
    }
}

// ---- The view ----------------------------------------------------------------------------------------

/// `Overlay::Settings`: the full settings sheet, open on `category`. Embeds
/// `accounts::settings_page` and `kits::settings_view`.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, category: SettingsCategory) -> Element<'a, Message> {
    let t = ctx.theme;
    let visible = search::filter(build_for(ctx.model, ctx.i18n), &st.query);
    let searching = !st.query.trim().is_empty();

    // The list on the left is every category that is left, with how many rows
    // each keeps while a search is on.
    let active_id = search::active(&visible, category).map(|c| c.id);
    let entries: Vec<(SettingsCategory, String, Icon, usize)> = visible
        .iter()
        .map(|c| (c.id, c.label.clone(), c.icon, c.item_count()))
        .collect();
    let active = visible.into_iter().find(|c| Some(c.id) == active_id);

    let search_field = input::search_field(
        t,
        &st.query,
        match ctx.i18n.t(Key::SettingsSearch) {
            std::borrow::Cow::Borrowed(words) => words,
            std::borrow::Cow::Owned(_) => "",
        },
    )
    .clear_tip(ctx.t(Key::SettingsSearchClear).into_owned())
    .on_input(|text| Message::Settings(Msg::Query(text)))
    .on_clear(Message::Settings(Msg::ClearQuery));

    let mut nav = column![].spacing(1);
    for (id, label, icon_name, count) in entries {
        let current = Some(id) == active_id;
        let mut line = row![
            icon(icon_name, 15.0),
            text(label)
                .size(font_size::BODY)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        if searching {
            line = line.push(
                text(count.to_string())
                    .size(font_size::LABEL)
                    .font(hangar_theme::fonts::mono())
                    .style(style::text::secondary(t)),
            );
        }
        nav = nav.push(
            plain_button(container(line).center_y(Length::Fill))
                .width(Length::Fill)
                .height(28)
                .padding(Padding::from([0.0, space::MD]))
                .style(style::button::row(t, current))
                .on_press(Message::Settings(Msg::Pick(id))),
        );
    }
    let nav_active = st.nav_active;
    let nav = keys::on_key(nav, move |key, _| {
        arrow_step(key)
            .filter(|_| nav_active)
            .map(|step| Message::Settings(Msg::Step(step)))
    });

    let side = container(column![
        container(search_field).padding(Padding {
            top: space::XL,
            right: space::LG + 2.0,
            bottom: space::LG + 2.0,
            left: space::LG + 2.0,
        }),
        scrollable(container(nav).padding(Padding::from([0.0, space::MD])))
            .height(Length::Fill)
            .style(style::scrollable::default(t)),
    ])
    .width(200)
    .height(Length::Fill)
    .style(style::container::sidebar(t));

    let title = active.as_ref().map_or_else(
        || ctx.t(Key::SettingsTitle).into_owned(),
        |c| c.label.clone(),
    );
    let close = Message::Intent(Intent::CloseOverlay);
    let head = container(
        row![
            container(typography::title(t, title)).width(Length::Fill),
            button::icon_button(t, Icon::Close)
                .tip(
                    ctx.i18n
                        .t_with(
                            Key::KeymapWithKeys,
                            &args!("label" => ctx.t(Key::SettingsClose), "keys" => "Esc"),
                        )
                        .into_owned(),
                )
                .on_press(close.clone()),
        ]
        .align_y(Alignment::Center),
    )
    .height(48)
    .padding(Padding {
        left: space::XXXL,
        right: space::LG,
        ..Padding::ZERO
    })
    .align_y(Alignment::Center);

    let body: Element<'a, Message> = match active {
        Some(category) => rows::page(ctx, st, category),
        None => container(typography::label(
            t,
            ctx.i18n
                .t_with(Key::SettingsNoResults, &args!("q" => st.query.as_str()))
                .into_owned(),
        ))
        .padding(Padding::from([56.0, 30.0]))
        .center_x(Length::Fill)
        .into(),
    };
    let content = scrollable(container(body).center_x(Length::Fill))
        .id(st.scroll.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::scrollable::default(t));

    // Content on a well, rows on raised groups; in the light the other way
    // round: white groups on a grey page, the way the system draws them.
    let page_color = if t.is_dark() {
        t.colors.bg_sunken
    } else {
        t.colors.chrome_bar
    };
    let line = t.colors.line;
    let main = container(column![head, hairline(line), content])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(page_color)),
            ..container::Style::default()
        });

    let sheet = container(row![side, main])
        .width(980)
        .height(700)
        .clip(true)
        .style(style::container::modal(t));

    // Escape belongs to the sheet: the search first, then the sheet, and never
    // while a shortcut is recording, which spends it itself.
    let recording = st.shortcuts.is_recording() || crate::keymap::suspended(ctx.model);
    let sheet = keys::on_key(sheet, move |key, _| match key {
        KeyboardKey::Named(Named::Escape) if !recording => Some(Message::Settings(Msg::Escape)),
        _ => None,
    });

    layer::modal(
        widget::Space::new()
            .width(Length::Fill)
            .height(Length::Fill),
        Some(sheet),
    )
    .veil(style::container::scrim(t))
    .on_dismiss(close)
    .escape(false)
    .into()
}

fn hairline<'a>(color: iced::Color) -> Element<'a, Message> {
    container(widget::Space::new())
        .width(Length::Fill)
        .height(1)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(color)),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests;
