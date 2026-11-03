//! The local model catalogue, and getting one onto the machine.
//!
//! Checkpoints are downloaded rather than shipped. Half a gigabyte in the
//! installer would be paid by every user on every update, including the ones
//! who never dictate — and the app updates itself, so that bill would come
//! round every release.
//!
//! One model is offered, not seven: a list of Whisper sizes would only ask the
//! user to answer a question they have no way to judge.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Where a model's files are expected, once installed.
const HOST: &str = "https://huggingface.co";

/// Long enough for half a gigabyte on a slow connection.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1800);

/// How often download progress reaches the interface. Every chunk would be
/// thousands of events for a bar that moves in pixels.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

struct Catalogue {
    id: &'static str,
    label: &'static str,
    repo: &'static str,
    /// Files that must be present for the model to load, in download order.
    files: &'static [&'static str],
    /// Roughly what the download weighs, for the interface to show before it
    /// starts. The exact figure is read from the server once it does.
    approx_bytes: u64,
    languages: &'static str,
    license: &'static str,
}

/// The one local model, and everything needed to fetch it.
///
/// `small`, and the reasoning that first picked large-v3-turbo was wrong twice.
///
/// On speed: turbo prunes the *decoder* from thirty-two layers to four, and on
/// a CPU the decoder is not what costs anything — the encoder is, and turbo's
/// is large-v3's, untouched. Measured on eleven seconds of speech and sixteen
/// cores: turbo 89s, small 17s. The distilled French checkpoints have the same
/// shape and so the same price.
///
/// On accuracy: a dictation is a few seconds long, and the large models are the
/// ones that handle that badly. Trained on thirty-second segments, they fill
/// short context with sentences nobody said. For clips this length small is not
/// the compromise, it is the better transcript.
///
/// Quantised to q5_1, which loses under a point of word error rate and takes
/// the download from 465 MB to 181.
const CATALOGUE: &[Catalogue] = &[Catalogue {
    id: "whisper-small-q5_1",
    label: "Whisper small",
    repo: "ggerganov/whisper.cpp",
    files: &["ggml-small-q5_1.bin"],
    approx_bytes: 181 * 1024 * 1024,
    languages: "99",
    license: "MIT",
}];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub approx_bytes: u64,
    pub languages: String,
    pub license: String,
    pub installed: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    id: String,
    received: u64,
    total: u64,
    done: bool,
}

/// The catalogue entry an id names.
///
/// An id this build has never heard of resolves to the one model there is,
/// which is how a settings file written by an earlier release keeps working:
/// it still names the checkpoint that release downloaded, and nothing would
/// ever rewrite it — the frontend only stores what it is given. Falling back
/// here means such a machine is told "not downloaded yet" about the model it
/// actually needs, rather than "unknown model" about one it no longer wants.
fn entry(id: &str) -> Option<&'static Catalogue> {
    CATALOGUE
        .iter()
        .find(|m| m.id == id)
        .or_else(|| CATALOGUE.first())
}

/// Where a model lives once installed.
pub fn model_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("no app data dir: {err}"))?;
    Ok(dir.join("models").join(id))
}

/// A model counts as installed only with every file present. A download
/// interrupted half way leaves a directory that would otherwise look ready and
/// fail at load, which is a much worse moment to find out.
fn installed(app: &AppHandle, model: &Catalogue) -> bool {
    let Ok(dir) = model_dir(app, model.id) else {
        return false;
    };
    model.files.iter().all(|file| dir.join(file).is_file())
}

/// Whether a catalogue id is ready to be loaded. Checked before the microphone
/// opens, so a missing download is reported to someone who has not spoken yet.
pub fn is_installed(app: &AppHandle, id: &str) -> bool {
    entry(id).is_some_and(|model| installed(app, model))
}

/// The checkpoint itself. whisper.cpp loads one file, but it is kept inside the
/// model's own directory anyway: that is what the staged download renames into
/// place, and what makes a future model with several files no different.
pub fn model_file(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let model = entry(id).ok_or_else(|| format!("unknown model {id}"))?;
    let file = model
        .files
        .first()
        .ok_or_else(|| format!("model {id} lists no files"))?;
    // `model.id`, not `id`: the two differ when an old settings file named a
    // checkpoint this build no longer carries, and every path has to land on
    // the directory `installed` checks and `download` fills.
    Ok(model_dir(app, model.id)?.join(file))
}

#[tauri::command]
pub fn voice_models(app: AppHandle) -> Vec<ModelInfo> {
    CATALOGUE
        .iter()
        .map(|model| ModelInfo {
            id: model.id.to_string(),
            label: model.label.to_string(),
            approx_bytes: model.approx_bytes,
            languages: model.languages.to_string(),
            license: model.license.to_string(),
            installed: installed(&app, model),
        })
        .collect()
}

/// Deletes every model directory the catalogue no longer lists.
///
/// A checkpoint is half a gigabyte and nothing else would ever remove it: the
/// releases that shipped Parakeet and large-v3-turbo left theirs behind, which
/// on this machine is 1.2 GB of models no build can load any more. Run after a
/// successful download, which is the moment the replacement is on disk and the
/// old one is provably not needed.
///
/// Failures are ignored on purpose. This is housekeeping — a file held open by
/// something else is worth nothing to report and not worth failing a download
/// that has already succeeded.
fn sweep(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir().map(|dir| dir.join("models")) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if CATALOGUE.iter().any(|model| model.id == name) {
            continue;
        }
        if entry.path().is_dir() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Downloads a model, reporting progress on `voice:download`.
#[tauri::command]
pub async fn voice_model_download(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || download(&app, &id))
        .await
        .map_err(|err| format!("download thread failed: {err}"))?
}

fn download(app: &AppHandle, id: &str) -> Result<(), String> {
    let model = entry(id).ok_or_else(|| format!("unknown model {id}"))?;
    // See `model_file`: the resolved entry decides where this goes, so a stale
    // id cannot fill a directory nothing will ever read.
    let id = model.id;
    let target = model_dir(app, id)?;
    if installed(app, model) {
        return Ok(());
    }

    // Staged next to the real directory and renamed at the end, so an
    // interrupted download — a closed laptop, a dropped connection — never
    // leaves something that looks installed.
    let staging = target.with_extension("part");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|err| format!("cannot create {staging:?}: {err}"))?;

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .new_agent();

    // Sized up front so the bar means something from the first byte. The
    // catalogue figure is only a fallback for a server that declines to say.
    let mut total = 0u64;
    for file in model.files {
        let url = format!("{HOST}/{}/resolve/main/{file}", model.repo);
        total += agent
            .head(&url)
            .call()
            .ok()
            .and_then(|response| {
                response
                    .headers()
                    .get("content-length")?
                    .to_str()
                    .ok()?
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0);
    }
    if total == 0 {
        total = model.approx_bytes;
    }

    let mut received = 0u64;
    let mut last_report = std::time::Instant::now();
    let _ = app.emit(
        "voice:download",
        Progress {
            id: id.to_string(),
            received,
            total,
            done: false,
        },
    );

    for file in model.files {
        let url = format!("{HOST}/{}/resolve/main/{file}", model.repo);
        let mut response = agent
            .get(&url)
            .call()
            .map_err(|err| format!("could not download {file}: {err}"))?;
        let mut reader = response.body_mut().as_reader();
        let mut out = fs::File::create(staging.join(file))
            .map_err(|err| format!("cannot write {file}: {err}"))?;

        let mut buffer = vec![0u8; 128 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|err| format!("download of {file} failed: {err}"))?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])
                .map_err(|err| format!("cannot write {file}: {err}"))?;
            received += read as u64;

            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = std::time::Instant::now();
                let _ = app.emit(
                    "voice:download",
                    Progress {
                        id: id.to_string(),
                        received,
                        // A server that under-reported would otherwise show a
                        // bar past its own end.
                        total: total.max(received),
                        done: false,
                    },
                );
            }
        }
    }

    // The old directory only goes once the new one is complete: a re-download
    // over a working model must not be able to leave the machine with neither.
    let _ = fs::remove_dir_all(&target);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::rename(&staging, &target).map_err(|err| format!("cannot install the model: {err}"))?;
    sweep(app);

    let _ = app.emit(
        "voice:download",
        Progress {
            id: id.to_string(),
            received,
            total: received.max(1),
            done: true,
        },
    );
    Ok(())
}
