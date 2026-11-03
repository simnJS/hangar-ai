//! The cases of `lib/kitIcon.test.ts` and of `KitActivation.test.ts` (`KitIcon`).
//!
//! The web app's tests were about the URL the icon was served from and the CSS
//! mask that coloured it; here the file is read from disk, so the same four
//! questions are asked of the file: which one, whether it is a kit's own, and
//! what a bad one does.

use std::path::PathBuf;

use hangar_core::kits::{KitInfo, Manifest};

use super::*;
use crate::kits::testing::{installed, manifest};

fn kit(icon: Option<&str>, manifest_id: &str, error: Option<&str>) -> KitInfo {
    let mut found: Manifest = manifest(manifest_id, "1.0.0", vec![]);
    found.icon = icon.map(str::to_owned);
    KitInfo {
        manifest: Some(found),
        error: error.map(str::to_owned),
        ..installed("hello-kit")
    }
}

/// A folder of its own under the temporary one, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path =
            std::env::temp_dir().join(format!("hangar-kits-icon-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("folder");
        Folder(path)
    }

    fn file(&self, relative: &str, bytes: &[u8]) -> &Folder {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parents");
        std::fs::write(path, bytes).expect("file");
        self
    }

    fn dir(&self) -> &str {
        self.0.to_str().expect("utf-8 path")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn names_the_manifest_s_icon_file() {
    assert_eq!(
        icon_file(&kit(Some("icon.svg"), "hello-kit", None)),
        Some("icon.svg")
    );
    assert_eq!(
        icon_file(&kit(Some("assets/icon.png"), "hello-kit", None)),
        Some("assets/icon.png")
    );
}

#[test]
fn gives_nothing_for_a_kit_without_icon_broken_or_under_another_id() {
    assert_eq!(icon_file(&kit(None, "hello-kit", None)), None);
    assert_eq!(icon_file(&kit(Some("  "), "hello-kit", None)), None);
    assert_eq!(
        icon_file(&kit(Some("i.svg"), "hello-kit", Some("bad manifest"))),
        None
    );
    assert_eq!(icon_file(&installed("x")), None);
    assert_eq!(icon_file(&kit(Some("i.svg"), "other", None)), None);
}

#[test]
fn the_cache_is_told_by_where_the_icon_is_and_the_version_it_came_from() {
    let a = kit(Some("icon.svg"), "hello-kit", None);
    let mut b = a.clone();
    b.installed_version = "1.1.0".into();
    assert_ne!(icon_key(&a), icon_key(&b));
    assert_eq!(icon_key(&kit(None, "hello-kit", None)), None);
}

#[test]
fn reads_an_svg_and_a_png_and_says_which() {
    let folder = Folder::new("kinds");
    folder
        .file("icon.svg", b"<svg/>")
        .file("sub/icon.PNG", b"\x89PNG");
    assert_eq!(
        read_icon(folder.dir(), "icon.svg"),
        Ok(IconBytes::Svg(b"<svg/>".to_vec()))
    );
    assert!(matches!(
        read_icon(folder.dir(), "sub/icon.PNG"),
        Ok(IconBytes::Png(_))
    ));
}

#[test]
fn refuses_what_is_not_an_image_file_of_a_reasonable_size() {
    let folder = Folder::new("refused");
    folder
        .file("page.html", b"<p/>")
        .file("big.svg", &vec![b' '; MAX_ICON_BYTES as usize + 1]);
    assert!(read_icon(folder.dir(), "page.html").is_err());
    assert!(read_icon(folder.dir(), "big.svg").is_err());
    assert!(read_icon(folder.dir(), "missing.svg").is_err());
    assert!(read_icon(folder.dir(), "../hangar-kits-icon-nothing.svg").is_err());
}

#[cfg(unix)]
#[test]
fn refuses_a_link_that_leads_out_of_the_kit() {
    let outside = Folder::new("outside");
    outside.file("secret.svg", b"<svg/>");
    let folder = Folder::new("linked");
    std::os::unix::fs::symlink(outside.0.join("secret.svg"), folder.0.join("icon.svg"))
        .expect("symlink");
    assert_eq!(
        read_icon(folder.dir(), "icon.svg"),
        Err("the icon is outside the kit's folder".into())
    );
}

#[test]
fn asks_for_the_icons_the_cache_does_not_hold_and_for_nothing_twice() {
    let with_icon = kit(Some("icon.svg"), "hello-kit", None);
    let without = KitInfo {
        id: "plain".into(),
        manifest: None,
        ..installed("plain")
    };
    let mut icons = Icons::default();
    let wanted = icons.missing(&[with_icon.clone(), without.clone()]);
    assert_eq!(wanted.len(), 1);
    assert_eq!(wanted[0].id, "hello-kit");
    assert_eq!(wanted[0].file, "icon.svg");

    icons.loaded(
        "hello-kit",
        wanted[0].key.clone(),
        Ok(IconBytes::Svg(b"<svg/>".to_vec())),
    );
    assert!(
        icons
            .missing(&[with_icon.clone(), without.clone()])
            .is_empty()
    );
    assert!(matches!(icons.of(&with_icon), Some(IconHandle::Svg(_))));

    // A new version of the kit asks again; the old picture is not shown for it.
    let mut newer = with_icon.clone();
    newer.installed_version = "2.0.0".into();
    assert_eq!(icons.missing(&[newer.clone()]).len(), 1);
    assert!(icons.of(&newer).is_none());

    icons.forget("hello-kit");
    assert_eq!(icons.missing(std::slice::from_ref(&with_icon)).len(), 1);
}

#[test]
fn an_icon_that_failed_to_load_falls_back_and_is_not_asked_for_again() {
    let with_icon = kit(Some("icon.svg"), "hello-kit", None);
    let mut icons = Icons::default();
    let key = icon_key(&with_icon).expect("key");
    icons.loaded("hello-kit", key, Err("gone".into()));
    assert!(icons.of(&with_icon).is_none());
    assert!(icons.missing(&[with_icon]).is_empty());
}

#[test]
fn forgets_the_kits_that_are_gone() {
    let with_icon = kit(Some("icon.svg"), "hello-kit", None);
    let mut icons = Icons::default();
    icons.loaded(
        "hello-kit",
        icon_key(&with_icon).expect("key"),
        Ok(IconBytes::Png(vec![1])),
    );
    icons.retain(&[installed("other")]);
    assert!(icons.of(&with_icon).is_none());
}

#[test]
fn draws_an_svg_a_png_and_the_plug_without_failing() {
    // The widgets are built, not laid out; what is checked is that each of the
    // three paths yields an element.
    let color = Color::WHITE;
    let with_icon = kit(Some("icon.svg"), "hello-kit", None);
    let mut icons = Icons::default();
    let _plug = kit_icon_in(&icons, &with_icon, 16.0, color);
    icons.loaded(
        "hello-kit",
        icon_key(&with_icon).expect("key"),
        Ok(IconBytes::Svg(b"<svg/>".to_vec())),
    );
    let _svg = kit_icon_in(&icons, &with_icon, 16.0, color);
    let png = kit(Some("icon.png"), "hello-kit", None);
    icons.loaded(
        "hello-kit",
        icon_key(&png).expect("key"),
        Ok(IconBytes::Png(vec![1])),
    );
    let _png = kit_icon_in(&icons, &png, 16.0, color);
}
