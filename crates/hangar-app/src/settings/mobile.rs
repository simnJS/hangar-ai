//! Settings › Mobile: the controller and the drawing of the page
//! (`MobileSettings.tsx`). What the page says is `mobile_page.rs`; what the
//! PC knows is hangar-core's `remote_*` functions.
//!
//! The page loads when its category is shown and keeps itself fresh while it
//! is: Hangar Mobile's status and the two options at once, then, only while
//! it is on, Tailscale, the paired phones and the app slots. A phone paired or
//! revoked (`CoreEvent::RemoteChanged`) re-reads the list.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use hangar_core::remote::{self as core, Remote};
use iced::widget::{column, container, row, svg};
use iced::{Alignment, Element, Length, Padding, Subscription, Task};

use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{fonts, style};
use hangar_ui::{button, form, input, text as typography};

use super::mobile_model::{
    AppPortProblem, AppSlot, MobileOptions, PairStart, RemoteDevice, RemoteStatus, TailscaleStatus,
    app_ports_of, check_app_port, hangar_ports, new_devices, sort_devices, with_app_port,
    without_app_port,
};
use super::mobile_page::{
    AppProject, Busy, Button, ButtonKind, Page, PairingRow, Props, Row, SwitchRow, Tone, page,
};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::services::Services;
use crate::state::{Model, Patch, StoreAction, Workspace};

/// How long the name of a phone that just paired stays on the page.
const FLASH: Duration = Duration::from_secs(6);

/// What the page does and is told (`MobileView`'s callbacks, and the answers
/// of the PC).
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    // The PC's answers.
    Status(Result<RemoteStatus, String>),
    Options(Result<MobileOptions, String>),
    Toggled(Result<RemoteStatus, String>),
    Tailscale(Result<TailscaleStatus, String>),
    /// `tailscale serve` ran.
    Served(Result<TailscaleStatus, String>),
    Devices(Result<Vec<RemoteDevice>, String>),
    Slots(Result<Vec<AppSlot>, String>),
    Paired(Result<PairStart, String>),
    Revoked(Result<(), String>),
    /// An option was set: it is read back.
    OptionSet(Result<(), String>),
    Opened(Result<(), String>),
    // The person.
    Toggle(bool),
    Serve,
    Recheck,
    OpenLink(String),
    PairStart,
    PairClose,
    RevokeAsk(Option<String>),
    Revoke(String),
    KeepAwake(bool),
    Autostart(bool),
    PortDraft(String, String),
    PortAdd(String),
    PortRemove(String, u16),
    SlotsRefresh,
    // Time.
    /// A second went by while a code is shown.
    Tick,
    /// The flash for the phone that paired is over (its number).
    FlashEnd(u64),
}

#[derive(Debug)]
struct Pairing {
    start: PairStart,
    /// The QR code, made once: the renderer keys its raster on the handle.
    qr: svg::Handle,
}

#[derive(Debug, Default)]
pub struct State {
    remote: Option<RemoteStatus>,
    tailscale: Option<TailscaleStatus>,
    devices: Vec<RemoteDevice>,
    pairing: Option<Pairing>,
    /// A phone that just paired, named for a moment, and the number of the
    /// flash that ends it.
    just_paired: Option<(String, u64)>,
    flashes: u64,
    revoking: Option<String>,
    options: Option<MobileOptions>,
    busy: Busy,
    error: Option<String>,
    /// Milliseconds since the epoch, kept fresh while a code is shown.
    now: u64,
    slots: Option<Vec<AppSlot>>,
    port_drafts: HashMap<String, String>,
    port_problems: HashMap<String, AppPortProblem>,
    /// Who was there when the code was drawn: a refresh that finds anyone else
    /// found the phone that paired.
    known: HashSet<String>,
    /// Core could not open Hangar Mobile's store, so there is nothing to ask:
    /// the page says so (`core.remote()` is `None`).
    unavailable: bool,
    /// Tailscale, the list and the slots have been asked for since Hangar
    /// Mobile was last turned on.
    loaded: bool,
}

/// The `Arc<Remote>` of the running core, when Hangar Mobile could be opened.
fn remote(sv: &Services) -> Option<Arc<Remote>> {
    sv.core().core().remote().cloned()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// Runs `job` off the interface's thread and hands its answer back as a
/// message of this page.
fn ask<T: Send + 'static>(
    job: impl Future<Output = Result<T, String>> + Send + 'static,
    then: fn(Result<T, String>) -> Msg,
) -> Task<Message> {
    Task::perform(job, move |answer| {
        Message::Settings(super::Msg::Mobile(then(answer)))
    })
}

/// A synchronous core call, on the blocking pool.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| format!("the task stopped: {err}"))?
}

impl State {
    /// The page is shown: what the PC knows now.
    pub fn enter(&mut self, sv: &Services) -> Task<Message> {
        *self = State::default();
        self.now = now_ms();
        let Some(remote) = remote(sv) else {
            self.unavailable = true;
            return Task::none();
        };
        let status = ask(
            {
                let remote = remote.clone();
                async move { core::remote_status(remote).await.map(RemoteStatus::from) }
            },
            Msg::Status,
        );
        let options = ask(
            blocking(move || Ok(MobileOptions::from(core::remote_options(&remote)))),
            Msg::Options,
        );
        Task::batch([status, options])
    }

    /// Tailscale, the phones and the slots, which only matter while Hangar
    /// Mobile is on.
    fn load_enabled(&mut self, sv: &Services) -> Task<Message> {
        self.loaded = true;
        let Some(remote) = remote(sv) else {
            return Task::none();
        };
        Task::batch([
            ask(
                {
                    let remote = remote.clone();
                    async move {
                        core::remote_tailscale(remote)
                            .await
                            .map(TailscaleStatus::from)
                    }
                },
                Msg::Tailscale,
            ),
            devices(remote.clone()),
            slots(remote),
        ])
    }

    /// The phones the PC lists, newest first. When a code is on show and the
    /// list holds a phone it did not before, that phone is the pairing that
    /// was waiting: the code closes and the phone is named for a moment.
    /// Answers the number of that flash.
    fn devices_arrived(&mut self, list: Vec<RemoteDevice>) -> Option<u64> {
        let list = sort_devices(&list);
        let mut flash = None;
        if self.pairing.is_some()
            && let Some(device) = new_devices(&self.known, &list).first()
        {
            self.pairing = None;
            self.flashes += 1;
            self.just_paired = Some((device.name.clone(), self.flashes));
            flash = Some(self.flashes);
        }
        self.known = list.iter().map(|device| device.id.clone()).collect();
        self.devices = list;
        flash
    }

    /// The port typed for workspace `id`, added to its list if the person may:
    /// the store action that writes it, or `None` after saying why not.
    fn add_port(&mut self, projects: &[AppProject], id: &str) -> Option<StoreAction> {
        let project = projects.iter().find(|project| project.id == id)?;
        let reserved = hangar_ports(self.remote.as_ref(), self.slots.as_deref());
        let typed = self.port_drafts.get(id).map_or("", String::as_str);
        match check_app_port(typed, &project.ports, &reserved) {
            Err(problem) => {
                self.port_problems.insert(id.to_string(), problem);
                None
            }
            Ok(port) => {
                self.port_drafts.remove(id);
                self.port_problems.remove(id);
                Some(ports_action(id, with_app_port(&project.ports, port)))
            }
        }
    }

    /// Whether a code is on show, which is when a clock is worth running.
    pub fn pairing_open(&self) -> bool {
        self.pairing.is_some()
    }

    /// A phone paired or was revoked.
    pub fn remote_changed(&mut self, sv: &Services) -> Task<Message> {
        if !self.loaded {
            return Task::none();
        }
        match remote(sv) {
            Some(remote) => devices(remote),
            None => Task::none(),
        }
    }
}

fn devices(remote: Arc<Remote>) -> Task<Message> {
    ask(
        blocking(move || {
            Ok(core::remote_devices(&remote)
                .into_iter()
                .map(RemoteDevice::from)
                .collect())
        }),
        Msg::Devices,
    )
}

fn slots(remote: Arc<Remote>) -> Task<Message> {
    ask(
        async move {
            core::remote_app_slots(remote)
                .await
                .map(|slots| slots.into_iter().map(AppSlot::from).collect())
        },
        Msg::Slots,
    )
}

fn options_of(remote: Arc<Remote>) -> Task<Message> {
    ask(
        blocking(move || Ok(MobileOptions::from(core::remote_options(&remote)))),
        Msg::Options,
    )
}

/// What a message that starts work does first: the last failure is stale
/// (`guard`).
fn begin(st: &mut State) {
    st.error = None;
}

/// A failure is the page's error line.
fn failed<T>(st: &mut State, answer: Result<T, String>) -> Option<T> {
    match answer {
        Ok(value) => Some(value),
        Err(error) => {
            st.error = Some(error);
            None
        }
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Status(answer) => {
            let Some(status) = failed(st, answer) else {
                return Task::none();
            };
            let enabled = status.enabled;
            st.remote = Some(status);
            if enabled && !st.loaded {
                st.load_enabled(sv)
            } else {
                Task::none()
            }
        }
        Msg::Options(answer) => {
            if let Some(options) = failed(st, answer) {
                st.options = Some(options);
            }
            Task::none()
        }
        Msg::Toggle(on) => {
            begin(st);
            let Some(remote) = remote(sv) else {
                st.unavailable = true;
                return Task::none();
            };
            st.busy.toggle = true;
            ask(
                async move {
                    core::remote_set_enabled(remote, on)
                        .await
                        .map(RemoteStatus::from)
                },
                Msg::Toggled,
            )
        }
        Msg::Toggled(answer) => {
            st.busy.toggle = false;
            let Some(status) = failed(st, answer) else {
                return Task::none();
            };
            let enabled = status.enabled;
            st.remote = Some(status);
            if enabled {
                if st.loaded {
                    Task::none()
                } else {
                    st.load_enabled(sv)
                }
            } else {
                st.pairing = None;
                st.revoking = None;
                st.loaded = false;
                Task::none()
            }
        }
        Msg::Tailscale(answer) => {
            if let Some(status) = failed(st, answer) {
                st.tailscale = Some(status);
            }
            Task::none()
        }
        Msg::Recheck => {
            begin(st);
            match remote(sv) {
                Some(remote) => ask(
                    async move {
                        core::remote_tailscale(remote)
                            .await
                            .map(TailscaleStatus::from)
                    },
                    Msg::Tailscale,
                ),
                None => Task::none(),
            }
        }
        Msg::Serve => {
            begin(st);
            let Some(remote) = remote(sv) else {
                return Task::none();
            };
            st.busy.serve = true;
            ask(
                async move {
                    core::remote_tailscale_serve(remote)
                        .await
                        .map(TailscaleStatus::from)
                },
                Msg::Served,
            )
        }
        Msg::Served(answer) => {
            st.busy.serve = false;
            if let Some(status) = failed(st, answer) {
                st.tailscale = Some(status);
            }
            Task::none()
        }
        Msg::Devices(answer) => {
            let Some(list) = failed(st, answer) else {
                return Task::none();
            };
            match st.devices_arrived(list) {
                Some(number) => Task::perform(
                    async move {
                        tokio::time::sleep(FLASH).await;
                        number
                    },
                    |number| wrap(Msg::FlashEnd(number)),
                ),
                None => Task::none(),
            }
        }
        Msg::Slots(answer) => {
            if let Some(list) = failed(st, answer) {
                st.slots = Some(list);
            }
            Task::none()
        }
        Msg::SlotsRefresh => {
            begin(st);
            match remote(sv) {
                Some(remote) => slots(remote),
                None => Task::none(),
            }
        }
        Msg::OpenLink(url) => {
            begin(st);
            ask(
                blocking(move || hangar_core::opener::open_url(&url)),
                Msg::Opened,
            )
        }
        Msg::Opened(answer) => {
            failed(st, answer);
            Task::none()
        }
        Msg::PairStart => {
            begin(st);
            let Some(remote) = remote(sv) else {
                return Task::none();
            };
            st.just_paired = None;
            st.known = st.devices.iter().map(|device| device.id.clone()).collect();
            st.busy.pair = true;
            ask(
                async move { core::remote_pair_start(remote).await.map(PairStart::from) },
                Msg::Paired,
            )
        }
        Msg::Paired(answer) => {
            st.busy.pair = false;
            if let Some(start) = failed(st, answer) {
                st.now = now_ms();
                let qr = svg::Handle::from_memory(start.qr_svg.clone().into_bytes());
                st.pairing = Some(Pairing { start, qr });
            }
            Task::none()
        }
        Msg::PairClose => {
            st.pairing = None;
            Task::none()
        }
        Msg::RevokeAsk(id) => {
            st.revoking = id;
            Task::none()
        }
        Msg::Revoke(id) => {
            begin(st);
            let Some(remote) = remote(sv) else {
                return Task::none();
            };
            ask(
                blocking(move || core::remote_revoke(&remote, id)),
                Msg::Revoked,
            )
        }
        Msg::Revoked(answer) => {
            if failed(st, answer).is_none() {
                return Task::none();
            }
            st.revoking = None;
            match remote(sv) {
                Some(remote) => devices(remote),
                None => Task::none(),
            }
        }
        Msg::KeepAwake(on) => set_option(st, sv, move |remote| {
            core::remote_set_keep_awake(remote, on)
        }),
        Msg::Autostart(on) => set_option(st, sv, move |_| core::remote_set_autostart(on)),
        Msg::OptionSet(answer) => {
            st.busy.options = false;
            if failed(st, answer).is_none() {
                return Task::none();
            }
            match remote(sv) {
                Some(remote) => options_of(remote),
                None => Task::none(),
            }
        }
        Msg::PortDraft(id, typed) => {
            st.port_drafts.insert(id.clone(), typed);
            st.port_problems.remove(&id);
            Task::none()
        }
        Msg::PortAdd(id) => {
            let projects = projects(m);
            if let Some(action) = st.add_port(&projects, &id) {
                m.apply(action);
            }
            Task::none()
        }
        Msg::PortRemove(id, port) => {
            let Some(project) = projects(m).into_iter().find(|project| project.id == id) else {
                return Task::none();
            };
            m.apply(ports_action(&id, without_app_port(&project.ports, port)));
            Task::none()
        }
        Msg::Tick => {
            st.now = now_ms();
            Task::none()
        }
        Msg::FlashEnd(number) => {
            if st.just_paired.as_ref().is_some_and(|(_, n)| *n == number) {
                st.just_paired = None;
            }
            Task::none()
        }
    }
}

/// Sets one of the two options, then reads them back (`work("options", …)`).
fn set_option(
    st: &mut State,
    sv: &Services,
    set: impl FnOnce(&Remote) -> Result<(), String> + Send + 'static,
) -> Task<Message> {
    begin(st);
    let Some(remote) = remote(sv) else {
        return Task::none();
    };
    st.busy.options = true;
    ask(blocking(move || set(&remote)), Msg::OptionSet)
}

/// The workspaces whose apps the phone may open, the HQ left out.
fn projects(m: &Model) -> Vec<AppProject> {
    m.state()
        .workspaces
        .iter()
        .filter(|ws| ws.hq != Some(true))
        .map(|ws| AppProject {
            id: ws.id.clone(),
            name: ws.name.clone(),
            ports: app_ports_of(ws),
        })
        .collect()
}

/// `updateWorkspace(id, {mobilePorts})`: left out of the file when empty.
fn ports_action(id: &str, ports: Vec<u16>) -> StoreAction {
    StoreAction::UpdateWorkspace(
        id.to_string(),
        Patch::new(move |ws: &mut Workspace| {
            ws.mobile_ports = (!ports.is_empty()).then(|| ports.clone());
        }),
    )
}

/// The clock of a shown code: once a second, for the countdown.
pub fn subscription(st: &State) -> Subscription<Message> {
    if st.pairing_open() {
        iced::time::every(Duration::from_secs(1))
            .map(|_| Message::Settings(super::Msg::Mobile(Msg::Tick)))
    } else {
        Subscription::none()
    }
}

// ---- Drawing ------------------------------------------------------------------------------------

/// The page, drawn from its description.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let projects = projects(ctx.model);
    let error = if st.unavailable {
        Some(ctx.t(hangar_i18n::Key::MobileUnavailable).into_owned())
    } else {
        st.error.clone()
    };
    let page = page(&Props {
        t: ctx.i18n,
        now: st.now,
        remote: st.remote.as_ref(),
        tailscale: st.tailscale.as_ref(),
        devices: &st.devices,
        pairing: st.pairing.as_ref().map(|p| &p.start),
        just_paired: st.just_paired.as_ref().map(|(name, _)| name.as_str()),
        revoking: st.revoking.as_deref(),
        busy: st.busy,
        error: error.as_deref(),
        options: st.options.as_ref(),
        projects: &projects,
        slots: st.slots.as_deref(),
        port_drafts: &st.port_drafts,
        port_problems: &st.port_problems,
    });
    render(st, ctx, page)
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Mobile(msg))
}

fn render<'a>(st: &'a State, ctx: Ctx<'a>, page: Page) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut stack = column![].spacing(20);

    let mut top = column![form::group(t, vec![switch_row(ctx, page.switch)]),].spacing(space::MD);
    if let Some(failure) = page.failure {
        top = top.push(container(typography::danger(t, failure)).padding(Padding {
            left: space::XS,
            top: space::MD,
            ..Padding::ZERO
        }));
    }
    stack = stack.push(top);

    for block in page.blocks {
        let mut head =
            column![typography::heading(t, block.title).size(font_size::BODY)].spacing(2);
        if let Some(hint) = block.hint {
            head = head.push(typography::hint(t, hint));
        }
        let rows = block
            .rows
            .into_iter()
            .map(|r| row_view(st, ctx, r))
            .collect();
        stack = stack.push(
            column![
                container(head).padding(Padding::from([0.0, space::XS])),
                form::group(t, rows)
            ]
            .spacing(space::MD),
        );
    }
    stack.into()
}

fn switch_row<'a>(ctx: Ctx<'a>, switch: SwitchRow) -> Element<'a, Message> {
    let t = ctx.theme;
    let control = {
        let mut control = input::switch(t, switch.on);
        if switch.enabled {
            let send = switch.clone();
            control = control.on_toggle(move |on| wrap(send.msg(on)));
        }
        control
    };
    form::row(t, switch.label, control).hint(switch.hint).into()
}

fn button_view<'a>(ctx: Ctx<'a>, spec: Button) -> Element<'a, Message> {
    let t = ctx.theme;
    let label = spec.label;
    let pressed = spec.enabled.then(|| wrap(spec.msg));
    match spec.kind {
        ButtonKind::Primary => button::primary(t, label).on_press_maybe(pressed).into(),
        ButtonKind::Secondary => button::secondary(t, label).on_press_maybe(pressed).into(),
        ButtonKind::Ghost => button::ghost(t, label).on_press_maybe(pressed).into(),
        ButtonKind::Danger => button::danger(t, label).on_press_maybe(pressed).into(),
    }
}

fn buttons_view<'a>(ctx: Ctx<'a>, buttons: Vec<Button>) -> Element<'a, Message> {
    row(buttons.into_iter().map(|b| button_view(ctx, b)))
        .spacing(space::LG)
        .wrap()
        .into()
}

/// A row of the page: padded the way a settings row is.
fn boxed<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content.into())
        .padding(Padding::from([space::XL, space::XL]))
        .width(Length::Fill)
        .into()
}

fn row_view<'a>(st: &'a State, ctx: Ctx<'a>, r: Row) -> Element<'a, Message> {
    let t = ctx.theme;
    match r {
        Row::Text { text, tone } => boxed(match tone {
            Tone::Plain => Element::from(typography::body(t, text)),
            Tone::Note => typography::hint(t, text).into(),
            Tone::Error => typography::danger(t, text).into(),
            Tone::Ok => row![dot(ctx), typography::body(t, text)]
                .spacing(space::LG)
                .align_y(Alignment::Center)
                .into(),
        }),
        Row::Rich(spans) => boxed(
            row(spans.into_iter().map(|span| {
                if span.code {
                    container(
                        iced::widget::text(span.text)
                            .size(font_size::CAPTION_LG)
                            .font(fonts::mono())
                            .style(style::text::primary(t)),
                    )
                    .padding(Padding::from([1.0, space::XS]))
                    .style(style::container::well(t))
                    .into()
                } else {
                    Element::from(typography::hint(t, span.text))
                }
            }))
            .align_y(Alignment::Center)
            .wrap(),
        ),
        Row::Buttons(buttons) => boxed(buttons_view(ctx, buttons)),
        Row::Address(address) => boxed(
            container(
                typography::mono(t, address)
                    .size(font_size::UI_LG)
                    .style(style::text::primary(t)),
            )
            .padding(Padding::from([space::MD, space::LG]))
            .style(style::container::well(t)),
        ),
        Row::Switch(switch) => switch_row(ctx, switch),
        Row::Pairing(pairing) => boxed(pairing_view(st, ctx, pairing)),
        Row::Device(device) => {
            let mut control = row![].spacing(space::LG).align_y(Alignment::Center);
            if let Some(ask) = device.ask {
                control = control.push(typography::ui(t, ask));
            }
            for b in device.buttons {
                control = control.push(button_view(ctx, b));
            }
            form::row(t, device.name, control).hint(device.hint).into()
        }
        Row::Hint(hint) => boxed(typography::hint(t, hint)),
        Row::Project(project) => boxed(project_view(st, ctx, project)),
        Row::Slot { label, hint } => form::row(t, label, iced::widget::Space::new())
            .hint(hint)
            .into(),
    }
}

/// The green dot of a step that is done.
fn dot<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    let c = &ctx.theme.colors;
    let color = c.success;
    container(iced::widget::Space::new())
        .width(8)
        .height(8)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: radius::FULL.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn pairing_view<'a>(st: &'a State, ctx: Ctx<'a>, pairing: PairingRow) -> Element<'a, Message> {
    let t = ctx.theme;
    let expired = pairing.expired;
    // White whatever the theme: a QR code on a dark ground does not scan.
    let ground = if expired {
        iced::Color {
            a: 0.25,
            ..iced::Color::WHITE
        }
    } else {
        iced::Color::WHITE
    };
    let qr: Element<'a, Message> = match &st.pairing {
        Some(shown) => svg(shown.qr.clone())
            .width(220)
            .height(220)
            .opacity(if expired { 0.25_f32 } else { 1.0 })
            .into(),
        None => iced::widget::Space::new().width(220).height(220).into(),
    };
    let qr = container(qr)
        .padding(space::XL)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(ground)),
            border: iced::Border {
                radius: radius::LG.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        });
    let countdown = if expired {
        typography::danger(t, pairing.countdown).size(font_size::UI)
    } else {
        typography::label(t, pairing.countdown).size(font_size::UI)
    };
    let side = column![
        typography::hint(t, pairing.code_label),
        iced::widget::text(pairing.code)
            .size(30)
            .font(fonts::mono())
            .style(style::text::primary(t)),
        countdown,
        buttons_view(ctx, pairing.buttons),
    ]
    .spacing(space::MD);
    row![qr, side].spacing(24).align_y(Alignment::Center).into()
}

fn project_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    project: super::mobile_page::ProjectRow,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let id = project.id.clone();
    let mut chips = row![].spacing(space::MD).align_y(Alignment::Center);
    if project.ports.is_empty() {
        chips = chips.push(typography::hint(t, project.none));
    }
    for chip in project.ports {
        let remove = wrap(Msg::PortRemove(id.clone(), chip.port));
        chips = chips.push(
            container(
                row![
                    iced::widget::text(chip.port.to_string())
                        .size(font_size::UI)
                        .font(fonts::mono())
                        .style(style::text::primary(t)),
                    hangar_ui::button::icon_button(t, hangar_ui::Icon::Close)
                        .side(20.0)
                        .icon_size(10.0)
                        .tip(chip.remove_label)
                        .danger()
                        .on_press(remove),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            )
            .padding(Padding {
                left: space::LG,
                right: 2.0,
                top: 2.0,
                bottom: 2.0,
            })
            .style(style::container::well(t)),
        );
    }
    let draft: &'a str = st.port_drafts.get(&project.id).map_or("", String::as_str);
    let typing = id.clone();
    let typed = input::text_field(t, draft)
        .placeholder("5173")
        .mono()
        .width(110)
        .on_input(move |text| wrap(Msg::PortDraft(typing.clone(), text)))
        .on_submit(wrap(Msg::PortAdd(id.clone())));
    let add = button::secondary(t, project.add_label)
        .on_press_maybe(project.add_enabled.then(|| wrap(Msg::PortAdd(id.clone()))));
    let mut stack = column![
        typography::body(t, project.name),
        chips.wrap(),
        row![typed, add]
            .spacing(space::LG)
            .align_y(Alignment::Center),
    ]
    .spacing(space::MD);
    if let Some(problem) = project.problem {
        stack = stack.push(typography::danger(t, problem));
    }
    stack.into()
}

#[cfg(test)]
mod tests;
