//! The « Comptes Claude » page of the settings: the list of accounts and what
//! can be done with each (`AccountsSettingsPage.tsx`,
//! `ConnectedAccountsSettings.tsx`).
//!
//! The login itself is the person's, in the browser: a visible Claude Code
//! pane opens on the profile and runs the login there (`open_login_pane`).

use hangar_i18n::{Key, Translator, args};
use hangar_theme::{style, tokens};
use hangar_ui::{Icon, badge, button as ui_button, form, icon, input, text as typography};
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::services::Services;
use crate::state::types::AgentId;
use crate::state::{Model, Pane, Patch, Settings, StoreAction, Workspace};

use super::model::{
    AccountId, ClaudeAccount, MAIN_ACCOUNT, names_account, resolve_account,
};
use super::{Msg, live};

/// What the page itself remembers: the form it has open, the command it
/// waits for, the last failure.
#[derive(Debug, Default)]
pub struct Page {
    /// The account being renamed, and the name as typed so far.
    renaming: Option<(AccountId, String)>,
    /// The account whose removal is being asked.
    removing: Option<AccountId>,
    /// The name of the account to add.
    name: String,
    /// The add form was sent once: its problem shows from then on.
    touched: bool,
    /// A command runs: everything is disabled until it ends.
    busy: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelProblem {
    NameEmpty,
    NameTaken,
}

/// Why `label` cannot name an account, or `None`. `except` is the one being
/// renamed.
pub fn label_problem(
    accounts: &[ClaudeAccount],
    label: &str,
    except: Option<&str>,
) -> Option<LabelProblem> {
    let wanted = label.trim().to_lowercase();
    if wanted.is_empty() {
        return Some(LabelProblem::NameEmpty);
    }
    let taken = accounts.iter().any(|account| {
        Some(account.id.as_str()) != except && account.label.trim().to_lowercase() == wanted
    });
    taken.then_some(LabelProblem::NameTaken)
}

fn problem_text(ctx: Ctx<'_>, problem: LabelProblem) -> String {
    match problem {
        LabelProblem::NameEmpty => ctx.t(Key::AccountsSettingsNameEmpty).into_owned(),
        LabelProblem::NameTaken => ctx.t(Key::AccountsSettingsNameTaken).into_owned(),
    }
}

// ── the updates ───────────────────────────────────────────────────────

fn failed(t: Translator, detail: &str) -> String {
    t.t_with(Key::AccountsSettingsFailed, &args!("detail" => detail))
        .into_owned()
}

/// A visible Claude Code pane on the profile of `id`, which runs the login
/// there. Without an open workspace, or without room, it says so.
fn open_login_pane(page: &mut Page, m: &mut Model, t: Translator, id: &str) {
    let no_pane = t.t(Key::AccountsSettingsNoPane).into_owned();
    let Some(ws) = m.active_workspace().map(|ws| ws.id.clone()) else {
        page.error = Some(no_pane);
        return;
    };
    let account = id.to_string();
    // The account and the login mark are set before anything spawns the
    // pane, so it launches on the right profile.
    let made = m.apply(StoreAction::AddPane {
        ws,
        agent: Some(AgentId::Claude),
        near: None,
        dir: None,
        cwd: None,
        name: None,
        init: Some(Patch::new(move |pane: &mut Pane| {
            pane.claude_account = Some(account.clone());
            pane.login = Some(true);
        })),
    });
    if made.pane.is_none() {
        page.error = Some(no_pane);
    }
}

/// What names a removed account, so that it can be cleared: the workspaces
/// with it as their own, the panes with it as theirs, and whether the default
/// was it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Forgotten {
    workspaces: Vec<String>,
    panes: Vec<(String, String)>,
    default: bool,
}

fn who_names(workspaces: &[Workspace], default: Option<&str>, id: &str) -> Forgotten {
    let mut forgotten = Forgotten {
        default: names_account(default, id),
        ..Forgotten::default()
    };
    for ws in workspaces {
        if names_account(ws.claude_account.as_deref(), id) {
            forgotten.workspaces.push(ws.id.clone());
        }
        for pane in &ws.panes {
            if names_account(pane.claude_account.as_deref(), id) {
                forgotten.panes.push((ws.id.clone(), pane.id.clone()));
            }
        }
    }
    forgotten
}

/// Whatever named a removed account falls back to the level above; say so in
/// the file too. A default that named it goes back to main.
fn forget_account(m: &mut Model, id: &str) {
    let forgotten = who_names(&m.state().workspaces, m.settings().claude_account.as_deref(), id);
    for ws in forgotten.workspaces {
        m.apply(StoreAction::UpdateWorkspace(
            ws,
            Patch::new(|ws: &mut Workspace| ws.claude_account = None),
        ));
    }
    for (ws, pane) in forgotten.panes {
        m.apply(StoreAction::UpdatePane {
            ws,
            pane,
            patch: Patch::new(|pane: &mut Pane| pane.claude_account = None),
        });
    }
    if forgotten.default {
        m.apply(StoreAction::UpdateSettings(Patch::new(
            |settings: &mut Settings| settings.claude_account = Some(MAIN_ACCOUNT.to_string()),
        )));
    }
}

pub fn update(page: &mut Page, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let t = Translator::new(m.locale());
    let accounts: Vec<ClaudeAccount> = m.live.accounts.accounts.clone().unwrap_or_default();
    match msg {
        Msg::RenameStart(id) => {
            let label = accounts
                .iter()
                .find(|account| account.id == id)
                .map(|account| account.label.clone())
                .unwrap_or_default();
            page.renaming = Some((id, label));
            Task::none()
        }
        Msg::RenameInput(value) => {
            if let Some((_, current)) = &mut page.renaming {
                *current = value;
            }
            Task::none()
        }
        Msg::RenameCancel => {
            page.renaming = None;
            Task::none()
        }
        Msg::RenameSubmit => {
            let Some((id, label)) = page.renaming.clone() else {
                return Task::none();
            };
            if label_problem(&accounts, &label, Some(&id)).is_some() {
                return Task::none();
            }
            page.busy = true;
            page.error = None;
            let host = sv.core().host().clone();
            let label = label.trim().to_string();
            Task::perform(
                run_blocking(move || {
                    hangar_core::accounts::account_rename(&host, id, label).map(|_| ())
                }),
                |result| Message::Accounts(Msg::Renamed(result)),
            )
        }
        Msg::Renamed(result) => {
            page.busy = false;
            match result {
                Ok(()) => {
                    page.renaming = None;
                    super::refresh()
                }
                // The error tells; the field stays open.
                Err(err) => {
                    page.error = Some(failed(t, &err));
                    Task::none()
                }
            }
        }

        Msg::RemoveAsk(id) => {
            page.removing = Some(id);
            Task::none()
        }
        Msg::RemoveCancel => {
            page.removing = None;
            Task::none()
        }
        Msg::RemoveConfirm(id) => {
            if id == MAIN_ACCOUNT {
                return Task::none();
            }
            page.busy = true;
            page.error = None;
            let host = sv.core().host().clone();
            let pty = sv.core().pty().clone();
            let removed = id.clone();
            Task::perform(
                run_blocking(move || hangar_core::accounts::account_remove(&host, &pty, id)),
                move |result| Message::Accounts(Msg::Removed(removed.clone(), result)),
            )
        }
        Msg::Removed(id, result) => {
            page.busy = false;
            match result {
                Ok(()) => {
                    forget_account(m, &id);
                    page.removing = None;
                    super::refresh()
                }
                // The error tells; the question stays.
                Err(err) => {
                    page.error = Some(failed(t, &err));
                    Task::none()
                }
            }
        }

        Msg::MakeDefault(id) => {
            m.apply(StoreAction::UpdateSettings(Patch::new(
                move |settings: &mut Settings| settings.claude_account = Some(id.clone()),
            )));
            Task::none()
        }
        Msg::LogIn(id) => {
            open_login_pane(page, m, t, &id);
            Task::none()
        }

        Msg::AddInput(value) => {
            page.name = value;
            Task::none()
        }
        Msg::AddSubmit => {
            page.touched = true;
            if label_problem(&accounts, &page.name, None).is_some() {
                return Task::none();
            }
            page.busy = true;
            page.error = None;
            let host = sv.core().host().clone();
            let label = page.name.trim().to_string();
            Task::perform(
                run_blocking(move || {
                    hangar_core::accounts::account_add(&host, label).map(|added| added.account.id)
                }),
                |result| Message::Accounts(Msg::Added(result)),
            )
        }
        Msg::Added(result) => {
            page.busy = false;
            match result {
                Ok(id) => {
                    page.name.clear();
                    page.touched = false;
                    open_login_pane(page, m, t, &id);
                    super::refresh()
                }
                // The name stays, to retry.
                Err(err) => {
                    page.error = Some(failed(t, &err));
                    Task::none()
                }
            }
        }
        // Not this page's.
        _ => Task::none(),
    }
}

/// A command of the backend, which reads and writes the accounts' files, off
/// the UI thread.
async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| format!("the task stopped: {err}"))?
}

// ── the view ──────────────────────────────────────────────────────────

fn press(msg: Msg) -> Message {
    Message::Accounts(msg)
}

pub fn view<'a>(page: &'a Page, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return Space::new().width(0).height(0).into();
    };
    let default = ctx.model.settings().claude_account.as_deref();
    let default_id = resolve_account(accounts, default, None, None).id;

    let mut body = column![].spacing(tokens::space::XL);

    if let Some(support) = live(ctx).support.as_ref().filter(|s| !s.isolated_logins) {
        let mut note = column![
            typography::body(t, ctx.t(Key::AccountsSettingsUnsupportedTitle))
                .font(hangar_theme::fonts::ui_semibold()),
            typography::hint(t, ctx.t(Key::AccountsSettingsUnsupportedBody)),
        ]
        .spacing(2);
        if let Some(extra) = &support.note {
            note = note.push(typography::hint(t, extra.as_str()));
        }
        body = body.push(
            container(
                row![icon(Icon::Alert, tokens::size::ICON).color(t.colors.danger), note]
                    .spacing(tokens::space::LG),
            )
            .padding(tokens::space::XL)
            .width(Length::Fill)
            .style(move |_: &iced::Theme| iced::widget::container::Style {
                background: Some(t.colors.danger_soft.into()),
                border: iced::Border {
                    color: t.colors.danger,
                    width: tokens::BORDER_WIDTH,
                    radius: tokens::radius::MD.into(),
                },
                ..iced::widget::container::Style::default()
            }),
        );
    }
    if let Some(error) = &page.error {
        body = body.push(typography::danger(t, error.as_str()));
    }

    let rows: Vec<Element<'a, Message>> = accounts
        .iter()
        .map(|account| account_row(page, ctx, accounts, account, &default_id))
        .collect();
    body = body.push(form::group(t, rows));
    body = body.push(add_form(page, ctx, accounts));

    form::section(t, ctx.t(Key::AccountsSettingsTitle))
        .hint(ctx.t(Key::AccountsSettingsIntro))
        .bare(body)
        .into()
}

fn account_row<'a>(
    page: &'a Page,
    ctx: Ctx<'a>,
    accounts: &'a [ClaudeAccount],
    account: &'a ClaudeAccount,
    default_id: &str,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let is_default = account.id == default_id;
    let users: Vec<&str> = ctx
        .model
        .state()
        .workspaces
        .iter()
        .filter(|ws| {
            resolve_account(
                accounts,
                ctx.model.settings().claude_account.as_deref(),
                ws.claude_account.as_deref(),
                None,
            )
            .id == account.id
        })
        .map(|ws| ws.name.as_str())
        .collect();

    let mut head = row![typography::body(t, account.label.as_str())
        .font(hangar_theme::fonts::ui_semibold())]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    if is_default {
        head = head.push(badge::tag(t, ctx.t(Key::AccountsSettingsDefault)));
    }
    head = head.push(if account.logged_in {
        badge::badge(t, ctx.t(Key::AccountsSettingsLoggedIn), badge::Tone::Success)
    } else {
        badge::badge(t, ctx.t(Key::AccountsSettingsLoggedOut), badge::Tone::Neutral)
    });

    let mut lines = column![head].spacing(tokens::space::SM);
    let identity: Vec<&str> = [account.email.as_deref(), account.plan.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    if !identity.is_empty() {
        lines = lines.push(typography::hint(t, identity.join(" · ")));
    }
    lines = lines.push(typography::hint(
        t,
        if users.is_empty() {
            ctx.t(Key::AccountsSettingsUsedByNone).into_owned()
        } else {
            ctx.i18n
                .t_with(
                    Key::AccountsSettingsUsedBy,
                    &args!("names" => users.join(", ")),
                )
                .into_owned()
        },
    ));

    let renaming = page
        .renaming
        .as_ref()
        .filter(|(id, _)| *id == account.id)
        .map(|(_, value)| value.as_str());
    let controls: Element<'a, Message> = if let Some(value) = renaming {
        rename_form(page, ctx, accounts, account, value)
    } else if page.removing.as_deref() == Some(account.id.as_str()) {
        remove_confirm(page, ctx, account)
    } else {
        actions(page, ctx, account, is_default)
    };
    lines = lines.push(controls);
    container(lines)
        .padding(Padding::from([tokens::space::XL, tokens::space::XL]))
        .width(Length::Fill)
        .into()
}

fn actions<'a>(
    page: &'a Page,
    ctx: Ctx<'a>,
    account: &'a ClaudeAccount,
    is_default: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let idle = (!page.busy).then_some(());
    let mut actions = row![].spacing(tokens::space::MD).align_y(Alignment::Center);
    if !account.logged_in {
        actions = actions.push(
            ui_button::primary(t, ctx.t(Key::AccountsSettingsLogIn))
                .small()
                .on_press_maybe(idle.map(|()| press(Msg::LogIn(account.id.clone())))),
        );
    }
    if !is_default {
        actions = actions.push(
            ui_button::secondary(t, ctx.t(Key::AccountsSettingsSetDefault))
                .small()
                .on_press_maybe(idle.map(|()| press(Msg::MakeDefault(account.id.clone())))),
        );
    }
    actions = actions.push(
        ui_button::ghost(t, ctx.t(Key::AccountsSettingsRename))
            .small()
            .on_press_maybe(idle.map(|()| press(Msg::RenameStart(account.id.clone())))),
    );
    if account.is_main {
        actions = actions.push(typography::hint(t, ctx.t(Key::AccountsSettingsMainFixed)));
    } else {
        actions = actions.push(
            ui_button::ghost(t, ctx.t(Key::AccountsSettingsRemove))
                .small()
                .on_press_maybe(idle.map(|()| press(Msg::RemoveAsk(account.id.clone())))),
        );
    }
    actions.into()
}

fn rename_form<'a>(
    page: &'a Page,
    ctx: Ctx<'a>,
    accounts: &'a [ClaudeAccount],
    account: &'a ClaudeAccount,
    value: &'a str,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let problem = label_problem(accounts, value, Some(&account.id));
    let field = input::text_field(t, value)
        .on_input(|value| press(Msg::RenameInput(value)))
        .on_submit(press(Msg::RenameSubmit))
        .error(problem.map(|problem| problem_text(ctx, problem)));
    let save = ui_button::primary(t, ctx.t(Key::AccountsSettingsSave))
        .small()
        .on_press_maybe((!page.busy && problem.is_none()).then(|| press(Msg::RenameSubmit)));
    let cancel = ui_button::ghost(t, ctx.t(Key::AccountsSettingsCancel))
        .small()
        .on_press(press(Msg::RenameCancel));
    column![
        field,
        row![save, cancel].spacing(tokens::space::MD),
    ]
    .spacing(tokens::space::MD)
    .into()
}

fn remove_confirm<'a>(
    page: &'a Page,
    ctx: Ctx<'a>,
    account: &'a ClaudeAccount,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let ask = ctx
        .i18n
        .t_with(
            Key::AccountsSettingsRemoveAsk,
            &args!("name" => account.label.as_str()),
        )
        .into_owned();
    container(
        column![
            typography::body(t, ask).font(hangar_theme::fonts::ui_semibold()),
            typography::hint(t, ctx.t(Key::AccountsSettingsRemoveBody)),
            row![
                ui_button::danger(t, ctx.t(Key::AccountsSettingsRemoveConfirm))
                    .small()
                    .on_press_maybe(
                        (!page.busy).then(|| press(Msg::RemoveConfirm(account.id.clone())))
                    ),
                ui_button::ghost(t, ctx.t(Key::AccountsSettingsCancel))
                    .small()
                    .on_press(press(Msg::RemoveCancel)),
            ]
            .spacing(tokens::space::MD),
        ]
        .spacing(tokens::space::MD),
    )
    .padding(tokens::space::XL)
    .width(Length::Fill)
    .style(style::container::well(t))
    .into()
}

fn add_form<'a>(
    page: &'a Page,
    ctx: Ctx<'a>,
    accounts: &'a [ClaudeAccount],
) -> Element<'a, Message> {
    let t = ctx.theme;
    let problem = (page.touched)
        .then(|| label_problem(accounts, &page.name, None))
        .flatten();
    let placeholder = static_text(ctx.t(Key::AccountsSettingsAddPlaceholder));
    let field = input::text_field(t, &page.name)
        .placeholder(placeholder)
        .on_input(|value| press(Msg::AddInput(value)))
        .on_submit(press(Msg::AddSubmit));
    let submit = ui_button::primary(t, ctx.t(Key::AccountsSettingsAdd))
        .on_press_maybe((!page.busy).then(|| press(Msg::AddSubmit)));
    let note = match problem {
        Some(problem) => typography::danger(t, problem_text(ctx, problem)),
        None => typography::hint(t, ctx.t(Key::AccountsSettingsAddHint)),
    };
    form::group(
        t,
        vec![
            container(
                column![
                    typography::body(t, ctx.t(Key::AccountsSettingsAddTitle))
                        .font(hangar_theme::fonts::ui_semibold()),
                    row![field, submit]
                        .spacing(tokens::space::MD)
                        .align_y(Alignment::Center),
                    note,
                ]
                .spacing(tokens::space::MD),
            )
            .padding(tokens::space::XL)
            .width(Length::Fill)
            .into(),
        ],
    )
}

/// A dictionary string that takes no argument: always borrowed from the
/// dictionary, which a field's placeholder needs to be.
fn static_text(text: std::borrow::Cow<'static, str>) -> &'static str {
    match text {
        std::borrow::Cow::Borrowed(text) => text,
        std::borrow::Cow::Owned(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, label: &str) -> ClaudeAccount {
        ClaudeAccount {
            id: id.into(),
            label: label.into(),
            is_main: id == "main",
            logged_in: true,
            email: None,
            plan: None,
        }
    }

    fn accounts() -> Vec<ClaudeAccount> {
        vec![account("main", "Principal"), account("perso", "Perso")]
    }

    #[test]
    fn a_name_is_refused_when_empty_or_when_another_account_has_it() {
        let all = accounts();
        assert_eq!(label_problem(&all, "", None), Some(LabelProblem::NameEmpty));
        assert_eq!(label_problem(&all, "  ", None), Some(LabelProblem::NameEmpty));
        assert_eq!(label_problem(&all, "Travail", None), None);
        assert_eq!(
            label_problem(&all, " perso ", None),
            Some(LabelProblem::NameTaken)
        );
        assert_eq!(
            label_problem(&all, "PRINCIPAL", None),
            Some(LabelProblem::NameTaken)
        );
    }

    #[test]
    fn the_account_being_renamed_may_keep_its_own_name() {
        let all = accounts();
        assert_eq!(label_problem(&all, "Perso", Some("perso")), None);
        assert_eq!(
            label_problem(&all, "Principal", Some("perso")),
            Some(LabelProblem::NameTaken)
        );
    }

    #[test]
    fn a_failure_is_shown_with_its_detail_in_the_language() {
        let en = failed(Translator::new(hangar_i18n::Locale::En), "disk full");
        assert_eq!(en, "Could not do that: disk full");
        let fr = failed(Translator::new(hangar_i18n::Locale::Fr), "disque plein");
        assert!(fr.starts_with("Impossible\u{202f}: "), "{fr}");
        assert!(fr.ends_with("disque plein"));
    }

    #[test]
    fn a_removed_account_is_forgotten_by_whatever_named_it() {
        let workspace = |id: &str, own: Option<&str>, panes: &[(&str, Option<&str>)]| Workspace {
            id: id.into(),
            claude_account: own.map(str::to_string),
            panes: panes
                .iter()
                .map(|(pane, account)| Pane {
                    id: (*pane).into(),
                    claude_account: account.map(str::to_string),
                    ..Pane::default()
                })
                .collect(),
            ..Workspace::default()
        };
        let all = [
            workspace("w1", Some("perso"), &[("a", None), ("b", Some(" perso "))]),
            workspace("w2", None, &[("c", Some("pro")), ("d", Some("perso"))]),
            workspace("w3", Some("pro"), &[]),
        ];
        assert_eq!(
            who_names(&all, Some("perso"), "perso"),
            Forgotten {
                workspaces: vec!["w1".into()],
                panes: vec![("w1".into(), "b".into()), ("w2".into(), "d".into())],
                default: true,
            }
        );
        assert_eq!(
            who_names(&all, Some("main"), "pro"),
            Forgotten {
                workspaces: vec!["w3".into()],
                panes: vec![("w2".into(), "c".into())],
                default: false,
            }
        );
        assert_eq!(who_names(&all, None, "gone"), Forgotten::default());
    }
}
