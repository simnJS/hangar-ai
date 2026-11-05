//! Where a git kit comes from: the GitHub link the person pasted, and the git
//! CLI that clones and updates it.
//!
//! Only `https://github.com/...` links are accepted from the person. The
//! repository is fetched shallow — one commit, no tags, no submodules — and
//! checked out with symlinks as plain files, so nothing in it runs and nothing
//! in it points outside the clone: no hook comes with a fetch, and Hangar runs
//! nothing from the folder afterwards.
//!
//! A link to a tag pins the kit to it: its ref is recorded by its full name,
//! `refs/tags/<tag>`, which git reads as that tag and no branch of the same
//! name. It moves only when the catalog names another tag (`update_target`).

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{commands, manifest};

/// How often a running git is looked at.
const POLL: Duration = Duration::from_millis(20);
/// How long its output is waited for once it ended.
const DRAIN: Duration = Duration::from_millis(500);

/// A ref under this names a tag, and only that.
const TAG_REF: &str = "refs/tags/";

/// What a git kit is cloned from, as the index records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitSource {
    /// `https://github.com/<owner>/<repo>`: what git fetches from.
    pub url: String,
    /// A branch or a commit, or `refs/tags/<tag>` for a kit pinned to a tag; the
    /// remote's default branch when absent.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// The kit's root inside the repository, when it is not the top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdir: Option<String>,
}

/// A link as pasted, before its `/tree/...` part is split into a ref and a
/// folder — which needs the remote's refs, since a branch can hold slashes.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub url: String,
    /// The path segments after `/tree/`, empty for a link to the repository.
    pub tree: Vec<String>,
}

/// `https://github.com/<owner>/<repo>`, with or without `.git`, and
/// `https://github.com/<owner>/<repo>/tree/<ref>[/<folder>]`.
pub fn parse_link(input: &str) -> Result<Link, String> {
    let input = input.trim();
    let Some(rest) = strip_prefix_ignore_case(input, "https://") else {
        return Err("only https:// links to GitHub are accepted".into());
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.contains('@') {
        return Err("a link must not carry a user name or a token".into());
    }
    let host = authority.to_ascii_lowercase();
    if host != "github.com" && host != "www.github.com" {
        return Err("only links to github.com are accepted".into());
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let [owner, repo, more @ ..] = segments.as_slice() else {
        return Err("a GitHub link names an owner and a repository".into());
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let name_ok = |name: &str| {
        !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !name_ok(owner) || !name_ok(repo) {
        return Err("the owner or the repository in this link is not a GitHub name".into());
    }
    let tree = match more {
        [] => Vec::new(),
        ["tree", tree @ ..] if !tree.is_empty() => tree.iter().map(|s| s.to_string()).collect(),
        _ => {
            return Err(
                "link to the repository, or to a folder in it (…/tree/<branch>/<folder>)".into(),
            )
        }
    };
    Ok(Link {
        url: format!("https://github.com/{owner}/{repo}"),
        tree,
    })
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// The ref and the folder a `/tree/` path means: the longest run of leading
/// segments that names one of the remote's branches or tags, the rest being
/// the folder. Failing that, the first segment is the ref — a commit, say.
pub fn split_tree(
    tree: &[String],
    refs: &[String],
) -> Result<(Option<String>, Option<String>), String> {
    if tree.is_empty() {
        return Ok((None, None));
    }
    let taken = (1..=tree.len())
        .rev()
        .find(|&n| refs.iter().any(|known| *known == tree[..n].join("/")))
        .unwrap_or(1);
    let git_ref = tree[..taken].join("/");
    check_ref(&git_ref)?;
    let subdir = match tree[taken..].join("/") {
        rest if rest.is_empty() => None,
        rest => match manifest::normalize(&rest)? {
            dot if dot == "." => None,
            path => Some(path),
        },
    };
    Ok((Some(git_ref), subdir))
}

/// What a link means on its remote. A tag is recorded by its full name, which is
/// what pins the kit to it; a link to the repository asks the remote nothing.
pub fn source_of(link: Link, refs: &RemoteRefs) -> Result<GitSource, String> {
    let (git_ref, subdir) = split_tree(&link.tree, &refs.names())?;
    Ok(GitSource {
        url: link.url,
        git_ref: git_ref.map(|name| {
            if refs.tags.contains(&name) {
                format!("{TAG_REF}{name}")
            } else {
                name
            }
        }),
        subdir,
    })
}

/// The tag a kit is pinned to, when it is.
pub fn pinned_tag(source: &GitSource) -> Option<&str> {
    source.git_ref.as_deref()?.strip_prefix(TAG_REF)
}

/// What an update of `source` fetches. A kit on a branch fetches its branch, and
/// the catalog has no say. A kit pinned to a tag moves only to another tag the
/// catalog names: `None` otherwise, as there is nothing to fetch, and a tag its
/// owner moved reaches nobody.
pub fn update_target(source: &GitSource, catalog_tag: Option<&str>) -> Option<GitSource> {
    let Some(pinned) = pinned_tag(source) else {
        return Some(source.clone());
    };
    let tag = catalog_tag.filter(|tag| *tag != pinned)?;
    Some(GitSource {
        git_ref: Some(format!("{TAG_REF}{tag}")),
        ..source.clone()
    })
}

/// What git would read as a ref and nothing else: never an option, never a
/// range or a revision expression.
pub fn check_ref(git_ref: &str) -> Result<(), String> {
    let ok = !git_ref.is_empty()
        && !git_ref.starts_with(['-', '/', '.'])
        && !git_ref.ends_with(['/', '.'])
        && !git_ref.contains("..")
        && !git_ref.contains("//")
        && !git_ref.ends_with(".lock")
        && git_ref
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+'));
    if ok {
        Ok(())
    } else {
        Err(format!("{git_ref:?} is not a branch, tag or commit name"))
    }
}

/// The longest any git call may take. A call over the network that stalls is
/// also ended by git itself, once it has moved less than `LOW_SPEED_BYTES` a
/// second for `LOW_SPEED_SECONDS`; this bounds the rest, a remote that trickles
/// included. Nothing waits on a stalled network for ever: an add or an update
/// fails, and says so.
pub const GIT_DEADLINE: Duration = Duration::from_secs(180);
const LOW_SPEED_BYTES: &str = "1000";
const LOW_SPEED_SECONDS: &str = "30";

#[cfg(test)]
thread_local! {
    /// A shorter deadline for the git calls of one test's thread.
    pub static TEST_DEADLINE: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

fn deadline() -> Duration {
    #[cfg(test)]
    if let Some(deadline) = TEST_DEADLINE.with(std::cell::Cell::get) {
        return deadline;
    }
    GIT_DEADLINE
}

/// Runs git, in `dir` when there is one, and hands back its stdout.
pub fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    git_within(dir, args, &[], deadline())
}

/// `git`, with variables of its own and a deadline past which git and what it
/// started are killed.
fn git_within(
    dir: Option<&Path>,
    args: &[&str],
    env: &[(&str, &OsStr)],
    deadline: Duration,
) -> Result<String, String> {
    // A bundled macOS app starts with launchd's bare PATH (path_env.rs).
    crate::path_env::ensure();
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    command
        .args([
            "-c",
            &format!("http.lowSpeedLimit={LOW_SPEED_BYTES}"),
            "-c",
            &format!("http.lowSpeedTime={LOW_SPEED_SECONDS}"),
        ])
        .args(args)
        .env("LC_ALL", "C")
        .env("LANGUAGE", "")
        // Nobody is there to type a password, and a large file is not worth
        // the wait: a kit is text.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        // No helper that would open a window to ask for a password either.
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Inherited from a Hangar started inside a git hook, these would aim
    // every call below at another repository.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    // Its own process group, so a deadline ends git-remote-https with it; no
    // console window on Windows.
    commands::prepare(&mut command);
    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "git was not found on PATH".to_string(),
        _ => format!("could not run git: {err}"),
    })?;
    let stdout = commands::Capture::start(child.stdout.take().expect("piped above"));
    let stderr = commands::Capture::start(child.stderr.take().expect("piped above"));
    let until = Instant::now() + deadline;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= until => {
                commands::kill_tree(&mut child);
                return Err(format!(
                    "git took longer than {} s and was stopped: is the network there?",
                    deadline.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(err) => {
                commands::kill_tree(&mut child);
                return Err(format!("could not run git: {err}"));
            }
        }
    };
    let drained = Instant::now() + DRAIN;
    let (stdout, stdout_cut) = commands::text_of(stdout.finish(drained));
    let (stderr, _) = commands::text_of(stderr.finish(drained));
    if status.success() {
        if stdout_cut {
            return Err("git answered more than a kit can hold".into());
        }
        Ok(stdout)
    } else {
        let line = stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
            .or_else(|| stderr.lines().map(str::trim).rfind(|line| !line.is_empty()))
            .unwrap_or("git failed");
        Err(line.to_string())
    }
}

/// The branches and tags of a remote, by name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteRefs {
    pub heads: Vec<String>,
    pub tags: Vec<String>,
}

impl RemoteRefs {
    fn names(&self) -> Vec<String> {
        self.heads.iter().chain(&self.tags).cloned().collect()
    }
}

/// What `git ls-remote` lists. An annotated tag comes twice, once peeled (`^{}`).
fn read_refs(listing: &str) -> RemoteRefs {
    let mut refs = RemoteRefs::default();
    for name in listing
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
    {
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            refs.heads.push(branch.to_string());
        } else if let Some(tag) = name.strip_prefix(TAG_REF) {
            let tag = tag.trim_end_matches("^{}");
            if !refs.tags.iter().any(|seen| seen == tag) {
                refs.tags.push(tag.to_string());
            }
        }
    }
    refs
}

/// The remote's branch and tag names.
pub fn remote_refs(url: &str) -> Result<RemoteRefs, String> {
    git(None, &["ls-remote", "--heads", "--tags", "--", url]).map(|listing| read_refs(&listing))
}

/// A full commit id, as `rev-parse` prints it: what is recorded and compared,
/// and what the calls below take as a revision.
fn commit_id(text: &str) -> Result<String, String> {
    let id = text.trim();
    if (id.len() == 40 || id.len() == 64) && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(id.to_ascii_lowercase())
    } else {
        Err(format!("git named no commit: {id:?}"))
    }
}

/// A fresh shallow clone of `source` in `dir`, which must not exist yet, and the
/// commit it holds.
pub fn clone(source: &GitSource, dir: &Path) -> Result<String, String> {
    let dir_text = dir.to_string_lossy();
    git(None, &["init", "-q", "--", &dir_text])?;
    // Before anything is checked out: a symlink in the repository becomes a
    // small text file, and can never point out of the kit.
    git(Some(dir), &["config", "core.symlinks", "false"])?;
    git(Some(dir), &["remote", "add", "--", "origin", &source.url])?;
    let commit = fetch(source, dir)?;
    git(
        Some(dir),
        &["-c", "advice.detachedHead=false", "checkout", "-q", &commit],
    )?;
    Ok(commit)
}

/// Fetches the one commit `source` points at now, and names it. It is named
/// right away and used by its id afterwards: what FETCH_HEAD holds is the last
/// fetch's, whoever made it.
pub fn fetch(source: &GitSource, dir: &Path) -> Result<String, String> {
    let wanted = source.git_ref.as_deref().unwrap_or("HEAD");
    check_ref(wanted)?;
    git(
        Some(dir),
        &[
            "fetch",
            "-q",
            "--depth",
            "1",
            "--no-tags",
            "--no-recurse-submodules",
            "origin",
            wanted,
        ],
    )?;
    // An annotated tag is fetched as the tag: the commit is what it points at.
    commit_id(&git(
        Some(dir),
        &["rev-parse", "--verify", "-q", "FETCH_HEAD^{commit}"],
    )?)
}

/// The commit the clone has checked out.
pub fn head(dir: &Path) -> Result<String, String> {
    commit_id(&git(
        Some(dir),
        &["rev-parse", "--verify", "-q", "HEAD^{commit}"],
    )?)
}

/// A file of a fetched commit, by its path in the repository.
pub fn file_at(dir: &Path, commit: &str, path: &str) -> Result<String, String> {
    commit_id(commit)?;
    git(Some(dir), &["show", &format!("{commit}:{path}")])
}

/// Writes the files of `commit` into `into`, a folder that must not exist yet,
/// leaving the clone's own checkout and index as they are: what an update would
/// bring, to be read before anything switches to it. Symlinks come out as small
/// text files, as in the clone.
pub fn export(dir: &Path, commit: &str, into: &Path) -> Result<(), String> {
    commit_id(commit)?;
    std::fs::create_dir(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let index = into.with_extension("index");
    let work_tree = into.to_string_lossy().into_owned();
    let result = git_within(
        Some(dir),
        &[
            "-c",
            "core.symlinks=false",
            "--work-tree",
            &work_tree,
            "checkout",
            "-q",
            commit,
            "--",
            ".",
        ],
        &[("GIT_INDEX_FILE", index.as_os_str())],
        deadline(),
    );
    let _ = std::fs::remove_file(&index);
    result.map(drop)
}

/// Moves the checkout to `commit`. Files the repository does not track — what a
/// setup step installed in the kit folder — stay.
pub fn checkout(dir: &Path, commit: &str) -> Result<(), String> {
    commit_id(commit)?;
    git(Some(dir), &["reset", "-q", "--hard", commit]).map(drop)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn link(input: &str) -> Link {
        parse_link(input).unwrap_or_else(|why| panic!("{input}: {why}"))
    }

    #[test]
    fn github_links_in_every_shape() {
        for input in [
            "https://github.com/simnJS/roblox-kit",
            "https://github.com/simnJS/roblox-kit/",
            "https://github.com/simnJS/roblox-kit.git",
            "HTTPS://GitHub.com/simnJS/roblox-kit?tab=readme#top",
            "  https://www.github.com/simnJS/roblox-kit  ",
        ] {
            assert_eq!(
                link(input),
                Link {
                    url: "https://github.com/simnJS/roblox-kit".into(),
                    tree: vec![]
                },
                "{input}"
            );
        }
        assert_eq!(
            link("https://github.com/o/r/tree/feature/x/kits/roblox").tree,
            ["feature", "x", "kits", "roblox"]
        );
    }

    #[test]
    fn only_https_github_links_are_taken() {
        for input in [
            "http://github.com/o/r",
            "git@github.com:o/r.git",
            "file:///C:/repos/kit",
            "ssh://github.com/o/r",
            "https://gitlab.com/o/r",
            "https://github.com.evil.example/o/r",
            "https://user:token@github.com/o/r",
            "https://github.com/o",
            "https://github.com/o/r/blob/main/README.md",
            "https://github.com/o/r/tree",
            "https://github.com/../r",
            "C:\\kits\\roblox",
        ] {
            assert!(parse_link(input).is_err(), "{input} was accepted");
        }
    }

    #[test]
    fn a_tree_path_splits_on_the_longest_known_ref() {
        let refs = vec![
            "main".to_string(),
            "feature/x".to_string(),
            "v1.0".to_string(),
        ];
        let split = |path: &str| {
            split_tree(
                &path.split('/').map(str::to_string).collect::<Vec<_>>(),
                &refs,
            )
            .unwrap()
        };
        assert_eq!(split("main"), (Some("main".into()), None));
        assert_eq!(
            split("main/kits/roblox"),
            (Some("main".into()), Some("kits/roblox".into()))
        );
        assert_eq!(
            split("feature/x/kit"),
            (Some("feature/x".into()), Some("kit".into()))
        );
        // Unknown: a commit, most likely.
        assert_eq!(
            split("4b62471/kit"),
            (Some("4b62471".into()), Some("kit".into()))
        );
        assert_eq!(split_tree(&[], &refs).unwrap(), (None, None));
        assert!(split_tree(&["main".into(), "..".into()], &refs).is_err());
        assert!(split_tree(&["-c".into()], &refs).is_err());
    }

    fn remote(heads: &[&str], tags: &[&str]) -> RemoteRefs {
        RemoteRefs {
            heads: heads.iter().map(|name| name.to_string()).collect(),
            tags: tags.iter().map(|name| name.to_string()).collect(),
        }
    }

    #[test]
    fn the_listing_of_a_remote_tells_tags_from_branches() {
        let listing = "1\trefs/heads/main\n\
                       2\trefs/heads/feature/x\n\
                       3\trefs/tags/v1.0.0\n\
                       4\trefs/tags/v2.0.0\n\
                       5\trefs/tags/v2.0.0^{}\n\
                       6\trefs/pull/1/head\n";
        assert_eq!(
            read_refs(listing),
            remote(&["main", "feature/x"], &["v1.0.0", "v2.0.0"])
        );
    }

    #[test]
    fn a_link_of_the_catalog_is_a_tag_with_no_folder_and_pins_the_kit() {
        let parsed = link("https://github.com/hangar-ai/ci-status/tree/v0.2.0");
        let refs = remote(&["main"], &["v0.1.0", "v0.2.0"]);
        let source = source_of(parsed, &refs).unwrap();
        assert_eq!(
            source,
            GitSource {
                url: "https://github.com/hangar-ai/ci-status".into(),
                git_ref: Some("refs/tags/v0.2.0".into()),
                subdir: None,
            }
        );
        assert_eq!(pinned_tag(&source), Some("v0.2.0"));
        assert!(check_ref(source.git_ref.as_deref().unwrap()).is_ok());
    }

    #[test]
    fn a_tag_link_can_name_a_folder_and_wins_over_a_branch_of_its_name() {
        let parsed = link("https://github.com/o/r/tree/v1/kits/roblox");
        let source = source_of(parsed, &remote(&["v1"], &["v1"])).unwrap();
        assert_eq!(pinned_tag(&source), Some("v1"));
        assert_eq!(source.subdir.as_deref(), Some("kits/roblox"));
    }

    #[test]
    fn a_link_to_a_repository_a_branch_or_a_commit_pins_nothing() {
        let refs = remote(&["main", "feature/x"], &["v1"]);
        for (input, wanted) in [
            ("https://github.com/o/r", None),
            ("https://github.com/o/r/tree/main", Some("main")),
            (
                "https://github.com/o/r/tree/feature/x/kit",
                Some("feature/x"),
            ),
            ("https://github.com/o/r/tree/4b62471", Some("4b62471")),
        ] {
            let source = source_of(link(input), &refs).unwrap();
            assert_eq!(source.git_ref.as_deref(), wanted, "{input}");
            assert_eq!(pinned_tag(&source), None, "{input}");
        }
    }

    #[test]
    fn a_pinned_kit_only_moves_to_another_tag_the_catalog_names() {
        let source = |git_ref: Option<&str>| GitSource {
            url: "https://github.com/o/r".into(),
            git_ref: git_ref.map(str::to_string),
            subdir: Some("kit".into()),
        };
        let pinned = source(Some("refs/tags/v1"));
        assert_eq!(update_target(&pinned, None), None);
        assert_eq!(update_target(&pinned, Some("v1")), None);
        assert_eq!(
            update_target(&pinned, Some("v2")),
            Some(source(Some("refs/tags/v2")))
        );

        // A kit on a branch fetches its branch, whatever the catalog says.
        for branch in [source(None), source(Some("main"))] {
            assert_eq!(update_target(&branch, None).as_ref(), Some(&branch));
            assert_eq!(update_target(&branch, Some("v2")).as_ref(), Some(&branch));
        }
    }

    /// An http server on loopback that takes connections and never answers:
    /// a network that stalled.
    pub(crate) fn stalled_server(offset: u16) -> String {
        let port = std::env::var("HANGAR_PORT_BASE")
            .ok()
            .and_then(|base| base.parse::<u16>().ok())
            .map_or(0, |base| base + offset);
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                held.push(stream);
            }
        });
        format!("http://127.0.0.1:{port}/o/r")
    }

    #[test]
    fn git_is_stopped_at_its_deadline_when_the_network_stalls() {
        let url = stalled_server(0);
        TEST_DEADLINE.with(|deadline| deadline.set(Some(Duration::from_secs(2))));
        let started = Instant::now();
        let err = remote_refs(&url).unwrap_err();
        assert!(err.contains("longer than 2 s"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn refs_that_git_could_misread_are_refused() {
        for bad in [
            "-upload-pack=x",
            "a..b",
            "HEAD~1",
            "x^",
            "a b",
            "a:b",
            "/x",
            "x/",
            "x.lock",
        ] {
            assert!(check_ref(bad).is_err(), "{bad}");
        }
        for good in ["main", "feature/x", "v1.2.0", "4b62471", "release-1_2+b"] {
            assert!(check_ref(good).is_ok(), "{good}");
        }
    }
}
