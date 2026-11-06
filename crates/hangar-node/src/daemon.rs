//! `serve` and `serve --detach`: one daemon per account, found again through
//! `node.json` and kept unique by the lock it holds (state.rs).
#![cfg(target_os = "linux")]

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::serve::ListenerExt;
use hangar_core::Host;

use crate::state::{self, NodeFile};

/// How long a `serve` waits on a daemon another one is starting, and
/// `--detach` on the one it started: the login shell's PATH is read first,
/// which a slow profile can stretch.
const START_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(50);

/// The node in the foreground, until SIGTERM, SIGINT or SIGHUP. A daemon of
/// the account's already running and answering is success: there is one.
pub fn serve() -> Result<(), String> {
    let dir = state::dir()?;
    state::ensure_dir(&dir)?;
    if let Some(node) = state::running(&dir) {
        eprintln!("hangar-node is already running (pid {})", node.pid);
        return Ok(());
    }
    let Some(lock) = state::try_lock(&dir)? else {
        // Another `serve` holds the lock and is starting: it is the node.
        let node = wait_for_running(&dir, || None)?;
        eprintln!("hangar-node is already running (pid {})", node.pid);
        return Ok(());
    };
    // Whatever `node.json` is left names no daemon: that one held the lock.
    let _ = std::fs::remove_file(state::node_file(&dir));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("no runtime: {err}"))?;
    let host = Host::new();
    let headless = hangar_core::headless::start(
        host.clone(),
        runtime.handle(),
        Some(state::endpoint_file(&dir)),
    )?;
    let outcome = runtime.block_on(run(&dir, host, &headless));
    state::clear(&dir);
    headless.shutdown();
    drop(lock);
    outcome
}

async fn run(
    dir: &Path,
    host: Host,
    headless: &hangar_core::headless::Headless,
) -> Result<(), String> {
    let token = hangar_core::endpoint::generate_token();
    let node = Arc::new(crate::api::Node::new(
        token.clone(),
        host,
        headless.pty().clone(),
        crate::proxy::Core {
            port: headless.port(),
            token: headless.token().to_string(),
        },
    ));
    crate::control::watch(node.host(), node.drivers.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|err| format!("the node API could not listen: {err}"))?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    state::write(
        dir,
        &NodeFile {
            version: crate::VERSION.to_string(),
            port,
            token,
            pid: std::process::id(),
        },
    )?;
    eprintln!(
        "hangar-node {} listening on 127.0.0.1:{port} (pid {})",
        crate::VERSION,
        std::process::id()
    );
    let app = crate::api::router(node.clone());
    // A key's echo is a few bytes: sent at once, not held back by Nagle's
    // algorithm until the last segment is acknowledged.
    let listener = listener.tap_io(|tcp| {
        let _ = tcp.set_nodelay(true);
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(stopped())
        .await
        .map_err(|err| format!("the node API stopped: {err}"))?;
    eprintln!("hangar-node stopping");
    node.panes.kill_all();
    Ok(())
}

/// SIGTERM (systemd, `kill`), SIGINT, or SIGHUP (the terminal of a `serve`
/// run by hand closing).
async fn stopped() {
    use tokio::signal::unix::{signal, SignalKind};
    async fn on(kind: SignalKind) {
        match signal(kind) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    }
    tokio::select! {
        _ = on(SignalKind::terminate()) => {}
        _ = on(SignalKind::interrupt()) => {}
        _ = on(SignalKind::hangup()) => {}
    }
}

/// `serve` again, in a session of its own (setsid) with its output appended
/// to `node.log`, and back once the account's daemon answers — this one, or
/// one that was already there.
pub fn detach() -> Result<(), String> {
    let dir = state::dir()?;
    state::ensure_dir(&dir)?;
    if state::running(&dir).is_some() {
        return Ok(());
    }
    let log_path = state::log_file(&dir);
    let log = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&log_path)
            .map_err(|err| format!("{}: {err}", log_path.display()))?
    };
    let err_log = log.try_clone().map_err(|err| err.to_string())?;
    let exe = std::env::current_exe().map_err(|err| format!("no executable: {err}"))?;
    let mut command = Command::new(exe);
    command
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err_log);
    // SAFETY: setsid is async-signal-safe, as pre_exec requires.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not start the daemon: {err}"))?;
    wait_for_running(&dir, || match child.try_wait() {
        // Exit 0: it found a daemon already there, which the wait finds too.
        Ok(Some(status)) if !status.success() => Some(format!(
            "the daemon exited ({status}); see {}",
            log_path.display()
        )),
        _ => None,
    })
    .map(|_| ())
}

/// Polls until the account's daemon answers, `failed` says why it never
/// will, or the time runs out.
fn wait_for_running(
    dir: &Path,
    mut failed: impl FnMut() -> Option<String>,
) -> Result<NodeFile, String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Some(node) = state::running(dir) {
            return Ok(node);
        }
        if let Some(reason) = failed() {
            return Err(reason);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "no daemon answered within {}s; see {}",
                START_TIMEOUT.as_secs(),
                state::log_file(dir).display()
            ));
        }
        std::thread::sleep(POLL);
    }
}
