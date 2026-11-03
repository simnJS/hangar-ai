//! `MobileSettings.test.ts`: the 24 cases, run in English and in French. The
//! TS drew the view to markup and searched it for text; these read the
//! description the view is drawn from.

use super::*;
use hangar_i18n::Locale;

/// 2026-10-28 20:00:00 UTC, in milliseconds.
const NOW: u64 = 1_793_217_600_000;

fn on() -> RemoteStatus {
    RemoteStatus {
        enabled: true,
        running: true,
        port: 4000,
        error: None,
    }
}

fn off() -> RemoteStatus {
    RemoteStatus {
        enabled: false,
        running: false,
        ..on()
    }
}

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

fn phone() -> RemoteDevice {
    RemoteDevice {
        id: "d1".into(),
        name: "iPhone de Sam".into(),
        created: NOW - 86_400_000,
        last_seen: NOW - 60_000,
    }
}

fn slot(index: usize, patch: impl FnOnce(&mut AppSlot)) -> AppSlot {
    let mut slot = AppSlot {
        slot: index,
        https_port: 8443 + index as u16,
        local_port: 47822 + index as u16,
        mapping: Some(Mapping::Free),
        app: None,
    };
    patch(&mut slot);
    slot
}

fn slots() -> Vec<AppSlot> {
    (0..6).map(|index| slot(index, |_| {})).collect()
}

fn pairing(expires_in: i64) -> PairStart {
    PairStart {
        code: "123456".into(),
        url: "https://pc.tail1234.ts.net/?code=123456".into(),
        qr_svg: r#"<svg viewBox="0 0 1 1"><rect width="1" height="1"/></svg>"#.into(),
        expires_at: (NOW as i64 + expires_in) as u64,
    }
}

/// What `props()` of the TS test held, owned.
struct Fixture {
    remote: Option<RemoteStatus>,
    tailscale: Option<TailscaleStatus>,
    devices: Vec<RemoteDevice>,
    pairing: Option<PairStart>,
    just_paired: Option<String>,
    revoking: Option<String>,
    busy: Busy,
    error: Option<String>,
    options: Option<MobileOptions>,
    projects: Vec<AppProject>,
    slots: Option<Vec<AppSlot>>,
    drafts: HashMap<String, String>,
    problems: HashMap<String, AppPortProblem>,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            remote: Some(on()),
            tailscale: Some(ts(|_| {})),
            devices: Vec::new(),
            pairing: None,
            just_paired: None,
            revoking: None,
            busy: Busy::default(),
            error: None,
            options: Some(MobileOptions {
                keep_awake: false,
                keep_awake_supported: true,
                autostart: false,
            }),
            projects: Vec::new(),
            slots: None,
            drafts: HashMap::new(),
            problems: HashMap::new(),
        }
    }

    fn page(&self, locale: Locale) -> Page {
        page(&Props {
            t: Translator::new(locale),
            now: NOW,
            remote: self.remote.as_ref(),
            tailscale: self.tailscale.as_ref(),
            devices: &self.devices,
            pairing: self.pairing.as_ref(),
            just_paired: self.just_paired.as_deref(),
            revoking: self.revoking.as_deref(),
            busy: self.busy,
            error: self.error.as_deref(),
            options: self.options.as_ref(),
            projects: &self.projects,
            slots: self.slots.as_deref(),
            port_drafts: &self.drafts,
            port_problems: &self.problems,
        })
    }
}

/// Runs a case in both languages, as `describe.each(["en", "fr"])`.
fn each(case: impl Fn(Locale, &dyn Fn(Key) -> String, &dyn Fn(Key, &[(&str, &str)]) -> String)) {
    for locale in Locale::ALL {
        let tr = Translator::new(locale);
        let plain = |key: Key| tr.t(key).into_owned();
        let with = |key: Key, params: &[(&str, &str)]| {
            let args: Vec<(&str, hangar_i18n::Arg)> = params
                .iter()
                .map(|(name, value)| (*name, hangar_i18n::Arg::from(*value)))
                .collect();
            tr.t_with(key, &args).into_owned()
        };
        case(locale, &plain, &with);
    }
}

fn project(id: &str, name: &str, ports: &[u16]) -> AppProject {
    AppProject {
        id: id.into(),
        name: name.into(),
        ports: ports.to_vec(),
    }
}

#[test]
fn off_only_the_switch_unchecked() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.remote = Some(off());
        let page = f.page(locale);
        let text = page.text();
        assert!(text.contains(&t(Key::MobileSwitch)));
        assert_eq!(page.switches_on(), 0);
        assert!(page.blocks.is_empty());
        assert!(!text.contains(&t(Key::MobilePair)));
    });
}

#[test]
fn before_the_pc_answers_the_switch_is_out_of_reach() {
    each(|locale, _, _| {
        let mut f = Fixture::new();
        f.remote = None;
        assert!(!f.page(locale).switch.enabled);
        assert_eq!(f.page(locale).disabled_controls(), 1);
    });
}

#[test]
fn a_toggle_in_flight_also_holds_the_switch() {
    each(|locale, _, _| {
        let mut f = Fixture::new();
        f.busy.toggle = true;
        assert!(!f.page(locale).switch.enabled);
    });
}

#[test]
fn tailscale_missing_says_what_it_is_and_links_to_the_download() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.tailscale = Some(ts(|s| {
            s.installed = false;
            s.running = false;
            s.dns_name = None;
            s.serving = false;
        }));
        let page = f.page(locale);
        let text = page.text();
        assert!(text.contains(&t(Key::MobileTailscaleMissing)));
        assert!(text.contains(&t(Key::MobileTailscaleDownload)));
        assert!(!text.contains(&t(Key::MobileServe)));
        // The button opens the page of the download.
        let download = page
            .rows()
            .filter_map(|row| match row {
                Row::Buttons(buttons) => Some(buttons),
                _ => None,
            })
            .flatten()
            .find(|b| b.label == t(Key::MobileTailscaleDownload))
            .expect("the download button");
        assert_eq!(
            download.msg,
            Msg::OpenLink("https://tailscale.com/download".into())
        );
    });
}

#[test]
fn tailscale_not_running_says_so_and_offers_to_check_again() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.tailscale = Some(ts(|s| {
            s.running = false;
            s.dns_name = None;
            s.serving = false;
        }));
        let text = f.page(locale).text();
        assert!(text.contains(&t(Key::MobileTailscaleStopped)));
        assert!(text.contains(&t(Key::MobileRecheck)));
    });
}

#[test]
fn not_serving_a_button_and_what_it_changes() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.tailscale = Some(ts(|s| s.serving = false));
        let text = f.page(locale).text();
        assert!(text.contains(&t(Key::MobileServe)));
        assert!(text.contains(&t(Key::MobileServeHint)));
        // The address is not handed out before it is served.
        assert!(!text.contains("https://pc.tail1234.ts.net"));
        f.busy.serve = true;
        let busy = f.page(locale);
        assert!(busy.text().contains(&t(Key::MobileServing)));
        // And the button cannot be pressed twice.
        // The serve button, and the pairing one that waits for it.
        assert_eq!(busy.disabled_controls(), 2);
    });
}

#[test]
fn ready_the_address_and_the_pairing_button_is_open() {
    each(|locale, t, _| {
        let page = Fixture::new().page(locale);
        let text = page.text();
        assert!(text.contains("https://pc.tail1234.ts.net"));
        assert!(text.contains(&t(Key::MobilePairShow)));
        assert!(!text.contains(&t(Key::MobilePairBlocked)));
        assert!(text.contains(&t(Key::MobileIphoneOpen)));
    });
}

#[test]
fn leaving_for_a_while_the_two_options_off_until_the_person_turns_them_on() {
    each(|locale, t, _| {
        let page = Fixture::new().page(locale);
        let text = page.text();
        assert!(text.contains(&t(Key::MobileAway)));
        assert!(text.contains(&t(Key::MobileKeepAwake)));
        assert!(text.contains(&t(Key::MobileKeepAwakeHint)));
        assert!(text.contains(&t(Key::MobileAutostart)));
        assert!(text.contains(&t(Key::MobileAutostartHint)));
        // The Hangar Mobile switch alone is on.
        assert_eq!(page.switches_on(), 1);
    });
}

#[test]
fn the_options_as_the_pc_has_them() {
    each(|locale, _, _| {
        let mut f = Fixture::new();
        f.options = Some(MobileOptions {
            keep_awake: true,
            keep_awake_supported: true,
            autostart: false,
        });
        assert_eq!(f.page(locale).switches_on(), 2);
        f.options = Some(MobileOptions {
            keep_awake: true,
            keep_awake_supported: true,
            autostart: true,
        });
        assert_eq!(f.page(locale).switches_on(), 3);
    });
}

#[test]
fn keeping_the_pc_awake_says_it_is_not_supported_where_it_is_not_and_cannot_be_turned_on() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.options = Some(MobileOptions {
            keep_awake: false,
            keep_awake_supported: false,
            autostart: false,
        });
        let page = f.page(locale);
        let text = page.text();
        assert!(text.contains(&t(Key::MobileKeepAwakeUnsupported)));
        assert!(!text.contains(&t(Key::MobileKeepAwakeHint)));
        // Only the unsupported option is out of reach.
        assert_eq!(page.disabled_controls(), 1);
    });
}

#[test]
fn the_options_are_not_drawn_before_the_pc_has_answered_nor_while_mobile_is_off() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.options = None;
        assert!(!f.page(locale).text().contains(&t(Key::MobileAway)));
        let mut off_page = Fixture::new();
        off_page.remote = Some(off());
        assert!(!off_page.page(locale).text().contains(&t(Key::MobileAway)));
    });
}

#[test]
fn an_option_being_set_holds_both() {
    each(|locale, _, _| {
        let mut f = Fixture::new();
        f.busy.options = true;
        assert_eq!(f.page(locale).disabled_controls(), 2);
    });
}

#[test]
fn pairing_is_blocked_until_tailscale_is_ready() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.tailscale = Some(ts(|s| s.serving = false));
        let page = f.page(locale);
        assert!(page.text().contains(&t(Key::MobilePairBlocked)));
        // The button is there and cannot be pressed.
        assert!(page.disabled_controls() >= 1);
    });
}

#[test]
fn pairing_the_qr_code_the_code_and_the_countdown() {
    each(|locale, t, with| {
        let mut f = Fixture::new();
        f.pairing = Some(pairing(272_000));
        let page = f.page(locale);
        let text = page.text();
        assert!(text.contains("<svg"));
        assert!(text.contains("123456"));
        assert!(text.contains(&with(Key::MobilePairExpires, &[("time", "4:32")])));
        assert!(!text.contains(&t(Key::MobilePairAgain)));
        assert!(text.contains(&t(Key::MobilePairClose)));
    });
}

#[test]
fn pairing_expired_a_new_code_is_offered() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.pairing = Some(pairing(-1000));
        let text = f.page(locale).text();
        assert!(text.contains(&t(Key::MobilePairExpired)));
        assert!(text.contains(&t(Key::MobilePairAgain)));
    });
}

#[test]
fn just_paired_the_phone_is_named() {
    each(|locale, _, _| {
        let mut f = Fixture::new();
        f.just_paired = Some("iPhone de Sam".into());
        assert!(f.page(locale).text().contains("iPhone de Sam"));
    });
}

#[test]
fn devices_none_then_the_list_with_its_dates() {
    each(|locale, t, with| {
        assert!(
            Fixture::new()
                .page(locale)
                .text()
                .contains(&t(Key::MobileDevicesEmpty))
        );
        let mut f = Fixture::new();
        f.devices = vec![
            phone(),
            RemoteDevice {
                id: "d2".into(),
                name: "Pixel".into(),
                last_seen: 0,
                ..phone()
            },
        ];
        let text = f.page(locale).text();
        assert!(text.contains("iPhone de Sam"));
        assert!(text.contains(&t(Key::MobileDeviceNeverSeen)));
        assert!(text.contains(&t(Key::MobileRevoke)));
        assert!(!text.contains(&with(Key::MobileRevokeAsk, &[("name", "iPhone de Sam")])));
        assert!(!text.contains(&t(Key::MobileDevicesEmpty)));
    });
}

#[test]
fn devices_are_dated_in_the_language() {
    let mut f = Fixture::new();
    f.devices = vec![phone()];
    let en = f.page(Locale::En).text();
    let fr = f.page(Locale::Fr).text();
    assert!(en.contains("Paired "), "{en}");
    assert!(fr.contains("Appairé "), "{fr}");
    assert!(en.contains("last seen "), "{en}");
    assert!(fr.contains("vu "), "{fr}");
}

#[test]
fn revoke_confirmation_inline_on_that_device_only() {
    each(|locale, t, with| {
        let mut f = Fixture::new();
        f.devices = vec![
            phone(),
            RemoteDevice {
                id: "d2".into(),
                name: "Pixel".into(),
                ..phone()
            },
        ];
        f.revoking = Some("d1".into());
        let page = f.page(locale);
        let text = page.text();
        assert!(text.contains(&with(Key::MobileRevokeAsk, &[("name", "iPhone de Sam")])));
        assert!(!text.contains(&with(Key::MobileRevokeAsk, &[("name", "Pixel")])));
        assert!(text.contains(&t(Key::MobileRevokeHint)));
        // Cancel and the confirmation, for that device.
        let asking = page
            .rows()
            .find_map(|row| match row {
                Row::Device(device) if device.id == "d1" => Some(device),
                _ => None,
            })
            .expect("the device");
        assert_eq!(asking.buttons.len(), 2);
        assert_eq!(asking.buttons[0].msg, Msg::RevokeAsk(None));
        assert_eq!(asking.buttons[1].msg, Msg::Revoke("d1".into()));
        assert_eq!(asking.buttons[1].kind, ButtonKind::Danger);
    });
}

#[test]
fn a_failure_to_start_is_shown() {
    each(|locale, _, with| {
        let mut f = Fixture::new();
        f.remote = Some(RemoteStatus {
            running: false,
            error: Some("port 4000 taken".into()),
            ..on()
        });
        let page = f.page(locale);
        assert!(page.text().contains("port 4000 taken"));
        assert_eq!(
            page.failure,
            Some(with(Key::MobileError, &[("error", "port 4000 taken")]))
        );
        // What the person just did wrong comes before what the PC reported.
        f.error = Some("the keychain said no".into());
        assert!(
            f.page(locale)
                .failure
                .is_some_and(|failure| failure.contains("the keychain said no"))
        );
    });
}

// ---- apps on the phone -----------------------------------------------------------------------------

fn projects() -> Vec<AppProject> {
    vec![
        project("w1", "Shop", &[3000, 5173]),
        project("w2", "Blog", &[]),
    ]
}

fn project_row<'a>(page: &'a Page, id: &str) -> &'a ProjectRow {
    page.rows()
        .find_map(|row| match row {
            Row::Project(project) if project.id == id => Some(project),
            _ => None,
        })
        .expect("the project")
}

#[test]
fn each_workspaces_ports_with_their_remove_button_and_a_field_to_add_one() {
    each(|locale, t, with| {
        let mut f = Fixture::new();
        f.projects = projects();
        let page = f.page(locale);
        let text = page.text();
        assert!(text.contains(&t(Key::MobileApps)));
        assert!(text.contains(&t(Key::MobileAppsHint)));
        assert!(text.contains("Shop"));
        assert!(text.contains("5173"));
        assert!(text.contains(&with(
            Key::MobileAppPortRemove,
            &[("port", "5173"), ("name", "Shop")]
        )));
        assert!(text.contains(&with(
            Key::MobileAppPortRemove,
            &[("port", "3000"), ("name", "Shop")]
        )));
        // Blog has none of its own: its workers' only.
        assert!(text.contains(&t(Key::MobileAppPortsNone)));
        // One numeric field per workspace.
        assert_eq!(
            page.rows()
                .filter(|row| matches!(row, Row::Project(_)))
                .count(),
            2
        );
        assert!(text.contains(&with(Key::MobileAppPortLabel, &[("name", "Blog")])));
        assert_eq!(project_row(&page, "w1").ports.len(), 2);
    });
}

#[test]
fn nothing_can_be_added_from_an_empty_field() {
    each(|locale, _, _| {
        let mut f = Fixture::new();
        f.projects = vec![project("w2", "Blog", &[])];
        assert!(!project_row(&f.page(locale), "w2").add_enabled);
        f.drafts.insert("w2".into(), "4321".into());
        let typed = f.page(locale);
        assert!(project_row(&typed, "w2").add_enabled);
        assert_eq!(project_row(&typed, "w2").draft, "4321");
        // Spaces alone are not a port either.
        f.drafts.insert("w2".into(), "   ".into());
        assert!(!project_row(&f.page(locale), "w2").add_enabled);
    });
}

#[test]
fn it_says_why_a_port_was_turned_down_on_its_workspace_only() {
    each(|locale, t, with| {
        let say = |problem: AppPortProblem, typed: &str| {
            let mut f = Fixture::new();
            f.projects = projects();
            f.drafts.insert("w1".into(), typed.into());
            f.problems.insert("w1".into(), problem);
            f.page(locale)
        };
        assert!(
            say(AppPortProblem::Invalid, "abc")
                .text()
                .contains(&t(Key::MobileAppPortInvalid))
        );
        assert!(
            say(AppPortProblem::Range, "80")
                .text()
                .contains(&t(Key::MobileAppPortRange))
        );
        assert!(
            say(AppPortProblem::Hangar, "47821")
                .text()
                .contains(&with(Key::MobileAppPortHangar, &[("port", "47821")]))
        );
        let duplicate = say(AppPortProblem::Duplicate, "5173");
        assert!(
            duplicate
                .text()
                .contains(&with(Key::MobileAppPortDuplicate, &[("port", "5173")]))
        );
        // One alert, for the workspace that was tried.
        let alerts = duplicate
            .rows()
            .filter(|row| matches!(row, Row::Project(project) if project.problem.is_some()))
            .count();
        assert_eq!(alerts, 1);
        assert!(project_row(&duplicate, "w2").problem.is_none());
    });
}

#[test]
fn it_says_so_when_there_is_no_workspace() {
    each(|locale, t, _| {
        assert!(
            Fixture::new()
                .page(locale)
                .text()
                .contains(&t(Key::MobileAppsNone))
        );
    });
}

#[test]
fn the_slots_none_open_then_the_apps_they_show_and_what_it_changes_in_tailscale() {
    each(|locale, t, with| {
        let mut f = Fixture::new();
        assert!(!f.page(locale).text().contains(&t(Key::MobileAppSlots)));

        f.slots = Some(slots());
        let none = f.page(locale);
        assert!(none.text().contains(&t(Key::MobileAppSlotsEmpty)));
        assert!(none.text().contains(&t(Key::MobileAppsTailscale)));
        let remove = none
            .rows()
            .find_map(|row| match row {
                Row::Rich(spans) => Some(spans),
                _ => None,
            })
            .expect("the note about removing one");
        let code: Vec<&str> = remove
            .iter()
            .filter(|span| span.code)
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(
            code,
            ["tailscale serve --https=8443 off", "tailscale serve reset"]
        );

        let mut open = slots();
        open[0] = slot(0, |s| {
            s.mapping = Some(Mapping::Mapped);
            s.app = Some(super::super::mobile_model::ShownApp {
                workspace_id: "w1".into(),
                workspace: "Shop".into(),
                port: 5173,
                last_used: NOW - 60_000,
            });
        });
        f.slots = Some(open);
        let shown = f.page(locale).text();
        assert!(shown.contains(&with(
            Key::MobileAppSlot,
            &[("workspace", "Shop"), ("port", "5173")]
        )));
        assert!(shown.contains("8443"));
        assert!(!shown.contains(&t(Key::MobileAppSlotsEmpty)));
    });
}

#[test]
fn it_names_a_slots_port_tailscale_serve_uses_for_something_else() {
    each(|locale, _, with| {
        let mut f = Fixture::new();
        f.slots = Some(vec![
            slot(0, |_| {}),
            slot(1, |s| s.mapping = Some(Mapping::Taken)),
        ]);
        let text = f.page(locale).text();
        assert!(text.contains(&with(Key::MobileAppSlotTaken, &[("https", "8444")])));
        assert!(!text.contains(&with(Key::MobileAppSlotTaken, &[("https", "8443")])));
    });
}

#[test]
fn the_apps_and_the_slots_are_not_drawn_while_mobile_is_off() {
    each(|locale, t, _| {
        let mut f = Fixture::new();
        f.remote = Some(off());
        f.projects = projects();
        f.slots = Some(slots());
        let text = f.page(locale).text();
        assert!(!text.contains(&t(Key::MobileApps)));
        assert!(!text.contains(&t(Key::MobileAppSlots)));
    });
}

// ---- the sentence with commands in it ---------------------------------------------------------------

#[test]
fn a_sentence_puts_code_around_the_names_it_is_given() {
    let spans = with_code(
        "To remove one: {command} (8444 to 8448 alike). {reset} removes them all.",
        &[("command", "a b"), ("reset", "c")],
    );
    assert_eq!(
        spans,
        [
            Span {
                text: "To remove one: ".into(),
                code: false
            },
            Span {
                text: "a b".into(),
                code: true
            },
            Span {
                text: " (8444 to 8448 alike). ".into(),
                code: false
            },
            Span {
                text: "c".into(),
                code: true
            },
            Span {
                text: " removes them all.".into(),
                code: false
            },
        ]
    );
}

#[test]
fn a_name_it_is_not_given_stays_as_written() {
    let spans = with_code("Port {port} and {command}", &[("command", "x")]);
    assert_eq!(
        spans,
        [
            Span {
                text: "Port {port} and ".into(),
                code: false
            },
            Span {
                text: "x".into(),
                code: true
            },
        ]
    );
    assert_eq!(
        with_code("no names", &[]),
        [Span {
            text: "no names".into(),
            code: false
        }]
    );
    assert!(with_code("", &[]).is_empty());
}
