//! The port field of a workspace, given Mod+Shift+Enter: the keymap's
//! shortcut is spent before the field sees it (`keymap::guard`).

use super::*;
use crate::settings::guard_testing::Fixture;

#[test]
fn the_port_field_leaves_the_shortcut_to_the_keymap() {
    let fixture = Fixture::new();
    let st = State {
        remote: Some(RemoteStatus {
            enabled: true,
            running: true,
            port: 47821,
            error: None,
        }),
        tailscale: Some(TailscaleStatus {
            installed: true,
            running: true,
            dns_name: Some("pc.tail1234.ts.net".into()),
            serving: true,
        }),
        ..State::default()
    };
    // The page holds one field for each workspace, and nothing else to type in.
    fixture.assert_spends_the_split_chord(view(&st, fixture.ctx()));
}
