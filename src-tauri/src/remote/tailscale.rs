//! Tailscale, as Hangar Mobile needs it: is it installed, is it connected,
//! what is this PC called on the tailnet, and does Tailscale Serve already
//! publish the mobile server. And the one command that changes anything,
//! `tailscale serve`, which only ever runs from the person's click: in the
//! settings for the mobile server, on the phone for an app's slot (apps.rs).
//!
//! A missing Tailscale is an answer — `installed: false` — never an error:
//! the settings are there to say what to install.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

pub const NOT_INSTALLED: &str = "Tailscale is not installed on this PC.";
pub const NOT_CONNECTED: &str = "Tailscale is not connected on this PC: open it and sign in.";

/// Hides the console window Windows would otherwise flash for every call.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `status` answers at once from a running daemon, and fails at once without
/// one; this only covers a daemon that hangs.
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// `serve` registers a certificate for the PC's name the first time, which
/// takes a few seconds. Long enough for that, not for waiting on a browser:
/// see `consent_url`.
const SERVE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleStatus {
    /// The CLI was found, on PATH or where its installer puts it.
    pub installed: bool,
    /// Signed in and connected to a tailnet.
    pub running: bool,
    /// The PC's name on the tailnet, `pc.tailnet.ts.net`, while connected.
    pub dns_name: Option<String>,
    /// Tailscale Serve publishes the mobile server's port on HTTPS 443.
    pub serving: bool,
}

pub async fn status(port: u16) -> TailscaleStatus {
    let Some(cli) = locate().await else {
        return TailscaleStatus::default();
    };
    let mut status = TailscaleStatus {
        installed: true,
        ..Default::default()
    };
    // Read whatever the exit status: a daemon that is stopped or signed out
    // still answers with its state.
    let Ok(ran) = run(&cli, &["status", "--json"], ASK_TIMEOUT, |_| false).await else {
        return status;
    };
    (status.running, status.dns_name) = parse_status(&ran.stdout);
    if status.running {
        if let Ok(ran) = run(&cli, &["serve", "status", "--json"], ASK_TIMEOUT, |_| false).await {
            status.serving = parse_serving(&ran.stdout, status.dns_name.as_deref(), port);
        }
    }
    status
}

/// `tailscale serve --bg --https=443 http://127.0.0.1:<port>`: the tailnet's
/// HTTPS address for this PC, proxied to the mobile server, kept by the
/// daemon across restarts.
pub async fn serve(port: u16) -> Result<TailscaleStatus, String> {
    publish(443, port).await?;
    Ok(status(port).await)
}

/// `tailscale serve --bg --https=<https> http://127.0.0.1:<port>`: this PC's
/// HTTPS address on that port proxied to the local one, kept by the daemon
/// across restarts until `tailscale serve --https=<https> off`.
pub async fn publish(https: u16, port: u16) -> Result<(), String> {
    let cli = locate().await.ok_or_else(|| NOT_INSTALLED.to_string())?;
    let target = format!("http://127.0.0.1:{port}");
    let https = format!("--https={https}");
    let ran = run(
        &cli,
        &["serve", "--bg", &https, &target],
        SERVE_TIMEOUT,
        |line| consent_url(line).is_some(),
    )
    .await?;
    if let Some(url) = consent_url(&ran.stdout).or_else(|| consent_url(&ran.stderr)) {
        return Err(format!(
            "Tailscale Serve is not enabled on your tailnet yet. Enable it at {url}, then try again."
        ));
    }
    if ran.timed_out {
        return Err("tailscale serve did not finish in time.".to_string());
    }
    if !ran.success {
        let said = if ran.stderr.trim().is_empty() {
            ran.stdout.trim()
        } else {
            ran.stderr.trim()
        };
        return Err(if said.is_empty() {
            "tailscale serve failed without saying why.".to_string()
        } else {
            format!("tailscale serve failed: {said}")
        });
    }
    Ok(())
}

/// The PC's name on the tailnet and what Tailscale Serve publishes on it
/// (`serve status --json`), or why Tailscale cannot say.
pub async fn serve_config() -> Result<(String, Value), String> {
    let cli = locate().await.ok_or_else(|| NOT_INSTALLED.to_string())?;
    let ran = run(&cli, &["status", "--json"], ASK_TIMEOUT, |_| false).await?;
    let (running, dns_name) = parse_status(&ran.stdout);
    let Some(dns_name) = dns_name.filter(|_| running) else {
        return Err(NOT_CONNECTED.to_string());
    };
    let ran = run(&cli, &["serve", "status", "--json"], ASK_TIMEOUT, |_| false).await?;
    let read = if ran.stdout.trim().is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str(&ran.stdout)
    };
    match read {
        Ok(config) if ran.success => Ok((dns_name, config)),
        _ => Err("Tailscale Serve did not say what it publishes.".to_string()),
    }
}

/// What Tailscale Serve does with one of this PC's HTTPS ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mapping {
    /// Nothing: the port is Hangar's to map.
    Free,
    /// Its root proxied to the local port asked about.
    Mapped,
    /// Something else — the person's own, or opened to the internet by
    /// Funnel — which Hangar never replaces.
    Taken,
}

/// `serve status --json`: what `https` on this PC does, `local` being where
/// Hangar would have it proxy to.
pub fn mapping(config: &Value, dns_name: &str, https: u16, local: u16) -> Mapping {
    let host = format!("{dns_name}:{https}");
    let entry = |key: &str| {
        config[key].as_object().and_then(|entries| {
            entries
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&host))
                .map(|(_, entry)| entry)
        })
    };
    if entry("AllowFunnel").is_some_and(|funnel| funnel == &Value::Bool(true)) {
        return Mapping::Taken;
    }
    let site = entry("Web");
    let ours = site
        .and_then(|site| site["Handlers"]["/"]["Proxy"].as_str())
        .is_some_and(|proxy| proxies_to(proxy, local));
    let tcp = &config["TCP"][https.to_string()];
    let https_served = tcp["HTTPS"] == Value::Bool(true);
    if ours && https_served {
        Mapping::Mapped
    } else if (!tcp.is_null() && !https_served) || (site.is_some() && !ours) {
        // A TCP forward, or a site of the person's.
        Mapping::Taken
    } else {
        // Nothing, or Hangar's own mapping half there: serving it again
        // completes it.
        Mapping::Free
    }
}

/// The CLI, found off the async runtime: on macOS the first PATH lookup asks
/// the login shell, which can take seconds (path_env.rs).
async fn locate() -> Option<PathBuf> {
    tokio::task::spawn_blocking(find).await.ok().flatten()
}

fn find() -> Option<PathBuf> {
    crate::path_env::ensure();
    let name = if cfg!(windows) {
        "tailscale.exe"
    } else {
        "tailscale"
    };
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|file| file.is_file())
    });
    on_path.or_else(|| usual_places().into_iter().find(|file| file.is_file()))
}

/// Where the installers put it when it is not on PATH.
#[cfg(windows)]
fn usual_places() -> Vec<PathBuf> {
    let mut places: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|dir| PathBuf::from(dir).join("Tailscale").join("tailscale.exe"))
        .collect();
    places.push(PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"));
    places
}

/// The app, from the App Store or the standalone download, is its own CLI.
#[cfg(target_os = "macos")]
fn usual_places() -> Vec<PathBuf> {
    [
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        "/opt/homebrew/bin/tailscale",
        "/usr/local/bin/tailscale",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn usual_places() -> Vec<PathBuf> {
    [
        "/usr/bin/tailscale",
        "/usr/sbin/tailscale",
        "/usr/local/bin/tailscale",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(not(any(windows, unix)))]
fn usual_places() -> Vec<PathBuf> {
    Vec::new()
}

struct Ran {
    success: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

/// Runs the CLI and reads what it says as it says it, until it exits, `limit`
/// runs out, or a line satisfies `enough`. The last is for `serve` on a
/// tailnet that has not allowed it: it prints where to allow it, then waits
/// for the person's browser for as long as it takes.
async fn run(
    cli: &Path,
    args: &[&str],
    limit: Duration,
    enough: impl Fn(&str) -> bool,
) -> Result<Ran, String> {
    let mut command = tokio::process::Command::new(cli);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not run tailscale: {err}"))?;

    let mut out = child.stdout.take().map(|pipe| BufReader::new(pipe).lines());
    let mut err = child.stderr.take().map(|pipe| BufReader::new(pipe).lines());
    let mut ran = Ran {
        success: false,
        timed_out: false,
        stdout: String::new(),
        stderr: String::new(),
    };
    let deadline = tokio::time::sleep(limit);
    tokio::pin!(deadline);

    let mut stopped_early = false;
    while out.is_some() || err.is_some() {
        let line = tokio::select! {
            line = next_line(&mut out) => line.map(|line| (line, &mut ran.stdout)),
            line = next_line(&mut err) => line.map(|line| (line, &mut ran.stderr)),
            _ = &mut deadline => {
                ran.timed_out = true;
                break;
            }
        };
        if let Some((line, into)) = line {
            into.push_str(&line);
            into.push('\n');
            if enough(&line) {
                stopped_early = true;
                break;
            }
        }
    }
    if ran.timed_out || stopped_early {
        let _ = child.kill().await;
        return Ok(ran);
    }
    // Both pipes closed: the process is exiting, if it has not already.
    match tokio::time::timeout_at(deadline.deadline(), child.wait()).await {
        Ok(Ok(exit)) => ran.success = exit.success(),
        Ok(Err(err)) => return Err(format!("could not run tailscale: {err}")),
        Err(_) => {
            ran.timed_out = true;
            let _ = child.kill().await;
        }
    }
    Ok(ran)
}

/// The next line of a pipe, or None once it is closed — after which the pipe
/// is dropped, and never polled again.
async fn next_line<R: tokio::io::AsyncBufRead + Unpin>(
    pipe: &mut Option<tokio::io::Lines<R>>,
) -> Option<String> {
    let Some(lines) = pipe.as_mut() else {
        return std::future::pending().await;
    };
    match lines.next_line().await {
        Ok(Some(line)) => Some(line),
        _ => {
            *pipe = None;
            None
        }
    }
}

/// `tailscale status --json`: connected or not, and the PC's name while it is.
fn parse_status(stdout: &str) -> (bool, Option<String>) {
    let Ok(status) = serde_json::from_str::<Value>(stdout) else {
        return (false, None);
    };
    if status["BackendState"] != "Running" {
        return (false, None);
    }
    let name = status["Self"]["DNSName"]
        .as_str()
        .map(|name| name.trim_end_matches('.'))
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    (true, name)
}

/// `tailscale serve status --json`: whether this PC's HTTPS address proxies
/// its root to the mobile server.
fn parse_serving(stdout: &str, dns_name: Option<&str>, port: u16) -> bool {
    let Ok(config) = serde_json::from_str::<Value>(stdout) else {
        return false;
    };
    let Some(sites) = config["Web"].as_object() else {
        return false;
    };
    let ours = dns_name.map(|name| format!("{name}:443"));
    sites
        .iter()
        .filter(|(host, _)| match &ours {
            Some(ours) => host.eq_ignore_ascii_case(ours),
            None => host.ends_with(":443"),
        })
        .filter_map(|(_, site)| site["Handlers"]["/"]["Proxy"].as_str())
        .any(|proxy| proxies_to(proxy, port))
}

fn proxies_to(proxy: &str, port: u16) -> bool {
    let target = proxy.strip_prefix("http://").unwrap_or(proxy);
    let target = target.trim_end_matches('/');
    target == format!("127.0.0.1:{port}") || target == format!("localhost:{port}")
}

/// The page where a tailnet's admin allows Serve, which `tailscale serve`
/// prints before it starts waiting.
fn consent_url(text: &str) -> Option<&str> {
    let start = text.find("https://login.tailscale.com/")?;
    text[start..].split_whitespace().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connected_pc_gives_its_name_without_the_root_dot() {
        let stdout = r#"{
            "BackendState": "Running",
            "Self": { "DNSName": "atelier.tail1234.ts.net.", "HostName": "atelier" }
        }"#;
        assert_eq!(
            parse_status(stdout),
            (true, Some("atelier.tail1234.ts.net".into()))
        );
    }

    #[test]
    fn a_pc_signed_out_or_stopped_is_not_running() {
        for state in ["NeedsLogin", "Stopped", "Starting", "NoState"] {
            let stdout = format!(
                r#"{{ "BackendState": "{state}", "Self": {{ "DNSName": "atelier.tail1234.ts.net." }} }}"#
            );
            assert_eq!(parse_status(&stdout), (false, None), "{state}");
        }
        assert_eq!(
            parse_status("failed to connect to local tailscaled"),
            (false, None)
        );
    }

    #[test]
    fn serving_is_this_pc_s_root_proxied_to_the_port() {
        let config = r#"{
            "TCP": { "443": { "HTTPS": true } },
            "Web": {
                "atelier.tail1234.ts.net:443": {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:47821" } }
                }
            }
        }"#;
        let name = Some("atelier.tail1234.ts.net");
        assert!(parse_serving(config, name, 47821));
        assert!(!parse_serving(config, name, 47822));
        assert!(!parse_serving(config, Some("other.tail1234.ts.net"), 47821));
        assert!(parse_serving(config, None, 47821));
    }

    #[test]
    fn serving_something_else_is_not_serving() {
        assert!(!parse_serving("{}", Some("a.ts.net"), 47821));
        assert!(!parse_serving("", Some("a.ts.net"), 47821));
        let elsewhere = r#"{ "Web": { "a.ts.net:443": {
            "Handlers": { "/docs": { "Proxy": "http://127.0.0.1:47821" },
                          "/": { "Path": "/srv/www" } } } } }"#;
        assert!(!parse_serving(elsewhere, Some("a.ts.net"), 47821));
        let other_port = r#"{ "Web": { "a.ts.net:8443": {
            "Handlers": { "/": { "Proxy": "http://127.0.0.1:47821" } } } } }"#;
        assert!(!parse_serving(other_port, Some("a.ts.net"), 47821));
    }

    #[test]
    fn a_proxy_is_recognised_however_it_is_spelled() {
        assert!(proxies_to("http://127.0.0.1:47821", 47821));
        assert!(proxies_to("http://127.0.0.1:47821/", 47821));
        assert!(proxies_to("http://localhost:47821", 47821));
        assert!(proxies_to("127.0.0.1:47821", 47821));
        assert!(!proxies_to("http://127.0.0.1:478210", 47821));
        assert!(!proxies_to("http://10.0.0.2:47821", 47821));
    }

    #[test]
    fn an_app_s_port_is_hangar_s_free_or_someone_else_s() {
        let config: Value = serde_json::from_str(
            r#"{
            "TCP": {
                "443": { "HTTPS": true },
                "8443": { "HTTPS": true },
                "8444": { "HTTPS": true },
                "8445": { "TCPForward": "127.0.0.1:22" },
                "8447": { "HTTPS": true }
            },
            "Web": {
                "atelier.tail1234.ts.net:443": {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:47821" } } },
                "atelier.tail1234.ts.net:8443": {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:47822" } } },
                "atelier.tail1234.ts.net:8444": {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:3000" } } },
                "atelier.tail1234.ts.net:8446": {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:47825" } } },
                "atelier.tail1234.ts.net:8447": {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:47826" } } }
            },
            "AllowFunnel": { "atelier.tail1234.ts.net:8447": true }
        }"#,
        )
        .unwrap();
        let name = "atelier.tail1234.ts.net";
        assert_eq!(mapping(&config, name, 8443, 47822), Mapping::Mapped);
        assert_eq!(
            mapping(&config, "ATELIER.tail1234.ts.net", 8443, 47822),
            Mapping::Mapped
        );
        // The person's own site, a TCP forward, a slot opened to the
        // internet: never Hangar's to replace.
        assert_eq!(mapping(&config, name, 8444, 47823), Mapping::Taken);
        assert_eq!(mapping(&config, name, 8445, 47824), Mapping::Taken);
        assert_eq!(mapping(&config, name, 8447, 47826), Mapping::Taken);
        // Hangar's own, half written: served again.
        assert_eq!(mapping(&config, name, 8446, 47825), Mapping::Free);
        assert_eq!(mapping(&config, name, 8448, 47827), Mapping::Free);
        assert_eq!(mapping(&json!({}), name, 8443, 47822), Mapping::Free);
        // The mobile server's port is the person's as far as a slot goes.
        assert_eq!(mapping(&config, name, 443, 47822), Mapping::Taken);
    }

    #[test]
    fn the_consent_page_is_picked_out_of_what_serve_prints() {
        let output = "Serve is not enabled on your tailnet.\nTo enable, visit:\n\n\
                      \thttps://login.tailscale.com/f/serve?node=nXyZ123\n\n";
        assert_eq!(
            consent_url(output),
            Some("https://login.tailscale.com/f/serve?node=nXyZ123")
        );
        assert_eq!(
            consent_url("Available within your tailnet: https://a.ts.net/"),
            None
        );
    }

    #[tokio::test]
    async fn a_missing_cli_is_an_error_to_run_not_a_panic() {
        let outcome = run(
            Path::new("definitely-not-a-tailscale-binary"),
            &["status"],
            Duration::from_secs(1),
            |_| false,
        )
        .await;
        assert!(outcome.is_err());
    }
}
