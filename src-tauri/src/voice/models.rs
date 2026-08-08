//! The local model catalogue, and getting one onto the machine.
//!
//! Checkpoints are downloaded rather than shipped. Half a gigabyte in the
//! installer would be paid by every user on every update, including the ones
//! who never dictate — and the app updates itself, so that bill would come
//! round every release.
//!
//! One model is offered, not seven. Parakeet v3 is both the fastest and the
//! most accurate open checkpoint that covers French, and a list of Whisper
//! sizes would only ask the user to answer a question they have no way to
//! judge: on any machine from the last decade, `tiny` is never the right
//! answer, and `large` has no advantage worth its cost here.

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
/// Int8 rather than fp32: it is a quarter of the download, loads in a quarter
/// of the memory, and the accuracy difference on dictation-length speech does
/// not survive contact with a real microphone.
const CATALOGUE: &[Catalogue] = &[Catalogue {
    id: "parakeet-tdt-0.6b-v3",
    label: "Parakeet TDT 0.6B v3",
    repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
    files: &[
        "encoder-model.int8.onnx",
        "decoder_joint-model.int8.onnx",
        "nemo128.onnx",
        "vocab.txt",
    ],
    approx_bytes: 660 * 1024 * 1024,
    languages: "en fr de es it pt nl pl ru uk sv da fi el cs sk sl hr bg hu ro lt lv et mt",
    license: "CC-BY-4.0",
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

fn entry(id: &str) -> Option<&'static Catalogue> {
    CATALOGUE.iter().find(|m| m.id == id)
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

/// Downloads a model, reporting progress on `voice:download`.
#[tauri::command]
pub async fn voice_model_download(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || download(&app, &id))
        .await
        .map_err(|err| format!("download thread failed: {err}"))?
}

fn download(app: &AppHandle, id: &str) -> Result<(), String> {
    let model = entry(id).ok_or_else(|| format!("unknown model {id}"))?;
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
