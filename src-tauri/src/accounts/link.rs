//! Directory links: a junction on Windows, a symbolic link elsewhere.
//!
//! A junction needs no privilege, where a Windows symbolic link wants
//! Developer Mode or an administrator, and it only ever leads to a directory
//! on a local volume — which is all a profile asks of it. Everything that
//! reads a path through it, Claude Code included, sees the directory it leads
//! to.

use std::fs;
use std::io;
use std::path::Path;

/// Makes `link` a directory link to `target`, which must be a directory.
/// `link` must not exist yet.
pub fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
    let target = dunce::canonicalize(target)?;
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the folder to share is not a directory",
        ));
    }
    imp::link_dir(&target, link)
}

/// A symbolic link or a junction: an entry that stands for another path.
pub fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Something is at `path`, a link that leads nowhere included.
pub fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// `a` and `b` are the same place once every link is followed.
pub fn same_place(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => same_spelling(&a, &b),
        _ => false,
    }
}

/// Paths equal as the file system compares them: without case on Windows.
pub fn same_spelling(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Removes the link itself, never what it leads to. A junction or a
/// directory link on Windows goes as a directory, a unix link as a file.
pub fn remove_link(link: &Path) -> io::Result<()> {
    if !is_link(link) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a link: left alone",
        ));
    }
    fs::remove_file(link).or_else(|_| fs::remove_dir(link))
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::fs::{self, OpenOptions};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::System::IO::DeviceIoControl;

    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    /// An empty directory turned into a mount point: what `mklink /J` does,
    /// without a `cmd` to quote the paths for.
    pub fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
        let print: Vec<u16> = target.as_os_str().encode_wide().collect();
        let substitute: Vec<u16> = OsStr::new(r"\??\")
            .encode_wide()
            .chain(print.iter().copied())
            .collect();
        let buffer = mount_point(&substitute, &print)?;
        fs::create_dir(link)?;
        let result = set_reparse_point(link, &buffer);
        if result.is_err() {
            let _ = fs::remove_dir(link);
        }
        result
    }

    /// A REPARSE_DATA_BUFFER for a mount point: the tag, the length of what
    /// follows, the offsets and lengths of both names, then the names, each
    /// ended by a NUL.
    fn mount_point(substitute: &[u16], print: &[u16]) -> io::Result<Vec<u8>> {
        let too_long = || io::Error::new(io::ErrorKind::InvalidInput, "path too long to link");
        let sub = u16::try_from(substitute.len() * 2).map_err(|_| too_long())?;
        let prn = u16::try_from(print.len() * 2).map_err(|_| too_long())?;
        let data = u16::try_from(8 + usize::from(sub) + 2 + usize::from(prn) + 2)
            .map_err(|_| too_long())?;
        let mut buf = Vec::with_capacity(8 + usize::from(data));
        buf.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buf.extend_from_slice(&data.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&sub.to_le_bytes());
        buf.extend_from_slice(&(sub + 2).to_le_bytes());
        buf.extend_from_slice(&prn.to_le_bytes());
        let names = substitute
            .iter()
            .chain(std::iter::once(&0))
            .chain(print)
            .chain(std::iter::once(&0));
        for unit in names {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        Ok(buf)
    }

    fn set_reparse_point(dir: &Path, buffer: &[u8]) -> io::Result<()> {
        let handle = OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)?;
        let mut returned = 0u32;
        // SAFETY: the handle is open for the whole call, the buffer is a
        // well-formed REPARSE_DATA_BUFFER of the length given, and there is no
        // output buffer nor overlapped I/O.
        let ok = unsafe {
            DeviceIoControl(
                handle.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn link_dir(_target: &Path, _link: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "folders cannot be linked on this system",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::testing::Scratch;

    #[test]
    fn a_link_leads_to_its_target_and_goes_without_it() {
        let scratch = Scratch::new("link");
        let target = scratch.dir("main/projects");
        fs::write(target.join("kept.jsonl"), "{}\n").unwrap();
        let link = scratch.root.join("profile-projects");

        link_dir(&target, &link).unwrap();
        assert!(is_link(&link));
        assert!(same_place(&link, &target));
        // Written through the link, found at the target.
        fs::write(link.join("through.jsonl"), "{}\n").unwrap();
        assert!(target.join("through.jsonl").is_file());

        remove_link(&link).unwrap();
        assert!(!exists(&link));
        assert!(target.join("kept.jsonl").is_file());
        assert!(target.join("through.jsonl").is_file());
    }

    #[test]
    fn only_links_are_removed_as_links() {
        let scratch = Scratch::new("link-real");
        let real = scratch.dir("real");
        fs::write(real.join("file"), "x").unwrap();
        assert!(!is_link(&real));
        assert!(remove_link(&real).is_err());
        assert!(real.join("file").is_file());
    }

    #[test]
    fn a_missing_target_is_not_linked() {
        let scratch = Scratch::new("link-missing");
        let link = scratch.root.join("link");
        assert!(link_dir(&scratch.root.join("nowhere"), &link).is_err());
        assert!(!exists(&link));
    }
}
