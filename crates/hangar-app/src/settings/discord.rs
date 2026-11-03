//! Discord Rich Presence: what the card says, and when it is republished
//! (`lib/discord.ts`).
//!
//! Core owns the connection and the rate limiting; everything here is about
//! the shape of the presence. It is rebuilt from the state and only crosses
//! to core when it actually differs, so the worker can compare payloads
//! instead of guessing what changed.

use hangar_core::discord::Presence;
use hangar_i18n::{Key, Translator, args};

use crate::state::types::{AgentId, Pane, Settings, Workspace};

/// The Discord application this app ships with. Its uploaded art is what the
/// asset keys below resolve to; a fork with its own art overrides it from the
/// settings rather than by rebuilding.
pub const DEFAULT_APP_ID: &str = "1534592584045957282";

pub const REPO_URL: &str = "https://github.com/hangar-ai/hangar";

/// The logo, served straight from the repository: the same file the README
/// shows, so the two never drift apart.
///
/// Discord takes a URL as readily as an uploaded asset key: it proxies the
/// image and caches the result, which saves everyone a trip to the developer
/// portal. The cost is that the URL has to resolve on `main`. The badge has
/// no such option (agent art has to be uploaded under these keys), but
/// Discord drops the keys it cannot resolve without complaining, so a card
/// missing its art is still a card.
const LARGE_IMAGE: &str = "https://raw.githubusercontent.com/hangar-ai/hangar/main/assets/logo.png";

fn agent_image(agent: AgentId) -> String {
    format!("agent-{}", agent.as_str())
}

/// What `buildPresence` is given.
pub struct PresenceInput<'a> {
    pub settings: &'a Settings,
    pub workspace: Option<&'a Workspace>,
    /// The pane in focus, whose agent gets the small badge.
    pub focused_pane: Option<&'a str>,
    pub t: Translator,
    /// Discord shows the elapsed time counting up from here. The app launch
    /// is the only start that stays put: pinning it to the open workspace
    /// would reset the timer on every switch, which reads as "just started"
    /// all day long. Unix milliseconds.
    pub started_at: i64,
}

/// Builds the card, or `None` when the feature is off, which is also what
/// tells the worker to take the presence down.
pub fn build_presence(input: &PresenceInput) -> Option<Presence> {
    let PresenceInput {
        settings,
        workspace,
        focused_pane,
        t,
        started_at,
    } = input;
    if !settings.discord_presence {
        return None;
    }

    let panes: &[Pane] = workspace.map_or(&[], |ws| &ws.panes);
    // A shell is a terminal, not an agent: counting it would claim five
    // agents are running on a workspace where nothing was launched.
    // Claude Code is the only agent (owner's decision, 2026-11-03): named, it
    // is named once; counted, each of its panes counts, where the old app
    // counted the kinds of agent running.
    let agents = panes
        .iter()
        .filter(|pane| pane.agent != AgentId::Shell)
        .count();
    let claude = AgentId::Claude;

    let details = match workspace {
        Some(ws) if settings.discord_show_workspace => ws.name.clone(),
        Some(_) => t.t(Key::DiscordWorking).into_owned(),
        None => t.t(Key::DiscordNoWorkspace).into_owned(),
    };

    let state = if workspace.is_none() {
        t.t(Key::DiscordIdle).into_owned()
    } else {
        let agents = if agents == 0 {
            t.t(Key::DiscordShellOnly).into_owned()
        } else if settings.discord_show_agents {
            claude.info().label.to_owned()
        } else {
            t.t_with(Key::DiscordAgents, &args!("n" => agents))
                .into_owned()
        };
        format!(
            "{} · {agents}",
            t.t_with(Key::DiscordPanes, &args!("n" => panes.len()))
        )
    };

    // The badge follows the pane you are looking at, falling back to the
    // agent that is running: a shell-only workspace gets no badge rather than
    // a meaningless one.
    let focused = panes
        .iter()
        .find(|pane| Some(pane.id.as_str()) == *focused_pane);
    let badge = if settings.discord_show_agents && agents > 0 {
        Some(match focused {
            Some(pane) if pane.agent != AgentId::Shell => pane.agent,
            _ => claude,
        })
    } else {
        None
    };

    let app_id = settings.discord_app_id.trim();
    Some(Presence {
        app_id: if app_id.is_empty() {
            DEFAULT_APP_ID.to_string()
        } else {
            app_id.to_string()
        },
        details,
        state,
        large_image: LARGE_IMAGE.to_string(),
        large_text: t.t(Key::DiscordTagline).into_owned(),
        // The repository is reachable two ways: the button, and the art
        // itself.
        large_url: Some(REPO_URL.to_string()),
        small_image: badge.map(agent_image),
        small_text: badge.map(|agent| agent.info().label.to_string()),
        started_at: Some(*started_at),
        button_label: Some(t.t(Key::DiscordButton).into_owned()),
        button_url: Some(REPO_URL.to_string()),
    })
}

/// Decides when the presence goes to core (`useDiscordPresence`): whenever
/// it differs from what was sent last, and not otherwise. The effect of the
/// web app keyed on the serialised payload because it was rebuilt on every
/// render; this compares it, for the same reason: an update every few
/// seconds is all Discord accepts.
#[derive(Debug, Default)]
pub struct Publisher {
    /// What core was last told: `None` before the first time, `Some(None)`
    /// when it was told to take the presence down.
    sent: Option<Option<Presence>>,
}

impl Publisher {
    /// What to tell core now, if anything: `Some(wanted)` when `wanted` is not
    /// what core already has. The first call always tells it, even to say
    /// there is nothing to show, so a presence left by a crashed run goes.
    pub fn changed(&mut self, wanted: Option<Presence>) -> Option<Option<Presence>> {
        if self.sent.as_ref() == Some(&wanted) {
            return None;
        }
        self.sent = Some(wanted.clone());
        Some(wanted)
    }

    /// Whether core has been told anything yet.
    pub fn has_sent(&self) -> bool {
        self.sent.is_some()
    }

    /// Whether anything has been sent that a later call could take down.
    pub fn is_showing(&self) -> bool {
        matches!(self.sent, Some(Some(_)))
    }
}

/// Now, in the unix milliseconds Discord counts from.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    fn pane(id: &str, agent: AgentId) -> Pane {
        Pane {
            id: id.into(),
            agent,
            ..Pane::default()
        }
    }

    fn workspace(name: &str, agents: &[AgentId]) -> Workspace {
        Workspace {
            id: "w1".into(),
            name: name.into(),
            panes: agents
                .iter()
                .enumerate()
                .map(|(at, agent)| pane(&format!("p{at}"), *agent))
                .collect(),
            ..Workspace::default()
        }
    }

    fn on() -> Settings {
        Settings {
            discord_presence: true,
            ..Settings::default()
        }
    }

    fn build(
        settings: &Settings,
        workspace: Option<&Workspace>,
        focused: Option<&str>,
    ) -> Option<Presence> {
        build_presence(&PresenceInput {
            settings,
            workspace,
            focused_pane: focused,
            t: Translator::new(Locale::En),
            started_at: 1_700_000_000_000,
        })
    }

    #[test]
    fn nothing_is_shown_while_the_feature_is_off() {
        let ws = workspace("Shop", &[AgentId::Claude]);
        assert_eq!(build(&Settings::default(), Some(&ws), None), None);
    }

    #[test]
    fn without_a_workspace_it_says_it_is_idle() {
        let card = build(&on(), None, None).unwrap();
        assert_eq!(card.details, "No workspace open");
        assert_eq!(card.state, "Idle");
        assert_eq!(card.small_image, None);
    }

    #[test]
    fn the_workspace_is_named_unless_the_person_hid_it() {
        let ws = workspace("Shop", &[AgentId::Claude]);
        assert_eq!(build(&on(), Some(&ws), None).unwrap().details, "Shop");
        let hidden = Settings {
            discord_show_workspace: false,
            ..on()
        };
        assert_eq!(
            build(&hidden, Some(&ws), None).unwrap().details,
            "Orchestrating coding agents"
        );
    }

    #[test]
    fn a_shell_is_a_terminal_and_not_an_agent() {
        let ws = workspace("Shop", &[AgentId::Shell, AgentId::Shell]);
        let card = build(&on(), Some(&ws), None).unwrap();
        assert_eq!(card.state, "2 panes · Shell only");
        // A shell-only workspace gets no badge rather than a meaningless one.
        assert_eq!(card.small_image, None);
        assert_eq!(card.small_text, None);
    }

    #[test]
    fn the_agents_are_named_once_each() {
        let ws = workspace(
            "Shop",
            &[
                AgentId::Claude,
                AgentId::Shell,
                AgentId::Claude,
                AgentId::Claude,
            ],
        );
        assert_eq!(
            build(&on(), Some(&ws), None).unwrap().state,
            "4 panes · Claude Code"
        );
    }

    #[test]
    fn the_agents_can_be_counted_instead_of_named() {
        let ws = workspace("Shop", &[AgentId::Claude, AgentId::Claude]);
        let hidden = Settings {
            discord_show_agents: false,
            ..on()
        };
        let card = build(&hidden, Some(&ws), None).unwrap();
        assert_eq!(card.state, "2 panes · 2 agents");
        // Not naming them means no badge either.
        assert_eq!(card.small_image, None);
        let one = workspace("Shop", &[AgentId::Claude]);
        assert_eq!(
            build(&hidden, Some(&one), None).unwrap().state,
            "1 pane · 1 agent"
        );
    }

    #[test]
    fn the_badge_follows_the_focused_pane_else_the_first_agent() {
        let ws = workspace("Shop", &[AgentId::Claude, AgentId::Claude, AgentId::Shell]);
        let first = build(&on(), Some(&ws), None).unwrap();
        assert_eq!(first.small_image.as_deref(), Some("agent-claude"));
        assert_eq!(first.small_text.as_deref(), Some("Claude Code"));
        let on_second = build(&on(), Some(&ws), Some("p1")).unwrap();
        assert_eq!(on_second.small_image.as_deref(), Some("agent-claude"));
        // Focus on a shell falls back to the running agent.
        let on_shell = build(&on(), Some(&ws), Some("p2")).unwrap();
        assert_eq!(on_shell.small_image.as_deref(), Some("agent-claude"));
        // So does a pane that no longer exists.
        let gone = build(&on(), Some(&ws), Some("nope")).unwrap();
        assert_eq!(gone.small_image.as_deref(), Some("agent-claude"));
    }

    #[test]
    fn the_application_is_the_one_in_the_settings_else_the_shipped_one() {
        let ws = workspace("Shop", &[]);
        assert_eq!(
            build(&on(), Some(&ws), None).unwrap().app_id,
            DEFAULT_APP_ID
        );
        let own = Settings {
            discord_app_id: "  1234  ".into(),
            ..on()
        };
        assert_eq!(build(&own, Some(&ws), None).unwrap().app_id, "1234");
        let blank = Settings {
            discord_app_id: "   ".into(),
            ..on()
        };
        assert_eq!(
            build(&blank, Some(&ws), None).unwrap().app_id,
            DEFAULT_APP_ID
        );
    }

    #[test]
    fn the_card_links_to_the_repository_twice_and_counts_from_the_launch() {
        let card = build(&on(), None, None).unwrap();
        assert_eq!(card.large_url.as_deref(), Some(REPO_URL));
        assert_eq!(card.button_url.as_deref(), Some(REPO_URL));
        assert_eq!(card.button_label.as_deref(), Some("Get Hangar.AI"));
        assert_eq!(card.started_at, Some(1_700_000_000_000));
        assert!(card.large_image.ends_with("/assets/logo.png"));
    }

    #[test]
    fn the_card_is_written_in_the_interface_language() {
        let ws = workspace("Shop", &[AgentId::Claude]);
        let settings = on();
        let card = build_presence(&PresenceInput {
            settings: &settings,
            workspace: Some(&ws),
            focused_pane: None,
            t: Translator::new(Locale::Fr),
            started_at: 0,
        })
        .unwrap();
        assert_eq!(card.state, "1 panneau · Claude Code");
    }

    #[test]
    fn core_is_told_once_per_change() {
        let mut publisher = Publisher::default();
        let card = build(&on(), None, None);
        // The first time it always speaks, even to say there is nothing.
        assert_eq!(publisher.changed(None), Some(None));
        assert_eq!(publisher.changed(None), None);
        assert!(!publisher.is_showing());
        assert_eq!(publisher.changed(card.clone()), Some(card.clone()));
        assert_eq!(publisher.changed(card.clone()), None);
        assert!(publisher.is_showing());
        let ws = workspace("Shop", &[]);
        let other = build(&on(), Some(&ws), None);
        assert_eq!(publisher.changed(other.clone()), Some(other));
        // Turned off: core is told to take it down, once.
        assert_eq!(publisher.changed(None), Some(None));
        assert_eq!(publisher.changed(None), None);
    }
}
