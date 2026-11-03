//! The cases of `kitGallery.test.ts` that drew `KitsGalleryView`, against its
//! model.

use hangar_core::kits::KitInfo;
use hangar_i18n::{Locale, Translator};

use super::*;
use crate::kits::gallery::{Gallery, GalleryKit, Origin};
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

fn live() -> Gallery {
    Gallery {
        origin: Origin::Live,
        kits: vec![entry()],
        fetched_at: Some(1),
    }
}

struct Draw {
    gallery: Option<Gallery>,
    error: Option<String>,
    refreshing: bool,
    installed: Option<Vec<KitInfo>>,
    busy: Option<String>,
    failure: Option<GalleryFailure>,
    query: String,
}

impl Draw {
    fn new() -> Draw {
        Draw {
            gallery: Some(live()),
            error: None,
            refreshing: false,
            installed: Some(Vec::new()),
            busy: None,
            failure: None,
            query: String::new(),
        }
    }

    fn model(&self, locale: Locale) -> GalleryModel {
        GalleryModel::build(
            Translator::new(locale),
            &GalleryInput {
                gallery: self.gallery.as_ref(),
                error: self.error.as_deref(),
                refreshing: self.refreshing,
                installed: self.installed.as_deref(),
                busy: self.busy.as_deref(),
                failure: self.failure.as_ref(),
                query: &self.query,
            },
        )
    }

    fn en(&self) -> GalleryModel {
        self.model(Locale::En)
    }
}

fn cards(model: &GalleryModel) -> &[Card] {
    let Loadable::Ready(cards) = &model.body else {
        panic!("the library lists kits");
    };
    cards
}

#[test]
fn shows_the_kit_with_its_tags_and_an_install_button_and_says_nothing_is_enabled() {
    let model = Draw::new().en();
    let out = model.text();
    assert!(out.contains("CI status"));
    assert!(out.contains("Follows a GitHub Actions run."));
    assert!(cards(&model)[0].tags.contains(&"github".to_owned()));
    assert!(out.contains("Install"));
    assert!(out.contains("nothing is enabled"));
    assert!(out.contains("Runs gh."));
}

#[test]
fn says_which_version_is_installed_and_offers_the_update() {
    let mut draw = Draw::new();
    draw.installed = Some(vec![KitInfo {
        installed_version: "0.2.0".into(),
        ..pinned()
    }]);
    assert!(draw.en().text().contains("Installed (v0.2.0)"));

    draw.installed = Some(vec![pinned()]);
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("Installed v0.1.0, v0.2.0 available"));
    assert!(out.contains("Update to v0.2.0"));
    assert!(matches!(
        &cards(&model)[0].action,
        CardAction::Update { enabled: true, .. }
    ));
}

#[test]
fn says_what_a_kit_installed_from_a_tag_is_pinned_to_with_its_version() {
    let mut draw = Draw::new();
    draw.gallery = Some(Gallery {
        kits: vec![GalleryKit {
            tag: "v0.1.0".into(),
            version: "0.1.0".into(),
            ..entry()
        }],
        ..live()
    });
    draw.installed = Some(vec![pinned()]);
    let out = draw.en().text();
    assert!(out.contains("Installed (v0.1.0)"));
    assert!(out.contains("Pinned to v0.1.0"));
    draw.gallery = Some(live());
    assert!(draw.en().text().contains("Pinned to v0.1.0"));
}

#[test]
fn says_a_linked_folder_holds_that_id_not_that_the_kit_is_installed_and_offers_nothing() {
    let mut draw = Draw::new();
    draw.installed = Some(vec![KitInfo {
        source: folder(),
        installed_version: "0.1.0".into(),
        ..installed("ci-status")
    }]);
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("linked, read live (v0.1.0)"));
    assert!(!out.contains("Installed"));
    assert!(!out.contains("Pinned to"));
    assert_eq!(cards(&model)[0].action, CardAction::None);
}

#[test]
fn does_not_say_pinned_of_a_kit_that_is_not() {
    let mut draw = Draw::new();
    draw.installed = Some(vec![KitInfo {
        source: git("https://github.com/o/r", Some("main")),
        ..installed("ci-status")
    }]);
    assert!(!draw.en().text().contains("Pinned to"));
    assert!(!Draw::new().en().text().contains("Pinned to"));
}

#[test]
fn names_the_repository_and_the_tag_a_kit_installs_from() {
    assert!(
        Draw::new()
            .en()
            .text()
            .contains("hangar-ai/ci-status · v0.2.0")
    );
}

#[test]
fn shows_a_verified_kit_with_its_badge_and_nothing_about_a_review_it_did_not_have() {
    let model = Draw::new().en();
    let out = model.text();
    assert!(out.contains("Verified"));
    assert_eq!(cards(&model)[0].verified.tone, TagTone::Ok);
    assert!(!out.contains("Community kit"));
    assert!(!out.contains("not reviewed"));
}

#[test]
fn shows_anything_else_as_a_community_kit_in_one_plain_sentence_installed_the_same_way() {
    let mut draw = Draw::new();
    draw.gallery = Some(Gallery {
        kits: vec![GalleryKit {
            verified: false,
            ..entry()
        }],
        ..live()
    });
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("Community kit"));
    assert!(out.contains("It was not reviewed by Hangar."));
    assert!(!out.contains("Verified"));
    assert!(matches!(
        &cards(&model)[0].action,
        CardAction::Install { label, enabled: true } if label == "Install"
    ));
    assert!(out.contains("nothing is enabled"));
}

#[test]
fn says_when_it_shows_the_kept_copy() {
    let mut draw = Draw::new();
    draw.gallery = Some(Gallery {
        origin: Origin::Cached,
        ..live()
    });
    let model = draw.en();
    assert!(matches!(&model.notices[0], Notice::Cached(text)
        if text.starts_with("The library could not be read: showing the copy kept on")));
}

#[test]
fn reads_an_empty_catalog_as_an_empty_library_not_as_a_failure() {
    let mut draw = Draw::new();
    draw.gallery = Some(Gallery {
        origin: Origin::Live,
        kits: vec![],
        fetched_at: Some(1),
    });
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("The library lists no kit yet."));
    assert!(!out.contains("cannot be reached"));
    assert!(!out.contains("could not be read"));
    assert!(model.notices.is_empty());
    assert!(!out.contains("No kit matches"));
}

#[test]
fn says_plainly_when_there_is_no_list_at_all() {
    let mut draw = Draw::new();
    draw.gallery = Some(Gallery {
        origin: Origin::Unavailable,
        kits: vec![],
        fetched_at: None,
    });
    let out = draw.en().text();
    assert!(out.contains("cannot be reached"));
    assert!(!out.contains("lists no kit"));
}

#[test]
fn says_so_when_the_library_cannot_be_asked_for_at_all() {
    let mut draw = Draw::new();
    draw.gallery = None;
    draw.error = Some("no runtime".into());
    let model = draw.en();
    assert!(matches!(&model.notices[0], Notice::Unavailable(_)));
    assert!(cards(&model).is_empty());
}

#[test]
fn reads_the_library_while_it_has_not_answered() {
    let mut draw = Draw::new();
    draw.gallery = None;
    assert!(matches!(draw.en().body, Loadable::Loading(_)));
}

#[test]
fn filters_and_says_when_nothing_matches() {
    let mut draw = Draw::new();
    draw.query = "zzz".into();
    assert!(draw.en().text().contains("No kit matches this filter."));
    draw.query = "ci".into();
    assert_eq!(cards(&draw.en()).len(), 1);
}

#[test]
fn names_the_filter_and_the_refresh_button() {
    let model = Draw::new().en();
    assert_eq!(model.filter_label, "Filter the library");
    assert_eq!(model.filter_placeholder, "Filter by name or tag");
    assert_eq!(model.refresh, "Refresh");
    let mut draw = Draw::new();
    draw.refreshing = true;
    let busy = draw.en();
    assert_eq!(busy.refresh, "Refreshing…");
    assert!(!busy.refresh_enabled);
}

#[test]
fn takes_the_dictionary_s_placeholder_without_copying_it() {
    for locale in [Locale::En, Locale::Fr] {
        let model = Draw::new().model(locale);
        assert!(!model.filter_placeholder.is_empty(), "{locale:?}");
    }
}

#[test]
fn reports_an_install_that_failed_by_name() {
    let mut draw = Draw::new();
    draw.failure = Some(GalleryFailure {
        id: "ci-status".into(),
        name: "CI status".into(),
        detail: "not a kit".into(),
    });
    let model = draw.en();
    assert!(matches!(&model.notices[0], Notice::InstallFailed(text)
        if text.contains("CI status") && text.contains("not a kit")));
}

#[test]
fn shows_the_one_being_installed_and_blocks_the_others() {
    let mut draw = Draw::new();
    draw.busy = Some("ci-status".into());
    assert!(matches!(
        &cards(&draw.en())[0].action,
        CardAction::Install { label, enabled: false } if label == "Installing…"
    ));
    draw.busy = Some("another".into());
    assert!(matches!(
        &cards(&draw.en())[0].action,
        CardAction::Install { enabled: false, .. }
    ));
    // Nor can anything be installed before the kits on the machine are known.
    let mut unknown = Draw::new();
    unknown.installed = None;
    assert!(matches!(
        &cards(&unknown.en())[0].action,
        CardAction::Install { enabled: false, .. }
    ));
}

#[test]
fn speaks_french() {
    let out = Draw::new().model(Locale::Fr).text();
    assert!(out.contains("Bibliothèque"));
}

#[test]
fn writes_the_date_of_a_kept_copy_the_way_each_language_does() {
    // 2026-11-03T12:00:00Z
    let ms = 1_793_707_200_000;
    assert_eq!(format_date(ms, Locale::En), "11/3/2026");
    assert_eq!(format_date(ms, Locale::Fr), "03/11/2026");
    assert_eq!(format_date(0, Locale::En), "1/1/1970");
    // A leap day.
    assert_eq!(format_date(1_709_164_800_000, Locale::Fr), "29/02/2024");
}
