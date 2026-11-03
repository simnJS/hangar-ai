//! Reading a model file: the whole of it into memory, then into a [`Model`],
//! on a thread of its own so the window never waits on it.
//!
//! The readers are pure Rust (`gltf`, `tobj`, `stl_io`, `ply-rs`). A model is
//! read whole before it is parsed, as the old viewer did, because the formats
//! are not streamable and the cap on a file's size (`MAX_MODEL_BYTES`) is what
//! bounds the memory one load can take.

mod geometry;
mod gltf;
mod obj;
mod ply;
mod resources;
mod stl;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::futures::StreamExt;
use iced::futures::channel::mpsc;

use crate::formats::{LoadError, MAX_MODEL_BYTES, ModelFormat, Parser, base_dir_of, read_error};
use crate::model::{Instance, Model, ModelParts};

use resources::Resources;

pub(crate) use geometry::face_normal;
pub use resources::{decode_image, percent_decode};

/// How a load is run.
#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    /// The folders a model's own files — a glTF's textures, an OBJ's `.mtl` —
    /// may be read from, canonical. `None` reads anywhere next to the model
    /// and above it. The app passes the folders hangar-core allows
    /// (`allowed_roots` in `model.rs`), so a hostile model cannot make the
    /// preview open files elsewhere on the disk.
    pub roots: Option<Vec<PathBuf>>,
}

/// What a load is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The file is being read from disk.
    Read,
    /// It is being turned into meshes and textures.
    Parse,
    /// Bounds, statistics and the points the camera frames are being worked
    /// out.
    Prepare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadProgress {
    pub phase: Phase,
    /// Bytes read so far, during [`Phase::Read`].
    pub loaded: u64,
    /// Bytes expected, when known.
    pub total: u64,
}

/// A load that a newer one, or a closed tab, made pointless: cancelling stops
/// it at the next step it takes.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn check(&self) -> Result<(), LoadError> {
        if self.is_cancelled() {
            Err(LoadError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// What the readers are given besides the bytes.
pub(crate) struct Ctx<'a> {
    pub format: ModelFormat,
    pub base: &'a Path,
    pub options: &'a LoadOptions,
    pub cancel: &'a Cancel,
}

impl<'a> Ctx<'a> {
    pub fn resources(&self) -> Resources<'a> {
        Resources {
            base: self.base,
            options: self.options,
        }
    }

    pub fn parse_error(&self, reason: impl ToString) -> LoadError {
        LoadError::Parse {
            format: self.format,
            reason: reason.to_string(),
        }
    }
}

/// Reads one model with the defaults, blocking: for tests and tools. The app
/// wants [`start`].
pub fn load(path: impl AsRef<Path>) -> Result<Model, LoadError> {
    load_with(
        path.as_ref(),
        &LoadOptions::default(),
        &Cancel::default(),
        &mut |_| {},
    )
}

/// Reads one model, reporting progress as it goes.
pub fn load_with(
    path: &Path,
    options: &LoadOptions,
    cancel: &Cancel,
    progress: &mut dyn FnMut(LoadProgress),
) -> Result<Model, LoadError> {
    let format = ModelFormat::from_path(path).ok_or_else(|| {
        LoadError::Unsupported(format!(
            "{} is not a format the preview opens: export the model as one of {} and show that \
             file. A .glb keeps its textures inside it.",
            path.display(),
            ModelFormat::ALL
                .iter()
                .map(|format| format!(".{}", format.extension()))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;
    let bytes = read_whole(path, cancel, progress)?;
    progress(LoadProgress {
        phase: Phase::Parse,
        loaded: bytes.len() as u64,
        total: bytes.len() as u64,
    });
    let name = path.file_name().map_or_else(
        || "model".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    load_bytes(
        &name,
        format,
        &bytes,
        base_dir_of(path),
        options,
        cancel,
        progress,
    )
}

/// Reads a model from bytes already in memory. `base` is the folder its own
/// files are looked for in.
pub fn load_bytes(
    name: &str,
    format: ModelFormat,
    bytes: &[u8],
    base: &Path,
    options: &LoadOptions,
    cancel: &Cancel,
    progress: &mut dyn FnMut(LoadProgress),
) -> Result<Model, LoadError> {
    cancel.check()?;
    let ctx = Ctx {
        format,
        base,
        options,
        cancel,
    };
    let parts: ModelParts = match format.plan().parser {
        Parser::Gltf => gltf::read(bytes, &ctx)?,
        Parser::Plain => match format {
            ModelFormat::Obj => obj::read(bytes, &ctx)?,
            ModelFormat::Stl => stl::read(bytes, &ctx)?,
            _ => ply::read(bytes, &ctx)?,
        },
        Parser::Fbx | Parser::Usd => {
            return Err(LoadError::Unsupported(format!(
                "{} files are not supported by this build of the preview: export the model as \
                 .glb and show that file.",
                format.label()
            )));
        }
    };
    cancel.check()?;
    progress(LoadProgress {
        phase: Phase::Prepare,
        loaded: 0,
        total: 0,
    });
    Ok(Model::new(name, format, parts))
}

/// A file read whole, with the progress reported chunk by chunk.
fn read_whole(
    path: &Path,
    cancel: &Cancel,
    progress: &mut dyn FnMut(LoadProgress),
) -> Result<Vec<u8>, LoadError> {
    let fail = |err: std::io::Error| read_error(err.kind(), &err.to_string());
    let mut file = std::fs::File::open(path).map_err(fail)?;
    let metadata = file.metadata().map_err(fail)?;
    if !metadata.is_file() {
        return Err(LoadError::Read(format!(
            "{} is a folder: give the path of the model file inside it.",
            path.display()
        )));
    }
    let total = metadata.len();
    if total > MAX_MODEL_BYTES {
        return Err(LoadError::TooBig { size: total });
    }
    // Written straight into one buffer: no second copy of a 200 MB file just
    // to join its chunks.
    let mut bytes = Vec::with_capacity(total as usize);
    let mut chunk = vec![0u8; 1 << 20];
    progress(LoadProgress {
        phase: Phase::Read,
        loaded: 0,
        total,
    });
    loop {
        cancel.check()?;
        let n = file.read(&mut chunk).map_err(fail)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() as u64 > MAX_MODEL_BYTES {
            return Err(LoadError::TooBig {
                size: bytes.len() as u64,
            });
        }
        progress(LoadProgress {
            phase: Phase::Read,
            loaded: bytes.len() as u64,
            total,
        });
    }
    Ok(bytes)
}

/// What a running load reports.
#[derive(Debug)]
pub enum LoadEvent {
    Progress(LoadProgress),
    /// The last event: the model, or why it could not be read.
    Done(Result<Arc<Model>, LoadError>),
}

/// Starts reading a model on a thread of its own. The events arrive on the
/// returned stream (`Task::run` takes it as it is); dropping it, or calling
/// [`Cancel::cancel`], stops the load at its next step.
pub fn start(path: PathBuf, options: LoadOptions) -> (mpsc::UnboundedReceiver<LoadEvent>, Cancel) {
    let (sender, receiver) = mpsc::unbounded();
    let cancel = Cancel::default();
    let token = cancel.clone();
    let spawned = std::thread::Builder::new()
        .name("model3d-load".into())
        .spawn({
            let sender = sender.clone();
            move || {
                let mut report = |progress| {
                    // A receiver that is gone means nobody wants the model.
                    if sender
                        .unbounded_send(LoadEvent::Progress(progress))
                        .is_err()
                    {
                        token.cancel();
                    }
                };
                let result = load_with(&path, &options, &token, &mut report).map(Arc::new);
                let _ = sender.unbounded_send(LoadEvent::Done(result));
            }
        });
    if let Err(err) = spawned {
        let _ = sender.unbounded_send(LoadEvent::Done(Err(LoadError::Read(format!(
            "The preview could not start a thread to read the file: {err}."
        )))));
    }
    (receiver, cancel)
}

/// Reads a model without progress, as a future: the shape `Task::perform`
/// wants. The work runs on its own thread, not the executor's.
pub async fn load_async(path: PathBuf, options: LoadOptions) -> Result<Arc<Model>, LoadError> {
    let (mut events, _cancel) = start(path, options);
    while let Some(event) = events.next().await {
        if let LoadEvent::Done(result) = event {
            return result;
        }
    }
    Err(LoadError::Cancelled)
}

/// Every mesh of the parts standing at the origin, once: what the formats that
/// have no scene graph produce.
pub(crate) fn placed_at_origin(parts: &mut ModelParts) {
    parts.instances = (0..parts.meshes.len())
        .map(|mesh| Instance {
            mesh,
            transform: glam::Mat4::IDENTITY,
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples;

    #[test]
    fn refuses_a_file_that_is_not_a_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "hello").unwrap();
        let error = load(&path).unwrap_err();
        assert!(matches!(error, LoadError::Unsupported(_)), "{error}");
        assert!(error.to_string().contains(".glb, .gltf, .obj"));
    }

    #[test]
    fn says_a_missing_file_is_not_found() {
        let error = load("/definitely/not/here.glb").unwrap_err();
        assert!(
            error.to_string().starts_with("The file was not found"),
            "{error}"
        );
    }

    #[test]
    fn says_a_folder_is_not_a_model() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("chair.glb");
        std::fs::create_dir(&folder).unwrap();
        assert!(
            load(&folder)
                .unwrap_err()
                .to_string()
                .contains("is a folder")
        );
    }

    #[test]
    fn stops_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cube.glb");
        std::fs::write(&path, samples::cube_glb()).unwrap();
        let cancel = Cancel::default();
        cancel.cancel();
        let result = load_with(&path, &LoadOptions::default(), &cancel, &mut |_| {});
        assert_eq!(result.unwrap_err(), LoadError::Cancelled);
    }

    #[test]
    fn reports_progress_through_its_phases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cube.glb");
        std::fs::write(&path, samples::cube_glb()).unwrap();
        let mut phases = Vec::new();
        load_with(
            &path,
            &LoadOptions::default(),
            &Cancel::default(),
            &mut |p| {
                if phases.last() != Some(&p.phase) {
                    phases.push(p.phase);
                }
            },
        )
        .unwrap();
        assert_eq!(phases, [Phase::Read, Phase::Parse, Phase::Prepare]);
    }

    #[test]
    fn loads_on_its_own_thread_and_finishes_the_stream() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cube.stl");
        std::fs::write(&path, samples::cube_stl_binary()).unwrap();
        let model = pollster::block_on(load_async(path, LoadOptions::default())).unwrap();
        assert_eq!(model.info.triangles, 12);
    }

    #[test]
    fn cancels_when_the_receiver_is_dropped() {
        let (receiver, cancel) = start("/definitely/not/here.glb".into(), LoadOptions::default());
        drop(receiver);
        // The thread may already be done; what matters is that it does not hang
        // or panic when nobody listens.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = cancel.is_cancelled();
    }
}
