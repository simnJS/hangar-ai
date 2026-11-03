//! Hangar Mobile, from the settings: the types the page reads and the little
//! logic it needs (`lib/remote.ts`, `settings/mobileOptions.ts`): which
//! Tailscale step the person is on, the pairing countdown, the ports a phone
//! may open.
//!
//! hangar-core answers with its own structs, which are not `Clone`, and a
//! message has to be. These mirror them one for one and are built `From` them,
//! so the page and its tests work on plain data.

use hangar_core::remote as core;

use crate::state::types::Workspace;

pub const TAILSCALE_DOWNLOAD_URL: &str = "https://tailscale.com/download";

/// Below 1024 a port is the system's, past 65535 there is none.
pub const APP_PORT_MIN: u16 = 1024;
pub const APP_PORT_MAX: u16 = 65535;

/// The slots' listeners, until the PC has listed them: beside the mobile
/// server's 47821, as `apps.rs` fixes them.
pub const DEFAULT_SLOT_PORTS: [u16; 6] = [47822, 47823, 47824, 47825, 47826, 47827];

/// `RemoteStatus`.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    /// Why the server could not start: the port taken, most likely.
    pub error: Option<String>,
}

/// `PairStart`.
#[derive(Debug, Clone, PartialEq)]
pub struct PairStart {
    pub code: String,
    /// What the QR code holds.
    pub url: String,
    pub qr_svg: String,
    /// Milliseconds since the epoch.
    pub expires_at: u64,
}

/// `DeviceView`; the dates are milliseconds since the epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDevice {
    pub id: String,
    pub name: String,
    pub created: u64,
    /// 0 until the phone has made a call.
    pub last_seen: u64,
}

/// `TailscaleStatus`.
#[derive(Debug, Clone, PartialEq)]
pub struct TailscaleStatus {
    pub installed: bool,
    pub running: bool,
    pub dns_name: Option<String>,
    pub serving: bool,
}

/// What Tailscale Serve does with a slot's HTTPS port: mapped to the slot,
/// free, or taken by something of the person's, which Hangar leaves alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    Mapped,
    Free,
    Taken,
}

/// The app a slot shows, while it does; freed 30 minutes after `last_used`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownApp {
    pub workspace_id: String,
    pub workspace: String,
    pub port: u16,
    pub last_used: u64,
}

/// `SlotView`: one of the six ports an app shown on the phone goes through,
/// HTTPS on the tailnet to a listener of Hangar's on 127.0.0.1.
#[derive(Debug, Clone, PartialEq)]
pub struct AppSlot {
    pub slot: usize,
    /// 8443 to 8448: `https://<pc>.<tailnet>.ts.net:<https_port>/`.
    pub https_port: u16,
    /// 47822 to 47827: Hangar's listener, which Tailscale Serve proxies to.
    pub local_port: u16,
    /// `None` when Tailscale could not be asked.
    pub mapping: Option<Mapping>,
    pub app: Option<ShownApp>,
}

/// The two options that keep Hangar reachable while the person is away,
/// beside the Mobile switch: the PC kept awake, and Hangar started at login
/// (`MobileOptions`).
#[derive(Debug, Clone, PartialEq)]
pub struct MobileOptions {
    /// The PC does not sleep while Hangar Mobile is on (the display still
    /// may).
    pub keep_awake: bool,
    /// False where it cannot work (Linux).
    pub keep_awake_supported: bool,
    /// Hangar starts when the person logs in.
    pub autostart: bool,
}

impl From<core::RemoteStatus> for RemoteStatus {
    fn from(status: core::RemoteStatus) -> Self {
        RemoteStatus {
            enabled: status.enabled,
            running: status.running,
            port: status.port,
            error: status.error,
        }
    }
}

impl From<core::PairStart> for PairStart {
    fn from(pair: core::PairStart) -> Self {
        PairStart {
            code: pair.code,
            url: pair.url,
            qr_svg: pair.qr_svg,
            expires_at: pair.expires_at,
        }
    }
}

impl From<core::DeviceView> for RemoteDevice {
    fn from(device: core::DeviceView) -> Self {
        RemoteDevice {
            id: device.id,
            name: device.name,
            created: device.created,
            last_seen: device.last_seen,
        }
    }
}

impl From<core::TailscaleStatus> for TailscaleStatus {
    fn from(status: core::TailscaleStatus) -> Self {
        TailscaleStatus {
            installed: status.installed,
            running: status.running,
            dns_name: status.dns_name,
            serving: status.serving,
        }
    }
}

impl From<core::Mapping> for Mapping {
    fn from(mapping: core::Mapping) -> Self {
        match mapping {
            core::Mapping::Mapped => Mapping::Mapped,
            core::Mapping::Free => Mapping::Free,
            core::Mapping::Taken => Mapping::Taken,
        }
    }
}

impl From<core::SlotView> for AppSlot {
    fn from(slot: core::SlotView) -> Self {
        AppSlot {
            slot: slot.slot,
            https_port: slot.https_port,
            local_port: slot.local_port,
            mapping: slot.mapping.map(Mapping::from),
            app: slot.app.map(|app| ShownApp {
                workspace_id: app.workspace_id,
                workspace: app.workspace,
                port: app.port,
                last_used: app.last_used,
            }),
        }
    }
}

impl From<core::Options> for MobileOptions {
    fn from(options: core::Options) -> Self {
        MobileOptions {
            keep_awake: options.keep_awake,
            keep_awake_supported: options.keep_awake_supported,
            autostart: options.autostart,
        }
    }
}

// ---- The apps the phone may open (lib/remoteApps) ---------------------------------------------

fn is_app_port(port: u16) -> bool {
    (APP_PORT_MIN..=APP_PORT_MAX).contains(&port)
}

/// The ports the person added to a workspace, as saved: whole ports in range,
/// each once, in order. Anything else in the file is left out.
pub fn app_ports_of(workspace: &Workspace) -> Vec<u16> {
    let mut ports: Vec<u16> = workspace
        .mobile_ports
        .iter()
        .flatten()
        .copied()
        .filter(|port| is_app_port(*port))
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Hangar's own ports, which no app may be: the mobile server's, and each
/// slot's listener (what Tailscale Serve proxies to), which the mobile server
/// refuses to show anyway.
pub fn hangar_ports(remote: Option<&RemoteStatus>, slots: Option<&[AppSlot]>) -> Vec<u16> {
    let mut own: Vec<u16> = match slots {
        Some(slots) if !slots.is_empty() => slots.iter().map(|slot| slot.local_port).collect(),
        _ => DEFAULT_SLOT_PORTS.to_vec(),
    };
    if let Some(remote) = remote {
        own.push(remote.port);
    }
    own.sort_unstable();
    own.dedup();
    own
}

/// Why a port cannot be added: not a number, out of range, Hangar's own,
/// already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPortProblem {
    Invalid,
    Range,
    Hangar,
    Duplicate,
}

/// The port typed, if the person may add it to `list`; why not otherwise.
pub fn check_app_port(text: &str, list: &[u16], reserved: &[u16]) -> Result<u16, AppPortProblem> {
    let typed = text.trim();
    // `^\d{1,6}$`: digits only, so a sign, a dot, an exponent or a space inside
    // is not a port.
    if typed.is_empty() || typed.len() > 6 || !typed.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppPortProblem::Invalid);
    }
    let port = typed
        .parse::<u32>()
        .ok()
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| is_app_port(*port))
        .ok_or(AppPortProblem::Range)?;
    if reserved.contains(&port) {
        return Err(AppPortProblem::Hangar);
    }
    if list.contains(&port) {
        return Err(AppPortProblem::Duplicate);
    }
    Ok(port)
}

pub fn with_app_port(list: &[u16], port: u16) -> Vec<u16> {
    let mut next = list.to_vec();
    next.push(port);
    next.sort_unstable();
    next.dedup();
    next
}

pub fn without_app_port(list: &[u16], port: u16) -> Vec<u16> {
    list.iter()
        .copied()
        .filter(|entry| *entry != port)
        .collect()
}

// ---- Tailscale, pairing, devices ---------------------------------------------------------------

/// Where the person is in setting Tailscale up, one step at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailscaleStep {
    Missing,
    Stopped,
    Unserved,
    Ready,
}

pub fn tailscale_step(status: &TailscaleStatus) -> TailscaleStep {
    if !status.installed {
        return TailscaleStep::Missing;
    }
    // Connected without a name on the tailnet is not an address to hand out.
    if !status.running || status.dns_name.is_none() {
        return TailscaleStep::Stopped;
    }
    if status.serving {
        TailscaleStep::Ready
    } else {
        TailscaleStep::Unserved
    }
}

/// The address the phone opens, once Tailscale has a name for the PC.
pub fn remote_address(status: &TailscaleStatus) -> Option<String> {
    if tailscale_step(status) == TailscaleStep::Missing {
        return None;
    }
    status
        .dns_name
        .as_ref()
        .map(|name| format!("https://{name}"))
}

/// Phones can only be paired once the server runs and the address is served.
pub fn can_pair(remote: Option<&RemoteStatus>, tailscale: Option<&TailscaleStatus>) -> bool {
    remote.is_some_and(|remote| remote.enabled && remote.running)
        && tailscale.is_some_and(|tailscale| tailscale_step(tailscale) == TailscaleStep::Ready)
}

/// Whole seconds left before `expires_at`, never negative (milliseconds in).
pub fn seconds_left(expires_at: u64, now: u64) -> i64 {
    let left = expires_at as i64 - now as i64;
    if left <= 0 { 0 } else { (left + 999) / 1000 }
}

/// Phones the list did not hold before: the pairing that was waiting went
/// through.
pub fn new_devices<'a>(
    before: &std::collections::HashSet<String>,
    after: &'a [RemoteDevice],
) -> Vec<&'a RemoteDevice> {
    after
        .iter()
        .filter(|device| !before.contains(&device.id))
        .collect()
}

/// Most recently paired first.
pub fn sort_devices(devices: &[RemoteDevice]) -> Vec<RemoteDevice> {
    let mut sorted = devices.to_vec();
    sorted.sort_by_key(|device| std::cmp::Reverse(device.created));
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ts(patch: impl FnOnce(&mut TailscaleStatus)) -> TailscaleStatus {
        let mut status = TailscaleStatus {
            installed: true,
            running: true,
            dns_name: Some("pc.tail1234.ts.net".into()),
            serving: true,
        };
        patch(&mut status);
        status
    }

    fn remote(patch: impl FnOnce(&mut RemoteStatus)) -> RemoteStatus {
        let mut status = RemoteStatus {
            enabled: true,
            running: true,
            port: 4000,
            error: None,
        };
        patch(&mut status);
        status
    }

    fn device(id: &str, created: u64) -> RemoteDevice {
        RemoteDevice {
            id: id.into(),
            name: id.into(),
            created,
            last_seen: 0,
        }
    }

    fn ids(devices: &[RemoteDevice]) -> Vec<&str> {
        devices.iter().map(|device| device.id.as_str()).collect()
    }

    #[test]
    fn tailscale_is_missing_when_the_cli_was_not_found() {
        let status = ts(|s| {
            s.installed = false;
            s.running = false;
            s.dns_name = None;
            s.serving = false;
        });
        assert_eq!(tailscale_step(&status), TailscaleStep::Missing);
    }

    #[test]
    fn tailscale_is_stopped_when_installed_but_not_connected() {
        let status = ts(|s| {
            s.running = false;
            s.dns_name = None;
            s.serving = false;
        });
        assert_eq!(tailscale_step(&status), TailscaleStep::Stopped);
    }

    #[test]
    fn tailscale_is_stopped_when_connected_without_a_name_to_give_out() {
        assert_eq!(
            tailscale_step(&ts(|s| s.dns_name = None)),
            TailscaleStep::Stopped
        );
    }

    #[test]
    fn tailscale_is_unserved_when_connected_but_not_published() {
        assert_eq!(
            tailscale_step(&ts(|s| s.serving = false)),
            TailscaleStep::Unserved
        );
    }

    #[test]
    fn tailscale_is_ready_when_connected_and_served() {
        assert_eq!(tailscale_step(&ts(|_| {})), TailscaleStep::Ready);
    }

    #[test]
    fn the_address_is_the_https_one_of_the_pcs_name() {
        assert_eq!(
            remote_address(&ts(|_| {})).as_deref(),
            Some("https://pc.tail1234.ts.net")
        );
    }

    #[test]
    fn there_is_no_address_without_tailscale_or_a_name() {
        assert_eq!(
            remote_address(&ts(|s| {
                s.installed = false;
                s.dns_name = None;
            })),
            None
        );
        assert_eq!(remote_address(&ts(|s| s.dns_name = None)), None);
    }

    #[test]
    fn pairing_needs_the_server_on_and_the_address_served() {
        assert!(can_pair(Some(&remote(|_| {})), Some(&ts(|_| {}))));
        assert!(!can_pair(
            Some(&remote(|r| r.enabled = false)),
            Some(&ts(|_| {}))
        ));
        assert!(!can_pair(
            Some(&remote(|r| r.running = false)),
            Some(&ts(|_| {}))
        ));
        assert!(!can_pair(
            Some(&remote(|_| {})),
            Some(&ts(|s| s.serving = false))
        ));
        assert!(!can_pair(None, Some(&ts(|_| {}))));
        assert!(!can_pair(Some(&remote(|_| {})), None));
    }

    #[test]
    fn the_countdown_rounds_up_to_whole_seconds_and_stops_at_zero() {
        assert_eq!(seconds_left(10_000, 9_001), 1);
        assert_eq!(seconds_left(10_000, 0), 10);
        assert_eq!(seconds_left(10_000, 10_000), 0);
        assert_eq!(seconds_left(10_000, 20_000), 0);
    }

    #[test]
    fn the_phones_a_refresh_brought_in_are_found() {
        let after = [device("a", 1), device("b", 2)];
        let before: HashSet<String> = ["a".to_string()].into();
        let joined = new_devices(&before, &after);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].id, "b");
        let both: HashSet<String> = ["a".to_string(), "b".to_string()].into();
        assert!(new_devices(&both, &after).is_empty());
    }

    #[test]
    fn the_latest_pairing_is_listed_first_without_touching_the_input() {
        let input = [device("a", 1), device("c", 3), device("b", 2)];
        assert_eq!(ids(&sort_devices(&input)), ["c", "b", "a"]);
        assert_eq!(ids(&input), ["a", "c", "b"]);
    }

    fn slot(index: usize, local_port: Option<u16>) -> AppSlot {
        AppSlot {
            slot: index,
            https_port: 8443 + index as u16,
            local_port: local_port.unwrap_or(47822 + index as u16),
            mapping: Some(Mapping::Free),
            app: None,
        }
    }

    fn slots() -> Vec<AppSlot> {
        (0..6).map(|index| slot(index, None)).collect()
    }

    #[test]
    fn a_whole_port_from_1024_to_65535_is_taken_with_spaces_around_it_or_not() {
        assert_eq!(check_app_port("5173", &[], &[]), Ok(5173));
        assert_eq!(check_app_port(" 3000 ", &[], &[]), Ok(3000));
        assert_eq!(check_app_port("1024", &[], &[]), Ok(1024));
        assert_eq!(check_app_port("65535", &[], &[]), Ok(65535));
    }

    #[test]
    fn anything_else_is_told_what_is_wrong_with_it() {
        for typed in ["", "  ", "abc", "51 73", "5173.5", "-80", "0x1F90", "1e4"] {
            assert_eq!(
                check_app_port(typed, &[], &[]),
                Err(AppPortProblem::Invalid),
                "{typed:?}"
            );
        }
        for typed in ["0", "80", "1023", "65536", "99999", "123456"] {
            assert_eq!(
                check_app_port(typed, &[], &[]),
                Err(AppPortProblem::Range),
                "{typed:?}"
            );
        }
    }

    #[test]
    fn hangars_own_ports_are_never_taken() {
        let reserved = hangar_ports(Some(&remote(|r| r.port = 47821)), Some(&slots()));
        assert_eq!(reserved, [47821, 47822, 47823, 47824, 47825, 47826, 47827]);
        assert_eq!(
            check_app_port("47821", &[], &reserved),
            Err(AppPortProblem::Hangar)
        );
        assert_eq!(
            check_app_port("47824", &[], &reserved),
            Err(AppPortProblem::Hangar)
        );
        // The HTTPS ports are Tailscale's, on the tailnet: a dev server may use
        // them on the PC.
        assert_eq!(check_app_port("8443", &[], &reserved), Ok(8443));
    }

    #[test]
    fn the_slots_default_ports_are_kept_out_before_the_pc_has_listed_them() {
        assert_eq!(hangar_ports(None, None), DEFAULT_SLOT_PORTS);
        let mut with_remote = vec![4000];
        with_remote.extend(DEFAULT_SLOT_PORTS);
        assert_eq!(
            hangar_ports(Some(&remote(|r| r.port = 4000)), Some(&[])),
            with_remote
        );
        assert_eq!(
            hangar_ports(
                Some(&remote(|r| r.port = 4000)),
                Some(&[slot(0, Some(50000))])
            ),
            [4000, 50000]
        );
    }

    #[test]
    fn a_port_is_not_added_twice() {
        assert_eq!(
            check_app_port("5173", &[3000, 5173], &[]),
            Err(AppPortProblem::Duplicate)
        );
    }

    #[test]
    fn a_workspaces_list_stays_in_order_each_port_once() {
        assert_eq!(with_app_port(&[3000, 8080], 5173), [3000, 5173, 8080]);
        assert_eq!(with_app_port(&[5173], 5173), [5173]);
        assert_eq!(without_app_port(&[3000, 5173, 8080], 5173), [3000, 8080]);
        assert_eq!(without_app_port(&[3000], 5173), [3000]);
    }

    fn with_ports(ports: Option<Vec<u16>>) -> Workspace {
        Workspace {
            mobile_ports: ports,
            ..Workspace::default()
        }
    }

    #[test]
    fn a_workspaces_ports_are_read_as_saved_leaving_out_what_is_not_one() {
        assert!(app_ports_of(&with_ports(None)).is_empty());
        assert_eq!(
            app_ports_of(&with_ports(Some(vec![8080, 3000, 8080]))),
            [3000, 8080]
        );
        assert_eq!(app_ports_of(&with_ports(Some(vec![80, 4000]))), [4000]);
    }

    // The TS dropped a non-integer (3000.5), a string, a null and a number past
    // 65535 from the saved list and kept the rest. The lenient reader of
    // `state/lenient.rs` reads a list entry by entry, so the good ports are
    // still there and the page keeps the one an app can use, as the TS did.
    #[test]
    fn a_saved_list_with_something_else_in_it_keeps_the_good_ports() {
        use crate::state::read_state;
        use serde_json::json;
        let state = read_state(json!({"workspaces": [
            {"id": "w", "mobilePorts": [80, 3000.5, 70000, "5173", null, 4000]},
            {"id": "x", "mobilePorts": "5173"}
        ]}));
        assert_eq!(app_ports_of(&state.workspaces[0]), [4000]);
        assert!(app_ports_of(&state.workspaces[1]).is_empty());
    }

    #[test]
    fn core_s_answers_become_the_pages_types() {
        let slot = AppSlot::from(core::SlotView {
            slot: 0,
            https_port: 8443,
            local_port: 47822,
            mapping: Some(core::Mapping::Taken),
            app: Some(core::ShownApp {
                workspace_id: "w1".into(),
                workspace: "Shop".into(),
                port: 5173,
                last_used: 7,
            }),
        });
        assert_eq!(slot.mapping, Some(Mapping::Taken));
        assert_eq!(slot.app.as_ref().map(|app| app.port), Some(5173));
        let options = MobileOptions::from(core::Options {
            keep_awake: true,
            keep_awake_supported: false,
            autostart: true,
        });
        assert!(options.keep_awake && !options.keep_awake_supported && options.autostart);
    }
}
