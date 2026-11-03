//! The cases of `lib/kitGallery.test.ts` that are about logic; the view's are in
//! `views/gallery/tests.rs`.

use hangar_core::kits::KitInfo;

use super::*;
use crate::kits::testing::{folder, git, installed};

fn entry() -> GalleryKit {
    GalleryKit {
        id: "ci-status".into(),
        name: "CI status".into(),
        description: "Follows a GitHub Actions run.".into(),
        repo: "hangar-ai/ci-status".into(),
        tag: "v0.2.0".into(),
        version: "0.2.0".into(),
        permissions_summary: "Runs gh.".into(),
        tags: vec!["ci".into(), "github".into()],
        verified: true,
    }
}

/// `ci-status` as the library installs it: pinned to the tag `v0.1.0`.
fn pinned() -> KitInfo {
    KitInfo {
        source: git(
            "https://github.com/hangar-ai/ci-status",
            Some("refs/tags/v0.1.0"),
        ),
        installed_version: "0.1.0".into(),
        ..installed("ci-status")
    }
}

#[test]
fn compares_numbers_not_text() {
    assert!(is_newer("0.10.0", "0.9.0"));
    assert!(!is_newer("1.0.0", "1.0.0"));
    assert!(!is_newer("1.0.0", "1.0.1"));
}

#[test]
fn puts_a_release_above_its_pre_release() {
    assert!(is_newer("1.0.0", "1.0.0-rc.1"));
    assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
}

#[test]
fn says_no_when_either_is_not_a_version() {
    assert!(!is_newer("latest", "1.0.0"));
    assert!(!is_newer("1.0.0", "1.0"));
    assert!(!is_newer("1.0.0.1", "1.0.0"));
    assert!(!is_newer("1.0.0-", "0.9.0"));
    assert!(!is_newer("1.0.0+", "0.9.0"));
    assert!(!is_newer("1.a.0", "0.9.0"));
}

#[test]
fn reads_build_metadata_and_whitespace_like_the_web_app_did() {
    assert!(is_newer(" 1.1.0+build.5 ", "1.0.9"));
    assert!(!is_newer("1.0.0+a", "1.0.0+b"));
    assert!(is_newer("1.0.0-rc.2", "1.0.0-rc.1"));
}

#[test]
fn is_new_when_nothing_of_that_id_is_installed() {
    assert_eq!(gallery_status(&entry(), &[]), GalleryStatus::New);
}

fn current() -> GalleryStatus {
    GalleryStatus::Installed {
        version: "0.1.0".into(),
        pinned: Some("v0.1.0".into()),
    }
}

#[test]
fn is_installed_at_the_version_it_has_pinned_to_its_tag() {
    let same = GalleryKit {
        tag: "v0.1.0".into(),
        version: "0.1.0".into(),
        ..entry()
    };
    assert_eq!(gallery_status(&same, &[pinned()]), current());
}

#[test]
fn offers_an_update_when_the_catalog_names_a_new_tag_with_a_newer_version() {
    assert_eq!(
        gallery_status(&entry(), &[pinned()]),
        GalleryStatus::Update {
            version: "0.1.0".into(),
            latest: "0.2.0".into(),
            pinned: "v0.1.0".into()
        }
    );
}

#[test]
fn finds_the_repository_whatever_its_case_as_github_does() {
    let shouting = GalleryKit {
        repo: "Hangar-AI/CI-status".into(),
        ..entry()
    };
    assert!(matches!(
        gallery_status(&shouting, &[pinned()]),
        GalleryStatus::Update { .. }
    ));
}

#[test]
fn offers_nothing_while_the_catalog_names_the_same_tag_whatever_version_it_says() {
    let same_tag = GalleryKit {
        tag: "v0.1.0".into(),
        ..entry()
    };
    assert_eq!(gallery_status(&same_tag, &[pinned()]), current());
}

#[test]
fn offers_nothing_when_the_new_tag_is_not_a_newer_version() {
    for version in ["0.1.0", "0.0.9"] {
        let older = GalleryKit {
            version: version.into(),
            ..entry()
        };
        assert_eq!(gallery_status(&older, &[pinned()]), current(), "{version}");
    }
}

#[test]
fn offers_nothing_for_the_same_id_in_another_repository() {
    let elsewhere = GalleryKit {
        repo: "someone/ci-status".into(),
        ..entry()
    };
    assert_eq!(gallery_status(&elsewhere, &[pinned()]), current());
}

#[test]
fn says_a_folder_of_that_id_is_linked_and_offers_neither_install_nor_update() {
    let linked = KitInfo {
        source: folder(),
        installed_version: "0.1.0".into(),
        ..installed("ci-status")
    };
    assert_eq!(
        gallery_status(&entry(), &[linked]),
        GalleryStatus::Linked {
            version: "0.1.0".into()
        }
    );
}

#[test]
fn does_not_offer_an_update_over_a_kit_on_a_branch_or_a_plain_link() {
    let plain = GalleryStatus::Installed {
        version: "0.1.0".into(),
        pinned: None,
    };
    let url = "https://github.com/hangar-ai/ci-status";
    for source in [git(url, None), git(url, Some("main"))] {
        let kit = KitInfo {
            source,
            installed_version: "0.1.0".into(),
            ..installed("ci-status")
        };
        assert_eq!(gallery_status(&entry(), &[kit]), plain);
    }
}

fn roblox() -> GalleryKit {
    GalleryKit {
        id: "roblox".into(),
        name: "Roblox".into(),
        description: "Places.".into(),
        tags: vec!["game".into()],
        ..entry()
    }
}

#[test]
fn keeps_everything_without_a_query() {
    let kits = [entry(), roblox()];
    assert_eq!(filter_gallery(&kits, "  ").len(), 2);
}

#[test]
fn matches_name_description_and_tags_every_word() {
    let kits = [entry(), roblox()];
    let ids = |query: &str| -> Vec<String> {
        filter_gallery(&kits, query)
            .iter()
            .map(|kit| kit.id.clone())
            .collect()
    };
    assert_eq!(ids("GAME"), ["roblox"]);
    assert_eq!(ids("actions ci"), ["ci-status"]);
    assert!(ids("actions game").is_empty());
}

#[test]
fn names_the_tag_of_the_kit_s_own_repository_with_no_folder() {
    assert_eq!(
        gallery_install_url(&entry()),
        "https://github.com/hangar-ai/ci-status/tree/v0.2.0"
    );
    let other = GalleryKit {
        repo: "someone/kit".into(),
        tag: "1.0.0".into(),
        ..entry()
    };
    assert_eq!(
        gallery_install_url(&other),
        "https://github.com/someone/kit/tree/1.0.0"
    );
}

#[test]
fn the_library_strings_are_in_both_languages_and_the_french_is_not_the_english() {
    use hangar_i18n::{Key, Locale, Translator};
    let keys: Vec<Key> = Key::ALL
        .iter()
        .copied()
        .filter(|key| key.id().starts_with("kitsManage.gallery"))
        .collect();
    assert!(keys.len() > 10);
    let (en, fr) = (Translator::new(Locale::En), Translator::new(Locale::Fr));
    for key in keys {
        assert!(!fr.t(key).is_empty(), "{}", key.id());
        assert_ne!(fr.t(key), en.t(key), "{}", key.id());
    }
    assert_eq!(fr.t(Key::KitsManageGalleryTitle), "Bibliothèque");
}
