//! Checks a release before it is published: the feed parses the way the updater
//! parses it, every build the updater can ask for is in it, and every artifact it
//! points at is there, is what its key says it is, and verifies against the
//! public key the installs carry.
//!
//! ```text
//! cargo run -p hangar-update --example verify_feed -- latest.json <artifacts-dir> \
//!     [--version 1.13.0] [--pubkey <base64 key, or a file holding it>]
//! ```
//!
//! `.github/workflows/release.yml` runs it between writing `latest.json` and
//! making the release public, so a signing key that is not the one the installs
//! trust (the failure that is silent until the first person tries to update) or a
//! platform missing from the feed keeps the release a draft instead. Without
//! `--pubkey` the key is [`hangar_update::PUBKEY`], the one compiled in; the
//! option is for trying the whole chain with a throwaway key.
//!
//! The artifacts are looked up by the last segment of their URL, in the folder.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hangar_update::{Install, Manifest, PUBKEY, Target, verify_file};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problems) => {
            for problem in &problems {
                eprintln!("error: {problem}");
            }
            eprintln!(
                "{} problem(s): this release must not be published",
                problems.len()
            );
            ExitCode::FAILURE
        }
    }
}

struct Args {
    feed: PathBuf,
    dir: PathBuf,
    version: Option<String>,
    pubkey: String,
}

fn parse_args() -> Result<Args, String> {
    let mut positional = Vec::new();
    let mut version = None;
    let mut pubkey = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" => version = Some(args.next().ok_or("--version needs a value")?),
            "--pubkey" => pubkey = Some(args.next().ok_or("--pubkey needs a value")?),
            _ => positional.push(arg),
        }
    }
    let [feed, dir] = <[String; 2]>::try_from(positional).map_err(
        |_| "usage: verify_feed <latest.json> <artifacts-dir> [--version V] [--pubkey KEY]",
    )?;
    let pubkey = match pubkey {
        // A file holding the key, as `cargo packager signer generate` writes it.
        Some(value) if Path::new(&value).is_file() => std::fs::read_to_string(&value)
            .map_err(|e| format!("reading {value}: {e}"))?
            .trim()
            .to_string(),
        Some(value) => value,
        None => PUBKEY.to_string(),
    };
    Ok(Args {
        feed: feed.into(),
        dir: dir.into(),
        version,
        pubkey,
    })
}

/// What the updater will ask the feed for, as (description, target, install,
/// the ending of the artifact it must answer with).
fn requests() -> Vec<(String, Target, Option<Install>, &'static str)> {
    let mut out = Vec::new();
    let linux = Target::new("linux", "x86_64");
    let windows = Target::new("windows", "x86_64");
    for (name, install, ending) in [
        (
            "an AppImage",
            Install::AppImage {
                path: PathBuf::from("/Hangar.AI.AppImage"),
            },
            ".AppImage",
        ),
        ("a .deb install", Install::Deb, ".deb"),
        ("an .rpm install", Install::Rpm, ".rpm"),
    ] {
        out.push((format!("Linux, {name}"), linux, Some(install), ending));
    }
    out.push((
        "Linux, an app of the old Tauri release".into(),
        linux,
        None,
        ".AppImage",
    ));
    out.push((
        "Windows, the NSIS installer".into(),
        windows,
        Some(Install::Nsis),
        "-setup.exe",
    ));
    out.push((
        "Windows, the old Tauri release".into(),
        windows,
        None,
        "-setup.exe",
    ));
    for arch in ["aarch64", "x86_64"] {
        let target = Target::new("darwin", arch);
        let bundle = Install::MacApp {
            bundle: PathBuf::from("/Applications/Hangar.AI.app"),
        };
        out.push((
            format!("macOS {arch}, the app"),
            target,
            Some(bundle),
            ".app.tar.gz",
        ));
        out.push((
            format!("macOS {arch}, the old Tauri release"),
            target,
            None,
            ".app.tar.gz",
        ));
    }
    out
}

fn run() -> Result<(), Vec<String>> {
    let args = parse_args().map_err(|e| vec![e])?;
    let mut problems = Vec::new();

    let json = std::fs::read_to_string(&args.feed)
        .map_err(|e| vec![format!("reading {}: {e}", args.feed.display())])?;
    let manifest = Manifest::parse(&json).map_err(|e| vec![e.to_string()])?;
    println!(
        "feed: version {}, {} entries",
        manifest.version,
        manifest.platform_keys().len()
    );

    if let Some(want) = &args.version {
        let want = semver::Version::parse(want.trim().trim_start_matches('v'))
            .map_err(|e| vec![format!("--version {want:?}: {e}")])?;
        if manifest.version != want {
            problems.push(format!(
                "the feed says version {}, the release is {want}",
                manifest.version
            ));
        }
    }
    if manifest.pub_date.is_none() {
        problems.push("the feed has no pub_date".into());
    }

    // Every kind of install finds a build, with the rules the updater itself uses.
    for (what, target, install, ending) in requests() {
        let keys = target.keys(install.as_ref());
        match manifest.platform(&keys) {
            Ok((key, platform)) if platform.url.ends_with(ending) => {
                println!("ok: {what} gets {key}");
            }
            Ok((key, platform)) => problems.push(format!(
                "{what}: {key} points at {}, which is not a {ending} file",
                platform.url
            )),
            Err(e) => problems.push(format!("{what}: {e}")),
        }
    }

    // Every artifact the feed names, once each however many keys name it.
    let mut checked = BTreeSet::new();
    for key in manifest.platform_keys() {
        let Ok((_, platform)) = manifest.platform(&[key.to_string()]) else {
            continue;
        };
        let name = platform
            .url
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        if !checked.insert(name.clone()) {
            continue;
        }
        let path = args.dir.join(&name);
        if !path.is_file() {
            problems.push(format!("{key}: {name} is not in {}", args.dir.display()));
            continue;
        }
        match verify_file(&path, &args.pubkey, &platform.signature) {
            Ok(()) => println!("ok: {name} is signed by the key"),
            Err(e) => {
                problems.push(format!("{name}: {e}"));
                continue;
            }
        }
        if let Err(why) = check_contents(&path, &name) {
            problems.push(format!("{name}: {why}"));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The artifact is the kind of file its name says, and a macOS bundle has what
/// the updater and the app expect in it.
fn check_contents(path: &Path, name: &str) -> Result<(), String> {
    let mut head = [0u8; 8];
    let read = File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("reading it: {e}"))?;
    let head = &head[..read];

    let (what, magic): (&str, &[u8]) = if name.ends_with(".AppImage") {
        ("an ELF file", b"\x7fELF")
    } else if name.ends_with(".deb") {
        ("an ar archive", b"!<arch>\n")
    } else if name.ends_with(".rpm") {
        ("an RPM package", b"\xed\xab\xee\xdb")
    } else if name.ends_with(".exe") {
        ("a Windows executable", b"MZ")
    } else if name.ends_with(".app.tar.gz") {
        return check_app_archive(path);
    } else {
        return Ok(());
    };
    if head.starts_with(magic) {
        Ok(())
    } else {
        Err(format!("is not {what}"))
    }
}

/// `hangar-update` unpacks a `.app.tar.gz` by dropping its first path component
/// and using the rest as the bundle, so it must have exactly one top-level
/// folder, named `*.app`. The helper must be beside the executable in it.
fn check_app_archive(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("reading it: {e}"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut top = BTreeSet::new();
    let mut files = BTreeSet::new();
    for entry in archive
        .entries()
        .map_err(|e| format!("not a gzipped tar: {e}"))?
    {
        let entry = entry.map_err(|e| format!("not a gzipped tar: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let mut parts = path.components().filter(|c| c.as_os_str() != ".");
        if let Some(first) = parts.next() {
            top.insert(first.as_os_str().to_string_lossy().into_owned());
        }
        files.insert(
            path.components()
                .filter(|c| c.as_os_str() != ".")
                .collect::<PathBuf>(),
        );
    }
    let [bundle] = <[String; 1]>::try_from(top.into_iter().collect::<Vec<_>>()).map_err(|top| {
        format!(
            "has {} top-level entries, wanted one `.app` folder",
            top.len()
        )
    })?;
    if !bundle.ends_with(".app") {
        return Err(format!("its top-level folder is {bundle}, not a .app"));
    }
    for needed in [
        "Contents/Info.plist",
        "Contents/MacOS/hangar-ai",
        "Contents/MacOS/hangar-webview",
    ] {
        if !files.contains(&Path::new(&bundle).join(needed)) {
            return Err(format!("{bundle} has no {needed}"));
        }
    }
    Ok(())
}
