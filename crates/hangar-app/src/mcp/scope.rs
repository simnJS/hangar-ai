//! The MCP servers a workspace's Claude Code panes start with
//! (`lib/mcpScope.ts`; the file they are written to is hangar-core's
//! `mcp_scope.rs`).
//!
//! Every Claude Code session starts its own copy of every stdio server it is
//! configured with, so a server one project needs runs once per pane in every
//! workspace. A workspace in `custom` mode names the servers its panes load;
//! the pane then starts Claude Code with `--mcp-config <file>
//! --strict-mcp-config`, the file being written by the core at each launch
//! from Claude Code's own configuration. The command typed names the file
//! through the shell's `HANGAR_MCP_CONFIG` (`mcp_config_arg`).
//!
//! `--strict-mcp-config` also drops the servers of the plugins the session
//! loads, so the file holds the workspace's kits' servers too (docs/kits.md).
//!
//! The pure rules are here, with their tests. The launch helpers
//! (`shell_family`, `quote_arg`, `mcp_config_arg`) are ported from the same TS
//! file and tested with it; they belong to the pane launch (P4/P5), which
//! calls them from `crate::mcp::scope`.

use serde_json::Value;

use hangar_core::mcp_scope::{ServerInfo, Status, Transport};

use crate::state::types::{McpMode, ShellInfo, Workspace, WorkspaceMcp};

/// Absent from every workspace saved before this existed: Claude Code's own.
pub fn inherit() -> WorkspaceMcp {
    WorkspaceMcp {
        mode: McpMode::Inherit,
        enabled: Vec::new(),
    }
}

/// A workspace's choice, read defensively: `state.json` is a file people
/// edit, and anything that is not a `custom` choice reads as today's
/// behaviour, with no names kept.
pub fn workspace_mcp(ws: Option<&Workspace>) -> WorkspaceMcp {
    match ws.and_then(|ws| ws.mcp.as_ref()) {
        Some(mcp) if mcp.mode == McpMode::Custom => WorkspaceMcp {
            mode: McpMode::Custom,
            enabled: mcp.enabled.clone(),
        },
        _ => inherit(),
    }
}

/// The servers Claude Code starts by itself here, Hangar's aside.
pub fn inherited_names(servers: &[ServerInfo]) -> Vec<String> {
    servers
        .iter()
        .filter(|server| server.status == Status::Active && !server.hangar)
        .map(|server| server.name.clone())
        .collect()
}

/// Whether a server is loaded under a choice. Hangar's own is always loaded in
/// `custom` mode (the panes need it to reach the board, each other and the
/// memory) and loaded by Claude Code's configuration otherwise.
pub fn is_loaded(server: &ServerInfo, mcp: &WorkspaceMcp) -> bool {
    match mcp.mode {
        McpMode::Inherit => server.status == Status::Active,
        McpMode::Custom => server.hangar || mcp.enabled.contains(&server.name),
    }
}

/// The `custom` choice that loads exactly what Claude Code would.
pub fn preset_all(servers: &[ServerInfo]) -> WorkspaceMcp {
    WorkspaceMcp {
        mode: McpMode::Custom,
        enabled: inherited_names(servers),
    }
}

pub fn preset_hangar_only() -> WorkspaceMcp {
    WorkspaceMcp {
        mode: McpMode::Custom,
        enabled: Vec::new(),
    }
}

/// One server switched on or off. From `inherit`, the first toggle starts from
/// what Claude Code would have loaded, so changing one server changes only
/// that one.
pub fn toggle_server(
    mcp: &WorkspaceMcp,
    servers: &[ServerInfo],
    name: &str,
    on: bool,
) -> WorkspaceMcp {
    let base = match mcp.mode {
        McpMode::Custom => mcp.enabled.clone(),
        McpMode::Inherit => inherited_names(servers),
    };
    let enabled = if on {
        let mut base = base;
        if !base.iter().any(|entry| entry == name) {
            base.push(name.to_owned());
        }
        base
    } else {
        base.into_iter().filter(|entry| entry != name).collect()
    };
    WorkspaceMcp {
        mode: McpMode::Custom,
        enabled,
    }
}

/// Server processes a session starts under a choice: the stdio servers, each
/// one at least a process of its own (`npx` and `uvx` add a launcher on top).
/// Remote servers are a connection, not a process.
pub fn stdio_count(servers: &[ServerInfo], mcp: &WorkspaceMcp) -> usize {
    let loaded = servers
        .iter()
        .filter(|server| server.transport == Transport::Stdio && is_loaded(server, mcp))
        .count();
    // Hangar's own server is added in `custom` mode even where no file
    // registers it.
    let hangar_extra =
        usize::from(mcp.mode == McpMode::Custom && !servers.iter().any(|s| s.hangar));
    loaded + hangar_extra
}

/// What a pane's Claude Code was started with (`PaneMcp` of
/// lib/paneInsight.ts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMcp {
    /// `inherit`: Claude Code's own configuration. `custom`: the servers the
    /// workspace chose, Hangar's included, as they went into the launch.
    pub mode: McpMode,
    /// The names in the launch's file; empty in `inherit`.
    pub servers: Vec<String>,
    /// Whether Hangar's own server went into the launch. `false` for a Claude
    /// Code started without a role: pinned later, it has none of the board,
    /// memory or worker tools until it restarts. `None` when the launch did
    /// not say.
    pub hangar: Option<bool>,
}

impl PaneMcp {
    /// Reads what a pane's insight holds (`PaneInsight.mcp`, raw JSON until P4
    /// types it): `{mode: "inherit"}` or `{mode: "custom", servers: [..]}`,
    /// each with an optional `hangar`.
    pub fn from_value(value: &Value) -> Option<PaneMcp> {
        let mode = match value.get("mode")?.as_str()? {
            "inherit" => McpMode::Inherit,
            "custom" => McpMode::Custom,
            _ => return None,
        };
        let servers = value
            .get("servers")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Some(PaneMcp {
            mode,
            servers,
            hangar: value.get("hangar").and_then(Value::as_bool),
        })
    }
}

/// Whether a pane's Claude Code runs with something else than the choice as it
/// stands: the panes « Relancer les agents » has to reach. A pane whose launch
/// this window never saw (`None`: another window draws it, or it has not
/// started) counts as out of date: restarting it costs nothing more than a
/// resume.
pub fn needs_restart(
    running: Option<&PaneMcp>,
    mcp: &WorkspaceMcp,
    servers: &[ServerInfo],
) -> bool {
    let Some(running) = running else {
        return true;
    };
    if running.mode != mcp.mode {
        return true;
    }
    if running.mode == McpMode::Inherit {
        return false;
    }
    // What a launch now would put in the file: Hangar's own, and every chosen
    // name something still defines.
    let defined: std::collections::HashSet<&str> = servers
        .iter()
        .filter(|server| !server.hangar)
        .map(|server| server.name.as_str())
        .collect();
    let mut expected: std::collections::HashSet<&str> = ["hangar"].into_iter().collect();
    expected.extend(
        mcp.enabled
            .iter()
            .map(String::as_str)
            .filter(|name| defined.contains(name)),
    );
    let actual: std::collections::HashSet<&str> =
        running.servers.iter().map(String::as_str).collect();
    expected != actual
}

/// How a shell reads a quoted argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    Powershell,
    Cmd,
    Posix,
    Fish,
    Nu,
    Wsl,
}

fn family_by_name(name: &str) -> Option<ShellFamily> {
    Some(match name {
        "pwsh" | "powershell" => ShellFamily::Powershell,
        "cmd" => ShellFamily::Cmd,
        "git-bash" | "msys2" | "cygwin" | "bash" | "zsh" | "sh" | "dash" | "ksh" => {
            ShellFamily::Posix
        }
        "fish" => ShellFamily::Fish,
        "nu" => ShellFamily::Nu,
        "wsl" => ShellFamily::Wsl,
        _ => return None,
    })
}

/// By the id the core's `shells.rs` gives it, else by its program's name;
/// `None` when unknown.
pub fn shell_family(shell: Option<&ShellInfo>) -> Option<ShellFamily> {
    let shell = shell?;
    if let Some(family) = family_by_name(&shell.id) {
        return Some(family);
    }
    let program = shell
        .program
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    family_by_name(program.strip_suffix(".exe").unwrap_or(&program))
}

/// A path as one argument the pane's shell types through untouched, or `None`
/// when that shell cannot be given it safely: the launch then goes without.
///
/// The path is the app's own (its data folder), but the folder holds the user
/// name, and a name can hold anything a file name can.
///
/// - PowerShell and the POSIX shells take single quotes literally: no `$`, no
///   backtick, no backslash escape. A quote inside is doubled (PowerShell) or
///   closed and escaped (POSIX).
/// - fish's single quotes still read `\\` and `\'`.
/// - Nushell's single quotes are raw and cannot hold a quote at all.
/// - cmd only has double quotes, and expands `%VAR%` inside them.
/// - An unknown shell gets single quotes, which every shell above but cmd
///   reads literally, as long as there is no quote to escape.
pub fn quote_arg(value: &str, family: Option<ShellFamily>) -> Option<String> {
    if value.contains(['\r', '\n', '\0']) {
        return None;
    }
    match family {
        Some(ShellFamily::Powershell) => Some(format!("'{}'", value.replace('\'', "''"))),
        Some(ShellFamily::Posix) => Some(format!("'{}'", value.replace('\'', "'\\''"))),
        Some(ShellFamily::Fish) => Some(format!(
            "'{}'",
            value.replace('\\', "\\\\").replace('\'', "\\'")
        )),
        Some(ShellFamily::Cmd) => {
            if value.contains(['"', '%']) {
                None
            } else {
                Some(format!("\"{value}\""))
            }
        }
        Some(ShellFamily::Nu) | None => {
            if value.contains('\'') {
                None
            } else {
                Some(format!("'{value}'"))
            }
        }
        // A Linux Claude Code reads its own configuration, and a Windows path
        // means nothing to it: a WSL pane keeps its own servers.
        Some(ShellFamily::Wsl) => None,
    }
}

/// Where the pane's shell holds the path of its `--mcp-config` file: pty.rs
/// sets it when the shell starts, the file being written just before.
pub const MCP_CONFIG_ENV: &str = "HANGAR_MCP_CONFIG";

/// The `--mcp-config` argument as the pane's shell is to read it: the variable
/// that holds the file's path rather than the path, which lies in the app's
/// data folder and so holds the user's name, and the typed command stays on
/// screen, in front of anyone watching, recording or streaming it. Every shell
/// below reads the variable as one argument, spaces and all.
///
/// A shell whose syntax is not known gets the path itself (`quote_arg`), and a
/// WSL pane nothing, as before.
pub fn mcp_config_arg(path: &str, family: Option<ShellFamily>) -> Option<String> {
    match family {
        Some(ShellFamily::Powershell) => Some(format!("$env:{MCP_CONFIG_ENV}")),
        // Expanded before the line is read, so quoted: a path may hold a
        // space, `&` or `^`, and never a double quote.
        Some(ShellFamily::Cmd) => Some(format!("\"%{MCP_CONFIG_ENV}%\"")),
        Some(ShellFamily::Posix | ShellFamily::Fish) => Some(format!("\"${MCP_CONFIG_ENV}\"")),
        Some(ShellFamily::Nu) => Some(format!("$env.{MCP_CONFIG_ENV}")),
        Some(ShellFamily::Wsl) | None => quote_arg(path, family),
    }
}

#[cfg(test)]
mod tests {
    use hangar_core::mcp_scope::Scope;

    use super::*;

    fn server(name: &str, patch: impl FnOnce(&mut ServerInfo)) -> ServerInfo {
        let mut server = ServerInfo {
            name: name.into(),
            scope: Scope::User,
            transport: Transport::Stdio,
            summary: name.into(),
            status: Status::Active,
            hangar: false,
            source: "~/.claude.json".into(),
        };
        patch(&mut server);
        server
    }

    fn servers() -> Vec<ServerInfo> {
        vec![
            server("hangar", |s| {
                s.scope = Scope::Project;
                s.hangar = true;
            }),
            server("robloxstudio", |_| {}),
            server("atlassian", |_| {}),
            server("docs", |s| s.transport = Transport::Http),
            server("repo-tool", |s| {
                s.scope = Scope::Project;
                s.status = Status::Unapproved;
            }),
            server("muted", |s| s.status = Status::Disabled),
        ]
    }

    fn shell(id: &str, program: Option<&str>) -> ShellInfo {
        ShellInfo {
            id: id.into(),
            label: id.into(),
            program: program.map_or_else(|| format!("{id}.exe"), str::to_owned),
            args: Vec::new(),
        }
    }

    fn custom(names: &[&str]) -> WorkspaceMcp {
        WorkspaceMcp {
            mode: McpMode::Custom,
            enabled: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn reads_a_missing_or_malformed_choice_as_claude_codes_own() {
        assert_eq!(workspace_mcp(None), inherit());
        assert_eq!(workspace_mcp(Some(&Workspace::default())), inherit());
        let with = |mcp| Workspace {
            mcp: Some(mcp),
            ..Workspace::default()
        };
        // Names kept next to `inherit` mean nothing.
        let stale = with(WorkspaceMcp {
            mode: McpMode::Inherit,
            enabled: vec!["x".into()],
        });
        assert_eq!(workspace_mcp(Some(&stale)), inherit());
        assert_eq!(
            workspace_mcp(Some(&with(custom(&["a", "b"])))),
            custom(&["a", "b"])
        );
    }

    #[test]
    fn starts_all_from_what_claude_code_would_load_hangar_aside() {
        assert_eq!(
            preset_all(&servers()),
            custom(&["robloxstudio", "atlassian", "docs"])
        );
    }

    #[test]
    fn keeps_hangar_loaded_with_nothing_else_chosen() {
        let none = preset_hangar_only();
        let loaded: Vec<String> = servers()
            .iter()
            .filter(|s| is_loaded(s, &none))
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(loaded, ["hangar"]);
    }

    #[test]
    fn changes_only_the_toggled_server_when_leaving_inherit() {
        let all = servers();
        let next = toggle_server(&inherit(), &all, "robloxstudio", false);
        assert_eq!(next, custom(&["atlassian", "docs"]));
        // An unapproved server can still be chosen on purpose.
        assert!(
            toggle_server(&next, &all, "repo-tool", true)
                .enabled
                .contains(&"repo-tool".to_owned())
        );
        // Toggling on twice lists it once.
        assert_eq!(
            toggle_server(&next, &all, "docs", true).enabled,
            ["atlassian", "docs"]
        );
    }

    #[test]
    fn counts_stdio_processes_not_remote_connections() {
        let all = servers();
        // hangar, robloxstudio, atlassian; docs is http, the others do not
        // start.
        assert_eq!(stdio_count(&all, &inherit()), 3);
        assert_eq!(stdio_count(&all, &preset_hangar_only()), 1);
        // Hangar's own server is added in custom mode even where nothing
        // registers it.
        let without: Vec<ServerInfo> = all.into_iter().filter(|s| !s.hangar).collect();
        assert_eq!(stdio_count(&without, &inherit()), 2);
        assert_eq!(stdio_count(&without, &preset_hangar_only()), 1);
    }

    fn running(names: &[&str]) -> PaneMcp {
        PaneMcp {
            mode: McpMode::Custom,
            servers: names.iter().map(|name| (*name).to_owned()).collect(),
            hangar: None,
        }
    }

    fn running_inherit() -> PaneMcp {
        PaneMcp {
            mode: McpMode::Inherit,
            servers: Vec::new(),
            hangar: None,
        }
    }

    #[test]
    fn leaves_a_pane_already_running_the_choice_alone() {
        let all = servers();
        let choice = custom(&["robloxstudio", "gone"]);
        assert!(!needs_restart(Some(&running_inherit()), &inherit(), &all));
        // A name nothing defines any more was never in the file to begin with.
        assert!(!needs_restart(
            Some(&running(&["robloxstudio", "hangar"])),
            &choice,
            &all
        ));
    }

    #[test]
    fn reaches_a_pane_started_under_another_choice_or_never_seen() {
        let all = servers();
        let choice = custom(&["robloxstudio", "gone"]);
        assert!(needs_restart(Some(&running_inherit()), &choice, &all));
        assert!(needs_restart(Some(&running(&["hangar"])), &choice, &all));
        assert!(needs_restart(Some(&running(&["hangar"])), &inherit(), &all));
        assert!(needs_restart(None, &inherit(), &all));
    }

    #[test]
    fn reads_what_a_panes_insight_holds() {
        use serde_json::json;
        assert_eq!(
            PaneMcp::from_value(&json!({"mode": "inherit"})),
            Some(running_inherit())
        );
        assert_eq!(
            PaneMcp::from_value(
                &json!({"mode": "custom", "servers": ["hangar", 3, "docs"], "hangar": true})
            ),
            Some(PaneMcp {
                hangar: Some(true),
                ..running(&["hangar", "docs"])
            })
        );
        assert_eq!(PaneMcp::from_value(&json!({"mode": "elsewhere"})), None);
        assert_eq!(PaneMcp::from_value(&json!("custom")), None);
    }

    #[test]
    fn knows_the_shells_hangar_detects() {
        let family = |id, program| shell_family(Some(&shell(id, program)));
        assert_eq!(family("pwsh", None), Some(ShellFamily::Powershell));
        assert_eq!(family("powershell", None), Some(ShellFamily::Powershell));
        assert_eq!(family("cmd", None), Some(ShellFamily::Cmd));
        assert_eq!(
            family("git-bash", Some("C:\\Program Files\\Git\\bin\\bash.exe")),
            Some(ShellFamily::Posix)
        );
        assert_eq!(family("zsh", Some("/bin/zsh")), Some(ShellFamily::Posix));
        assert_eq!(
            family("fish", Some("/usr/bin/fish")),
            Some(ShellFamily::Fish)
        );
        assert_eq!(family("nu", None), Some(ShellFamily::Nu));
        assert_eq!(family("wsl", None), Some(ShellFamily::Wsl));
    }

    #[test]
    fn falls_back_on_the_programs_name_then_on_not_knowing() {
        let family = |id, program| shell_family(Some(&shell(id, program)));
        assert_eq!(
            family("custom", Some("C:\\tools\\PWSH.EXE")),
            Some(ShellFamily::Powershell)
        );
        assert_eq!(family("custom", Some("/opt/bin/xonsh")), None);
        assert_eq!(shell_family(None), None);
    }

    const PATH: &str = "C:\\Users\\Ann\\AppData\\Roaming\\com.simnjs.hangar\\mcp\\ws-1.json";

    #[test]
    fn quotes_a_windows_path_so_no_shell_reads_its_backslashes() {
        use ShellFamily::*;
        assert_eq!(quote_arg(PATH, Some(Powershell)), Some(format!("'{PATH}'")));
        assert_eq!(quote_arg(PATH, Some(Posix)), Some(format!("'{PATH}'")));
        assert_eq!(quote_arg(PATH, Some(Cmd)), Some(format!("\"{PATH}\"")));
        assert_eq!(quote_arg(PATH, Some(Nu)), Some(format!("'{PATH}'")));
        assert_eq!(quote_arg(PATH, None), Some(format!("'{PATH}'")));
        assert_eq!(
            quote_arg("/home/ann/.local/share/mcp/ws.json", Some(Fish)).as_deref(),
            Some("'/home/ann/.local/share/mcp/ws.json'")
        );
        assert_eq!(
            quote_arg(PATH, Some(Fish)),
            Some(format!("'{}'", PATH.replace('\\', "\\\\")))
        );
    }

    #[test]
    fn keeps_what_a_shell_would_expand_literal() {
        use ShellFamily::*;
        let odd = "C:\\Users\\$env:x `whoami` $(id)\\ws.json";
        assert_eq!(quote_arg(odd, Some(Powershell)), Some(format!("'{odd}'")));
        assert_eq!(quote_arg(odd, Some(Posix)), Some(format!("'{odd}'")));
        assert_eq!(quote_arg("C:\\Users\\%USERNAME%\\ws.json", Some(Cmd)), None);
    }

    #[test]
    fn escapes_a_quote_the_way_each_shell_wants_it() {
        use ShellFamily::*;
        let quoted = "C:\\Users\\O'Neil\\ws.json";
        assert_eq!(
            quote_arg(quoted, Some(Powershell)).as_deref(),
            Some("'C:\\Users\\O''Neil\\ws.json'")
        );
        assert_eq!(
            quote_arg(quoted, Some(Posix)).as_deref(),
            Some("'C:\\Users\\O'\\''Neil\\ws.json'")
        );
        assert_eq!(
            quote_arg("/home/o'neil/ws.json", Some(Fish)).as_deref(),
            Some("'/home/o\\'neil/ws.json'")
        );
        assert_eq!(quote_arg(quoted, Some(Cmd)), Some(format!("\"{quoted}\"")));
        assert_eq!(quote_arg(quoted, Some(Nu)), None);
        assert_eq!(quote_arg(quoted, None), None);
    }

    #[test]
    fn refuses_a_line_break_and_every_wsl_pane() {
        assert_eq!(quote_arg("a\nb", Some(ShellFamily::Powershell)), None);
        assert_eq!(quote_arg("a\rb", Some(ShellFamily::Posix)), None);
        assert_eq!(quote_arg(PATH, Some(ShellFamily::Wsl)), None);
    }

    #[test]
    fn names_the_shells_variable_never_the_path_that_holds_the_users_name() {
        use ShellFamily::*;
        let path = "C:\\Users\\Ann Lee\\AppData\\Roaming\\com.simnjs.hangar\\mcp\\ws-1.json";
        assert_eq!(MCP_CONFIG_ENV, "HANGAR_MCP_CONFIG");
        assert_eq!(
            mcp_config_arg(path, Some(Powershell)).as_deref(),
            Some("$env:HANGAR_MCP_CONFIG")
        );
        assert_eq!(
            mcp_config_arg(path, Some(Cmd)).as_deref(),
            Some("\"%HANGAR_MCP_CONFIG%\"")
        );
        assert_eq!(
            mcp_config_arg(path, Some(Posix)).as_deref(),
            Some("\"$HANGAR_MCP_CONFIG\"")
        );
        assert_eq!(
            mcp_config_arg("/home/ann/.local/share/mcp/ws.json", Some(Fish)).as_deref(),
            Some("\"$HANGAR_MCP_CONFIG\"")
        );
        assert_eq!(
            mcp_config_arg(path, Some(Nu)).as_deref(),
            Some("$env.HANGAR_MCP_CONFIG")
        );
    }

    #[test]
    fn takes_any_path_a_variable_can_hold_the_ones_quoting_had_to_refuse_included() {
        assert_eq!(
            mcp_config_arg("C:\\Users\\100%\\ws.json", Some(ShellFamily::Cmd)).as_deref(),
            Some("\"%HANGAR_MCP_CONFIG%\"")
        );
        assert_eq!(
            mcp_config_arg("C:\\Users\\O'Neil\\ws.json", Some(ShellFamily::Nu)).as_deref(),
            Some("$env.HANGAR_MCP_CONFIG")
        );
    }

    #[test]
    fn gives_an_unknown_shell_the_path_itself_and_a_wsl_pane_nothing_as_before() {
        assert_eq!(mcp_config_arg(PATH, None), Some(format!("'{PATH}'")));
        assert_eq!(mcp_config_arg("C:\\Users\\O'Neil\\ws.json", None), None);
        assert_eq!(mcp_config_arg(PATH, Some(ShellFamily::Wsl)), None);
    }
}
