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
use sha2::{Digest, Sha256};

use crate::host::{Event, Host};

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
    /// The commit the files are fetched at. `main` is whatever upstream pushed
    /// last, and what is downloaded here is parsed by native code: a pinned
    /// revision plus the hash below is what makes the file the one reviewed.
    revision: &'static str,
    /// Files that must be present for the model to load, in download order.
    files: &'static [ModelFile],
    /// Roughly what the download weighs, for the interface to show before it
    /// starts. The progress bar uses the files' exact `bytes`.
    approx_bytes: u64,
    languages: &'static str,
    license: &'static str,
}

/// One file of a model, as it is at `Catalogue::revision`.
struct ModelFile {
    name: &'static str,
    /// Lowercase hex SHA-256 of the whole file (HuggingFace's LFS `sha256`).
    sha256: &'static str,
    /// Exact size, checked by `installed` so a truncated file is not "ready".
    bytes: u64,
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
    revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
    files: &[ModelFile {
        name: "ggml-small-q5_1.bin",
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        bytes: 190_085_487,
    }],
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

/// A download, as it goes: `Event::VoiceDownload`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub done: bool,
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
pub fn model_dir(host: &Host, id: &str) -> Result<PathBuf, String> {
    let dir = host
        .paths()
        .app_data_dir()
        .map_err(|err| format!("no app data dir: {err}"))?;
    Ok(dir.join("models").join(id))
}

/// A model counts as installed only with every file present at its exact
/// size. A download interrupted half way, or a file cut short by a power loss,
/// would otherwise look ready and fail at load, which is a much worse moment to
/// find out. Cheap on purpose: it runs before every dictation. The hash is
/// checked by `download`, which is where a repair is asked for.
fn installed(host: &Host, model: &Catalogue) -> bool {
    let Ok(dir) = model_dir(host, model.id) else {
        return false;
    };
    model.files.iter().all(|file| {
        fs::metadata(dir.join(file.name))
            .is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
    })
}

/// Installed, and every file hashes to what the catalogue says.
fn verified(host: &Host, model: &Catalogue) -> bool {
    let Ok(dir) = model_dir(host, model.id) else {
        return false;
    };
    installed(host, model)
        && model
            .files
            .iter()
            .all(|file| hash_file(&dir.join(file.name)).is_ok_and(|hash| hash == file.sha256))
}

fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut reader = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Where a file of a model is fetched from: always the pinned revision.
fn url_of(model: &Catalogue, file: &ModelFile) -> String {
    format!(
        "{HOST}/{}/resolve/{}/{}",
        model.repo, model.revision, file.name
    )
}

/// Whether a catalogue id is ready to be loaded. Checked before the microphone
/// opens, so a missing download is reported to someone who has not spoken yet.
pub fn is_installed(host: &Host, id: &str) -> bool {
    entry(id).is_some_and(|model| installed(host, model))
}

/// The checkpoint itself. whisper.cpp loads one file, but it is kept inside the
/// model's own directory anyway: that is what the staged download renames into
/// place, and what makes a future model with several files no different.
pub fn model_file(host: &Host, id: &str) -> Result<PathBuf, String> {
    let model = entry(id).ok_or_else(|| format!("unknown model {id}"))?;
    let file = model
        .files
        .first()
        .map(|file| file.name)
        .ok_or_else(|| format!("model {id} lists no files"))?;
    // `model.id`, not `id`: the two differ when an old settings file named a
    // checkpoint this build no longer carries, and every path has to land on
    // the directory `installed` checks and `download` fills.
    Ok(model_dir(host, model.id)?.join(file))
}

pub fn voice_models(host: &Host) -> Vec<ModelInfo> {
    CATALOGUE
        .iter()
        .map(|model| ModelInfo {
            id: model.id.to_string(),
            label: model.label.to_string(),
            approx_bytes: model.approx_bytes,
            languages: model.languages.to_string(),
            license: model.license.to_string(),
            installed: installed(host, model),
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
fn sweep(host: &Host) {
    let Ok(dir) = host.paths().app_data_dir().map(|dir| dir.join("models")) else {
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

/// Downloads a model, reporting progress as `Event::VoiceDownload`.
///
/// A model already on disk is only kept when every file hashes right, so
/// "Download again" on a model that is there but damaged repairs it.
pub async fn voice_model_download(host: Host, id: String) -> Result<(), String> {
    voice_model_fetch(host, id, false).await
}

/// Downloads a model even when an intact copy is installed: the explicit
/// "Download again".
pub async fn voice_model_redownload(host: Host, id: String) -> Result<(), String> {
    voice_model_fetch(host, id, true).await
}

async fn voice_model_fetch(host: Host, id: String, force: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || download(&host, &id, force))
        .await
        .map_err(|err| format!("download thread failed: {err}"))?
}

fn download(host: &Host, id: &str, force: bool) -> Result<(), String> {
    let model = entry(id).ok_or_else(|| format!("unknown model {id}"))?;
    // See `model_file`: the resolved entry decides where this goes, so a stale
    // id cannot fill a directory nothing will ever read.
    let id = model.id;
    let target = model_dir(host, id)?;
    if !force && verified(host, model) {
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

    // The pinned files' sizes are known, so the bar means something from the
    // first byte without asking the server.
    let total: u64 = model
        .files
        .iter()
        .map(|file| file.bytes)
        .sum::<u64>()
        .max(1);

    let mut received = 0u64;
    let mut last_report = std::time::Instant::now();
    let _ = host.emit(Event::VoiceDownload(Progress {
        id: id.to_string(),
        received,
        total,
        done: false,
    }));

    for file in model.files {
        let name = file.name;
        let url = url_of(model, file);
        let mut response = agent
            .get(&url)
            .call()
            .map_err(|err| format!("could not download {name}: {err}"))?;
        let mut reader = response.body_mut().as_reader();
        let mut out = fs::File::create(staging.join(name))
            .map_err(|err| format!("cannot write {name}: {err}"))?;
        let mut hasher = Sha256::new();

        let mut buffer = vec![0u8; 128 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|err| format!("download of {name} failed: {err}"))?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])
                .map_err(|err| format!("cannot write {name}: {err}"))?;
            hasher.update(&buffer[..read]);
            received += read as u64;

            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = std::time::Instant::now();
                let _ = host.emit(Event::VoiceDownload(Progress {
                    id: id.to_string(),
                    received,
                    // A server that under-reported would otherwise show a
                    // bar past its own end.
                    total: total.max(received),
                    done: false,
                }));
            }
        }

        // Checked before anything is renamed into place: a file that is not
        // the reviewed one is never handed to whisper.cpp.
        let hash = hex(&hasher.finalize());
        if hash != file.sha256 {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!(
                "{name} does not match the expected checksum (got {hash}); the download was discarded"
            ));
        }
        // On disk before the rename, so a power loss right after cannot leave
        // an installed name in front of a file the kernel never wrote out.
        out.sync_all()
            .map_err(|err| format!("cannot write {name}: {err}"))?;
    }

    // The old directory only goes once the new one is complete: a re-download
    // over a working model must not be able to leave the machine with neither.
    let _ = fs::remove_dir_all(&target);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::rename(&staging, &target).map_err(|err| format!("cannot install the model: {err}"))?;
    sweep(host);

    let _ = host.emit(Event::VoiceDownload(Progress {
        id: id.to_string(),
        received,
        total: received.max(1),
        done: true,
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hangar-voice-models-{tag}-{}-{}",
            std::process::id(),
            // Digits only: a SystemTime's Debug form has characters Windows
            // refuses in a folder name.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const BODY: &[u8] = b"not really a whisper checkpoint";
    // `printf 'not really a whisper checkpoint' | sha256sum`
    const BODY_SHA: &str = "f0d4050461c2801083e8d43638eb01f261d563e5353cff16fdbaaa6f054cf3c1";

    fn fake(sha256: &'static str) -> Catalogue {
        Catalogue {
            id: "fake",
            label: "Fake",
            repo: "someone/somewhere",
            revision: "0123456789abcdef0123456789abcdef01234567",
            files: Box::leak(Box::new([ModelFile {
                name: "model.bin",
                sha256,
                bytes: BODY.len() as u64,
            }])),
            approx_bytes: 0,
            languages: "1",
            license: "MIT",
        }
    }

    fn host_with(tag: &str, contents: Option<&[u8]>) -> Host {
        let root = temp_dir(tag);
        let host = Host::with_paths(Paths::rooted(&root));
        if let Some(contents) = contents {
            let dir = model_dir(&host, "fake").unwrap();
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("model.bin"), contents).unwrap();
        }
        host
    }

    #[test]
    fn every_model_is_fetched_at_a_pinned_revision() {
        for model in CATALOGUE {
            assert_eq!(model.revision.len(), 40, "{} is not a commit", model.id);
            for file in model.files {
                let url = url_of(model, file);
                assert!(!url.contains("/resolve/main/"), "{url}");
                assert!(url.contains(model.revision), "{url}");
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(file.bytes > 0);
            }
        }
    }

    #[test]
    fn the_hash_of_a_file_is_its_sha256() {
        let dir = temp_dir("hash");
        let path = dir.join("f");
        fs::write(&path, BODY).unwrap();
        assert_eq!(hash_file(&path).unwrap(), BODY_SHA);
    }

    #[test]
    fn a_truncated_model_is_not_installed() {
        let model = fake(BODY_SHA);
        let host = host_with("truncated", Some(&BODY[..BODY.len() - 3]));
        assert!(!installed(&host, &model));
        assert!(!verified(&host, &model));
    }

    #[test]
    fn a_damaged_model_of_the_right_size_is_installed_but_not_verified() {
        let model = fake(BODY_SHA);
        let mut damaged = BODY.to_vec();
        damaged[0] ^= 0xff;
        let host = host_with("damaged", Some(&damaged));
        assert!(installed(&host, &model));
        // So `download` fetches it again instead of saying it is done.
        assert!(!verified(&host, &model));
    }

    #[test]
    fn an_intact_model_is_verified() {
        let model = fake(BODY_SHA);
        let host = host_with("intact", Some(BODY));
        assert!(installed(&host, &model));
        assert!(verified(&host, &model));
    }

    #[test]
    fn a_missing_model_is_neither() {
        let model = fake(BODY_SHA);
        let host = host_with("missing", None);
        assert!(!installed(&host, &model));
        assert!(!verified(&host, &model));
    }
}
