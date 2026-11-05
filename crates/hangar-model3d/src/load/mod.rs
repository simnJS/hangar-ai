//! Reading a model file: the whole of it into memory, then into a [`Model`],
//! on a thread of its own so the window never waits on it.
//!
//! The readers are pure Rust (`gltf`, `tobj`, `stl_io`, `ply-rs`). A model is
//! read whole before it is parsed, as the old viewer did, because the formats
//! are not streamable and the cap on a file's size (`MAX_MODEL_BYTES`) is what
//! bounds the memory one load can take.

mod fbx;
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

pub(crate) use geometry::{face_normal, flat_normals_in_place};
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
        Parser::Fbx => fbx::read(bytes, &ctx)?,
        Parser::Usd => {
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
    // Asked before opening: Windows refuses to open a folder at all ("Access
    // is denied"), which would hide the one thing worth saying about it.
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(LoadError::Read(format!(
            "{} is a folder: give the path of the model file inside it.",
            path.display()
        )));
    }
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
    let mut pace = Pace::new();
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
        let loaded = bytes.len() as u64;
        if pace.due(loaded, total) {
            progress(LoadProgress {
                phase: Phase::Read,
                loaded,
                total,
            });
        }
    }
    // The last word of the read is always said.
    if pace.said != bytes.len() as u64 {
        progress(LoadProgress {
            phase: Phase::Read,
            loaded: bytes.len() as u64,
            total,
        });
    }
    Ok(bytes)
}

/// How often a read reports its progress: each report makes the app look at
/// every open model again and build its view, so a 200 MB file must not send
/// 200 of them. One every [`Pace::STEP`] of the file or [`Pace::EVERY`],
/// whichever comes first.
struct Pace {
    /// The bytes last reported.
    said: u64,
    at: std::time::Instant,
}

impl Pace {
    /// A twentieth of the file.
    const STEP: u64 = 20;
    const EVERY: std::time::Duration = std::time::Duration::from_millis(100);

    fn new() -> Pace {
        Pace {
            said: 0,
            at: std::time::Instant::now(),
        }
    }

    /// Whether `loaded` bytes of `total` are worth a report, and if so,
    /// counted as reported.
    fn due(&mut self, loaded: u64, total: u64) -> bool {
        let step = (total / Self::STEP).max(1);
        let due = loaded >= total
            || loaded.saturating_sub(self.said) >= step
            || self.at.elapsed() >= Self::EVERY;
        if due {
            self.said = loaded;
            self.at = std::time::Instant::now();
        }
        due
    }
}

/// What a running load reports.
#[derive(Clone, Debug)]
pub enum LoadEvent {
    Progress(LoadProgress),
    /// The last event: the model, or why it could not be read.
    Done(Result<Arc<Model>, LoadError>),
}

/// Starts reading a model on a thread of its own. The events arrive on the
/// returned stream (`Task::run` takes it as it is); dropping it, or calling
/// [`Cancel::cancel`], stops the load at its next step.
pub fn start(path: PathBuf, options: LoadOptions) -> (mpsc::UnboundedReceiver<LoadEvent>, Cancel) {
    let format = ModelFormat::from_path(&path);
    start_with(format, move |cancel, report| {
        load_with(&path, &options, cancel, report)
    })
}

/// What a panic carried, as a sentence.
pub(crate) fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".into())
}

/// A reader that panicked, as the error the load ends with: the parsers are
/// third-party code, and a load that ends without a word would spin forever.
fn panicked(format: Option<ModelFormat>, panic: &(dyn std::any::Any + Send)) -> LoadError {
    let reason = format!(
        "the reader stopped on an internal error ({}).",
        panic_message(panic)
    );
    match format {
        Some(format) => LoadError::Parse { format, reason },
        None => LoadError::Read(format!("The file could not be read: {reason}")),
    }
}

/// [`start`], with the reading itself given: the load runs `read` on its
/// thread, and the stream always ends with [`LoadEvent::Done`], a panic of
/// `read` included.
pub(crate) fn start_with<F>(
    format: Option<ModelFormat>,
    read: F,
) -> (mpsc::UnboundedReceiver<LoadEvent>, Cancel)
where
    F: FnOnce(&Cancel, &mut dyn FnMut(LoadProgress)) -> Result<Model, LoadError> + Send + 'static,
{
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
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        read(&token, &mut report)
                    }))
                    .unwrap_or_else(|panic| Err(panicked(format, panic.as_ref())))
                    .map(Arc::new);
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
    #[test]
    fn a_reader_that_panics_ends_the_load_with_an_error() {
        let (events, _cancel) = start_with(Some(ModelFormat::Fbx), |_, report| {
            report(LoadProgress {
                phase: Phase::Parse,
                loaded: 0,
                total: 0,
            });
            panic!("index out of bounds in a third-party parser");
        });
        let events: Vec<LoadEvent> = pollster::block_on(events.collect());
        let Some(LoadEvent::Done(Err(error))) = events.last() else {
            panic!("the stream ends with Done: {events:?}");
        };
        assert!(matches!(error, LoadError::Parse { format: ModelFormat::Fbx, .. }), "{error}");
        let said = error.to_string();
        assert!(said.contains("index out of bounds"), "{said}");
        assert!(said.contains("internal error"), "{said}");
    }

    #[test]
    fn a_reader_that_panics_without_a_known_format_says_it_could_not_read() {
        let (events, _cancel) = start_with(None, |_, _| std::panic::panic_any(7_u8));
        let events: Vec<LoadEvent> = pollster::block_on(events.collect());
        assert!(matches!(
            events.last(),
            Some(LoadEvent::Done(Err(LoadError::Read(message)))) if message.contains("no message")
        ));
    }

    #[test]
    fn a_large_read_reports_its_progress_a_bounded_number_of_times() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        // 64 chunks of a MiB: at most one report a twentieth, plus the first
        // and the last (the clock may add some on a slow machine).
        std::fs::write(&path, vec![0u8; 64 << 20]).unwrap();
        let mut reports = Vec::new();
        let bytes = read_whole(&path, &Cancel::default(), &mut |p| reports.push(p.loaded)).unwrap();
        assert_eq!(bytes.len(), 64 << 20);
        assert_eq!(reports.first(), Some(&0));
        assert_eq!(reports.last(), Some(&(64 << 20)));
        assert!(reports.len() < 64, "{} reports", reports.len());
        assert!(reports.windows(2).all(|pair| pair[0] < pair[1]), "{reports:?}");
    }

    #[test]
    fn the_pace_reports_each_step_and_the_end() {
        let mut pace = Pace::new();
        assert!(!pace.due(1, 100));
        assert!(pace.due(5, 100));
        assert!(!pace.due(6, 100));
        assert!(pace.due(10, 100));
        assert!(pace.due(100, 100));
    }
}
