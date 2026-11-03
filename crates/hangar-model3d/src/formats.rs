//! Which files the preview opens, and how each format is read.
//!
//! One table decides it, so no other module guesses from a file name a second
//! time. The list is the one hangar-core's `MODEL_EXTENSIONS` serves
//! (`crates/hangar-core/src/model.rs`).

use std::fmt;
use std::io::ErrorKind;
use std::path::Path;

/// Largest model the preview accepts, textures not included. The same cap
/// hangar-core enforces when it checks a file, repeated here so a caller that
/// skipped that check still cannot make the loader read a multi-gigabyte file
/// into memory.
pub const MAX_MODEL_BYTES: u64 = 256 * 1024 * 1024;

/// A model format the preview opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelFormat {
    Glb,
    Gltf,
    Obj,
    Stl,
    Ply,
    Fbx,
    Usdz,
}

/// Which reader a format goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parser {
    /// The `gltf` crate, for both the binary and the JSON flavour.
    Gltf,
    Fbx,
    Usd,
    /// Plain geometry: OBJ, STL and PLY are text or flat arrays that a reader
    /// walks vertex by vertex, which for a 50 MB file is seconds of work.
    /// They are the formats worth reading on a thread of their own, as the
    /// old viewer did with a worker; here every format loads off the window's
    /// thread, so this only names the group.
    Plain,
}

/// How a format is read and laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatPlan {
    pub parser: Parser,
    /// The unit the format promises, when it promises one: glTF is in metres
    /// by specification. The others are in whatever the exporter wrote.
    pub unit: Option<&'static str>,
    /// The file is laid out Z-up and is turned upright for display. STL comes
    /// out of CAD and slicers that way; the sizes the viewer reports stay the
    /// file's own.
    pub z_up: bool,
}

impl ModelFormat {
    /// Every format, in the order the old viewer listed them.
    pub const ALL: [ModelFormat; 7] = [
        ModelFormat::Glb,
        ModelFormat::Gltf,
        ModelFormat::Obj,
        ModelFormat::Stl,
        ModelFormat::Ply,
        ModelFormat::Fbx,
        ModelFormat::Usdz,
    ];

    /// The file extension, lower case and without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ModelFormat::Glb => "glb",
            ModelFormat::Gltf => "gltf",
            ModelFormat::Obj => "obj",
            ModelFormat::Stl => "stl",
            ModelFormat::Ply => "ply",
            ModelFormat::Fbx => "fbx",
            ModelFormat::Usdz => "usdz",
        }
    }

    /// The name an error message calls the format by.
    pub fn label(self) -> &'static str {
        match self {
            ModelFormat::Glb => "GLB",
            ModelFormat::Gltf => "glTF",
            ModelFormat::Obj => "OBJ",
            ModelFormat::Stl => "STL",
            ModelFormat::Ply => "PLY",
            ModelFormat::Fbx => "FBX",
            ModelFormat::Usdz => "USDZ",
        }
    }

    /// The format an extension names, whatever its case.
    pub fn from_extension(extension: &str) -> Option<ModelFormat> {
        let extension = extension.to_ascii_lowercase();
        ModelFormat::ALL
            .into_iter()
            .find(|format| format.extension() == extension)
    }

    /// The format a file name says, or `None` for anything the preview cannot
    /// open. A Windows path is read as one on every platform, because a path an
    /// agent wrote can come from a machine other than this one.
    pub fn from_path(path: impl AsRef<Path>) -> Option<ModelFormat> {
        let name = path.as_ref().to_string_lossy();
        let file = name.rsplit(['/', '\\']).next().unwrap_or(&name);
        let (_, extension) = file.rsplit_once('.')?;
        ModelFormat::from_extension(extension)
    }

    pub fn plan(self) -> FormatPlan {
        match self {
            ModelFormat::Glb | ModelFormat::Gltf => FormatPlan {
                parser: Parser::Gltf,
                unit: Some("m"),
                z_up: false,
            },
            ModelFormat::Fbx => FormatPlan {
                parser: Parser::Fbx,
                unit: None,
                z_up: false,
            },
            ModelFormat::Usdz => FormatPlan {
                parser: Parser::Usd,
                unit: None,
                z_up: false,
            },
            ModelFormat::Obj | ModelFormat::Ply => FormatPlan {
                parser: Parser::Plain,
                unit: None,
                z_up: false,
            },
            ModelFormat::Stl => FormatPlan {
                parser: Parser::Plain,
                unit: None,
                z_up: true,
            },
        }
    }
}

impl fmt::Display for ModelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What goes wrong when a model is loaded. Each message is a sentence an agent
/// can act on: the app hands it back word for word as the answer to a request
/// to show the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// A newer load, or a closed tab, made this one pointless.
    Cancelled,
    /// The file (or one it points at) could not be read.
    Read(String),
    /// The file was read but is not a valid model of its format.
    Parse { format: ModelFormat, reason: String },
    /// The file is valid, but uses something this build cannot decode.
    Unsupported(String),
    /// Over [`MAX_MODEL_BYTES`].
    TooBig { size: u64 },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Cancelled => f.write_str("The load was cancelled."),
            LoadError::Read(message) | LoadError::Unsupported(message) => f.write_str(message),
            LoadError::Parse { format, reason } => {
                write!(
                    f,
                    "This {} file could not be read: {reason}",
                    format.label().to_ascii_uppercase()
                )
            }
            LoadError::TooBig { size } => write!(
                f,
                "The file is {} MB, over the preview's {} MB limit: decimate the mesh or \
                 compress it and show the smaller file.",
                size.div_ceil(1024 * 1024),
                MAX_MODEL_BYTES / (1024 * 1024)
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// What a failed read says, from the kind of failure and the reason the
/// system gave. The old viewer built the same sentences from an HTTP status;
/// the files are read directly now, so the kinds of `io::Error` stand in for
/// 404 and 403.
pub fn read_error(kind: ErrorKind, reason: &str) -> LoadError {
    let reason = reason
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    LoadError::Read(match kind {
        ErrorKind::NotFound => format!("The file was not found ({reason})."),
        ErrorKind::PermissionDenied => format!("Hangar may not read this file ({reason})."),
        _ if reason.is_empty() => "The file could not be read.".to_string(),
        _ => format!("The file could not be read: {reason}."),
    })
}

/// The folder a model sits in: what a glTF's `textures/wood.png` or an OBJ's
/// `.mtl` resolves against.
pub fn base_dir_of(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_a_way_to_read_every_format_the_viewer_accepts() {
        for format in ModelFormat::ALL {
            let plan = format.plan();
            assert!(matches!(
                plan.parser,
                Parser::Gltf | Parser::Fbx | Parser::Usd | Parser::Plain
            ));
            assert_eq!(
                ModelFormat::from_extension(format.extension()),
                Some(format)
            );
        }
    }

    #[test]
    fn groups_the_flat_geometry_formats_as_plain() {
        assert_eq!(ModelFormat::Obj.plan().parser, Parser::Plain);
        assert_eq!(ModelFormat::Stl.plan().parser, Parser::Plain);
        assert_eq!(ModelFormat::Ply.plan().parser, Parser::Plain);
    }

    #[test]
    fn sends_both_gltf_flavours_through_the_gltf_reader_in_metres() {
        assert_eq!(
            ModelFormat::Glb.plan(),
            FormatPlan {
                parser: Parser::Gltf,
                unit: Some("m"),
                z_up: false
            }
        );
        assert_eq!(ModelFormat::Gltf.plan(), ModelFormat::Glb.plan());
    }

    #[test]
    fn turns_only_stl_upright() {
        for format in ModelFormat::ALL {
            assert_eq!(format.plan().z_up, format == ModelFormat::Stl, "{format}");
        }
    }

    #[test]
    fn dispatches_a_file_by_its_extension_whatever_its_case() {
        let fbx = ModelFormat::from_path("C:\\art\\Chair.FBX").unwrap();
        assert_eq!(fbx.plan().parser, Parser::Fbx);
        let ply = ModelFormat::from_path("/home/me/scan.ply").unwrap();
        assert_eq!(ply.plan().parser, Parser::Plain);
        assert_eq!(ModelFormat::from_path("/home/me/notes.txt"), None);
    }

    #[test]
    fn reads_no_format_from_a_dot_in_a_folder_name() {
        assert_eq!(ModelFormat::from_path("/home/me/v1.glb/readme"), None);
        assert_eq!(ModelFormat::from_path("/home/me/chair"), None);
        assert_eq!(
            ModelFormat::from_path("/home/me/.glb"),
            Some(ModelFormat::Glb)
        );
    }

    #[test]
    fn is_the_folder_of_the_model_for_the_files_beside_it() {
        assert_eq!(
            base_dir_of(Path::new("/game/props/crate.gltf")),
            Path::new("/game/props")
        );
        assert_eq!(base_dir_of(Path::new("crate.gltf")), Path::new(""));
    }

    #[test]
    fn says_why_a_file_could_not_be_read() {
        assert_eq!(
            read_error(
                ErrorKind::PermissionDenied,
                "outside every allowed folder\n"
            )
            .to_string(),
            "Hangar may not read this file (outside every allowed folder)."
        );
        assert_eq!(
            read_error(ErrorKind::NotFound, "No such file or directory").to_string(),
            "The file was not found (No such file or directory)."
        );
        assert_eq!(
            read_error(ErrorKind::Other, "").to_string(),
            "The file could not be read."
        );
        assert_eq!(
            read_error(ErrorKind::InvalidData, "bad block").to_string(),
            "The file could not be read: bad block."
        );
    }

    #[test]
    fn names_the_format_in_a_parse_error() {
        let error = LoadError::Parse {
            format: ModelFormat::Glb,
            reason: "unexpected end".into(),
        };
        assert_eq!(
            error.to_string(),
            "This GLB file could not be read: unexpected end"
        );
    }
}
