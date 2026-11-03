//! « Comptes Claude »: several Claude subscriptions in Hangar, one profile per
//! account through Claude Code's `CLAUDE_CONFIG_DIR` (`lib/accounts.ts`). The
//! account is chosen per workspace first, with a global default and a per-pane
//! override. Switching is always the person's click, never an automatic
//! rotation.
//!
//! The backend (`hangar_core::accounts`) lists, adds, renames and removes the
//! accounts; this file is the pure part around it: reading a choice,
//! resolving the account of a pane, and the registry of the accounts panes
//! were launched on.
//!
//! Only Claude Code panes run on an account: P5 resolves it at each launch
//! (`launch_account`) and hands it to `pty_spawn`, which sets
//! `CLAUDE_CONFIG_DIR` for a profile. Workers follow their workspace, like any
//! pane without an account of its own.

use std::collections::HashMap;

use serde_json::Value;

pub use hangar_core::accounts::Account as ClaudeAccount;

use crate::state::PaneId;

/// `"main"` is the default Claude Code config: always present, never removed.
pub type AccountId = String;

pub const MAIN_ACCOUNT: &str = "main";

/// Logs the profile a pane runs on in, in the browser (Claude Code 2.1.316;
/// `claude` then `/login` does the same). Typed by a `Pane.login` pane.
pub const LOGIN_COMMAND: &str = "claude auth login";

/// `state.json` is hand-edited: anything that is not a non-empty string is no
/// choice at all.
pub fn read_account_choice(choice: Option<&str>) -> Option<String> {
    let id = choice?.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// The same for a value straight out of the JSON, which can be anything. The
/// lenient reader of `state.json` already drops a field of the wrong type, so
/// the typed levels cannot hold one; this is for what has not been through it.
pub fn read_account_choice_value(value: &Value) -> Option<String> {
    read_account_choice(value.as_str())
}

/// Whether a hand-edited choice names account `id`.
pub fn names_account(choice: Option<&str>, id: &str) -> bool {
    read_account_choice(choice).as_deref() == Some(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSource {
    Pane,
    Workspace,
    Default,
    Main,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAccount {
    pub id: AccountId,
    pub source: AccountSource,
    /// The id that was asked for but does not exist (any more); the fallback
    /// applies.
    pub missing: Option<String>,
}

/// Pane > workspace > settings > main. A choice naming an unknown account is
/// skipped for the next level, and reported in `missing` (the first one met).
/// Main is always known, listed or not: it is never removed.
pub fn resolve_account(
    accounts: &[ClaudeAccount],
    default: Option<&str>,
    workspace: Option<&str>,
    pane: Option<&str>,
) -> ResolvedAccount {
    let levels = [
        (AccountSource::Pane, pane),
        (AccountSource::Workspace, workspace),
        (AccountSource::Default, default),
    ];
    let mut missing: Option<String> = None;
    for (source, raw) in levels {
        let Some(id) = read_account_choice(raw) else {
            continue;
        };
        if id == MAIN_ACCOUNT || accounts.iter().any(|account| account.id == id) {
            return ResolvedAccount {
                id,
                source,
                missing,
            };
        }
        missing.get_or_insert(id);
    }
    ResolvedAccount {
        id: MAIN_ACCOUNT.to_string(),
        source: AccountSource::Main,
        missing,
    }
}

/// The account a Claude Code pane starts on, as [`resolve_account`] decides:
/// asking for the accounts only when some level names one other than main, so
/// a machine with a single account never asks. When the list cannot be had,
/// every other account counts as gone: the pane starts on main, and `missing`
/// says which was asked for.
pub fn launch_account_with(
    list: impl FnOnce() -> Result<Vec<ClaudeAccount>, String>,
    default: Option<&str>,
    workspace: Option<&str>,
    pane: Option<&str>,
) -> ResolvedAccount {
    let named = [pane, workspace, default].into_iter().any(|level| {
        read_account_choice(level).is_some_and(|id| id != MAIN_ACCOUNT)
    });
    let accounts = if named { list().unwrap_or_default() } else { Vec::new() };
    resolve_account(&accounts, default, workspace, pane)
}

/// [`launch_account_with`] on the accounts of this machine
/// (`hangar_core::accounts::accounts_list`, which reads a few small files).
pub fn launch_account(
    default: Option<&str>,
    workspace: Option<&str>,
    pane: Option<&str>,
) -> ResolvedAccount {
    launch_account_with(
        || Ok(hangar_core::accounts::accounts_list()),
        default,
        workspace,
        pane,
    )
}

/// The account each running Claude Code pane was launched on, by pane id. A
/// pane keeps the account it started on until it restarts, whatever is chosen
/// meanwhile: this is what its usage counts against
/// ([`usage_by_account`](super::switch::usage_by_account)).
///
/// It lives in `Model.live.accounts.launched`: P5 notes the account when it
/// launches a Claude Code pane (`note(pane, Some(account))`) and forgets it
/// when the pane ends (`note(pane, None)`). It is app state, so no
/// subscription is needed: a view reads it when it is drawn.
#[derive(Debug, Default, Clone)]
pub struct Launched(HashMap<PaneId, AccountId>);

impl Launched {
    /// `account` `None`: the pane no longer runs Claude Code on any account.
    /// Returns whether anything changed.
    pub fn note(&mut self, pane: &str, account: Option<&str>) -> bool {
        match account {
            None => self.0.remove(pane).is_some(),
            Some(account) => {
                if self.0.get(pane).map(String::as_str) == Some(account) {
                    return false;
                }
                self.0.insert(pane.to_string(), account.to_string());
                true
            }
        }
    }

    pub fn account(&self, pane: &str) -> Option<&str> {
        self.0.get(pane).map(String::as_str)
    }

    /// "Running" is having launched an agent.
    pub fn is_running(&self, pane: &str) -> bool {
        self.0.contains_key(pane)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn account(id: &str, is_main: bool) -> ClaudeAccount {
        ClaudeAccount {
            id: id.into(),
            label: id.into(),
            is_main,
            logged_in: true,
            email: None,
            plan: None,
        }
    }

    fn accounts() -> Vec<ClaudeAccount> {
        vec![account("main", true), account("pro", false), account("perso", false)]
    }

    fn resolved(id: &str, source: AccountSource, missing: Option<&str>) -> ResolvedAccount {
        ResolvedAccount {
            id: id.into(),
            source,
            missing: missing.map(str::to_string),
        }
    }

    #[test]
    fn a_non_empty_string_is_a_choice() {
        assert_eq!(read_account_choice(Some("pro")).as_deref(), Some("pro"));
        assert_eq!(read_account_choice(Some("  pro ")).as_deref(), Some("pro"));
    }

    #[test]
    fn anything_malformed_is_no_choice() {
        assert_eq!(read_account_choice(None), None);
        assert_eq!(read_account_choice(Some("")), None);
        assert_eq!(read_account_choice(Some("   ")), None);
        for bad in [json!(null), json!(3), json!(true), json!({}), json!([]), json!(["pro"])] {
            assert_eq!(read_account_choice_value(&bad), None, "{bad}");
        }
        assert_eq!(
            read_account_choice_value(&json!(" pro ")).as_deref(),
            Some("pro")
        );
    }

    #[test]
    fn falls_to_main_with_nothing_chosen() {
        assert_eq!(
            resolve_account(&accounts(), None, None, None),
            resolved("main", AccountSource::Main, None)
        );
    }

    #[test]
    fn prefers_pane_over_workspace_over_default() {
        let all = accounts();
        assert_eq!(
            resolve_account(&all, Some("perso"), None, None),
            resolved("perso", AccountSource::Default, None)
        );
        assert_eq!(
            resolve_account(&all, Some("perso"), Some("pro"), None),
            resolved("pro", AccountSource::Workspace, None)
        );
        assert_eq!(
            resolve_account(&all, Some("perso"), Some("pro"), Some("main")),
            resolved("main", AccountSource::Pane, None)
        );
    }

    #[test]
    fn falls_back_from_an_unknown_id_and_says_which() {
        let all = accounts();
        assert_eq!(
            resolve_account(&all, None, Some("gone"), None),
            resolved("main", AccountSource::Main, Some("gone"))
        );
        assert_eq!(
            resolve_account(&all, Some("perso"), Some("gone"), None),
            resolved("perso", AccountSource::Default, Some("gone"))
        );
    }

    #[test]
    fn knows_main_even_when_the_list_does_not() {
        assert_eq!(
            resolve_account(&[], Some("main"), None, None),
            resolved("main", AccountSource::Default, None)
        );
        assert_eq!(
            resolve_account(&[], None, Some(" main "), None),
            resolved("main", AccountSource::Workspace, None)
        );
    }

    #[test]
    fn takes_malformed_values_at_every_level_as_no_choice() {
        let all = accounts();
        for junk in [None, Some(""), Some("   ")] {
            assert_eq!(
                resolve_account(&all, junk, junk, junk),
                resolved("main", AccountSource::Main, None)
            );
            assert_eq!(
                resolve_account(&all, Some("perso"), junk, junk),
                resolved("perso", AccountSource::Default, None)
            );
        }
    }

    #[test]
    fn a_hand_edited_state_file_with_values_of_the_wrong_type_reads_as_no_choice() {
        let state = crate::state::read_state(json!({
            "settings": { "claudeAccount": 42 },
            "workspaces": [{ "id": "w", "claudeAccount": ["pro"], "panes": [
                { "id": "p", "claudeAccount": {} }
            ]}]
        }));
        let ws = &state.workspaces[0];
        // Every level that held junk reads as unset; the settings' default
        // (`"main"`) is what is left.
        assert_eq!(ws.claude_account, None);
        assert_eq!(ws.panes[0].claude_account, None);
        assert_eq!(
            resolve_account(
                &accounts(),
                state.settings.claude_account.as_deref(),
                ws.claude_account.as_deref(),
                ws.panes.first().and_then(|pane| pane.claude_account.as_deref()),
            ),
            resolved("main", AccountSource::Default, None)
        );
    }

    #[test]
    fn trims_a_choice_before_looking_it_up() {
        assert_eq!(
            resolve_account(&accounts(), None, None, Some(" pro ")),
            resolved("pro", AccountSource::Pane, None)
        );
    }

    #[test]
    fn reports_only_the_first_gone_account_and_keeps_falling() {
        assert_eq!(
            resolve_account(
                &accounts(),
                Some("old-default"),
                Some("old-ws"),
                Some("old-pane")
            ),
            resolved("main", AccountSource::Main, Some("old-pane"))
        );
    }

    #[test]
    fn does_not_ask_for_the_accounts_when_only_main_is_chosen() {
        let calls = std::cell::Cell::new(0);
        let list = || {
            calls.set(calls.get() + 1);
            Ok(accounts())
        };
        assert_eq!(
            launch_account_with(list, Some("main"), None, None),
            resolved("main", AccountSource::Default, None)
        );
        assert_eq!(
            launch_account_with(list, None, None, None),
            resolved("main", AccountSource::Main, None)
        );
        assert_eq!(
            launch_account_with(list, None, None, Some("")),
            resolved("main", AccountSource::Main, None)
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn resolves_against_the_accounts_when_one_is_named() {
        let calls = std::cell::Cell::new(0);
        let list = || {
            calls.set(calls.get() + 1);
            Ok(accounts())
        };
        assert_eq!(
            launch_account_with(list, Some("main"), Some("pro"), None),
            resolved("pro", AccountSource::Workspace, None)
        );
        assert_eq!(
            launch_account_with(list, None, Some("pro"), Some("gone")),
            resolved("pro", AccountSource::Workspace, Some("gone"))
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn starts_on_main_when_the_accounts_cannot_be_listed_and_says_which_was_asked() {
        let list = || Err("backend down".to_string());
        assert_eq!(
            launch_account_with(list, Some("main"), Some("pro"), None),
            resolved("main", AccountSource::Default, Some("pro"))
        );
    }

    #[test]
    fn launched_accounts_remember_the_account_each_pane_started_on_until_it_goes() {
        let mut launched = Launched::default();
        assert_eq!(launched.account("pane-a"), None);
        assert!(launched.note("pane-a", Some("pro")));
        assert!(launched.note("pane-b", Some("main")));
        assert_eq!(launched.account("pane-a"), Some("pro"));
        assert_eq!(launched.account("pane-b"), Some("main"));
        assert!(launched.is_running("pane-a"));
        // The same again changes nothing, and tells nobody.
        assert!(!launched.note("pane-a", Some("pro")));
        assert!(launched.note("pane-a", None));
        assert_eq!(launched.account("pane-a"), None);
        assert!(!launched.is_running("pane-a"));
        assert!(!launched.note("pane-a", None));
    }

    #[test]
    fn names_account_reads_a_hand_edited_choice_defensively() {
        assert!(names_account(Some(" perso "), "perso"));
        assert!(!names_account(None, "perso"));
        assert!(!names_account(Some(""), "perso"));
    }
}
