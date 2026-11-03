//! The entries of a menu that choose a Claude account
//! (`components/accounts/accountMenuItems.ts` and `PaneAccountItems.tsx`):
//! « Default (Perso) » first, then one entry per account, the current choice
//! checked. They sit at the end of the navigator's workspace menu and of a
//! pane's « … » menu, which draw them as they draw their own lines.

use hangar_i18n::{Key, Translator};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::MenuEntry;
use crate::state::types::AgentId;
use crate::state::{PaneId, WsId};

use super::model::{ClaudeAccount, read_account_choice};
use super::switch::default_label;
use super::{Msg, live};

/// One line, as plain data: what it says, whether it is the current choice,
/// and what picking it chooses (`None`: follow the default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMenuItem {
    pub key: String,
    pub label: String,
    pub checked: bool,
    pub select: Option<String>,
}

/// `default` names the account « Default » stands for, `value` is the own
/// account of what the menu is for (a workspace, a pane); `None` when it
/// follows the default. Labels only, never emails.
pub fn account_menu_items(
    t: Translator,
    accounts: &[ClaudeAccount],
    default: Option<&str>,
    value: Option<&str>,
) -> Vec<AccountMenuItem> {
    let own = read_account_choice(value);
    let known_own = own
        .as_deref()
        .filter(|own| accounts.iter().any(|account| account.id == *own));
    let template = t.t(Key::AccountsPickerDefault);
    let unknown = t.t(Key::AccountsPickerUnknown);
    let mut items = vec![AccountMenuItem {
        key: "default".into(),
        label: default_label(accounts, default, &template, &unknown),
        checked: known_own.is_none(),
        select: None,
    }];
    items.extend(accounts.iter().map(|account| AccountMenuItem {
        key: account.id.clone(),
        label: account.label.clone(),
        checked: known_own == Some(account.id.as_str()),
        select: Some(account.id.clone()),
    }));
    items
}

fn radio(item: AccountMenuItem, pick: impl Fn(Option<String>) -> Message) -> MenuEntry {
    MenuEntry::Item {
        label: item.label,
        icon: None,
        danger: false,
        checked: Some(item.checked),
        disabled: None,
        on_select: pick(item.select),
    }
}

/// The account lines of the navigator's menu (`pane` `None`: the workspace's
/// own choice) or of a pane's menu (that pane's own choice, over its
/// workspace's). Nothing before the accounts are known, and nothing for a
/// pane that does not run Claude Code, or that was opened to log in.
pub fn menu_entries(ctx: Ctx<'_>, ws: &WsId, pane: Option<&PaneId>) -> Vec<MenuEntry> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return Vec::new();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return Vec::new();
    };
    let settings = ctx.model.settings().claude_account.as_deref();
    match pane {
        None => {
            // The same as the toolbar's click: this workspace's own account.
            let items = account_menu_items(
                ctx.i18n,
                accounts,
                settings,
                workspace.claude_account.as_deref(),
            );
            let mut entries = vec![
                MenuEntry::Separator,
                MenuEntry::Heading(ctx.t(Key::AccountsMenuTitle).into_owned()),
            ];
            entries.extend(items.into_iter().map(|item| {
                radio(item, |id| {
                    Message::Accounts(Msg::SetWorkspace {
                        ws: ws.clone(),
                        id,
                        restart: false,
                    })
                })
            }));
            entries
        }
        Some(pane_id) => {
            let Some(pane) = workspace.panes.iter().find(|pane| pane.id == *pane_id) else {
                return Vec::new();
            };
            if pane.agent != AgentId::Claude || pane.login == Some(true) {
                return Vec::new();
            }
            // « Default » here means the pane follows its workspace, so the
            // label names the account the workspace is on.
            let follows = workspace.claude_account.as_deref().or(settings);
            let items = account_menu_items(
                ctx.i18n,
                accounts,
                follows,
                pane.claude_account.as_deref(),
            );
            let mut entries = vec![
                MenuEntry::Separator,
                MenuEntry::Heading(ctx.t(Key::AccountsPaneMenuHeading).into_owned()),
            ];
            entries.extend(items.into_iter().map(|item| {
                radio(item, |id| {
                    Message::Accounts(Msg::SetPane {
                        ws: ws.clone(),
                        pane: pane_id.clone(),
                        id,
                    })
                })
            }));
            entries
        }
    }
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;

    use super::*;

    fn accounts() -> Vec<ClaudeAccount> {
        vec![
            ClaudeAccount {
                id: "main".into(),
                label: "Principal".into(),
                is_main: true,
                logged_in: true,
                email: None,
                plan: None,
            },
            ClaudeAccount {
                id: "perso".into(),
                label: "Perso".into(),
                is_main: false,
                logged_in: true,
                email: Some("me@example.com".into()),
                plan: None,
            },
        ]
    }

    const T: Translator = Translator::new(Locale::En);

    #[test]
    fn lists_the_default_first_and_checks_it_when_the_workspace_has_no_account() {
        let items = account_menu_items(T, &accounts(), Some("perso"), None);
        let shown: Vec<_> = items
            .iter()
            .map(|item| (item.label.as_str(), item.checked))
            .collect();
        assert_eq!(
            shown,
            [("Default (Perso)", true), ("Principal", false), ("Perso", false)]
        );
    }

    #[test]
    fn checks_the_workspaces_own_account_and_reports_the_choice() {
        let items = account_menu_items(T, &accounts(), None, Some("perso"));
        let checked: Vec<_> = items.iter().map(|item| item.checked).collect();
        assert_eq!(checked, [false, false, true]);
        let picked: Vec<_> = items.iter().take(2).map(|item| item.select.clone()).collect();
        assert_eq!(picked, [None, Some("main".to_string())]);
    }

    #[test]
    fn treats_a_dead_id_as_the_default_and_never_prints_an_email() {
        let items = account_menu_items(T, &accounts(), None, Some("gone"));
        assert!(items[0].checked);
        assert!(items.iter().all(|item| !item.label.contains('@')));
    }

    #[test]
    fn says_the_same_in_french() {
        let items = account_menu_items(
            Translator::new(Locale::Fr),
            &accounts(),
            Some("perso"),
            None,
        );
        assert_eq!(items[0].label, "Par défaut (Perso)");
    }
}
