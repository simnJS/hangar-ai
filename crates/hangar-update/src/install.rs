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
//!
//! # Verified twice
//!
//! The download is checked against its signature when it finishes, and then
//! waits, possibly for hours, until the person clicks *Restart*. Anything
//! running as the same user could rewrite it in between, and a package or an
//! admin bundle swap runs as root behind a password the person types for
//! Hangar. So:
//!
//! - everything but an AppImage is downloaded into a folder created fresh for
//!   it, `0700`, in the temp folder, which keeps other users out;
//! - [`apply`] checks the signature again just before anything is installed,
//!   reading the file once through a handle opened on a plain file (a symlink
//!   is refused), and keeps the SHA-256 of exactly the bytes it checked;
//! - the privileged step (`pkexec`/`sudo` for a package, the administrator
//!   AppleScript on macOS) copies the file into a folder only root can write,
//!   and refuses it unless that copy has the same SHA-256. What root installs
//!   is therefore the bytes that were verified, whatever happened to the file
//!   in the user's folder;
//! - on Windows the file is opened with no write or delete sharing before it
//!   is verified, and that handle stays open until the process exits, so it
//!   cannot be changed between the check and the installer reading it.
//!
//! An AppImage is the user's own file, replaced with the user's rights: no
//! privilege is crossed, and it is re-verified like the others all the same.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use std::time::{Duration, SystemTime};

use flate2::read::GzDecoder;
use tempfile::{TempDir, TempPath};

use crate::error::{Error, Result};
use crate::platform::{Install, PackageManager};
use crate::sha256;
use crate::signature;

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

/// A download waiting to be installed: the file, and the private folder it
/// was written in (none for an AppImage, which is staged beside the image so
/// that putting it in place is a rename). Dropping it deletes both.
#[derive(Debug)]
pub(crate) struct Staged {
    // Declared before `dir` so the file goes first.
    path: TempPath,
    dir: Option<TempDir>,
}

impl Staged {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn from_path(path: TempPath) -> Self {
        Self { path, dir: None }
    }
}

/// What the artifact must verify against.
#[derive(Clone, Copy)]
pub(crate) struct Signed<'a> {
    pub pubkey: &'a str,
    pub signature: &'a str,
}

/// A folder only this user can enter, created fresh for one download.
fn private_dir() -> Result<TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(SCRATCH_PREFIX);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir()
        .map_err(|e| Error::io("creating a private folder for the download", e))
}

/// Where the download is written. It is the place the install will read it
/// from: for an AppImage the folder of the image itself, so that putting it in
/// place is one rename on one volume; for everything else a private folder of
/// its own in the temp folder.
pub(crate) fn create_staging(install: &Install) -> Result<(File, Staged)> {
    let (dir, private, suffix): (PathBuf, Option<TempDir>, &str) = match install {
        Install::AppImage { path } => {
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            (dir, None, ".part")
        }
        other => {
            let suffix = match other {
                Install::Deb => ".deb",
                Install::Rpm => ".rpm",
                Install::MacApp { .. } => ".app.tar.gz",
                Install::Nsis => ".exe",
                _ => ".msi",
            };
            let private = private_dir()?;
            (private.path().to_path_buf(), Some(private), suffix)
        }
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
    let (file, path) = file.into_parts();
    Ok((file, Staged { path, dir: private }))
}

/// Opens `path` for the second check: it must still be a plain file, not a
/// symlink put in its place. On Windows nobody may write, rename or delete it
/// while the handle is open.
fn open_plain_file(path: &Path) -> Result<File> {
    let changed =
        || Error::Install("the download is no longer a plain file; nothing was installed".into());
    let link = fs::symlink_metadata(path).map_err(|e| Error::io("reading the download", e))?;
    if !link.file_type().is_file() {
        return Err(changed());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        options.share_mode(FILE_SHARE_READ);
    }
    let file = options
        .open(path)
        .map_err(|e| Error::io("reading the download", e))?;
    // The file opened is the one that was looked at, not something swapped in
    // between the two calls.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file
            .metadata()
            .map_err(|e| Error::io("reading the download", e))?;
        if !opened.is_file() || opened.dev() != link.dev() || opened.ino() != link.ino() {
            return Err(changed());
        }
    }
    Ok(file)
}

/// Checks the signature of `path` again, reading it once through a handle on
/// a plain file, and returns the SHA-256 of the bytes checked with that handle.
fn reverify(path: &Path, signed: Signed<'_>) -> Result<([u8; 32], File)> {
    let mut file = open_plain_file(path)?;
    let digest = signature::verify_reader(&mut file, signed.pubkey, signed.signature)?;
    Ok((digest, file))
}

/// Installs `staged`, the artifact for `install`, after checking its signature
/// once more (see the module's documentation).
pub(crate) fn apply(
    install: &Install,
    staged: Staged,
    signed: Signed<'_>,
    app_args: &[OsString],
) -> Result<InstallOutcome> {
    let (digest, file) = reverify(staged.path(), signed)?;
    match install {
        Install::AppImage { path } => {
            drop(file);
            replace_appimage(staged.path, path)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::Deb => {
            drop(file);
            install_package(staged.path(), PackageManager::Dpkg, &digest)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::Rpm => {
            drop(file);
            install_package(staged.path(), PackageManager::Rpm, &digest)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::MacApp { bundle } => {
            drop(file);
            replace_mac_bundle(staged.path(), bundle, &digest)?;
            Ok(InstallOutcome::Replaced)
        }
        Install::Nsis => start_windows_installer(staged, file, WindowsInstaller::Nsis, app_args),
        Install::Msi => start_windows_installer(staged, file, WindowsInstaller::Msi, app_args),
    }
}

/// Deletes what earlier updates left in `dir` (the temp folder): a Windows
/// installer has to outlive the process that started it, so it is kept, and
/// removed here at a later start once it is older than `older_than`.
pub(crate) fn sweep_stale(dir: &Path, older_than: Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(SCRATCH_PREFIX)
        {
            continue;
        }
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= older_than);
        if !old {
            continue;
        }
        let _ = if meta.is_dir() {
            fs::remove_dir_all(entry.path())
        } else {
            fs::remove_file(entry.path())
        };
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

/// Exit status of a privileged script that found the file changed since it
/// was verified.
const CHANGED_AFTER_CHECK: i32 = 97;

/// The shell script run as root that installs a package: it copies `$1` into a
/// folder only root can write, refuses the copy unless its SHA-256 is `$2`
/// (the digest of the bytes whose signature was checked), and installs the
/// copy. A file swapped in the user's folder at any time before the copy is
/// refused; one swapped after it is not what is installed.
fn package_script(manager: PackageManager) -> String {
    let (name, tool) = match manager {
        PackageManager::Dpkg => ("update.deb", "dpkg -i"),
        PackageManager::Rpm => ("update.rpm", "rpm -U"),
    };
    format!(
        r#"set -eu
umask 022
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -- "$1" "$work/{name}"
sum=$(sha256sum "$work/{name}")
if [ "${{sum%% *}}" != "$2" ]; then
  echo "hangar-update: the package changed after it was verified" >&2
  exit {CHANGED_AFTER_CHECK}
fi
{tool} "$work/{name}""#
    )
}

/// The commands that install `package`, in the order they are tried.
///
/// `pkexec` asks through the desktop's own password dialog. The plugin then
/// fell back to a password box of its own through `zenity` or `kdialog`, and
/// to a terminal `sudo`; neither is carried over: a password typed into
/// someone else's window is not a thing an updater should be collecting, and a
/// GUI app has no terminal to ask on. `sudo -n` is kept for a machine where
/// the person has made it passwordless.
fn package_commands(manager: PackageManager, package: &Path, digest: &str) -> Vec<Vec<OsString>> {
    let script = package_script(manager);
    let with = |prefix: &[&str]| -> Vec<OsString> {
        prefix
            .iter()
            .map(OsString::from)
            .chain([
                OsString::from("/bin/sh"),
                OsString::from("-c"),
                OsString::from(&script),
                OsString::from("hangar-update"),
                package.as_os_str().to_owned(),
                OsString::from(digest),
            ])
            .collect()
    };
    vec![with(&["pkexec"]), with(&["sudo", "-n"])]
}

fn install_package(package: &Path, manager: PackageManager, digest: &[u8; 32]) -> Result<()> {
    let mut head = [0u8; 8];
    let read = File::open(package)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| Error::io("reading the download", e))?;
    if !looks_like_package(manager, &head[..read]) {
        return Err(Error::Install(
            "the download is not the package it should be".into(),
        ));
    }

    let commands = package_commands(manager, package, &sha256::hex(digest));
    for command in &commands {
        let ran = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::null())
            .status();
        match ran {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) if status.code() == Some(CHANGED_AFTER_CHECK) => {
                return Err(changed_after_check());
            }
            _ => {}
        }
    }

    // The file is not kept for the person to install by hand: it sits in a
    // folder they can write to, which is the very thing the checks above are
    // for. *Retry* downloads it again.
    Err(Error::Install(
        "administrator rights were not granted, so nothing was installed".into(),
    ))
}

fn changed_after_check() -> Error {
    Error::Signature("the download changed after it was verified; nothing was installed".into())
}

// ------------------------------------------------------------------- macOS

/// Unpacks a `.app.tar.gz` into `dest`.
///
/// The archive holds one directory, `Hangar.AI.app`, which is dropped: `dest`
/// becomes the bundle. Paths that would land outside `dest` are refused rather
/// than trusted: `..` and absolute ones, and any that go through a symlink an
/// earlier entry made (`X -> /Users/me/.ssh`, then `X/authorized_keys`). The
/// signature already vouches for the archive; this is for the day it vouches
/// for a mistake.
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
        if goes_through_symlink(dest, &inside) {
            return Err(Error::Install(format!(
                "the archive has an unsafe path: {} goes through a symlink",
                path.display()
            )));
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

/// Whether `dest/inside` is reached through a symlink already on disk under
/// `dest`, or is one itself: writing there would follow it.
fn goes_through_symlink(dest: &Path, inside: &Path) -> bool {
    let mut at = dest.to_path_buf();
    inside.components().any(|part| {
        at.push(part);
        fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink())
    })
}

/// Swaps `bundle` for the app in `archive`: the old one is moved aside, the new
/// one moved in, and the old one deleted. If the move in fails, the old one is
/// moved back. `digest` is the SHA-256 of the verified archive, which the
/// administrator path checks again as root.
fn replace_mac_bundle(archive: &Path, bundle: &Path, digest: &[u8; 32]) -> Result<()> {
    let parent = bundle
        .parent()
        .ok_or_else(|| Error::Install(format!("{} has no folder", bundle.display())))?;

    let work = match tempfile::Builder::new()
        .prefix(SCRATCH_PREFIX)
        .tempdir_in(parent)
    {
        Ok(work) => work,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return replace_mac_bundle_as_admin(archive, bundle, digest);
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
/// user, by an administrator): macOS asks for the password, and everything
/// from there runs as root in [`admin_swap_command`], unpacking included, so
/// that no file the person's account can write is used after the check.
fn replace_mac_bundle_as_admin(archive: &Path, bundle: &Path, digest: &[u8; 32]) -> Result<()> {
    let script = admin_move_script(&admin_swap_command(archive, &sha256::hex(digest), bundle)?);
    let status = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| Error::io("asking macOS for administrator rights", e))?;
    if status.code() == Some(CHANGED_AFTER_CHECK) {
        return Err(changed_after_check());
    }
    if !status.success() {
        return Err(Error::Install(
            "administrator rights were not granted, or the new app could not be put in place; the old one was kept".into(),
        ));
    }
    let _ = Command::new("touch").arg(bundle).status();
    Ok(())
}

/// Quotes `p` for the shell.
fn sh_quote(p: &OsStr) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// The shell command that replaces `bundle` with the app in `archive`, run as
/// root. It copies the archive into a folder of its own beside the bundle
/// (root's, since the person cannot write there), refuses the copy unless its
/// SHA-256 is `digest`, unpacks it there, and swaps the bundles the way the
/// unprivileged path does: the old one moved aside, the new one moved in, the
/// old one moved back if that fails, and deleted only once it worked.
fn admin_swap_command(archive: &Path, digest: &str, bundle: &Path) -> Result<String> {
    let parent = bundle
        .parent()
        .ok_or_else(|| Error::Install(format!("{} has no folder", bundle.display())))?;
    Ok(format!(
        r#"set -eu
umask 022
src={src}
sum={sum}
bundle={bundle}
work=$(mktemp -d {parent}/{SCRATCH_PREFIX}XXXXXX)
trap 'rm -rf "$work"' EXIT
cp -- "$src" "$work/update.tar.gz"
got=$(shasum -a 256 "$work/update.tar.gz")
if [ "${{got%% *}}" != "$sum" ]; then
  echo "hangar-update: the archive changed after it was verified" >&2
  exit {CHANGED_AFTER_CHECK}
fi
mkdir "$work/new"
tar -xzf "$work/update.tar.gz" --no-same-owner -C "$work/new"
set -- "$work/new"/*.app
if [ "$#" -ne 1 ] || [ -L "$1" ] || [ ! -d "$1" ]; then
  echo "hangar-update: the archive does not hold one app" >&2
  exit 98
fi
mv -- "$bundle" "$work/old.app"
if ! mv -- "$1" "$bundle"; then
  if ! mv -- "$work/old.app" "$bundle"; then
    trap - EXIT
    echo "hangar-update: the old app could not be put back; it is in $work/old.app" >&2
  fi
  exit 99
fi"#,
        src = sh_quote(archive.as_os_str()),
        sum = sh_quote(OsStr::new(digest)),
        bundle = sh_quote(bundle.as_os_str()),
        parent = sh_quote(parent.as_os_str()),
    ))
}

/// The AppleScript that runs `command` as administrator. The command is put
/// in an AppleScript string, where a backslash, a quote and a line break are
/// escaped.
fn admin_move_script(command: &str) -> String {
    let applescript = command
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('\n', r"\n");
    format!("do shell script \"{applescript}\" with administrator privileges")
}

// ----------------------------------------------------------------- Windows

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsInstaller {
    Nsis,
    Msi,
}

/// Starts the installer in `staged`. `verified` is the handle the signature
/// was just checked through; on Windows it allows no writing, renaming or
/// deleting, and it is left open until the process exits so the installer
/// starts on the bytes that were checked.
fn start_windows_installer(
    staged: Staged,
    mut verified: File,
    kind: WindowsInstaller,
    app_args: &[OsString],
) -> Result<InstallOutcome> {
    use std::io::{Seek, SeekFrom};
    let mut head = [0u8; 8];
    let read = verified
        .seek(SeekFrom::Start(0))
        .and_then(|_| verified.read(&mut head))
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
    // it; `sweep_stale` deletes it at a later start.
    let Staged { path, dir } = staged;
    let path = path
        .keep()
        .map_err(|e| Error::io("keeping the installer", e.error))?;
    if let Some(dir) = dir {
        let _ = dir.keep();
    }

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
    // Held, write-locked, until the process ends.
    std::mem::forget(verified);
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
/// command waits for `parent_pid` to be gone (30 seconds at most) and only
/// then starts the app, which also means the new copy never finds the old one
/// still holding its single-instance lock or its window. If the old process is
/// still there after that, nothing is started: a second instance beside a
/// stuck one is worse than starting Hangar by hand.
pub(crate) fn relaunch_command(
    install: &Install,
    exe: &Path,
    app_args: &[OsString],
    parent_pid: u32,
) -> Option<Command> {
    const WAIT_THEN_EXEC: &str = r#"pid=$1; shift; n=0
while kill -0 "$pid" 2>/dev/null; do
  [ "$n" -ge 150 ] && exit 1
  sleep 0.2; n=$((n + 1))
done
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
    use crate::fixtures::{PUBKEY, SIG_PREHASHED, payload};
    use std::io::Write;

    const SIGNED: Signed<'static> = Signed {
        pubkey: PUBKEY,
        signature: SIG_PREHASHED,
    };

    #[cfg(unix)]
    fn digest_of(bytes: &[u8]) -> [u8; 32] {
        let mut h = sha256::Sha256::new();
        h.update(bytes);
        h.finalize()
    }

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
    fn staged_appimage(dir: &Path, old: &[u8], mode: u32) -> (Install, File, Staged) {
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
        file.write_all(&payload()).unwrap();
        drop(file);

        // The running process has the old image open, as an AppImage does.
        let mut running = File::open(path).unwrap();

        let outcome = apply(&install, staged, SIGNED, &[]).unwrap();
        assert_eq!(outcome, InstallOutcome::Replaced);

        assert_eq!(fs::read(path).unwrap(), payload());
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
        file.write_all(&payload()).unwrap();
        drop(file);
        apply(&install, staged, SIGNED, &[]).unwrap();
        assert_eq!(file_mode(path).unwrap() & 0o777, 0o740);
    }

    #[test]
    fn the_staging_file_goes_beside_the_appimage_and_cleans_up_after_itself() {
        let dir = tempfile::tempdir().unwrap();
        let (_, file, staged) = staged_appimage(dir.path(), b"old", 0o755);
        assert_eq!(staged.path().parent(), Some(dir.path()));
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

        // Past the signature check, which a made-up archive cannot pass.
        replace_appimage(staged.path, path).unwrap();
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

        match replace_appimage(staged.path, path) {
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
        fs::write(&staged, payload()).unwrap();
        let staged = Staged::from_path(TempPath::try_from_path(staged).unwrap());

        apply(&install, staged, SIGNED, &[]).unwrap();
        assert_eq!(fs::read(&path).unwrap(), payload());
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

        replace_mac_bundle(&archive_path, &bundle, &[0; 32]).unwrap();

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

    // The paths are macOS ones: on Windows a backslash in them is a separator.
    #[cfg(unix)]
    #[test]
    fn the_administrator_script_survives_hostile_paths() {
        let command = admin_swap_command(
            Path::new("/tmp/it's/update.tar.gz"),
            "ab12",
            Path::new(r#"/Applications/My "App" \ Hangar.app"#),
        )
        .unwrap();
        assert!(
            command.contains(r"src='/tmp/it'\''s/update.tar.gz'"),
            "{command}"
        );
        assert!(
            command.contains(r#"bundle='/Applications/My "App" \ Hangar.app'"#),
            "{command}"
        );
        assert!(
            command.contains(r#"mktemp -d '/Applications'/.hangar-update-XXXXXX"#),
            "{command}"
        );
        let script = admin_move_script(&command);
        assert!(script.starts_with("do shell script \""));
        assert!(script.ends_with("\" with administrator privileges"));
        assert!(!script.contains('\n'), "line breaks are escaped");
        assert!(
            script.contains(r#"bundle='/Applications/My \"App\" \\ Hangar.app'"#),
            "{script}"
        );
        // Unescaping the AppleScript string gives the shell command back.
        let inner = &script
            ["do shell script \"".len()..script.len() - "\" with administrator privileges".len()];
        let mut unescaped = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => unescaped.push('\n'),
                    Some(other) => unescaped.push(other),
                    None => panic!("dangling escape"),
                }
            } else {
                unescaped.push(c);
            }
        }
        assert_eq!(unescaped, command);
    }

    #[test]
    fn packages_are_installed_through_pkexec_then_passwordless_sudo() {
        let deb = package_commands(PackageManager::Dpkg, Path::new("/tmp/x.deb"), "ab12");
        let script = package_script(PackageManager::Dpkg);
        let tail = |prefix: &[&str]| -> Vec<OsString> {
            prefix
                .iter()
                .copied()
                .chain([
                    "/bin/sh",
                    "-c",
                    &script,
                    "hangar-update",
                    "/tmp/x.deb",
                    "ab12",
                ])
                .map(OsString::from)
                .collect()
        };
        assert_eq!(deb, [tail(&["pkexec"]), tail(&["sudo", "-n"])]);
        assert!(
            script.ends_with(r#"dpkg -i "$work/update.deb""#),
            "{script}"
        );
        let rpm = package_script(PackageManager::Rpm);
        assert!(rpm.ends_with(r#"rpm -U "$work/update.rpm""#), "{rpm}");
    }

    /// Runs the root script of a package install as this user, with a fake
    /// `dpkg` that records what it was given.
    #[cfg(unix)]
    fn run_package_script(package: &Path, digest: &str) -> (std::process::Output, PathBuf) {
        let tools = tempfile::tempdir().unwrap().keep();
        let received = tools.join("received.deb");
        let dpkg = tools.join("dpkg");
        fs::write(
            &dpkg,
            format!(
                "#!/bin/sh\n[ \"$1\" = -i ] && cp \"$2\" '{}'\n",
                received.display()
            ),
        )
        .unwrap();
        set_file_mode(&dpkg, 0o755).unwrap();
        let path = format!(
            "{}:{}",
            tools.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(package_script(PackageManager::Dpkg))
            .arg("hangar-update")
            .arg(package)
            .arg(digest)
            .env("PATH", path)
            .output()
            .unwrap();
        (output, received)
    }

    #[test]
    #[cfg(unix)]
    fn the_root_script_installs_exactly_the_verified_bytes() {
        if Command::new("sha256sum").arg("--version").output().is_err() {
            return; // not a GNU userland; the macOS script is tested below
        }
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("update.deb");
        fs::write(&package, payload()).unwrap();
        let digest = sha256::hex(&digest_of(&payload()));

        let (output, received) = run_package_script(&package, &digest);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(fs::read(&received).unwrap(), payload());
    }

    #[test]
    #[cfg(unix)]
    fn the_root_script_refuses_a_package_swapped_after_the_check() {
        if Command::new("sha256sum").arg("--version").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("update.deb");
        // Verified as the payload, then rewritten by someone else.
        let digest = sha256::hex(&digest_of(&payload()));
        fs::write(&package, b"!<arch>\nsomething else").unwrap();

        let (output, received) = run_package_script(&package, &digest);
        assert_eq!(
            output.status.code(),
            Some(CHANGED_AFTER_CHECK),
            "{output:?}"
        );
        assert!(!received.exists(), "dpkg never ran");
    }

    /// Runs the macOS root command as this user against a bundle in a
    /// folder of its own; `None` where `shasum` or `tar` is missing.
    #[cfg(unix)]
    fn run_admin_swap(
        archive: &[u8],
        digest: &str,
    ) -> Option<(std::process::Output, tempfile::TempDir)> {
        let have = |tool: &str, arg: &str| {
            Command::new(tool)
                .arg(arg)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !have("shasum", "--version") || !have("tar", "--version") {
            return None;
        }
        let apps = tempfile::tempdir().unwrap();
        let bundle = apps.path().join("Hangar.AI.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/MacOS/hangar-ai"), b"old binary").unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let path = downloads.path().join("update.tar.gz");
        fs::write(&path, archive).unwrap();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(admin_swap_command(&path, digest, &bundle).unwrap())
            .output()
            .unwrap();
        Some((output, apps))
    }

    #[test]
    #[cfg(unix)]
    fn the_mac_root_command_unpacks_and_swaps_the_verified_archive() {
        let archive = tar_gz(&[(
            "Hangar.AI.app/Contents/MacOS/hangar-ai",
            b"new binary",
            0o755,
        )]);
        let Some((output, apps)) = run_admin_swap(&archive, &sha256::hex(&digest_of(&archive)))
        else {
            return;
        };
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            fs::read(apps.path().join("Hangar.AI.app/Contents/MacOS/hangar-ai")).unwrap(),
            b"new binary"
        );
        assert_eq!(
            names(apps.path()),
            ["Hangar.AI.app"],
            "the work folder is gone"
        );
    }

    #[test]
    #[cfg(unix)]
    fn the_mac_root_command_refuses_an_archive_swapped_after_the_check() {
        let verified = tar_gz(&[(
            "Hangar.AI.app/Contents/MacOS/hangar-ai",
            b"new binary",
            0o755,
        )]);
        let swapped = tar_gz(&[(
            "Hangar.AI.app/Contents/MacOS/hangar-ai",
            b"evil binary",
            0o755,
        )]);
        let Some((output, apps)) = run_admin_swap(&swapped, &sha256::hex(&digest_of(&verified)))
        else {
            return;
        };
        assert_eq!(
            output.status.code(),
            Some(CHANGED_AFTER_CHECK),
            "{output:?}"
        );
        assert_eq!(
            fs::read(apps.path().join("Hangar.AI.app/Contents/MacOS/hangar-ai")).unwrap(),
            b"old binary"
        );
        assert_eq!(names(apps.path()), ["Hangar.AI.app"]);
    }

    #[test]
    #[cfg(unix)]
    fn the_mac_root_command_keeps_the_old_app_when_the_archive_holds_none() {
        let archive = tar_gz(&[("README", b"no app here", 0o644)]);
        let Some((output, apps)) = run_admin_swap(&archive, &sha256::hex(&digest_of(&archive)))
        else {
            return;
        };
        assert_eq!(output.status.code(), Some(98), "{output:?}");
        assert_eq!(
            fs::read(apps.path().join("Hangar.AI.app/Contents/MacOS/hangar-ai")).unwrap(),
            b"old binary"
        );
        assert_eq!(names(apps.path()), ["Hangar.AI.app"]);
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
        match install_package(file.path(), PackageManager::Dpkg, &[0; 32]) {
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

    #[test]
    #[cfg(unix)]
    fn a_download_rewritten_after_its_check_is_never_installed() {
        let dir = tempfile::tempdir().unwrap();
        let (install, mut file, staged) = staged_appimage(dir.path(), b"old", 0o755);
        let Install::AppImage { path } = &install else {
            unreachable!()
        };
        file.write_all(&payload()).unwrap();
        drop(file);
        // Checked when the download ended; then another process rewrites it.
        signature::verify_file(staged.path(), PUBKEY, SIG_PREHASHED).unwrap();
        let mut swapped = payload();
        swapped[10] ^= 0xff;
        fs::write(staged.path(), &swapped).unwrap();

        match apply(&install, staged, SIGNED, &[]) {
            Err(Error::Signature(why)) => assert!(why.contains("does not match"), "{why}"),
            other => panic!("expected the swapped file to be refused, got {other:?}"),
        }
        assert_eq!(fs::read(path).unwrap(), b"old");
        assert_eq!(names(dir.path()), ["Hangar.AI.AppImage"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_put_in_place_of_the_download_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (install, file, staged) = staged_appimage(dir.path(), b"old", 0o755);
        drop(file);
        // Even pointing at a file that would verify.
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("signed");
        fs::write(&target, payload()).unwrap();
        fs::remove_file(staged.path()).unwrap();
        std::os::unix::fs::symlink(&target, staged.path()).unwrap();

        match apply(&install, staged, SIGNED, &[]) {
            Err(Error::Install(why)) => assert!(
                why.contains("not a plain file") || why.contains("no longer"),
                "{why}"
            ),
            other => panic!("expected the symlink to be refused, got {other:?}"),
        }
        let Install::AppImage { path } = &install else {
            unreachable!()
        };
        assert_eq!(fs::read(path).unwrap(), b"old");
    }

    #[test]
    #[cfg(unix)]
    fn a_package_is_downloaded_into_a_private_folder_of_its_own() {
        use std::os::unix::fs::PermissionsExt;
        let (_file, staged) = create_staging(&Install::Deb).unwrap();
        let (_other_file, other) = create_staging(&Install::Deb).unwrap();
        let folder = staged.path().parent().unwrap().to_path_buf();
        assert_ne!(
            Some(folder.as_path()),
            other.path().parent(),
            "created fresh each time"
        );
        assert!(folder.starts_with(std::env::temp_dir()));
        let mode = fs::metadata(&folder).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        drop(staged);
        assert!(!folder.exists(), "dropping the download removes its folder");
    }

    #[test]
    #[cfg(unix)]
    fn a_mac_archive_cannot_write_through_a_symlink_it_made() {
        let work = tempfile::tempdir().unwrap();
        let outside = work.path().join("outside");
        fs::create_dir(&outside).unwrap();

        let mut builder = tar::Builder::new(Vec::new());
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        builder
            .append_link(&mut link, "Hangar.AI.app/X", &outside)
            .unwrap();
        let mut file = tar::Header::new_gnu();
        file.set_size(4);
        file.set_mode(0o644);
        file.set_cksum();
        builder
            .append_data(&mut file, "Hangar.AI.app/X/authorized_keys", &b"evil"[..])
            .unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&builder.into_inner().unwrap()).unwrap();
        let archive = work.path().join("evil.tar.gz");
        fs::write(&archive, gz.finish().unwrap()).unwrap();

        match extract_app_archive(&archive, &work.path().join("out.app")) {
            Err(Error::Install(why)) => assert!(why.contains("symlink"), "{why}"),
            other => panic!("expected the path to be refused, got {other:?}"),
        }
        assert!(
            names(&outside).is_empty(),
            "nothing written outside the bundle"
        );
    }

    #[test]
    fn leftovers_of_earlier_updates_are_swept_once_old() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join(".hangar-update-old.exe");
        let fresh = dir.path().join(".hangar-update-fresh.msi");
        let old_folder = dir.path().join(".hangar-update-folder");
        let unrelated = dir.path().join("someone-else.exe");
        for file in [&old, &fresh, &unrelated] {
            fs::write(file, b"x").unwrap();
        }
        fs::create_dir(&old_folder).unwrap();
        fs::write(old_folder.join("setup.exe"), b"x").unwrap();
        let long_ago = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        for path in [&old, &unrelated] {
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        #[cfg(unix)]
        File::open(&old_folder)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();

        sweep_stale(dir.path(), Duration::from_secs(60 * 60));
        assert!(!old.exists());
        assert!(fresh.exists(), "a recent one may still be in use");
        assert!(unrelated.exists(), "only the updater's own files");
        #[cfg(unix)]
        assert!(!old_folder.exists());
    }

    #[test]
    #[cfg(unix)]
    fn the_launcher_starts_nothing_while_the_old_process_lives_on() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("started");
        let mut stuck = Command::new("sleep").arg("5").spawn().unwrap();
        let install = Install::Deb;
        let command = relaunch_command(&install, Path::new("/ignored"), &[], stuck.id()).unwrap();
        // The real script, with its 30 s wait cut to two tries.
        let line = command_line(&command);
        let script = line[2].replace("-ge 150", "-ge 2");
        assert_ne!(script, line[2], "the limit is where the test expects it");
        let status = Command::new("sh")
            .arg("-c")
            .arg(script)
            .arg("hangar-relaunch")
            .arg(stuck.id().to_string())
            .arg("touch")
            .arg(&marker)
            .status()
            .unwrap();
        stuck.kill().unwrap();
        let _ = stuck.wait();
        assert!(!status.success());
        assert!(!marker.exists(), "no second instance beside a stuck one");
    }
}
