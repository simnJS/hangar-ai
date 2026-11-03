//! Putting a downloaded, verified artifact in place, and starting the new
//! version afterwards.
//!
//! One function per kind of install, as `tauri-plugin-updater` had, with the
//! parts that can fail halfway made to leave the old version working:
//!
//! - **AppImage**: the new file is written beside the old one and renamed over
//!   it. A rename is atomic, so there is never a moment with half an app on
//!   disk, and the running process keeps reading the old file it already has
//!   open. (The plugin moved the old file away and then wrote the new one in
//!   its place, and restored it if the write failed.)
//! - **`.deb` / `.rpm`**: handed to the package manager with administrator
//!   rights, asked for through `pkexec`.
//! - **macOS `.app`**: unpacked next to the old bundle and swapped in with two
//!   renames; asking for administrator rights only when the folder is not the
//!   person's to write to.
//! - **Windows**: the installer is started and the app has to exit so it can
//!   replace the files; it starts the new version itself.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use flate2::read::GzDecoder;
use tempfile::TempPath;

use crate::error::{Error, Result};
use crate::platform::{Install, PackageManager};

/// Prefix of every file the updater leaves in a folder it does not own, so
/// that one found there after a crash can be recognised and deleted.
const SCRATCH_PREFIX: &str = ".hangar-update-";

/// What [`crate::Updater::install`] did, and so what the app has to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The new version is on disk in place of the old one. Save state, call
    /// [`crate::Updater::relaunch`], and exit.
    Replaced,
    /// The platform's installer was started and is waiting for this process to
    /// end before it replaces the files, and it starts the new version when it
    /// is done. Save state and exit; do not relaunch.
    InstallerStarted,
}

/// Where the download is written. It is the place the install will read it
/// from, which for an AppImage is the folder of the image itself so that
/// putting it in place is one rename on one volume.
pub(crate) fn create_staging(install: &Install) -> Result<(File, TempPath)> {
    let (dir, suffix): (PathBuf, &str) = match install {
        Install::AppImage { path } => {
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            (dir, ".part")
        }
        Install::Deb => (std::env::temp_dir(), ".deb"),
        Install::Rpm => (std::env::temp_dir(), ".rpm"),
        Install::MacApp { .. } => (std::env::temp_dir(), ".app.tar.gz"),
        Install::Nsis => (std::env::temp_dir(), ".exe"),
        Install::Msi => (std::env::temp_dir(), ".msi"),
    };
    let file = tempfile::Builder::new()
        .prefix(SCRATCH_PREFIX)
        .suffix(suffix)
        .tempfile_in(&dir)
        .map_err(|e| match install {
            Install::AppImage { path } => Error::Install(format!(
                "cannot write next to {}: {e}. Move the AppImage to a folder you can write to.",
                path.display()
            )),
            _ => Error::io(format!("creating a file in {}", dir.display()), e),
        })?;
    Ok(file.into_parts())
}

/// Installs `staged`, which is the verified artifact for `install`.
pub(crate) fn apply(
    install: &Install,
    staged: TempPath,
    app_args: &[OsString],
) -> Result<InstallOutcome> {
    match install {
        Install::AppImage { path } => {
            replace_appimage(staged, path)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::Deb => {
            install_package(staged, PackageManager::Dpkg)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::Rpm => {
            install_package(staged, PackageManager::Rpm)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::MacApp { bundle } => {
            replace_mac_bundle(&staged, bundle)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::Nsis => start_windows_installer(staged, WindowsInstaller::Nsis, app_args),
        Install::Msi => start_windows_installer(staged, WindowsInstaller::Msi, app_args),
    }
}

// ---------------------------------------------------------------- AppImage

fn is_gzip(path: &Path) -> Result<bool> {
    let mut magic = [0u8; 2];
    let mut file = File::open(path).map_err(|e| Error::io("reading the download", e))?;
    Ok(file.read_exact(&mut magic).is_ok() && magic == [0x1f, 0x8b])
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).ok().map(|m| m.permissions().mode())
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn file_mode(_: &Path) -> Option<u32> {
    None
}

#[cfg(not(unix))]
fn set_file_mode(_: &Path, _: u32) -> io::Result<()> {
    Ok(())
}

/// Replaces `target` with the AppImage in `staged`, which is either the image
/// itself or the `.tar.gz` older bundlers wrapped it in.
fn replace_appimage(staged: TempPath, target: &Path) -> Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| Error::Install(format!("{} has no folder", target.display())))?;
    let scratch = |what: &str| {
        tempfile::Builder::new()
            .prefix(SCRATCH_PREFIX)
            .suffix(".new")
            .tempfile_in(dir)
            .map_err(|e| Error::io(format!("{what} in {}", dir.display()), e))
    };

    let fresh: TempPath = if is_gzip(&staged)? {
        let mut out = scratch("unpacking the AppImage")?;
        extract_entry(&staged, "AppImage", out.as_file_mut())?;
        out.into_temp_path()
    } else if staged.parent() == Some(dir) {
        staged
    } else {
        // Written somewhere else (a caller that staged it by hand). Copying it
        // beside the target is what makes the final step a rename.
        let out = scratch("copying the AppImage")?;
        fs::copy(&staged, out.path()).map_err(|e| Error::io("copying the AppImage", e))?;
        out.into_temp_path()
    };

    // The image must stay runnable by whoever ran the old one, however it was
    // set up: same permission bits, and the owner can always execute it.
    let mode = file_mode(target).unwrap_or(0o755) | 0o100;
    set_file_mode(&fresh, mode).map_err(|e| Error::io("setting the AppImage's permissions", e))?;

    fresh
        .persist(target)
        .map_err(|e| Error::io(format!("replacing {}", target.display()), e.error))
}

/// Writes the first regular file in a `.tar.gz` whose extension is `extension`
/// to `out`.
fn extract_entry(archive: &Path, extension: &str, out: &mut File) -> Result<()> {
    let file = File::open(archive).map_err(|e| Error::io("reading the download", e))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let entries = archive
        .entries()
        .map_err(|e| Error::Install(format!("the archive cannot be read: {e}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| Error::Install(format!("the archive cannot be read: {e}")))?;
        let wanted = entry.header().entry_type().is_file()
            && entry
                .path()
                .is_ok_and(|p| p.extension() == Some(OsStr::new(extension)));
        if wanted {
            io::copy(&mut entry, out)
                .map_err(|e| Error::Install(format!("unpacking the archive: {e}")))?;
            return Ok(());
        }
    }
    Err(Error::Install(format!(
        "the archive holds no .{extension} file"
    )))
}

// ----------------------------------------------------------- .deb and .rpm

fn looks_like_package(manager: PackageManager, head: &[u8]) -> bool {
    match manager {
        // An `ar` archive.
        PackageManager::Dpkg => head.starts_with(b"!<arch>\n"),
        PackageManager::Rpm => head.starts_with(&[0xed, 0xab, 0xee, 0xdb]),
    }
}

/// The commands that install `package`, in the order they are tried.
///
/// `pkexec` asks through the desktop's own password dialog. The plugin then
/// fell back to a password box of its own through `zenity` or `kdialog`, and
/// to a terminal `sudo`; neither is carried over: a password typed into
/// someone else's window is not a thing an updater should be collecting, and a
/// GUI app has no terminal to ask on. `sudo -n` is kept for a machine where
/// the person has made it passwordless.
fn package_commands(manager: PackageManager, package: &Path) -> Vec<Vec<OsString>> {
    let tool: &[&str] = match manager {
        PackageManager::Dpkg => &["dpkg", "-i"],
        PackageManager::Rpm => &["rpm", "-U"],
    };
    let with = |prefix: &[&str]| -> Vec<OsString> {
        prefix
            .iter()
            .chain(tool)
            .map(OsString::from)
            .chain(Some(package.as_os_str().to_owned()))
            .collect()
    };
    vec![with(&["pkexec"]), with(&["sudo", "-n"])]
}

fn install_package(staged: TempPath, manager: PackageManager) -> Result<()> {
    let mut head = [0u8; 8];
    let read = File::open(&staged)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| Error::io("reading the download", e))?;
    if !looks_like_package(manager, &head[..read]) {
        return Err(Error::Install(
            "the download is not the package it should be".into(),
        ));
    }

    let commands = package_commands(manager, &staged);
    for command in &commands {
        let ran = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::null())
            .status();
        if ran.is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }

    // Keep the file: the person can run the command themselves, and the
    // message says which.
    let kept = staged
        .keep()
        .map_err(|e| Error::io("keeping the package", e.error))?;
    Err(Error::Install(format!(
        "administrator rights were not granted. The update was downloaded and checked; install it with: {}",
        manual_command(manager, &kept)
    )))
}

fn manual_command(manager: PackageManager, package: &Path) -> String {
    let tool = match manager {
        PackageManager::Dpkg => "dpkg -i",
        PackageManager::Rpm => "rpm -U",
    };
    format!("sudo {tool} '{}'", package.display())
}

// ------------------------------------------------------------------- macOS

/// Unpacks a `.app.tar.gz` into `dest`.
///
/// The archive holds one directory, `Hangar.AI.app`, which is dropped: `dest`
/// becomes the bundle. Paths that would land outside `dest` (`..`, absolute
/// ones) are refused rather than trusted; the signature already vouches for
/// the archive, and this is for the day it vouches for a mistake.
fn extract_app_archive(archive: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive).map_err(|e| Error::io("reading the download", e))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    archive.set_preserve_permissions(true);
    archive.set_unpack_xattrs(false);
    fs::create_dir_all(dest).map_err(|e| Error::io("creating the new bundle", e))?;

    let unreadable = |e: io::Error| Error::Install(format!("the archive cannot be read: {e}"));
    for entry in archive.entries().map_err(unreadable)? {
        let mut entry = entry.map_err(unreadable)?;
        let path = entry.path().map_err(unreadable)?.into_owned();
        let mut inside = PathBuf::new();
        for component in path
            .components()
            .filter(|c| *c != Component::CurDir)
            .skip(1)
        {
            match component {
                Component::Normal(part) => inside.push(part),
                _ => {
                    return Err(Error::Install(format!(
                        "the archive has an unsafe path: {}",
                        path.display()
                    )));
                }
            }
        }
        if inside.as_os_str().is_empty() {
            continue; // the bundle directory itself
        }
        let target = dest.join(&inside);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io("unpacking the app", e))?;
        }
        entry
            .unpack(&target)
            .map_err(|e| Error::Install(format!("unpacking {}: {e}", inside.display())))?;
    }
    Ok(())
}

/// Swaps `bundle` for the app in `archive`: the old one is moved aside, the new
/// one moved in, and the old one deleted. If the move in fails, the old one is
/// moved back.
fn replace_mac_bundle(archive: &Path, bundle: &Path) -> Result<()> {
    let parent = bundle
        .parent()
        .ok_or_else(|| Error::Install(format!("{} has no folder", bundle.display())))?;

    let work = match tempfile::Builder::new()
        .prefix(SCRATCH_PREFIX)
        .tempdir_in(parent)
    {
        Ok(work) => work,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return replace_mac_bundle_as_admin(archive, bundle);
        }
        Err(e) => return Err(Error::io(format!("working in {}", parent.display()), e)),
    };

    let fresh = work.path().join("new.app");
    extract_app_archive(archive, &fresh)?;

    let old = work.path().join("old.app");
    fs::rename(bundle, &old).map_err(|e| Error::io("moving the old app aside", e))?;
    if let Err(e) = fs::rename(&fresh, bundle) {
        // Put the old one back; if even that fails the work folder is kept
        // (dropped by hand below only on success) so nothing is lost.
        return match fs::rename(&old, bundle) {
            Ok(()) => Err(Error::io("moving the new app in", e)),
            Err(restore) => {
                let kept = work.keep();
                Err(Error::Install(format!(
                    "moving the new app in failed ({e}) and the old one could not be put back ({restore}); it is in {}",
                    kept.display()
                )))
            }
        };
    }
    // Finder and LaunchServices notice an app by its modification time.
    let _ = Command::new("touch").arg(bundle).status();
    Ok(())
}

/// The folder is not the person's to write to (an app installed for every
/// user, by an administrator): unpack somewhere that is, and let macOS ask for
/// the password to move it.
fn replace_mac_bundle_as_admin(archive: &Path, bundle: &Path) -> Result<()> {
    let work = tempfile::Builder::new()
        .prefix(SCRATCH_PREFIX)
        .tempdir()
        .map_err(|e| Error::io("creating a work folder", e))?;
    let fresh = work.path().join("new.app");
    extract_app_archive(archive, &fresh)?;

    let script = admin_move_script(&fresh, bundle);
    let status = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| Error::io("asking macOS for administrator rights", e))?;
    if !status.success() {
        return Err(Error::Install(
            "administrator rights were not granted, so the app was not replaced".into(),
        ));
    }
    let _ = Command::new("touch").arg(bundle).status();
    Ok(())
}

/// The AppleScript that replaces `bundle` with `fresh` as administrator.
///
/// The paths go through two layers of quoting, the shell's and AppleScript's,
/// and are applied innermost first.
fn admin_move_script(fresh: &Path, bundle: &Path) -> String {
    let sh = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"));
    let command = format!(
        "rm -rf {bundle} && mv -f {fresh} {bundle}",
        bundle = sh(bundle),
        fresh = sh(fresh)
    );
    let applescript = command.replace('\\', r"\\").replace('"', r#"\""#);
    format!("do shell script \"{applescript}\" with administrator privileges")
}

// ----------------------------------------------------------------- Windows

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsInstaller {
    Nsis,
    Msi,
}

fn start_windows_installer(
    staged: TempPath,
    kind: WindowsInstaller,
    app_args: &[OsString],
) -> Result<InstallOutcome> {
    let mut head = [0u8; 8];
    let read = File::open(&staged)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| Error::io("reading the download", e))?;
    let head = &head[..read];
    let plausible = match kind {
        WindowsInstaller::Nsis => head.starts_with(b"MZ"),
        // The compound file header Windows Installer packages start with.
        WindowsInstaller::Msi => {
            head.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])
        }
    };
    if !plausible {
        return Err(Error::Install(
            "the download is not a Windows installer (a .zip wrapper is not supported)".into(),
        ));
    }

    // The installer runs after this process is gone, so the file must outlive
    // it: the temp folder is the system's to clean.
    let path = staged
        .keep()
        .map_err(|e| Error::io("keeping the installer", e.error))?;

    let (file, parameters) = match kind {
        WindowsInstaller::Nsis => (path.into_os_string(), nsis_arguments(app_args).join(" ")),
        WindowsInstaller::Msi => {
            let msiexec = std::env::var_os("SYSTEMROOT")
                .map(|root| PathBuf::from(root).join("System32").join("msiexec.exe"))
                .unwrap_or_else(|| PathBuf::from("msiexec.exe"));
            (
                msiexec.into_os_string(),
                msi_arguments(&path, app_args).join(" "),
            )
        }
    };
    shell_execute(&file, &parameters)?;
    Ok(InstallOutcome::InstallerStarted)
}

/// The installer's command line. `/P` is "passive" (a progress bar and no
/// questions), `/R` makes it start the app again when it is done, `/UPDATE`
/// tells Hangar's installer script it is an upgrade, and what follows `/ARGS`
/// is the command line to start the app with. They are the plugin's, and
/// `src-tauri/installer/installer.nsi` reads them; the script that replaces it
/// has to keep doing so.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn nsis_arguments(app_args: &[OsString]) -> Vec<String> {
    ["/P", "/R", "/UPDATE", "/ARGS"]
        .into_iter()
        .map(String::from)
        .chain(app_args.iter().map(|a| escape_nsis_arg(a)))
        .collect()
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn msi_arguments(package: &Path, app_args: &[OsString]) -> Vec<String> {
    let launch_args = app_args
        .iter()
        .map(|a| escape_msi_property_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        "/i".into(),
        format!("\"{}\"", package.display()),
        "/passive".into(),
        "/promptrestart".into(),
        "AUTOLAUNCHAPP=True".into(),
        format!("LAUNCHAPPARGS=\"{launch_args}\""),
    ]
}

/// Quotes one argument for the NSIS command line. Adapted from the plugin,
/// which adapted the standard library's Windows argument quoting; the
/// difference is that `/` is quoted too, so that NSIS does not take a path
/// segment for one of its own switches.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn escape_nsis_arg(arg: &OsStr) -> String {
    let arg = arg.to_string_lossy();
    let quote = arg.is_empty() || arg.chars().any(|c| matches!(c, ' ' | '\t' | '/'));
    let mut out = String::new();
    if quote {
        out.push('"');
    }
    let mut backslashes = 0usize;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // 2n+1 backslashes before an embedded quote.
                out.extend(std::iter::repeat_n('\\', backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(c);
    }
    if quote {
        // 2n before the closing one.
        out.extend(std::iter::repeat_n('\\', backslashes));
        out.push('"');
    }
    out
}

/// Quotes one argument for the `LAUNCHAPPARGS="…"` property of `msiexec`,
/// where a quote is written as two.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn escape_msi_property_arg(arg: &OsStr) -> String {
    let mut arg = arg.to_string_lossy().into_owned();
    if arg.is_empty() {
        // Otherwise the argument would be lost on the way through ShellExecute.
        return "\"\"\"\"".into();
    }
    if !arg.contains(' ') && !arg.contains('"') {
        return arg;
    }
    if arg.contains('"') {
        arg = arg.replace('"', r#""""""#);
    }
    match arg.split_once('=') {
        Some((flag, value)) if arg.starts_with('-') => format!("{flag}=\"\"{value}\"\""),
        _ => format!("\"\"{arg}\"\""),
    }
}

#[cfg(windows)]
fn shell_execute(file: &OsStr, parameters: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

    let wide = |text: &OsStr| -> Vec<u16> { text.encode_wide().chain(Some(0)).collect() };
    let verb = wide(OsStr::new("open"));
    let file = wide(file);
    let parameters = wide(OsStr::new(parameters));

    // SAFETY: every pointer is to a NUL-terminated UTF-16 string that outlives
    // the call, or null where the function accepts it.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOW,
        )
    };
    // Values above 32 are success, whatever their meaning; below, an error code.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(Error::Install(format!(
            "Windows could not start the installer (code {})",
            result as isize
        )))
    }
}

#[cfg(not(windows))]
fn shell_execute(_: &OsStr, _: &str) -> Result<()> {
    Err(Error::Unsupported(
        "the Windows installer can only be started on Windows".into(),
    ))
}

// --------------------------------------------------------------- relaunch

/// The command that starts the new version once this process has exited, or
/// `None` where the installer does that itself (Windows).
///
/// The process is not replaced from the inside: Hangar has terminals to hang
/// up and state to save before it ends, and has to be the one to end. So the
/// command waits for `parent_pid` to be gone (for a few seconds at most) and
/// only then starts the app, which also means the new copy never finds the old
/// one still holding its single-instance lock or its window.
pub(crate) fn relaunch_command(
    install: &Install,
    exe: &Path,
    app_args: &[OsString],
    parent_pid: u32,
) -> Option<Command> {
    const WAIT_THEN_EXEC: &str = r#"pid=$1; shift; n=0
while kill -0 "$pid" 2>/dev/null && [ "$n" -lt 150 ]; do sleep 0.2; n=$((n + 1)); done
exec "$@""#;

    let (program, args): (OsString, Vec<OsString>) = match install {
        Install::Nsis | Install::Msi => return None,
        Install::AppImage { path } => (path.clone().into_os_string(), app_args.to_vec()),
        Install::Deb | Install::Rpm => (exe.as_os_str().to_owned(), app_args.to_vec()),
        Install::MacApp { bundle } => {
            let mut args = vec![OsString::from("-n"), bundle.clone().into_os_string()];
            if !app_args.is_empty() {
                args.push("--args".into());
                args.extend_from_slice(app_args);
            }
            ("open".into(), args)
        }
    };

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(WAIT_THEN_EXEC)
        .arg("hangar-relaunch")
        .arg(parent_pid.to_string())
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if matches!(install, Install::AppImage { .. }) {
        // These describe the mount of the image that is about to be replaced.
        // The runtime sets them again for the new one, and would otherwise be
        // starting beside a stale `APPDIR`.
        command
            .env_remove("APPDIR")
            .env_remove("ARGV0")
            .env_remove("OWD");
    }

    // Its own process group, so a signal sent to Hangar's group while it shuts
    // down does not take the launcher with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tar_gz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, data, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// An AppImage standing in `dir`, and a staging file for `install`.
    fn staged_appimage(dir: &Path, old: &[u8], mode: u32) -> (Install, File, TempPath) {
        let path = dir.join("Hangar.AI.AppImage");
        fs::write(&path, old).unwrap();
        set_file_mode(&path, mode).unwrap();
        let install = Install::AppImage { path };
        let (file, staged) = create_staging(&install).unwrap();
        (install, file, staged)
    }

    #[test]
    #[cfg(unix)]
    fn an_appimage_is_replaced_atomically_and_keeps_its_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let (install, mut file, staged) = staged_appimage(dir.path(), b"old image", 0o750);
        let Install::AppImage { path } = &install else {
            unreachable!()
        };
        file.write_all(b"new image").unwrap();
        drop(file);

        // The running process has the old image open, as an AppImage does.
        let mut running = File::open(path).unwrap();

        let outcome = apply(&install, staged, &[]).unwrap();
        assert_eq!(outcome, InstallOutcome::Replaced);

        assert_eq!(fs::read(path).unwrap(), b"new image");
        assert_eq!(file_mode(path).unwrap() & 0o777, 0o750);
        // It kept reading the old file: the new one is a different inode.
        let mut seen = String::new();
        running.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, "old image");
        // And nothing is left beside it.
        assert_eq!(names(dir.path()), ["Hangar.AI.AppImage"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_replaced_appimage_can_always_be_run_by_its_owner() {
        let dir = tempfile::tempdir().unwrap();
        // An image somebody left without the execute bit.
        let (install, mut file, staged) = staged_appimage(dir.path(), b"old", 0o640);
        let Install::AppImage { path } = &install else {
            unreachable!()
        };
        file.write_all(b"new").unwrap();
        drop(file);
        apply(&install, staged, &[]).unwrap();
        assert_eq!(file_mode(path).unwrap() & 0o777, 0o740);
    }

    #[test]
    fn the_staging_file_goes_beside_the_appimage_and_cleans_up_after_itself() {
        let dir = tempfile::tempdir().unwrap();
        let (_, file, staged) = staged_appimage(dir.path(), b"old", 0o755);
        assert_eq!(staged.parent(), Some(dir.path()));
        assert_eq!(names(dir.path()).len(), 2);
        // A cancelled or failed update drops the staging file and leaves the
        // folder as it was.
        drop(file);
        drop(staged);
        assert_eq!(names(dir.path()), ["Hangar.AI.AppImage"]);
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_said_before_the_download() {
        let install = Install::AppImage {
            path: "/nonexistent-folder/Hangar.AI.AppImage".into(),
        };
        match create_staging(&install) {
            Err(Error::Install(why)) => assert!(why.contains("Move the AppImage"), "{why}"),
            other => panic!("expected an install error, got {other:?}"),
        }
    }

    #[test]
    fn an_appimage_in_a_tarball_is_unpacked() {
        let dir = tempfile::tempdir().unwrap();
        let (install, mut file, staged) = staged_appimage(dir.path(), b"old", 0o755);
        let Install::AppImage { path } = &install else {
            unreachable!()
        };
        let archive = tar_gz(&[
            ("README", b"not this one", 0o644),
            ("Hangar.AI_1.13.0_amd64.AppImage", b"unpacked image", 0o755),
        ]);
        file.write_all(&archive).unwrap();
        drop(file);

        apply(&install, staged, &[]).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"unpacked image");
        assert_eq!(names(dir.path()), ["Hangar.AI.AppImage"]);
    }

    #[test]
    fn a_tarball_without_an_appimage_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (install, mut file, staged) = staged_appimage(dir.path(), b"old", 0o755);
        let Install::AppImage { path } = &install else {
            unreachable!()
        };
        file.write_all(&tar_gz(&[("README", b"nothing to run", 0o644)]))
            .unwrap();
        drop(file);

        match apply(&install, staged, &[]) {
            Err(Error::Install(why)) => assert!(why.contains("no .AppImage"), "{why}"),
            other => panic!("expected an install error, got {other:?}"),
        }
        assert_eq!(fs::read(path).unwrap(), b"old");
        assert_eq!(
            names(dir.path()),
            ["Hangar.AI.AppImage"],
            "no scratch file left behind"
        );
    }

    #[test]
    fn a_download_staged_elsewhere_is_copied_beside_the_target() {
        let app_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let path = app_dir.path().join("Hangar.AI.AppImage");
        fs::write(&path, b"old").unwrap();
        let install = Install::AppImage { path: path.clone() };

        let staged = elsewhere.path().join("download");
        fs::write(&staged, b"new").unwrap();
        let staged = TempPath::try_from_path(staged).unwrap();

        apply(&install, staged, &[]).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(names(app_dir.path()), ["Hangar.AI.AppImage"]);
        assert!(
            names(elsewhere.path()).is_empty(),
            "the download is removed once used"
        );
    }

    #[test]
    fn a_mac_archive_unpacks_without_its_top_folder() {
        let work = tempfile::tempdir().unwrap();
        let archive_path = work.path().join("Hangar.AI.app.tar.gz");
        fs::write(
            &archive_path,
            tar_gz(&[
                ("Hangar.AI.app/Contents/Info.plist", b"<plist/>", 0o644),
                (
                    "./Hangar.AI.app/Contents/MacOS/hangar-ai",
                    b"#!binary",
                    0o755,
                ),
            ]),
        )
        .unwrap();

        let dest = work.path().join("out.app");
        extract_app_archive(&archive_path, &dest).unwrap();
        assert_eq!(
            fs::read(dest.join("Contents/Info.plist")).unwrap(),
            b"<plist/>"
        );
        assert_eq!(
            fs::read(dest.join("Contents/MacOS/hangar-ai")).unwrap(),
            b"#!binary"
        );
        #[cfg(unix)]
        assert_eq!(
            file_mode(&dest.join("Contents/MacOS/hangar-ai")).unwrap() & 0o777,
            0o755
        );
    }

    #[test]
    fn a_mac_archive_cannot_write_outside_the_bundle() {
        let work = tempfile::tempdir().unwrap();
        let archive_path = work.path().join("evil.tar.gz");
        // `tar::Builder` refuses to write `..`, so the header is written raw.
        let mut raw = tar::Header::new_gnu();
        let name = b"Hangar.AI.app/../../escaped";
        raw.as_old_mut().name[..name.len()].copy_from_slice(name);
        raw.set_size(1);
        raw.set_mode(0o644);
        raw.set_cksum();
        let mut tar_bytes = raw.as_bytes().to_vec();
        tar_bytes.extend_from_slice(&[b'x'; 512]);
        tar_bytes.extend_from_slice(&[0u8; 1024]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        fs::write(&archive_path, gz.finish().unwrap()).unwrap();

        let dest = work.path().join("inner/out.app");
        match extract_app_archive(&archive_path, &dest) {
            Err(Error::Install(why)) => assert!(why.contains("unsafe path"), "{why}"),
            other => panic!("expected the path to be refused, got {other:?}"),
        }
        assert!(!work.path().join("escaped").exists());
        assert!(!work.path().join("inner/escaped").exists());
    }

    #[test]
    #[cfg(unix)]
    fn a_mac_bundle_is_swapped_for_the_new_one() {
        let apps = tempfile::tempdir().unwrap();
        let bundle = apps.path().join("Hangar.AI.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/MacOS/hangar-ai"), b"old binary").unwrap();
        fs::write(bundle.join("Contents/old-only"), b"gone after the update").unwrap();

        let archive_path = apps.path().join("update.tar.gz");
        fs::write(
            &archive_path,
            tar_gz(&[(
                "Hangar.AI.app/Contents/MacOS/hangar-ai",
                b"new binary",
                0o755,
            )]),
        )
        .unwrap();

        replace_mac_bundle(&archive_path, &bundle).unwrap();

        assert_eq!(
            fs::read(bundle.join("Contents/MacOS/hangar-ai")).unwrap(),
            b"new binary"
        );
        assert!(!bundle.join("Contents/old-only").exists());
        assert_eq!(
            names(apps.path()),
            ["Hangar.AI.app", "update.tar.gz"],
            "the old bundle and the work folder are gone"
        );
    }

    #[test]
    fn the_administrator_script_survives_hostile_paths() {
        let script = admin_move_script(
            Path::new("/tmp/it's/new.app"),
            Path::new(r#"/Applications/My "App" \ Hangar.app"#),
        );
        assert_eq!(
            script,
            r#"do shell script "rm -rf '/Applications/My \"App\" \\ Hangar.app' && mv -f '/tmp/it'\\''s/new.app' '/Applications/My \"App\" \\ Hangar.app'" with administrator privileges"#
        );
    }

    #[test]
    fn packages_are_installed_through_pkexec_then_passwordless_sudo() {
        let deb = package_commands(PackageManager::Dpkg, Path::new("/tmp/x.deb"));
        assert_eq!(
            deb,
            [
                ["pkexec", "dpkg", "-i", "/tmp/x.deb"]
                    .map(OsString::from)
                    .to_vec(),
                ["sudo", "-n", "dpkg", "-i", "/tmp/x.deb"]
                    .map(OsString::from)
                    .to_vec(),
            ]
        );
        let rpm = package_commands(PackageManager::Rpm, Path::new("/tmp/x.rpm"));
        assert_eq!(
            rpm[0],
            ["pkexec", "rpm", "-U", "/tmp/x.rpm"]
                .map(OsString::from)
                .to_vec()
        );
    }

    #[test]
    fn a_package_must_look_like_one() {
        assert!(looks_like_package(
            PackageManager::Dpkg,
            b"!<arch>\ndebian-binary"
        ));
        assert!(!looks_like_package(PackageManager::Dpkg, b"\x7fELF"));
        assert!(looks_like_package(
            PackageManager::Rpm,
            &[0xed, 0xab, 0xee, 0xdb, 3, 0]
        ));
        assert!(!looks_like_package(PackageManager::Rpm, b"!<arch>\n"));
        assert!(!looks_like_package(PackageManager::Rpm, b""));

        // The check runs before anything is asked of the person.
        let mut file = tempfile::Builder::new().suffix(".deb").tempfile().unwrap();
        file.write_all(b"\x7fELF not a package").unwrap();
        match install_package(file.into_temp_path(), PackageManager::Dpkg) {
            Err(Error::Install(why)) => assert!(why.contains("not the package"), "{why}"),
            other => panic!("expected the file to be refused, got {other:?}"),
        }
    }

    // The cases and their expected output are the plugin's own tests for these
    // two functions, so a command line that worked there works here.
    const ARG_CASES: [&str; 11] = [
        "something",
        "--flag",
        "--empty=",
        "--arg=value",
        "some space",
        "--arg value",
        "--arg=unwrapped space",
        "--arg=\"wrapped\"",
        "--arg=\"wrapped space\"",
        "--arg=midword\"wrapped space\"",
        "",
    ];

    #[test]
    fn nsis_arguments_are_quoted_as_the_plugin_did() {
        let expected = [
            "something",
            "--flag",
            "--empty=",
            "--arg=value",
            "\"some space\"",
            "\"--arg value\"",
            "\"--arg=unwrapped space\"",
            "--arg=\\\"wrapped\\\"",
            "\"--arg=\\\"wrapped space\\\"\"",
            "\"--arg=midword\\\"wrapped space\\\"\"",
            "\"\"",
        ];
        for (arg, want) in ARG_CASES.iter().zip(expected) {
            assert_eq!(escape_nsis_arg(OsStr::new(arg)), want, "for {arg:?}");
        }
        // A path segment is not an NSIS switch.
        assert_eq!(escape_nsis_arg(OsStr::new("/D=C:\\x")), "\"/D=C:\\x\"");
    }

    #[test]
    fn msi_arguments_are_quoted_as_the_plugin_did() {
        let expected = [
            "something",
            "--flag",
            "--empty=",
            "--arg=value",
            "\"\"some space\"\"",
            "\"\"--arg value\"\"",
            "--arg=\"\"unwrapped space\"\"",
            r#"--arg=""""""wrapped"""""""#,
            r#"--arg=""""""wrapped space"""""""#,
            r#"--arg=""midword""""wrapped space"""""""#,
            "\"\"\"\"",
        ];
        for (arg, want) in ARG_CASES.iter().zip(expected) {
            assert_eq!(
                escape_msi_property_arg(OsStr::new(arg)),
                want,
                "for {arg:?}"
            );
        }
    }

    #[test]
    fn the_windows_command_lines_match_what_the_installer_reads() {
        assert_eq!(nsis_arguments(&[]), ["/P", "/R", "/UPDATE", "/ARGS"]);
        assert_eq!(
            nsis_arguments(&[OsString::from("--workspace"), OsString::from("my project")]),
            [
                "/P",
                "/R",
                "/UPDATE",
                "/ARGS",
                "--workspace",
                "\"my project\""
            ]
        );
        assert_eq!(
            msi_arguments(Path::new(r"C:\Temp\Hangar.msi"), &[]),
            [
                "/i",
                r#""C:\Temp\Hangar.msi""#,
                "/passive",
                "/promptrestart",
                "AUTOLAUNCHAPP=True",
                r#"LAUNCHAPPARGS="""#,
            ]
        );
    }

    fn command_line(command: &Command) -> Vec<String> {
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_new_version_is_started_from_the_right_place() {
        let args = [OsString::from("--flag")];
        let waiting = |install: &Install, exe: &str| {
            let command = relaunch_command(install, Path::new(exe), &args, 4242).unwrap();
            let line = command_line(&command);
            assert_eq!(&line[..2], ["sh", "-c"]);
            assert_eq!(&line[3..5], ["hangar-relaunch", "4242"]);
            line[5..].to_vec()
        };

        // An AppImage is started as the image, never as the file inside its mount.
        let appimage = Install::AppImage {
            path: "/home/me/Hangar.AI.AppImage".into(),
        };
        assert_eq!(
            waiting(&appimage, "/tmp/.mount_x/usr/bin/hangar-ai"),
            ["/home/me/Hangar.AI.AppImage", "--flag"]
        );
        assert_eq!(
            waiting(&Install::Deb, "/usr/bin/hangar-ai"),
            ["/usr/bin/hangar-ai", "--flag"]
        );
        let mac = Install::MacApp {
            bundle: "/Applications/Hangar.AI.app".into(),
        };
        assert_eq!(
            waiting(&mac, "/Applications/Hangar.AI.app/Contents/MacOS/hangar-ai"),
            [
                "open",
                "-n",
                "/Applications/Hangar.AI.app",
                "--args",
                "--flag"
            ]
        );
    }

    #[test]
    fn windows_starts_itself_through_the_installer() {
        assert!(relaunch_command(&Install::Nsis, Path::new("x.exe"), &[], 1).is_none());
        assert!(relaunch_command(&Install::Msi, Path::new("x.exe"), &[], 1).is_none());
    }

    #[test]
    #[cfg(unix)]
    fn the_launcher_waits_for_the_old_process_then_runs_the_new_one() {
        // The real script, against a real process that ends, starting a script
        // that records it ran.
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("started");
        let launched = dir.path().join("Hangar.AI.AppImage");
        fs::write(
            &launched,
            format!("#!/bin/sh\necho \"$1\" > '{}'\n", marker.display()),
        )
        .unwrap();
        set_file_mode(&launched, 0o755).unwrap();

        let mut old = Command::new("sleep").arg("0.5").spawn().unwrap();
        let install = Install::AppImage { path: launched };
        let mut command = relaunch_command(
            &install,
            Path::new("/ignored"),
            &[OsString::from("hello")],
            old.id(),
        )
        .unwrap();
        let mut launcher = command.spawn().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(!marker.exists(), "it must wait for the old process");
        old.wait().unwrap();
        launcher.wait().unwrap();
        assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "hello");
    }
}
