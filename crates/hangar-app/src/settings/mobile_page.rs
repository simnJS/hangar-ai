//! The "Mobile" page as a description (`MobileView` of `MobileSettings.tsx`):
//! turn Hangar Mobile on, get Tailscale ready, pair a phone, then see and
//! revoke the phones, and say which apps the phone may open. One step at a
//! time: each block only appears once the one above it is done.
//!
//! [`page`] is pure: everything comes from its arguments, so every state can
//! be described without the PC's side, and the tests of
//! `MobileSettings.test.ts` read the description where the TS read the
//! markup. `mobile.rs` keeps the state and draws what this returns.

use std::collections::HashMap;

use hangar_i18n::{Key, Translator, args};

use super::format::{format_countdown, format_stamp};
use super::mobile::Msg;
use super::mobile_model::{
    AppPortProblem, AppSlot, Mapping, MobileOptions, PairStart, RemoteDevice, RemoteStatus,
    TAILSCALE_DOWNLOAD_URL, TailscaleStatus, TailscaleStep, can_pair, remote_address, seconds_left,
    tailscale_step,
};

/// What is being waited for (`busy`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Busy {
    pub toggle: bool,
    pub serve: bool,
    pub pair: bool,
    pub options: bool,
}

/// A workspace whose apps the phone may open, the HQ left out, with the ports
/// the person added (`AppProject`).
#[derive(Debug, Clone, PartialEq)]
pub struct AppProject {
    pub id: String,
    pub name: String,
    pub ports: Vec<u16>,
}

/// Everything the page is made from (`MobileViewProps`).
pub struct Props<'a> {
    pub t: Translator,
    /// Milliseconds since the epoch.
    pub now: u64,
    /// `None` until the PC has answered.
    pub remote: Option<&'a RemoteStatus>,
    pub tailscale: Option<&'a TailscaleStatus>,
    pub devices: &'a [RemoteDevice],
    pub pairing: Option<&'a PairStart>,
    /// A phone that just paired, named for a moment.
    pub just_paired: Option<&'a str>,
    /// The device whose revocation is waiting for its second click.
    pub revoking: Option<&'a str>,
    pub busy: Busy,
    pub error: Option<&'a str>,
    /// `None` until the PC has answered.
    pub options: Option<&'a MobileOptions>,
    pub projects: &'a [AppProject],
    /// `None` until the PC has answered.
    pub slots: Option<&'a [AppSlot]>,
    /// What is typed in each workspace's port field, by workspace id.
    pub port_drafts: &'a HashMap<String, String>,
    /// Why the port tried last could not be added, by workspace id.
    pub port_problems: &'a HashMap<String, AppPortProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub label: String,
    pub kind: ButtonKind,
    pub enabled: bool,
    pub msg: Msg,
}

/// How a sentence reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// `.mob-text`.
    Plain,
    /// `.mob-text.mob-ok`: with the green dot.
    Ok,
    /// `.mob-note`: smaller, quieter.
    Note,
}

/// A piece of a sentence with commands in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub code: bool,
}

/// A switch of the page; what a click on it sends depends on which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchKind {
    HangarMobile,
    KeepAwake,
    Autostart,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchRow {
    pub kind: SwitchKind,
    pub label: String,
    pub hint: String,
    pub on: bool,
    pub enabled: bool,
}

impl SwitchRow {
    /// What toggling the switch to `on` sends.
    pub fn msg(&self, on: bool) -> Msg {
        match self.kind {
            SwitchKind::HangarMobile => Msg::Toggle(on),
            SwitchKind::KeepAwake => Msg::KeepAwake(on),
            SwitchKind::Autostart => Msg::Autostart(on),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairingRow {
    /// The backend's own SVG (`pairing.rs`), never anything a phone sent.
    pub qr_svg: String,
    pub qr_label: String,
    pub code_label: String,
    pub code: String,
    pub countdown: String,
    pub expired: bool,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub hint: String,
    /// The question of the inline two-step revocation, while it is asked.
    pub ask: Option<String>,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    pub ports: Vec<PortChip>,
    /// Shown instead of the chips when there are none.
    pub none: String,
    /// The accessible name of the field.
    pub field_label: String,
    pub draft: String,
    pub add_label: String,
    pub add_enabled: bool,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PortChip {
    pub port: u16,
    /// What the cross says it does.
    pub remove_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Text {
        text: String,
        tone: Tone,
    },
    /// A sentence with commands in it.
    Rich(Vec<Span>),
    Buttons(Vec<Button>),
    /// The address the phone opens.
    Address(String),
    Switch(SwitchRow),
    Pairing(PairingRow),
    Device(DeviceRow),
    /// A line of the quiet kind, alone in its row (`.set-row__hint`).
    Hint(String),
    Project(ProjectRow),
    /// An app a slot shows: what it is, and where.
    Slot {
        label: String,
        hint: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub title: String,
    pub hint: Option<String>,
    pub rows: Vec<Row>,
}

/// The whole page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub switch: SwitchRow,
    /// Why Hangar Mobile could not start, or what went wrong since.
    pub failure: Option<String>,
    /// Empty while Hangar Mobile is off.
    pub blocks: Vec<Block>,
}

fn button(label: impl Into<String>, kind: ButtonKind, enabled: bool, msg: Msg) -> Button {
    Button {
        label: label.into(),
        kind,
        enabled,
        msg,
    }
}

fn text(text: impl Into<String>, tone: Tone) -> Row {
    Row::Text {
        text: text.into(),
        tone,
    }
}

/// A sentence with commands in it: each `{name}` becomes code around its
/// value (`withCode`).
pub fn with_code(sentence: &str, values: &[(&str, &str)]) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = sentence;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|at| open + at) else {
            break;
        };
        let name = &rest[open + 1..close];
        let word = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
        match values.iter().find(|(key, _)| *key == name).filter(|_| word) {
            Some((_, value)) => {
                plain.push_str(&rest[..open]);
                if !plain.is_empty() {
                    spans.push(Span {
                        text: std::mem::take(&mut plain),
                        code: false,
                    });
                }
                spans.push(Span {
                    text: (*value).to_string(),
                    code: true,
                });
            }
            // Not one of the names: it stays as written.
            None => plain.push_str(&rest[..=close]),
        }
        rest = &rest[close + 1..];
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        spans.push(Span {
            text: plain,
            code: false,
        });
    }
    spans
}

fn problem_key(problem: AppPortProblem) -> Key {
    match problem {
        AppPortProblem::Invalid => Key::MobileAppPortInvalid,
        AppPortProblem::Range => Key::MobileAppPortRange,
        AppPortProblem::Hangar => Key::MobileAppPortHangar,
        AppPortProblem::Duplicate => Key::MobileAppPortDuplicate,
    }
}

fn tailscale_block(props: &Props) -> Option<Block> {
    let tailscale = props.tailscale?;
    let t = props.t;
    let step = tailscale_step(tailscale);
    let rows = match step {
        TailscaleStep::Missing => vec![
            text(t.t(Key::MobileTailscaleMissing), Tone::Plain),
            Row::Buttons(vec![
                button(
                    t.t(Key::MobileTailscaleDownload),
                    ButtonKind::Primary,
                    true,
                    Msg::OpenLink(TAILSCALE_DOWNLOAD_URL.to_string()),
                ),
                button(
                    t.t(Key::MobileRecheck),
                    ButtonKind::Ghost,
                    true,
                    Msg::Recheck,
                ),
            ]),
        ],
        TailscaleStep::Stopped => vec![
            text(t.t(Key::MobileTailscaleStopped), Tone::Plain),
            Row::Buttons(vec![button(
                t.t(Key::MobileRecheck),
                ButtonKind::Secondary,
                true,
                Msg::Recheck,
            )]),
        ],
        TailscaleStep::Unserved => vec![
            text(t.t(Key::MobileTailscaleUnserved), Tone::Plain),
            // Only ever this click: nothing runs `tailscale serve` on its own.
            Row::Buttons(vec![button(
                if props.busy.serve {
                    t.t(Key::MobileServing)
                } else {
                    t.t(Key::MobileServe)
                },
                ButtonKind::Primary,
                !props.busy.serve,
                Msg::Serve,
            )]),
            text(t.t(Key::MobileServeHint), Tone::Note),
        ],
        TailscaleStep::Ready => match remote_address(tailscale) {
            Some(address) => vec![
                text(t.t(Key::MobileTailscaleReady), Tone::Ok),
                Row::Address(address),
            ],
            None => Vec::new(),
        },
    };
    Some(Block {
        title: t.t(Key::MobileTailscale).into_owned(),
        hint: None,
        rows,
    })
}

fn pair_block(props: &Props) -> Block {
    let t = props.t;
    let ready = can_pair(props.remote, props.tailscale);
    let mut rows = Vec::new();
    if let Some(name) = props.just_paired {
        rows.push(text(
            t.t_with(Key::MobilePaired, &args!("name" => name)),
            Tone::Ok,
        ));
    }
    match props.pairing {
        None => {
            rows.push(Row::Buttons(vec![button(
                if props.busy.pair {
                    t.t(Key::MobilePairBusy)
                } else {
                    t.t(Key::MobilePairShow)
                },
                ButtonKind::Primary,
                ready && !props.busy.pair,
                Msg::PairStart,
            )]));
            if !ready {
                rows.push(text(t.t(Key::MobilePairBlocked), Tone::Note));
            }
        }
        Some(pairing) => {
            let left = seconds_left(pairing.expires_at, props.now);
            let expired = left == 0;
            let mut buttons = Vec::new();
            if expired {
                buttons.push(button(
                    t.t(Key::MobilePairAgain),
                    ButtonKind::Primary,
                    !props.busy.pair,
                    Msg::PairStart,
                ));
            }
            buttons.push(button(
                t.t(Key::MobilePairClose),
                ButtonKind::Ghost,
                true,
                Msg::PairClose,
            ));
            rows.push(Row::Pairing(PairingRow {
                qr_svg: pairing.qr_svg.clone(),
                qr_label: t.t(Key::MobilePairQr).into_owned(),
                code_label: t.t(Key::MobilePairCode).into_owned(),
                code: pairing.code.clone(),
                countdown: if expired {
                    t.t(Key::MobilePairExpired).into_owned()
                } else {
                    t.t_with(
                        Key::MobilePairExpires,
                        &args!("time" => format_countdown(left)),
                    )
                    .into_owned()
                },
                expired,
                buttons,
            }));
        }
    }
    Block {
        title: t.t(Key::MobilePair).into_owned(),
        hint: Some(t.t(Key::MobilePairHint).into_owned()),
        rows,
    }
}

fn devices_block(props: &Props) -> Block {
    let t = props.t;
    let locale = t.locale();
    let mut rows = Vec::new();
    if props.devices.is_empty() {
        rows.push(Row::Hint(t.t(Key::MobileDevicesEmpty).into_owned()));
    }
    for device in props.devices {
        let asking = props.revoking == Some(device.id.as_str());
        let hint = if asking {
            t.t(Key::MobileRevokeHint).into_owned()
        } else {
            [
                t.t_with(
                    Key::MobileDevicePaired,
                    &args!("date" => format_stamp(locale, device.created, props.now)),
                )
                .into_owned(),
                if device.last_seen > 0 {
                    t.t_with(
                        Key::MobileDeviceSeen,
                        &args!("date" => format_stamp(locale, device.last_seen, props.now)),
                    )
                    .into_owned()
                } else {
                    t.t(Key::MobileDeviceNeverSeen).into_owned()
                },
            ]
            .join(" · ")
        };
        let buttons = if asking {
            vec![
                button(
                    t.t(Key::CreateCancel),
                    ButtonKind::Ghost,
                    true,
                    Msg::RevokeAsk(None),
                ),
                button(
                    t.t(Key::MobileRevoke),
                    ButtonKind::Danger,
                    true,
                    Msg::Revoke(device.id.clone()),
                ),
            ]
        } else {
            vec![button(
                t.t(Key::MobileRevoke),
                ButtonKind::Secondary,
                true,
                Msg::RevokeAsk(Some(device.id.clone())),
            )]
        };
        rows.push(Row::Device(DeviceRow {
            id: device.id.clone(),
            name: device.name.clone(),
            hint,
            ask: asking.then(|| {
                t.t_with(Key::MobileRevokeAsk, &args!("name" => device.name.as_str()))
                    .into_owned()
            }),
            buttons,
        }));
    }
    Block {
        title: t.t(Key::MobileDevices).into_owned(),
        hint: None,
        rows,
    }
}

/// The ports the phone may open, per workspace: its workers' are found by
/// themselves, the person adds their own. Only the lists change here; the
/// window answers the phone from them.
fn apps_block(props: &Props) -> Block {
    let t = props.t;
    let mut rows = Vec::new();
    if props.projects.is_empty() {
        rows.push(Row::Hint(t.t(Key::MobileAppsNone).into_owned()));
    }
    for project in props.projects {
        let draft = props
            .port_drafts
            .get(&project.id)
            .cloned()
            .unwrap_or_default();
        let problem = props.port_problems.get(&project.id).map(|problem| {
            t.t_with(problem_key(*problem), &args!("port" => draft.trim()))
                .into_owned()
        });
        rows.push(Row::Project(ProjectRow {
            id: project.id.clone(),
            name: project.name.clone(),
            ports: project
                .ports
                .iter()
                .map(|port| PortChip {
                    port: *port,
                    remove_label: t
                        .t_with(
                            Key::MobileAppPortRemove,
                            &args!("port" => port.to_string(), "name" => project.name.as_str()),
                        )
                        .into_owned(),
                })
                .collect(),
            none: t.t(Key::MobileAppPortsNone).into_owned(),
            field_label: t
                .t_with(
                    Key::MobileAppPortLabel,
                    &args!("name" => project.name.as_str()),
                )
                .into_owned(),
            add_enabled: !draft.trim().is_empty(),
            draft,
            add_label: t.t(Key::MobileAppPortAdd).into_owned(),
            problem,
        }));
    }
    Block {
        title: t.t(Key::MobileApps).into_owned(),
        hint: Some(t.t(Key::MobileAppsHint).into_owned()),
        rows,
    }
}

/// The slots an app goes through, the apps they show, and what that changes
/// in Tailscale.
fn slots_block(props: &Props) -> Option<Block> {
    let slots = props.slots?;
    let t = props.t;
    let locale = t.locale();
    let shown: Vec<&AppSlot> = slots.iter().filter(|slot| slot.app.is_some()).collect();
    let mut rows = Vec::new();
    if shown.is_empty() {
        rows.push(Row::Hint(t.t(Key::MobileAppSlotsEmpty).into_owned()));
    }
    for slot in shown {
        let Some(app) = &slot.app else { continue };
        rows.push(Row::Slot {
            label: t
                .t_with(
                    Key::MobileAppSlot,
                    &args!("workspace" => app.workspace.as_str(), "port" => app.port.to_string()),
                )
                .into_owned(),
            hint: t
                .t_with(
                    Key::MobileAppSlotVia,
                    &args!(
                        "https" => slot.https_port.to_string(),
                        "date" => format_stamp(locale, app.last_used, props.now),
                    ),
                )
                .into_owned(),
        });
    }
    for slot in slots
        .iter()
        .filter(|slot| slot.mapping == Some(Mapping::Taken))
    {
        rows.push(text(
            t.t_with(
                Key::MobileAppSlotTaken,
                &args!("https" => slot.https_port.to_string()),
            ),
            Tone::Note,
        ));
    }
    rows.push(text(t.t(Key::MobileAppsTailscale), Tone::Note));
    rows.push(Row::Rich(with_code(
        &t.t(Key::MobileAppsTailscaleRemove),
        &[
            ("command", "tailscale serve --https=8443 off"),
            ("reset", "tailscale serve reset"),
        ],
    )));
    rows.push(Row::Buttons(vec![button(
        t.t(Key::MobileRecheck),
        ButtonKind::Ghost,
        true,
        Msg::SlotsRefresh,
    )]));
    Some(Block {
        title: t.t(Key::MobileAppSlots).into_owned(),
        hint: Some(t.t(Key::MobileAppSlotsHint).into_owned()),
        rows,
    })
}

/// Leaving for a week: the PC kept awake, and Hangar there when it is back
/// from a restart.
fn away_block(props: &Props) -> Option<Block> {
    let options = props.options?;
    let t = props.t;
    Some(Block {
        title: t.t(Key::MobileAway).into_owned(),
        hint: Some(t.t(Key::MobileAwayHint).into_owned()),
        rows: vec![
            Row::Switch(SwitchRow {
                kind: SwitchKind::KeepAwake,
                label: t.t(Key::MobileKeepAwake).into_owned(),
                hint: if options.keep_awake_supported {
                    t.t(Key::MobileKeepAwakeHint).into_owned()
                } else {
                    t.t(Key::MobileKeepAwakeUnsupported).into_owned()
                },
                on: options.keep_awake,
                enabled: !props.busy.options && options.keep_awake_supported,
            }),
            Row::Switch(SwitchRow {
                kind: SwitchKind::Autostart,
                label: t.t(Key::MobileAutostart).into_owned(),
                hint: t.t(Key::MobileAutostartHint).into_owned(),
                on: options.autostart,
                enabled: !props.busy.options,
            }),
        ],
    })
}

/// The page for `props`.
pub fn page(props: &Props) -> Page {
    let t = props.t;
    let enabled = props.remote.is_some_and(|remote| remote.enabled);
    let failure = props
        .error
        .map(str::to_string)
        .or_else(|| props.remote.and_then(|remote| remote.error.clone()))
        .map(|error| {
            t.t_with(Key::MobileError, &args!("error" => error.as_str()))
                .into_owned()
        });

    let mut blocks = Vec::new();
    if enabled {
        blocks.extend(tailscale_block(props));
        blocks.push(pair_block(props));
        blocks.push(devices_block(props));
        blocks.push(apps_block(props));
        blocks.extend(slots_block(props));
        blocks.extend(away_block(props));
        blocks.push(Block {
            title: t.t(Key::MobileIphone).into_owned(),
            hint: None,
            rows: vec![
                text(t.t(Key::MobileIphoneOpen), Tone::Plain),
                text(t.t(Key::MobileIphoneNotify), Tone::Note),
            ],
        });
    }

    Page {
        switch: SwitchRow {
            kind: SwitchKind::HangarMobile,
            label: t.t(Key::MobileSwitch).into_owned(),
            hint: t.t(Key::MobileSwitchHint).into_owned(),
            on: enabled,
            enabled: props.remote.is_some() && !props.busy.toggle,
        },
        failure,
        blocks,
    }
}

impl Row {
    #[cfg(test)]
    fn strings(&self, out: &mut Vec<String>) {
        match self {
            Row::Text { text, .. } => out.push(text.clone()),
            Row::Rich(spans) => out.extend(spans.iter().map(|span| span.text.clone())),
            Row::Buttons(buttons) => out.extend(buttons.iter().map(|b| b.label.clone())),
            Row::Address(address) => out.push(address.clone()),
            Row::Switch(switch) => out.extend([switch.label.clone(), switch.hint.clone()]),
            Row::Pairing(pairing) => {
                out.extend([
                    pairing.qr_svg.clone(),
                    pairing.qr_label.clone(),
                    pairing.code_label.clone(),
                    pairing.code.clone(),
                    pairing.countdown.clone(),
                ]);
                out.extend(pairing.buttons.iter().map(|b| b.label.clone()));
            }
            Row::Device(device) => {
                out.extend([device.name.clone(), device.hint.clone()]);
                out.extend(device.ask.clone());
                out.extend(device.buttons.iter().map(|b| b.label.clone()));
            }
            Row::Hint(hint) => out.push(hint.clone()),
            Row::Project(project) => {
                out.extend([
                    project.name.clone(),
                    project.field_label.clone(),
                    project.draft.clone(),
                    project.add_label.clone(),
                ]);
                if project.ports.is_empty() {
                    out.push(project.none.clone());
                }
                for chip in &project.ports {
                    out.extend([chip.port.to_string(), chip.remove_label.clone()]);
                }
                out.extend(project.problem.clone());
            }
            Row::Slot { label, hint } => out.extend([label.clone(), hint.clone()]),
        }
    }
}

impl Page {
    /// Every row, in page order.
    #[cfg(test)]
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.blocks.iter().flat_map(|block| &block.rows)
    }

    /// Everything the page says, in one string: what the markup of the TS
    /// tests was searched for.
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut out = vec![self.switch.label.clone(), self.switch.hint.clone()];
        out.extend(self.failure.clone());
        for block in &self.blocks {
            out.push(block.title.clone());
            out.extend(block.hint.clone());
            for row in &block.rows {
                row.strings(&mut out);
            }
        }
        out.join("\n")
    }

    /// The switches that are on, the page's own included.
    #[cfg(test)]
    pub fn switches_on(&self) -> usize {
        let own = usize::from(self.switch.on);
        own + self
            .rows()
            .filter(|row| matches!(row, Row::Switch(switch) if switch.on))
            .count()
    }

    /// The controls that cannot be used right now, the page's switch included.
    #[cfg(test)]
    pub fn disabled_controls(&self) -> usize {
        let own = usize::from(!self.switch.enabled);
        let rest: usize = self
            .rows()
            .map(|row| match row {
                Row::Switch(switch) => usize::from(!switch.enabled),
                Row::Buttons(buttons) => buttons.iter().filter(|b| !b.enabled).count(),
                Row::Pairing(pairing) => pairing.buttons.iter().filter(|b| !b.enabled).count(),
                Row::Device(device) => device.buttons.iter().filter(|b| !b.enabled).count(),
                Row::Project(project) => usize::from(!project.add_enabled),
                _ => 0,
            })
            .sum();
        own + rest
    }
}

#[cfg(test)]
mod tests;
