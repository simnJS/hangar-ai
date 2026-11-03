//! The agents a pane can run, and the command typed to start one
//! (`lib/agents.ts`). The transcript hunt that follows a pane's session is
//! in `session_watch`.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::state::types::AgentId;

/// What each agent is started with when nothing was customised. `None` for
/// the plain shell, which types nothing.
pub fn default_agent_command(agent: AgentId) -> Option<&'static str> {
    match agent {
        AgentId::Shell => None,
        AgentId::Claude => Some("claude"),
        AgentId::Codex => Some("codex"),
        AgentId::Gemini => Some("gemini"),
        AgentId::Opencode => Some("opencode"),
    }
}

/// JavaScript's `String.prototype.trim`: it also strips the byte order mark.
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// The command a pane starts an agent with: whatever the settings hold for
/// it, or the plain binary name.
///
/// Trimmed, and an empty override reads as "not customised" rather than as a
/// pane that types nothing: clearing the field in the settings is how you go
/// back to the default.
pub fn agent_command(agent: AgentId, commands: Option<&Map<String, Value>>) -> Option<String> {
    let default = default_agent_command(agent)?;
    let custom = commands
        .and_then(|commands| commands.get(agent.as_str()))
        .and_then(Value::as_str)
        .map(js_trim)
        .filter(|command| !command.is_empty());
    Some(custom.unwrap_or(default).to_string())
}

/// A path safe to put between double quotes in a command line.
///
/// These roots are read out of a `.code-workspace` file, which can arrive
/// with a cloned repository: nobody types them. A newline would run whatever
/// follows it as its own command, and a quote, backtick or `$` would end the
/// quoting and let the rest of the path be read as shell syntax. None of
/// them belong in a project path, so a root carrying one is dropped rather
/// than escaped.
fn is_quotable(root: &str) -> bool {
    !root.contains(['"', '`', '$', '\r', '\n'])
}

/// JavaScript's `\w`.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether the whole text is one or more characters of the set (`^[...]+$`).
fn all_of(text: &str, allowed: impl Fn(char) -> bool) -> bool {
    !text.is_empty() && text.chars().all(allowed)
}

/// What a launch adds to the agent's command (the options object of
/// `launchCommand`). Kept as one struct so a caller that only cares about one
/// of them does not have to name the others.
#[derive(Debug, Default, Clone)]
pub struct LaunchOptions<'a> {
    /// The folders a multi-root workspace has beyond the one the terminal
    /// opened in. Only Claude Code takes them on the command line; the others
    /// are left with their working directory rather than handed a flag they
    /// would refuse to start with.
    pub extra_roots: &'a [String],
    pub commands: Option<&'a Map<String, Value>>,
    /// The session's name (`session_name` of the pane names). Claude Code
    /// sessions on one machine can message each other by name, and naming
    /// each after its pane is what lets an agent reach another one.
    pub name: Option<&'a str>,
    /// The MCP config file this session loads instead of every other one, as
    /// the pane's shell is to read it: the variable holding its path, or the
    /// path quoted (`mcpConfigArg`). Absent, Claude Code starts with its own
    /// servers, as it always did.
    pub mcp_config: Option<&'a str>,
    /// False: `mcp_config` comes on top of Claude Code's own servers rather
    /// than in their place. That is how an orchestration pane gets Hangar's
    /// server where nothing else would give it one, a worker's worktree for
    /// one. `None` is the same as true.
    pub mcp_strict: Option<bool>,
    /// `--permission-mode`; absent, the person's own default.
    pub permission_mode: Option<&'a str>,
    /// `--allowedTools`: rules let through without asking, on top of theirs.
    pub allowed_tools: &'a [String],
    /// The model Claude Code starts with (`--model`): an orchestrator's or a
    /// worker's, from the pane. Absent, its own default.
    pub model: Option<&'a str>,
}

/// Builds the command typed into a freshly opened shell. `None` means "leave
/// the user at a plain prompt".
///
/// The resume arguments are appended to the command rather than replacing
/// it, so a custom launcher (an alias carrying flags of its own, a wrapper
/// script) resumes a conversation as well as the bare binary does.
pub fn launch_command(
    agent: AgentId,
    session_id: Option<&str>,
    options: &LaunchOptions,
) -> Option<String> {
    let command = agent_command(agent, options.commands)?;
    let session_id = session_id.filter(|id| !id.is_empty());

    match agent {
        AgentId::Claude => {
            let resume = session_id.map_or_else(String::new, |id| format!(" --resume {id}"));
            let name = options
                .name
                .filter(|name| !name.is_empty() && is_quotable(name))
                .map_or_else(String::new, |name| {
                    if all_of(name, |c| is_word(c) || c == '-') {
                        format!(" --name {name}")
                    } else {
                        format!(" --name \"{name}\"")
                    }
                });
            // An alias or an id: `sonnet`, `claude-sonnet-4-5`, `opus[1m]`;
            // the last quoted, since brackets are a glob to bash and zsh.
            let chosen = options
                .model
                .map(js_trim)
                .filter(|model| !model.is_empty() && is_quotable(model))
                .map_or_else(String::new, |model| {
                    if all_of(model, |c| is_word(c) || c == '.' || c == '-') {
                        format!(" --model {model}")
                    } else {
                        format!(" --model \"{model}\"")
                    }
                });
            let permission = options
                .permission_mode
                .map(js_trim)
                .filter(|mode| all_of(mode, |c| c.is_ascii_alphabetic()))
                .map_or_else(String::new, |mode| format!(" --permission-mode {mode}"));
            // Variadic, like the two below: the flag after it closes its list.
            let rules: Vec<&str> = options
                .allowed_tools
                .iter()
                .map(String::as_str)
                .filter(|rule| all_of(rule, |c| is_word(c) || c == '*' || c == '.' || c == '-'))
                .collect();
            let allowed = if rules.is_empty() {
                String::new()
            } else {
                format!(" --allowedTools {}", rules.join(" "))
            };
            // `--mcp-config` is variadic as well; the flag right behind it
            // closes its list, and `--strict-mcp-config` is what makes it the
            // only source. Not strict, it may be the last flag, or be closed
            // by `--add-dir`.
            let mcp = match options.mcp_config.filter(|arg| !arg.is_empty()) {
                Some(arg) if options.mcp_strict == Some(false) => format!(" --mcp-config {arg}"),
                Some(arg) => format!(" --mcp-config {arg} --strict-mcp-config"),
                None => String::new(),
            };
            let roots: Vec<String> = options
                .extra_roots
                .iter()
                .filter(|root| is_quotable(root))
                .map(|root| format!("\"{root}\""))
                .collect();
            // `--add-dir` is variadic, so it comes last: a following flag (one
            // the custom command carries, or the resume above) would be
            // swallowed as one more directory.
            let added = if roots.is_empty() {
                String::new()
            } else {
                format!(" --add-dir {}", roots.join(" "))
            };
            Some(format!(
                "{command}{resume}{name}{chosen}{permission}{allowed}{mcp}{added}"
            ))
        }
        AgentId::Codex => Some(match session_id {
            Some(id) => format!("{command} resume {id}"),
            None => command,
        }),
        _ => Some(command),
    }
}

/// Whether a pane's agent can be started again on the session it was in.
pub fn is_resumable(agent: AgentId) -> bool {
    matches!(agent, AgentId::Claude | AgentId::Codex)
}

/// Session ids already handed to a pane during this app run. Without this,
/// two panes started in the same directory would both latch onto the same
/// new transcript file.
#[derive(Debug, Default)]
pub struct Claims(HashSet<String>);

impl Claims {
    pub fn claim(&mut self, session_id: &str) {
        self.0.insert(session_id.to_string());
    }

    pub fn release(&mut self, session_id: Option<&str>) {
        if let Some(id) = session_id {
            self.0.remove(id);
        }
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.0.contains(session_id)
    }
}

/// The session ids an agent already has under this folder, read before a
/// launch so that a brand new transcript stands out afterwards. Empty for an
/// agent that keeps none, and when they cannot be listed.
pub async fn known_session_ids(agent: AgentId, cwd: String) -> HashSet<String> {
    if !is_resumable(agent) {
        return HashSet::new();
    }
    let agent = agent.as_str().to_string();
    tokio::task::spawn_blocking(move || {
        hangar_core::sessions::list_sessions(agent, cwd)
            .into_iter()
            .map(|session| session.id)
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Whether a screen of Claude Code is its question about a folder it has not
/// seen: "Quick safety check: Is this a project you created or one you
/// trust?", answered with "Yes, I trust this folder". It comes before the
/// session exists, so none of the mod's events can announce it, and the pane
/// would otherwise read as done. Both phrases are needed: a transcript that
/// merely talks about trust is not the question.
pub fn is_trust_prompt(screen: &str) -> bool {
    let text = screen
        .split(|c: char| c.is_whitespace() || c == '\u{feff}')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    text.contains("quick safety check") && text.contains("trust this folder")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(agent: AgentId, session: Option<&str>, options: LaunchOptions) -> Option<String> {
        launch_command(agent, session, &options)
    }

    fn claude(options: LaunchOptions) -> String {
        launch(AgentId::Claude, None, options).unwrap()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    // launchCommand with a model

    #[test]
    fn starts_claude_code_on_the_panes_model() {
        assert_eq!(
            claude(LaunchOptions {
                model: Some("sonnet"),
                ..Default::default()
            }),
            "claude --model sonnet"
        );
        assert_eq!(
            launch(
                AgentId::Claude,
                Some("abc"),
                LaunchOptions {
                    model: Some("claude-sonnet-4-5"),
                    name: Some("Ava"),
                    ..Default::default()
                }
            )
            .unwrap(),
            "claude --resume abc --name Ava --model claude-sonnet-4-5"
        );
    }

    #[test]
    fn quotes_a_model_a_shell_would_read_as_a_glob_and_drops_one_it_could_not_quote() {
        assert_eq!(
            claude(LaunchOptions {
                model: Some("opus[1m]"),
                ..Default::default()
            }),
            "claude --model \"opus[1m]\""
        );
        assert_eq!(
            claude(LaunchOptions {
                model: Some("opus\"; rm -rf ~"),
                ..Default::default()
            }),
            "claude"
        );
    }

    #[test]
    fn keeps_the_model_ahead_of_the_variadic_flags() {
        let roots = strings(&["/x"]);
        assert_eq!(
            claude(LaunchOptions {
                model: Some("haiku"),
                mcp_config: Some("/m.json"),
                extra_roots: &roots,
                ..Default::default()
            }),
            "claude --model haiku --mcp-config /m.json --strict-mcp-config --add-dir \"/x\""
        );
    }

    #[test]
    fn changes_nothing_without_a_model_nor_for_the_other_agents() {
        assert_eq!(claude(LaunchOptions::default()), "claude");
        assert_eq!(
            claude(LaunchOptions {
                model: Some("  "),
                ..Default::default()
            }),
            "claude"
        );
        let sonnet = LaunchOptions {
            model: Some("sonnet"),
            ..Default::default()
        };
        assert_eq!(
            launch(AgentId::Codex, None, sonnet.clone()).as_deref(),
            Some("codex")
        );
        assert_eq!(launch(AgentId::Shell, None, sonnet), None);
    }

    // launchCommand for an orchestration pane

    #[test]
    fn adds_hangars_server_on_top_of_claude_codes_own_and_lets_its_tools_through() {
        let tools = strings(&["mcp__hangar"]);
        let roots = strings(&["/x"]);
        assert_eq!(
            claude(LaunchOptions {
                model: Some("sonnet"),
                permission_mode: Some("auto"),
                allowed_tools: &tools,
                mcp_config: Some("/h.json"),
                mcp_strict: Some(false),
                extra_roots: &roots,
                ..Default::default()
            }),
            "claude --model sonnet --permission-mode auto --allowedTools mcp__hangar --mcp-config /h.json --add-dir \"/x\""
        );
        // The last flag, nothing to close its list.
        assert_eq!(
            claude(LaunchOptions {
                mcp_config: Some("/h.json"),
                mcp_strict: Some(false),
                ..Default::default()
            }),
            "claude --mcp-config /h.json"
        );
    }

    /// `mcpConfigArg` of lib/mcpScope.ts for the shell families the test
    /// names; the real one is the MCP package's (P10).
    fn mcp_config_arg(family: &str) -> &'static str {
        match family {
            "powershell" => "$env:HANGAR_MCP_CONFIG",
            "cmd" => "\"%HANGAR_MCP_CONFIG%\"",
            "posix" | "fish" => "\"$HANGAR_MCP_CONFIG\"",
            "nu" => "$env.HANGAR_MCP_CONFIG",
            other => panic!("no such family {other}"),
        }
    }

    #[test]
    fn never_shows_the_config_files_path_in_any_shell_that_holds_it_in_a_variable() {
        let tools = strings(&["mcp__hangar"]);
        let roots = strings(&["D:\\shared"]);
        let line = |family: &str, strict: bool| {
            launch(
                AgentId::Claude,
                Some("abc"),
                LaunchOptions {
                    name: Some("Bo"),
                    model: Some("sonnet"),
                    permission_mode: Some("auto"),
                    allowed_tools: &tools,
                    mcp_config: Some(mcp_config_arg(family)),
                    mcp_strict: Some(strict),
                    extra_roots: &roots,
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let flags = "claude --resume abc --name Bo --model sonnet --permission-mode auto --allowedTools mcp__hangar";
        assert_eq!(
            line("powershell", false),
            format!("{flags} --mcp-config $env:HANGAR_MCP_CONFIG --add-dir \"D:\\shared\"")
        );
        assert_eq!(
            line("cmd", false),
            format!("{flags} --mcp-config \"%HANGAR_MCP_CONFIG%\" --add-dir \"D:\\shared\"")
        );
        assert_eq!(
            line("posix", false),
            format!("{flags} --mcp-config \"$HANGAR_MCP_CONFIG\" --add-dir \"D:\\shared\"")
        );
        assert_eq!(
            line("fish", false),
            format!("{flags} --mcp-config \"$HANGAR_MCP_CONFIG\" --add-dir \"D:\\shared\"")
        );
        assert_eq!(
            line("nu", false),
            format!("{flags} --mcp-config $env.HANGAR_MCP_CONFIG --add-dir \"D:\\shared\"")
        );
        // A workspace's own server choice: the same variable, strict.
        assert_eq!(
            line("powershell", true),
            format!(
                "{flags} --mcp-config $env:HANGAR_MCP_CONFIG --strict-mcp-config --add-dir \"D:\\shared\""
            )
        );
        for family in ["powershell", "cmd", "posix", "fish", "nu"] {
            let typed = line(family, true);
            assert!(
                !["Ann Lee", "AppData", "ws-1"]
                    .iter()
                    .any(|secret| typed.contains(secret)),
                "{family}: {typed}"
            );
        }
    }

    #[test]
    fn keeps_the_persons_mode_without_one_and_drops_what_a_shell_would_misread() {
        assert_eq!(claude(LaunchOptions::default()), "claude");
        assert_eq!(
            claude(LaunchOptions {
                permission_mode: Some("auto; rm -rf ~"),
                ..Default::default()
            }),
            "claude"
        );
        let tools = strings(&["mcp__hangar", "Bash(rm *)"]);
        assert_eq!(
            claude(LaunchOptions {
                allowed_tools: &tools,
                ..Default::default()
            }),
            "claude --allowedTools mcp__hangar"
        );
    }

    // What the TS suite did not spell out, but its code does.

    #[test]
    fn a_custom_command_is_trimmed_and_an_empty_one_is_the_default() {
        let mut commands = Map::new();
        commands.insert("claude".into(), Value::String("  ccr code  ".into()));
        commands.insert("codex".into(), Value::String("   ".into()));
        assert_eq!(
            agent_command(AgentId::Claude, Some(&commands)).as_deref(),
            Some("ccr code")
        );
        assert_eq!(
            agent_command(AgentId::Codex, Some(&commands)).as_deref(),
            Some("codex")
        );
        assert_eq!(agent_command(AgentId::Shell, Some(&commands)), None);
        // The resume arguments go after a custom launcher.
        assert_eq!(
            launch(
                AgentId::Claude,
                Some("abc"),
                LaunchOptions {
                    commands: Some(&commands),
                    ..Default::default()
                }
            )
            .unwrap(),
            "ccr code --resume abc"
        );
    }

    #[test]
    fn resumes_codex_by_a_subcommand_and_names_a_session_with_a_space_in_quotes() {
        assert_eq!(
            launch(AgentId::Codex, Some("s1"), LaunchOptions::default()).as_deref(),
            Some("codex resume s1")
        );
        assert_eq!(
            claude(LaunchOptions {
                name: Some("Ava (My Work)"),
                ..Default::default()
            }),
            "claude --name \"Ava (My Work)\""
        );
        // A name that could end the quoting is not typed.
        assert_eq!(
            claude(LaunchOptions {
                name: Some("Ava \"x"),
                ..Default::default()
            }),
            "claude"
        );
        // Gemini and OpenCode have no resume.
        assert_eq!(
            launch(AgentId::Gemini, Some("s1"), LaunchOptions::default()).as_deref(),
            Some("gemini")
        );
    }

    #[test]
    fn drops_the_extra_roots_a_shell_could_misread() {
        let roots = strings(&["/ok one", "/bad$HOME", "/bad`x`", "/bad\nrm"]);
        assert_eq!(
            claude(LaunchOptions {
                extra_roots: &roots,
                ..Default::default()
            }),
            "claude --add-dir \"/ok one\""
        );
    }

    #[test]
    fn only_claude_code_and_codex_resume() {
        assert!(is_resumable(AgentId::Claude));
        assert!(is_resumable(AgentId::Codex));
        assert!(!is_resumable(AgentId::Gemini));
        assert!(!is_resumable(AgentId::Opencode));
        assert!(!is_resumable(AgentId::Shell));
    }

    #[test]
    fn claims_are_released_by_id() {
        let mut claims = Claims::default();
        claims.claim("s1");
        assert!(claims.contains("s1"));
        claims.release(None);
        claims.release(Some("s2"));
        assert!(claims.contains("s1"));
        claims.release(Some("s1"));
        assert!(!claims.contains("s1"));
    }

    // isTrustPrompt

    const SCREEN: &str = " Accessing workspace:\n  C:\\Users\\me\\projects\\tool\n Quick safety check: Is this a project you created or one you trust? (Like your own code,\n a well-known open source project, or work from your team).\n \u{276f} 1. Yes, I trust this folder\n   2. No, exit";

    #[test]
    fn recognises_the_trust_question_wrapped_or_not() {
        assert!(is_trust_prompt(SCREEN));
        assert!(is_trust_prompt(
            &SCREEN.replace("safety check", "safety\ncheck")
        ));
    }

    #[test]
    fn does_not_take_a_conversation_about_it_or_a_plain_screen_for_the_question() {
        assert!(!is_trust_prompt("> explain the quick safety check"));
        assert!(!is_trust_prompt("I trust this folder"));
        assert!(!is_trust_prompt(""));
    }
}
