//! Where a git kit comes from: the GitHub link the person pasted, and the git
//! CLI that clones and updates it.
//!
//! Only `https://github.com/...` links are accepted from the person. The
//! repository is fetched shallow — one commit, no tags, no submodules — and
//! checked out with symlinks as plain files, so nothing in it runs and nothing
//! in it points outside the clone: no hook comes with a fetch, and Hangar runs
//! nothing from the folder afterwards.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::manifest;

/// Hides the console window Windows would flash for every git call.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a git kit is cloned from, as the index records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitSource {
    /// `https://github.com/<owner>/<repo>`: what git fetches from.
    pub url: String,
    /// A branch, a tag or a commit; the remote's default branch when absent.
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

/// Runs git, in `dir` when there is one, and hands back its stdout.
pub fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    // A bundled macOS app starts with launchd's bare PATH (path_env.rs).
    crate::path_env::ensure();
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    command
        .args(args)
        .env("LC_ALL", "C")
        .env("LANGUAGE", "")
        // Nobody is there to type a password, and a large file is not worth
        // the wait: a kit is text.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1");
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "git was not found on PATH".to_string(),
        _ => format!("could not run git: {err}"),
    })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
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

/// The remote's branch and tag names.
pub fn remote_refs(url: &str) -> Result<Vec<String>, String> {
    let listing = git(None, &["ls-remote", "--heads", "--tags", "--", url])?;
    Ok(listing
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter_map(|name| {
            name.strip_prefix("refs/heads/")
                .or_else(|| name.strip_prefix("refs/tags/"))
        })
        .map(|name| name.trim_end_matches("^{}").to_string())
        .collect())
}

/// A fresh shallow clone of `source` in `dir`, which must not exist yet.
pub fn clone(source: &GitSource, dir: &Path) -> Result<(), String> {
    let dir_text = dir.to_string_lossy();
    git(None, &["init", "-q", "--", &dir_text])?;
    // Before anything is checked out: a symlink in the repository becomes a
    // small text file, and can never point out of the kit.
    git(Some(dir), &["config", "core.symlinks", "false"])?;
    git(Some(dir), &["remote", "add", "--", "origin", &source.url])?;
    fetch(source, dir)?;
    git(
        Some(dir),
        &[
            "-c",
            "advice.detachedHead=false",
            "checkout",
            "-q",
            "FETCH_HEAD",
        ],
    )?;
    Ok(())
}

/// The one commit `source` points at now, as FETCH_HEAD.
pub fn fetch(source: &GitSource, dir: &Path) -> Result<(), String> {
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
    )
    .map(drop)
}

/// A file of the fetched commit, by its path in the repository.
pub fn fetched_file(dir: &Path, path: &str) -> Result<String, String> {
    git(Some(dir), &["show", &format!("FETCH_HEAD:{path}")])
}

/// Moves the checkout to the fetched commit. Files the repository does not
/// track — what a setup step installed in the kit folder — stay.
pub fn checkout_fetched(dir: &Path) -> Result<(), String> {
    git(Some(dir), &["reset", "-q", "--hard", "FETCH_HEAD"]).map(drop)
}

#[cfg(test)]
mod tests {
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
