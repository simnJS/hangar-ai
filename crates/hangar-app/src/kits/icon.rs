//! A kit's own icon (`manifest.icon`, an SVG or a PNG of its folder): which
//! file it is, how it is read, and how it is drawn. `lib/kitIcon.ts` and
//! `KitIcon` of `KitActivation.tsx`.
//!
//! The web app served the file through the `hangar-kit` scheme and drew it in
//! an `<img>`, so a kit's SVG was never put in the page and could not run
//! anything. Here the bytes are read from disk by the same rule (a file of the
//! kit's own folder, links followed and kept inside it) and handed to iced's
//! SVG and image widgets, which draw them and run nothing. An SVG takes the
//! colour of the text around it, as Hangar's own icons do: drawn as it is,
//! `stroke="currentColor"` would be black, and on a dark theme every kit's
//! icon would have disappeared.

use std::collections::HashMap;
use std::path::Path;

use iced::widget::{image, svg};
use iced::{Color, Element, Length};

use hangar_core::kits::KitInfo;
use hangar_ui::{Icon, icon};

use crate::ctx::Ctx;
use crate::message::Message;

/// The largest icon file read. An icon is a few kilobytes; this keeps a kit
/// from handing the window a file of any size.
pub const MAX_ICON_BYTES: u64 = 512 * 1024;

/// The icon's file, relative to the kit's folder as the manifest normalized it;
/// `None` when the kit has none or cannot be used, so the caller keeps its
/// generic glyph.
pub fn icon_file(kit: &KitInfo) -> Option<&str> {
    let manifest = kit.manifest.as_ref()?;
    if kit.error.is_some() || manifest.id != kit.id {
        return None;
    }
    manifest
        .icon
        .as_deref()
        .map(str::trim)
        .filter(|icon| !icon.is_empty())
}

/// What tells one icon from another on the cache: where it is and which
/// version of the kit it was read from.
pub fn icon_key(kit: &KitInfo) -> Option<String> {
    icon_file(kit).map(|file| format!("{}\n{file}\n{}", kit.dir, kit.installed_version))
}

/// An icon file's bytes, by what they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconBytes {
    Svg(Vec<u8>),
    Png(Vec<u8>),
}

/// Reads the kit's icon: a `.svg` or `.png` inside the kit's folder, at most
/// [`MAX_ICON_BYTES`]. Blocking: call it off the UI thread.
pub fn read_icon(dir: &str, file: &str) -> Result<IconBytes, String> {
    let extension = Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    let svg = match extension.as_deref() {
        Some("svg") => true,
        Some("png") => false,
        _ => return Err("an icon is a .svg or a .png".into()),
    };
    let root = std::fs::canonicalize(dir).map_err(|err| err.to_string())?;
    // Links are followed, then the result has to be inside the folder: a link
    // that leads out of the kit is no icon of it.
    let path = std::fs::canonicalize(root.join(file)).map_err(|err| err.to_string())?;
    if !path.starts_with(&root) {
        return Err("the icon is outside the kit's folder".into());
    }
    let meta = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    if !meta.is_file() || meta.len() > MAX_ICON_BYTES {
        return Err("the icon is not a file of a reasonable size".into());
    }
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    Ok(if svg {
        IconBytes::Svg(bytes)
    } else {
        IconBytes::Png(bytes)
    })
}

/// [`read_icon`] off the UI thread.
pub async fn load_icon(dir: String, file: String) -> Result<IconBytes, String> {
    tokio::task::spawn_blocking(move || read_icon(&dir, &file))
        .await
        .map_err(|err| err.to_string())?
}

/// An icon ready to draw.
#[derive(Debug, Clone)]
pub enum IconHandle {
    Svg(svg::Handle),
    Png(image::Handle),
}

impl From<IconBytes> for IconHandle {
    fn from(bytes: IconBytes) -> Self {
        match bytes {
            IconBytes::Svg(bytes) => IconHandle::Svg(svg::Handle::from_memory(bytes)),
            IconBytes::Png(bytes) => IconHandle::Png(image::Handle::from_bytes(bytes)),
        }
    }
}

/// The icons read so far, by kit id, in `Live`. An entry whose `handle` is
/// `None` was read and failed: it is not asked for again until the kit's icon
/// changes, and the plug stands in.
#[derive(Debug, Clone, Default)]
pub struct Icons {
    by_kit: HashMap<String, Loaded>,
}

#[derive(Debug, Clone)]
struct Loaded {
    key: String,
    handle: Option<IconHandle>,
}

/// An icon to read: which kit's, under which key, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconRead {
    pub id: String,
    pub key: String,
    pub dir: String,
    pub file: String,
}

impl Icons {
    /// The icons that have to be read for these kits: the ones with a file of
    /// their own that the cache does not hold for the kit as it is now.
    pub fn missing(&self, kits: &[KitInfo]) -> Vec<IconRead> {
        kits.iter()
            .filter_map(|kit| {
                let key = icon_key(kit)?;
                if self.by_kit.get(&kit.id).is_some_and(|have| have.key == key) {
                    return None;
                }
                Some(IconRead {
                    id: kit.id.clone(),
                    key,
                    dir: kit.dir.clone(),
                    file: icon_file(kit)?.to_owned(),
                })
            })
            .collect()
    }

    /// Keeps only the kits that are still installed.
    pub fn retain(&mut self, kits: &[KitInfo]) {
        self.by_kit
            .retain(|id, _| kits.iter().any(|kit| &kit.id == id));
    }

    /// Takes in what a read answered.
    pub fn loaded(&mut self, id: &str, key: String, result: Result<IconBytes, String>) {
        self.by_kit.insert(
            id.to_owned(),
            Loaded {
                key,
                handle: result.ok().map(IconHandle::from),
            },
        );
    }

    /// Forgets a kit's icon so that the next list reads it again: after a
    /// reload, when the file may have been edited under the same version.
    pub fn forget(&mut self, id: &str) {
        self.by_kit.remove(id);
    }

    /// The icon of this kit as it is now, if it has been read.
    pub fn of(&self, kit: &KitInfo) -> Option<&IconHandle> {
        let key = icon_key(kit)?;
        let found = self.by_kit.get(&kit.id)?;
        (found.key == key)
            .then_some(found.handle.as_ref())
            .flatten()
    }
}

/// The kit's icon at `side` pixels, in the colour of the text around it
/// (`color`), or the plug when it has none or it does not load.
pub fn kit_icon<'a>(ctx: Ctx<'a>, kit: &KitInfo, side: f32) -> Element<'a, Message> {
    kit_icon_in(&ctx.model.live.kits.icons, kit, side, ctx.theme.colors.fg)
}

fn kit_icon_in<'a>(icons: &Icons, kit: &KitInfo, side: f32, color: Color) -> Element<'a, Message> {
    match icons.of(kit) {
        Some(IconHandle::Svg(handle)) => svg(handle.clone())
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .style(move |_, _| svg::Style { color: Some(color) })
            .into(),
        Some(IconHandle::Png(handle)) => image(handle.clone())
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .into(),
        None => icon(Icon::Plug, side).into(),
    }
}

#[cfg(test)]
mod tests;
