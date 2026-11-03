//! What a pane types into its shell once it is up: the agent's command, with
//! the resume of its last session (`launchCommand`, `agentCommand` of
//! `src/lib/agents.ts`), the name its Claude Code session runs under
//! (`sessionName` of `src/lib/paneNames.ts`), and the login of a pane opened
//! to log an account in. And the screen it reads to see Claude Code asking
//! whether to trust a folder (`isTrustPrompt`).
//!
//! These belong to P4 (`panes_logic::agents`) and P0b (`state::pane_names`),
//! which port the TS modules with their test suites; they are here, with the
//! cases the launch depends on, because a pane cannot start an agent without
//! them and both packages were being written at the same time as this one.
//! When `panes_logic::agents::launch_command` lands, the controller should
//! call it and this file should shrink to nothing.

use serde_json::{Map, Value};

use crate::state::types::AgentId;

/// What a pane opened to log a Claude account in types (`LOGIN_COMMAND`).
pub const LOGIN_COMMAND: &str = "claude auth login";

/// The tools an orchestrator and its workers are let through without asking
/// (`ORCHESTRATION_ALLOWED_TOOLS`).
pub const ORCHESTRATION_ALLOWED_TOOLS: [&str; 1] = ["mcp__hangar"];

/// The command a pane starts an agent with: what the settings hold for it,
/// trimmed, or the plain binary name; an empty override reads as "not
/// customised" (`agentCommand`).
pub fn agent_command(agent: AgentId, commands: &Map<String, Value>) -> Option<String> {
    if agent == AgentId::Shell {
        return None;
    }
    let custom = commands
        .get(agent.as_str())
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty());
    Some(custom.unwrap_or(agent.as_str()).to_owned())
}

/// A path or a name safe between double quotes on a command line: a newline
/// would run what follows as its own command, and a quote, a backtick or `$`
/// would end the quoting. One holding any is dropped rather than escaped.
fn is_quotable(text: &str) -> bool {
    !text.contains(['"', '`', '$', '\r', '\n'])
}

fn is_word(text: &str, extra: &[char]) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || extra.contains(&c))
}

/// What `launch_command` is told besides the agent and the session.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions<'a> {
    pub extra_roots: &'a [String],
    pub commands: Option<&'a Map<String, Value>>,
    /// The session's name (`session_name`).
    pub name: Option<String>,
    pub model: Option<&'a str>,
    pub permission_mode: Option<&'a str>,
    pub allowed_tools: &'a [&'a str],
}

/// The command typed into a freshly opened shell; `None` leaves the person at
/// a plain prompt (`launchCommand`). The resume arguments are appended to the
/// command rather than replacing it, so a custom launcher resumes as well as
/// the bare binary does.
///
/// The `--mcp-config` arguments of the TS are not here: the workspace's MCP
/// scope file is P10's (`mcpScope`), and a pane without it starts with its
/// agent's own servers, as it did before that feature.
pub fn launch_command(
    agent: AgentId,
    session: Option<&str>,
    options: &LaunchOptions,
) -> Option<String> {
    let empty = Map::new();
    let command = agent_command(agent, options.commands.unwrap_or(&empty))?;
    Some(match agent {
        AgentId::Claude => {
            let mut out = command;
            if let Some(id) = session {
                out.push_str(&format!(" --resume {id}"));
            }
            if let Some(name) = options.name.as_deref().filter(|name| is_quotable(name)) {
                if is_word(name, &['-']) {
                    out.push_str(&format!(" --name {name}"));
                } else {
                    out.push_str(&format!(" --name \"{name}\""));
                }
            }
            // An alias or an id: `sonnet`, `claude-sonnet-4-5`, `opus[1m]`,
            // the last quoted, since brackets are a glob to bash and zsh.
            if let Some(model) = options
                .model
                .map(str::trim)
                .filter(|m| !m.is_empty() && is_quotable(m))
            {
                if is_word(model, &['.', '-']) {
                    out.push_str(&format!(" --model {model}"));
                } else {
                    out.push_str(&format!(" --model \"{model}\""));
                }
            }
            if let Some(mode) = options
                .permission_mode
                .map(str::trim)
                .filter(|mode| !mode.is_empty() && mode.chars().all(|c| c.is_ascii_alphabetic()))
            {
                out.push_str(&format!(" --permission-mode {mode}"));
            }
            let rules: Vec<&str> = options
                .allowed_tools
                .iter()
                .copied()
                .filter(|rule| is_word(rule, &['*', '.', '-']))
                .collect();
            if !rules.is_empty() {
                out.push_str(&format!(" --allowedTools {}", rules.join(" ")));
            }
            // `--add-dir` is variadic, so it comes last: a flag after it
            // would be swallowed as one more directory.
            let roots: Vec<String> = options
                .extra_roots
                .iter()
                .filter(|root| is_quotable(root))
                .map(|root| format!("\"{root}\""))
                .collect();
            if !roots.is_empty() {
                out.push_str(&format!(" --add-dir {}", roots.join(" ")));
            }
            out
        }
        AgentId::Codex => match session {
            Some(id) => format!("{command} resume {id}"),
            None => command,
        },
        _ => command,
    })
}

/// Whether the agent can be relaunched on a previous session (`isResumable`).
pub fn is_resumable(agent: AgentId) -> bool {
    agent.info().resumable
}

/// The name a Claude Code session runs under, what other sessions list and
/// message it by: the pane's name, then its workspace's, with what a shell
/// would read as quoting or expansion left out (`sessionName`).
pub fn session_name(pane: &str, workspace: &str) -> String {
    let cleaned: String = workspace
        .chars()
        .filter(|c| !"\"'`$\\%!^&|<>".contains(*c))
        .collect();
    let workspace = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if workspace.is_empty() {
        pane.to_owned()
    } else {
        format!("{pane} ({workspace})")
    }
}

/// Whether a screen of Claude Code is its question about a folder it has not
/// seen. Both phrases are needed: a transcript that merely talks about trust
/// is not the question (`isTrustPrompt`).
pub fn is_trust_prompt(screen: &str) -> bool {
    let text = screen
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    text.contains("quick safety check") && text.contains("trust this folder")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_shell_types_nothing() {
        assert_eq!(
            launch_command(AgentId::Shell, None, &LaunchOptions::default()),
            None
        );
    }

    #[test]
    fn an_override_wins_and_an_empty_one_reads_as_the_default() {
        let commands = json!({"claude": "  my-claude --fast ", "codex": "   "});
        let commands = commands.as_object().unwrap();
        assert_eq!(
            agent_command(AgentId::Claude, commands).as_deref(),
            Some("my-claude --fast")
        );
        assert_eq!(
            agent_command(AgentId::Codex, commands).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn claude_resumes_by_flag_and_codex_by_subcommand() {
        let options = LaunchOptions::default();
        assert_eq!(
            launch_command(AgentId::Claude, Some("abc-1"), &options).as_deref(),
            Some("claude --resume abc-1")
        );
        assert_eq!(
            launch_command(AgentId::Codex, Some("abc-1"), &options).as_deref(),
            Some("codex resume abc-1")
        );
        assert_eq!(
            launch_command(AgentId::Gemini, Some("abc"), &options).as_deref(),
            Some("gemini")
        );
    }

    #[test]
    fn claude_flags_come_in_order_and_add_dir_last() {
        let roots = vec!["/a b".to_string(), "/bad\"root".to_string()];
        let options = LaunchOptions {
            extra_roots: &roots,
            name: Some(session_name("Ava", "My app")),
            model: Some("opus[1m]"),
            permission_mode: Some("acceptEdits"),
            allowed_tools: &ORCHESTRATION_ALLOWED_TOOLS,
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, None, &options).as_deref(),
            Some(
                "claude --name \"Ava (My app)\" --model \"opus[1m]\" --permission-mode acceptEdits \
                 --allowedTools mcp__hangar --add-dir \"/a b\""
            )
        );
    }

    #[test]
    fn a_session_name_drops_what_a_shell_would_expand() {
        assert_eq!(session_name("Ava", "Hangar"), "Ava (Hangar)");
        assert_eq!(session_name("Ava", "  it's  $HOME  "), "Ava (its HOME)");
        assert_eq!(session_name("Ava", " "), "Ava");
    }

    #[test]
    fn the_trust_question_needs_both_phrases() {
        assert!(is_trust_prompt(
            "Quick safety check: Is this a project you created?\n ❯ 1. Yes, I trust   this folder"
        ));
        assert!(!is_trust_prompt("Do you trust this folder?"));
    }
}
