//! The form behind « From a Git repository » (the form half of
//! `lib/workspaceFromGit.ts`): what state it is in, what its button says, and
//! what a key does. The dialog draws it; this decides.

use hangar_i18n::Key;

use crate::git_flow::clone::{ParseResult, clean_repo_input};

/// What the « From a Git repository » form can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromGitStatus {
    /// Nothing typed yet.
    Empty,
    /// The address is being read.
    Checking,
    /// The address is refused: the reason shows under the field.
    Refused,
    /// The address is good, but there is no folder to clone into.
    NoFolder,
    Ready,
    /// A clone, or the steps after it, is running.
    Running,
}

pub struct FromGitForm<'a> {
    /// The field as typed.
    pub input: &'a str,
    /// What reading `input` said; `None` while it has not answered.
    pub parse: Option<&'a ParseResult>,
    /// The folder it will be cloned into, once resolved.
    pub parent: Option<&'a str>,
    pub running: bool,
}

pub fn from_git_status(form: &FromGitForm) -> FromGitStatus {
    if form.running {
        return FromGitStatus::Running;
    }
    if clean_repo_input(form.input).is_empty() {
        return FromGitStatus::Empty;
    }
    match form.parse {
        None => FromGitStatus::Checking,
        Some(ParseResult::Refused { .. }) => FromGitStatus::Refused,
        Some(ParseResult::Ok { .. }) => {
            if form.parent.is_some_and(|parent| !parent.trim().is_empty()) {
                FromGitStatus::Ready
            } else {
                FromGitStatus::NoFolder
            }
        }
    }
}

/// The submit button: only a form that is ready can be sent. Its label (a
/// dictionary key) and whether it is disabled.
pub fn from_git_button(status: FromGitStatus) -> (Key, bool) {
    (
        if status == FromGitStatus::Running {
            Key::CreateGitRunning
        } else {
            Key::CreateGitSubmit
        },
        status != FromGitStatus::Ready,
    )
}

/// What stays disabled while a clone runs: everything that would change the
/// request.
pub fn fields_disabled(status: FromGitStatus) -> bool {
    status == FromGitStatus::Running
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKey {
    Start,
    Close,
}

/// What a key does in the form: Enter starts it when it is ready; Escape
/// closes the dialog, and does nothing while something runs (Cancel is the
/// way out).
pub fn from_git_key(key: &str, status: FromGitStatus) -> Option<FormKey> {
    match key {
        "Enter" => (status == FromGitStatus::Ready).then_some(FormKey::Start),
        "Escape" => (status != FromGitStatus::Running).then_some(FormKey::Close),
        _ => None,
    }
}

/// A failure on one line: the first line that says anything, spaces
/// collapsed, cut at `max` characters.
pub fn one_line(reason: &str, max: usize) -> String {
    let line = reason
        .lines()
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|part| !part.is_empty());
    let Some(line) = line else {
        return String::new();
    };
    if line.chars().count() > max {
        let cut: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use crate::git_flow::clone::RepoRef;
    use crate::git_flow::paths::{clone_destination, resolve_parent};

    use super::*;

    fn ok() -> ParseResult {
        ParseResult::Ok {
            repo: RepoRef {
                url: "https://github.com/acme/shop.git".into(),
                host: "github.com".into(),
                owner: "acme".into(),
                name: "shop".into(),
            },
        }
    }

    fn refused() -> ParseResult {
        ParseResult::Refused {
            reason: "the address is not valid".into(),
        }
    }

    fn status(input: &str, parse: Option<&ParseResult>, parent: Option<&str>, running: bool) -> FromGitStatus {
        from_git_status(&FromGitForm {
            input,
            parse,
            parent,
            running,
        })
    }

    const PARENT: Option<&str> = Some("D:\\Projects");

    #[test]
    fn reads_the_forms_status() {
        let ok = ok();
        assert_eq!(status("  \n", Some(&ok), PARENT, false), FromGitStatus::Empty);
        assert_eq!(status("acme/shop", None, PARENT, false), FromGitStatus::Checking);
        assert_eq!(
            status("acme/shop", Some(&refused()), PARENT, false),
            FromGitStatus::Refused
        );
        assert_eq!(status("acme/shop", Some(&ok), None, false), FromGitStatus::NoFolder);
        assert_eq!(
            status("acme/shop", Some(&ok), Some("  "), false),
            FromGitStatus::NoFolder
        );
        assert_eq!(status("acme/shop", Some(&ok), PARENT, false), FromGitStatus::Ready);
        assert_eq!(status("", Some(&ok), PARENT, true), FromGitStatus::Running);
    }

    #[test]
    fn says_what_the_button_says_and_enables_it_only_when_ready() {
        assert_eq!(
            from_git_button(FromGitStatus::Ready),
            (Key::CreateGitSubmit, false)
        );
        assert_eq!(
            from_git_button(FromGitStatus::Running),
            (Key::CreateGitRunning, true)
        );
        for status in [
            FromGitStatus::Empty,
            FromGitStatus::Checking,
            FromGitStatus::Refused,
            FromGitStatus::NoFolder,
        ] {
            assert_eq!(from_git_button(status), (Key::CreateGitSubmit, true));
        }
        assert!(fields_disabled(FromGitStatus::Running));
        assert!(!fields_disabled(FromGitStatus::Ready));
    }

    #[test]
    fn enter_starts_a_ready_form_escape_closes_unless_something_runs() {
        assert_eq!(from_git_key("Enter", FromGitStatus::Ready), Some(FormKey::Start));
        for status in [
            FromGitStatus::Empty,
            FromGitStatus::Checking,
            FromGitStatus::Refused,
            FromGitStatus::NoFolder,
            FromGitStatus::Running,
        ] {
            assert_eq!(from_git_key("Enter", status), None);
        }
        assert_eq!(from_git_key("Escape", FromGitStatus::Ready), Some(FormKey::Close));
        assert_eq!(from_git_key("Escape", FromGitStatus::Refused), Some(FormKey::Close));
        assert_eq!(from_git_key("Escape", FromGitStatus::Running), None);
        assert_eq!(from_git_key("a", FromGitStatus::Ready), None);
    }

    #[test]
    fn keeps_a_failure_to_one_short_line() {
        assert_eq!(
            one_line("The clone failed: repository not found", 160),
            "The clone failed: repository not found"
        );
        assert_eq!(one_line("\n  first   line \nsecond", 160), "first line");
        assert_eq!(one_line("", 160), "");
        let cut = one_line(&"x".repeat(400), 160);
        assert_eq!(cut.chars().count(), 160);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn the_dialogs_destination_follows_the_typed_folder_then_the_setting() {
        // The two path functions the dialog calls together, as it calls them.
        let parent = resolve_parent(Some(""), Some("D:\\Projects"), Some("C:\\Work\\app"));
        assert_eq!(
            clone_destination(parent.as_deref(), Some("shop")).as_deref(),
            Some("D:\\Projects\\shop")
        );
        let beside = resolve_parent(None, None, Some("C:\\Work\\app"));
        assert_eq!(
            clone_destination(beside.as_deref(), Some("shop")).as_deref(),
            Some("C:\\Work\\shop")
        );
    }
}
