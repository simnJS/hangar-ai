//! The kit gallery: `lib/kitGallery.ts`.
//!
//! The public kits of the `hangar-ai` organization, listed by a static file of
//! its catalog repository that hangar-core reads and checks. No server of
//! ours, and one source. Each kit is pinned to a tag of its own repository,
//! and installing is the existing `kit_add_git`, on the link to that tag.

pub use hangar_core::kits::gallery::{Gallery, GalleryKit, Origin};
use hangar_core::kits::{KitInfo, Source};

use super::model::kit_pinned_tag;

/// `kit_gallery_list`.
pub async fn kit_gallery_list() -> Result<Gallery, String> {
    hangar_core::kits::gallery::kit_gallery_list().await
}

/// The link `kit_add_git` is given for a gallery kit.
pub fn gallery_install_url(kit: &GalleryKit) -> String {
    format!("https://github.com/{}/tree/{}", kit.repo, kit.tag)
}

/// What a gallery card says of a kit on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GalleryStatus {
    New,
    /// `pinned` is the tag it is pinned to.
    Installed {
        version: String,
        pinned: Option<String>,
    },
    /// A folder of the same id is linked: it is the person's own copy, not
    /// this catalog kit.
    Linked {
        version: String,
    },
    Update {
        version: String,
        latest: String,
        pinned: String,
    },
}

/// `1.2.3-rc.1` as numbers and whether it is a pre-release; `None` when it is
/// not semver.
fn parse(version: &str) -> Option<([f64; 3], Option<&str>)> {
    let version = version.trim();
    let (core, build) = match version.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (version, None),
    };
    let tag_char = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    if build.is_some_and(|build| build.is_empty() || !build.chars().all(tag_char)) {
        return None;
    }
    let (numbers, pre) = match core.split_once('-') {
        Some((numbers, pre)) => (numbers, Some(pre)),
        None => (core, None),
    };
    if pre.is_some_and(|pre| pre.is_empty() || !pre.chars().all(tag_char)) {
        return None;
    }
    let mut parts = numbers.split('.');
    let mut nums = [0.0; 3];
    for slot in &mut nums {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some((nums, pre))
}

/// Whether `a` is a later version than `b`; false when either is not semver.
pub fn is_newer(a: &str, b: &str) -> bool {
    let (Some((left, left_pre)), Some((right, right_pre))) = (parse(a), parse(b)) else {
        return false;
    };
    for at in 0..3 {
        if left[at] != right[at] {
            return left[at] > right[at];
        }
    }
    match (left_pre, right_pre) {
        (left, right) if left == right => false,
        (None, _) => true,
        (_, None) => false,
        (Some(left), Some(right)) => left > right,
    }
}

fn from_repo(kit: &KitInfo, repo: &str) -> bool {
    match &kit.source {
        Source::Git(git) => git
            .url
            .eq_ignore_ascii_case(&format!("https://github.com/{repo}")),
        Source::Folder { .. } => false,
    }
}

pub fn gallery_status(entry: &GalleryKit, installed: &[KitInfo]) -> GalleryStatus {
    let Some(kit) = installed.iter().find(|item| item.id == entry.id) else {
        return GalleryStatus::New;
    };
    let version = kit.installed_version.clone();
    // Not installed from the catalog, and installing would clash on the id:
    // say so.
    if matches!(kit.source, Source::Folder { .. }) {
        return GalleryStatus::Linked { version };
    }
    let pinned = kit_pinned_tag(&kit.source);
    // As Rust goes by (kits/gallery.rs, `tag_for`): the catalog only has a say
    // on a kit pinned from this very repository. A linked folder is the
    // person's own copy, a kit on a branch follows its branch, and another
    // repository is another kit.
    match pinned {
        Some(pinned)
            if pinned != entry.tag
                && from_repo(kit, &entry.repo)
                && is_newer(&entry.version, &version) =>
        {
            GalleryStatus::Update {
                version,
                latest: entry.version.clone(),
                pinned: pinned.to_owned(),
            }
        }
        pinned => GalleryStatus::Installed {
            version,
            pinned: pinned.map(str::to_owned),
        },
    }
}

/// The kits whose name, description or a tag holds every word of the query.
pub fn filter_gallery<'a>(kits: &'a [GalleryKit], query: &str) -> Vec<&'a GalleryKit> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    kits.iter()
        .filter(|kit| {
            let hay = std::iter::once(kit.id.as_str())
                .chain([kit.name.as_str(), kit.description.as_str()])
                .chain(kit.tags.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            words.iter().all(|word| hay.contains(word))
        })
        .collect()
}

#[cfg(test)]
mod tests;
