//! The preview operations: `model_show`, `model_snapshot`, `model_close`,
//! `preview_show`, `preview_close` (control.ts, "the preview"), and the
//! folders core may serve the preview's files from.
//!
//! Every one acts on the caller's own workspace's preview, never the one on
//! screen nor another workspace's, and says straight whether the person can
//! see what was shown: an agent saying "look at the preview" would otherwise
//! send them looking for nothing.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::state::Workspace;

use super::panes::{announce, tell};
use super::paths::{basename, comparable, inside, resolve_path};
use super::port::{
    EntryStatus, ModelCmd, ModelFileInfo, OpenEntry, Port, PreviewFile, PreviewHost, Showing,
    SnapshotAngle, is_asleep,
};
use super::run::{effective_cwd, number_arg, on_screen};
use super::types::{Caller, ControlEvent, same, text};

/// How long `model_show` waits for the viewer before answering "loading".
/// Core gives a model_* call 60 s in all.
pub const SETTLE_MS: u64 = 45_000;
/// Shorter for a snapshot, which still has to render once the load is done.
pub const SNAPSHOT_SETTLE_MS: u64 = 40_000;

const ANGLES: [(&str, SnapshotAngle); 7] = [
    ("front", SnapshotAngle::Front),
    ("back", SnapshotAngle::Back),
    ("left", SnapshotAngle::Left),
    ("right", SnapshotAngle::Right),
    ("top", SnapshotAngle::Top),
    ("bottom", SnapshotAngle::Bottom),
    ("iso", SnapshotAngle::Iso),
];
const SNAPSHOT_WIDTH: u32 = 1024;
const SNAPSHOT_HEIGHT: u32 = 768;
const SNAPSHOT_MIN: f64 = 64.0;
const SNAPSHOT_MAX: f64 = 2048.0;

/// The 3D formats the viewer opens (`MODEL_FORMATS`).
pub const MODEL_FORMATS: [&str; 7] = ["glb", "gltf", "obj", "stl", "ply", "fbx", "usdz"];
/// Largest model the viewer accepts, textures not included.
pub const MAX_MODEL_BYTES: u64 = 256 * 1024 * 1024;
/// What the preview shows (`PREVIEW_KINDS`).
pub const PREVIEW_KINDS: [&str; 8] = [
    "model", "image", "video", "audio", "text", "markdown", "pdf", "html",
];

/// The format a file name says, or `None` for anything the viewer cannot
/// open (`modelFormatOf`).
pub fn model_format_of(path: &str) -> Option<&'static str> {
    let (_, extension) = path.rsplit_once('.')?;
    let extension = extension.to_lowercase();
    MODEL_FORMATS
        .iter()
        .find(|format| **format == extension)
        .copied()
}

fn formats() -> String {
    MODEL_FORMATS.join(", ")
}

/// The folders an agent may show files from: its workspace's main folder,
/// extra roots and pane folders, plus every worktree of their repositories
/// (`modelRoots`). An agent working in a worktree must not be refused its own
/// files; a folder that is not a repository just adds none.
pub async fn model_roots(ws: &Workspace, port: &Port) -> Vec<String> {
    let repos: Vec<String> = std::iter::once(ws.cwd.clone())
        .chain(ws.extra_roots.iter().cloned())
        .collect();
    let panes = ws
        .panes
        .iter()
        .filter_map(|pane| pane.cwd.as_deref().filter(|cwd| !cwd.is_empty()))
        .map(|cwd| resolve_path(&ws.cwd, cwd));
    let mut trees = Vec::new();
    for repo in &repos {
        let listed = port.worktrees(repo).await.unwrap_or_default();
        trees.extend(
            listed
                .into_iter()
                .filter(|tree| !tree.bare && tree.prunable.is_none())
                .map(|tree| tree.path),
        );
    }
    let mut seen = Vec::new();
    let mut roots = Vec::new();
    for root in repos.iter().cloned().chain(panes).chain(trees) {
        let key = comparable(&root);
        if root.trim().is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        roots.push(root);
    }
    roots
}

/// The folders granted to each workspace that showed a file, sent to core
/// as one list (`createRootGrants`): the viewer keeps fetching a model's
/// textures, and polls its file, long after the call that showed it, so the
/// latest caller's folders must not replace every other workspace's.
#[derive(Debug, Default)]
pub struct RootGrants {
    granted: Vec<(String, Vec<String>)>,
}

impl RootGrants {
    /// Grants `roots` to `ws`, forgets the workspaces not in `live`, and
    /// answers what core is to allow: every granted folder, once.
    pub fn grant(&mut self, ws: &str, roots: Vec<String>, live: &[String]) -> Vec<String> {
        self.granted.retain(|(id, _)| live.iter().any(|kept| kept == id));
        match self.granted.iter_mut().find(|(id, _)| id == ws) {
            Some((_, held)) => *held = roots,
            None => self.granted.push((ws.to_string(), roots)),
        }
        let mut seen = Vec::new();
        let mut union = Vec::new();
        for root in self.granted.iter().flat_map(|(_, roots)| roots) {
            let key = root
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            union.push(root.clone());
        }
        union
    }
}

/// Where a relative path is read from: the caller's pane folder (where
/// Hangar launched the agent, a worktree for instance), rather than the
/// folder its MCP server reports, which a project-wide config pins to the
/// workspace root (`callerFolder`).
pub fn caller_folder(ws: &Workspace, caller: &Caller) -> String {
    let pane = caller
        .pane_id
        .as_deref()
        .and_then(|id| ws.panes.iter().find(|pane| pane.id == id));
    match pane {
        Some(pane) => effective_cwd(ws, pane),
        None => caller.cwd.clone().unwrap_or_else(|| ws.cwd.clone()),
    }
}

/// The workspace's own preview, drawn by its window of its own when it has
/// one (`previewOf`).
pub async fn preview_of(ws: &Workspace, port: &Port) -> Result<Arc<dyn PreviewHost>, String> {
    port.preview(&ws.id, ws.detached == Some(true))
        .await
        .ok_or_else(|| {
            format!(
                "\"{}\" is shown in a window of its own, and that window is not answering, so the \
                 preview cannot open there. Try again in a few seconds.",
                ws.name
            )
        })
}

/// `preview_inspect`'s answer, checked: a kind this window has no viewer for
/// is refused here rather than drawn as an empty tab (`checkPreviewFile`).
pub fn check_preview_file(answer: &Value) -> Result<PreviewFile, String> {
    let path = answer.get("path").and_then(Value::as_str).unwrap_or("");
    let name = match answer.get("name").and_then(Value::as_str) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => path
            .rsplit(['\\', '/'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(path)
            .to_string(),
    };
    if path.is_empty() {
        return Err("The preview got no path back for that file. Try again.".into());
    }
    let kind = answer.get("kind").and_then(Value::as_str);
    let Some(kind) = kind.filter(|kind| PREVIEW_KINDS.contains(kind)) else {
        let said = kind.map(|kind| format!(" ({kind})")).unwrap_or_default();
        return Err(format!(
            "The preview has no viewer for {name}{said}. It shows: {}.",
            PREVIEW_KINDS.join(", ")
        ));
    };
    let count = |field: &str| {
        answer
            .get(field)
            .and_then(Value::as_f64)
            .filter(|value| *value >= 0.0)
            .map_or(0, |value| value as u64)
    };
    Ok(PreviewFile {
        path: path.to_string(),
        kind: kind.to_string(),
        mime: answer
            .get("mime")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        size: count("size"),
        name,
        modified_ms: count("modifiedMs"),
    })
}

async fn inspect_file(port: &Port, path: &str) -> Result<PreviewFile, String> {
    check_preview_file(&port.preview_inspect(path).await?)
}

/// What a canonical path is had from: `model_stat` or `preview_inspect`.
#[derive(Clone, Copy)]
enum Canon {
    Model,
    File,
}

impl Canon {
    async fn ask(self, port: &Port, path: &str) -> Result<String, String> {
        match self {
            Canon::Model => port.model_stat(path).await.map(|file| file.path),
            Canon::File => inspect_file(port, path).await.map(|file| file.path),
        }
    }
}

/// The form a path was stored in when it was shown: core's canonical one,
/// whatever casing, link or Git Bash spelling (`/c/…`) the agent used this
/// time; the plain resolution when the file is gone (`canonicalPath`).
async fn canonical_path(port: &Port, base: &str, given: &str, canon: Canon) -> String {
    let resolved = resolve_path(base, given);
    canon.ask(port, &resolved).await.unwrap_or(resolved)
}

/// An open entry by path (already canonical), or by its file name alone when
/// only one open entry has it (`findOpen`). `none` is the word for an empty
/// preview.
fn find_open<'a>(
    open: &[&'a OpenEntry],
    given: &str,
    resolved: &str,
    none: String,
) -> Result<&'a OpenEntry, String> {
    let target = comparable(resolved);
    if let Some(exact) = open.iter().find(|entry| comparable(&entry.path) == target) {
        return Ok(exact);
    }
    // The same file reached through a link, or named without its folder.
    let name = basename(given).to_lowercase();
    let named: Vec<&&OpenEntry> = open
        .iter()
        .filter(|entry| basename(&entry.path).to_lowercase() == name || same(&entry.title, given))
        .collect();
    if let [only] = named.as_slice() {
        return Ok(only);
    }
    if open.is_empty() {
        return Err(none);
    }
    let what = if open.iter().all(|entry| entry.is_model()) {
        "model"
    } else {
        "file"
    };
    let paths = open
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "No open {what} matches \"{given}\". Open: {paths}."
    ))
}

fn find_model<'a>(
    models: &[&'a OpenEntry],
    given: &str,
    resolved: &str,
) -> Result<&'a OpenEntry, String> {
    find_open(
        models,
        given,
        resolved,
        format!("No 3D model is open, so none matches \"{given}\". Show it with model_show first."),
    )
}

/// A path that names what it answers for.
trait Pathed {
    fn path(&self) -> &str;
}

impl Pathed for ModelFileInfo {
    fn path(&self) -> &str {
        &self.path
    }
}

impl Pathed for PreviewFile {
    fn path(&self) -> &str {
        &self.path
    }
}

/// Lets core serve the workspace's folders, then asks it about `path`: what
/// it found, or the refusal worth passing on (`admit`).
async fn admit<T: Pathed>(
    ws: &Workspace,
    path: &str,
    what: &str,
    port: &Port,
    ask: impl AsyncFnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    let roots = model_roots(ws, port).await;
    let outside = || {
        format!(
            "{path} is outside your workspace. The preview only opens {what} from {} (and their \
             subfolders). Move or copy the file into one of them, then show it from there.",
            roots.join(", ")
        )
    };
    port.model_allow_roots(&ws.id, roots.clone()).await?;
    // Core is the real gate, on the canonical path. The folders it allows are
    // every workspace's that showed a file, though, and this one is checked
    // against the caller's own: the path as given, or where its links lead.
    let given_inside = inside(path, &roots);
    let file = match ask(path).await {
        Ok(file) => file,
        // Refused for being out of bounds, or missing out of bounds: either
        // way, the workspace is the thing worth naming.
        Err(_) if !given_inside => return Err(outside()),
        Err(error) => return Err(error),
    };
    if !given_inside && !inside(file.path(), &roots) {
        return Err(outside());
    }
    Ok(file)
}

/// Whatever showing something in a workspace's preview does to it: it is
/// using it, as sending to its panes is, and an asleep one wakes as
/// `pane_send` wakes it (`wakeFor`). Says whether it woke.
pub(super) async fn wake_for(ws: &Workspace, port: &Port) -> bool {
    let woke = is_asleep(&port.snapshot(), &ws.id);
    if woke {
        tell(port, ModelCmd::OpenWorkspace(ws.id.clone())).await;
    }
    tell(port, ModelCmd::Touch(ws.id.clone())).await;
    woke
}

/// Whether the person can see what was just shown, told to the agent
/// straight (`sight`): `workspace`, `visible`, and `woke` and `note` when
/// they apply.
pub(super) async fn sight(
    ws: &Workspace,
    preview: &Arc<dyn PreviewHost>,
    woke: bool,
) -> (Map<String, Value>, Option<String>) {
    let listing = preview.list().await.ok();
    let visible = listing.as_ref().is_some_and(|listing| listing.shown);
    let mut notes = Vec::new();
    if woke {
        notes.push(format!(
            "\"{}\" was asleep: it is waking up now, its agents resuming their sessions.",
            ws.name
        ));
    }
    if !visible {
        notes.push(if listing.as_ref().is_some_and(|listing| listing.on_screen) {
            format!(
                "\"{}\" is on screen, but its preview is not: another view is open, or there is no \
                 room beside the terminals. The preview's toolbar button tells the user there is \
                 something to see.",
                ws.name
            )
        } else {
            format!(
                "It is open in the preview of \"{}\", which is not the workspace on screen: the user \
                 sees it when they go to that workspace. Tell them it is there if they should look now.",
                ws.name
            )
        });
    }
    let mut seen = Map::new();
    seen.insert("workspace".into(), json!(ws.name));
    seen.insert("visible".into(), json!(visible));
    if woke {
        seen.insert("woke".into(), json!(true));
    }
    let note = (!notes.is_empty()).then(|| notes.join(" "));
    if let Some(note) = &note {
        seen.insert("note".into(), json!(note));
    }
    (seen, note)
}

/// `ModelShown` in the workspace on screen, `ModelShownElsewhere` in another.
pub(super) fn shown(port: &Port, ws: &Workspace, by: Option<String>, file: String) -> ControlEvent {
    let workspace = ws.name.clone();
    if on_screen(&port.snapshot().state, ws) {
        ControlEvent::ModelShown {
            by,
            file,
            workspace,
        }
    } else {
        ControlEvent::ModelShownElsewhere {
            by,
            file,
            workspace,
        }
    }
}

pub(super) fn closed(port: &Port, ws: &Workspace, by: Option<String>, file: String) -> ControlEvent {
    let workspace = ws.name.clone();
    if on_screen(&port.snapshot().state, ws) {
        ControlEvent::ModelClosed {
            by,
            file,
            workspace,
        }
    } else {
        ControlEvent::ModelClosedElsewhere {
            by,
            file,
            workspace,
        }
    }
}

pub async fn model_show(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let Some(given) = text(args.get("path")) else {
        return Err(
            "Give the path of the model to show, absolute or relative to your folder.".into(),
        );
    };
    let path = resolve_path(&caller_folder(ws, caller), &given);
    if model_format_of(&path).is_none() {
        return Err(format!(
            "\"{given}\" is not a 3D model the preview can open. Formats: {}.",
            formats()
        ));
    }
    let file = admit(ws, &path, "models", port, async |path: &str| {
        port.model_stat(path).await
    })
    .await?;
    open_model(ws, file, &path, args, caller, port)
        .await
        .map(Value::Object)
}

/// A model core let through, opened in the workspace's preview and waited for
/// (`openModel`).
async fn open_model(
    ws: &Workspace,
    file: ModelFileInfo,
    path: &str,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Map<String, Value>, String> {
    let title = text(args.get("title")).unwrap_or_else(|| basename(&file.path));
    if file.size > MAX_MODEL_BYTES {
        let mb = |bytes: u64| (bytes as f64 / (1024.0 * 1024.0)).round() as u64;
        return Err(format!(
            "{title} is {} MB, and the preview opens models up to {} MB. Export a lighter version \
             (decimated, or compressed with Draco or meshopt) and show that.",
            mb(file.size),
            mb(MAX_MODEL_BYTES)
        ));
    }
    let Some(format) = model_format_of(&file.path).or_else(|| model_format_of(path)) else {
        return Err(format!(
            "{} is not a 3D model the preview can open. Formats: {}.",
            basename(path),
            formats()
        ));
    };

    let preview = preview_of(ws, port).await?;
    let woke = wake_for(ws, port).await;
    let id = preview
        .show(Showing::Model {
            path: file.path.clone(),
            title: title.clone(),
            format: format.to_string(),
            opened_by: caller.pane_name.clone(),
            modified_ms: file.modified_ms,
        })
        .await?;
    announce(port, shown(port, ws, caller.pane_name.clone(), title.clone())).await;

    let settled = preview.when_settled(&id, SETTLE_MS).await;
    if let Some(entry) = &settled
        && entry.status == EntryStatus::Error
    {
        return Err(format!(
            "The 3D preview could not load {title}: {}",
            entry.error.as_deref().unwrap_or("unknown error")
        ));
    }
    // Asked once the load is over, when the window has drawn what it opened.
    let (seen, note) = sight(ws, &preview, woke).await;
    let mut answer = Map::new();
    answer.insert("opened".into(), json!(true));
    match settled {
        None => {
            answer.insert("status".into(), json!("loading"));
            answer.insert("path".into(), json!(file.path));
            answer.insert("format".into(), json!(format));
            answer.insert("title".into(), json!(title));
            answer.extend(seen);
            let loading = format!(
                "The preview is still loading it after {} s; a big file takes a while. \
                 model_snapshot waits for the load, so call it when you want to look.",
                SETTLE_MS / 1000
            );
            let note = match note {
                Some(note) => format!("{loading} {note}"),
                None => loading,
            };
            answer.insert("note".into(), json!(note));
        }
        Some(entry) => {
            answer.insert("path".into(), json!(file.path));
            answer.insert("format".into(), json!(format));
            answer.insert("title".into(), json!(entry.title));
            answer.insert("info".into(), entry.info().cloned().unwrap_or(Value::Null));
            answer.extend(seen);
        }
    }
    Ok(answer)
}

/// The size of a snapshot side: a number or a numeric string, clamped and
/// rounded; anything else is the default (`sizeArg`).
fn size_arg(value: Option<&Value>, fallback: u32) -> u32 {
    match number_arg(value) {
        Some(wanted) => wanted.round().clamp(SNAPSHOT_MIN, SNAPSHOT_MAX) as u32,
        None => fallback,
    }
}

fn angle_arg(value: Option<&Value>) -> Result<(&'static str, SnapshotAngle), String> {
    let Some(wanted) = text(value).map(|wanted| wanted.to_lowercase()) else {
        return Ok(("iso", SnapshotAngle::Iso));
    };
    ANGLES
        .iter()
        .find(|(name, _)| *name == wanted)
        .copied()
        .ok_or_else(|| {
            let names = ANGLES.iter().map(|(name, _)| *name).collect::<Vec<_>>();
            format!("Unknown angle \"{wanted}\". One of: {}.", names.join(", "))
        })
}

pub async fn model_snapshot(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let (angle_name, angle) = angle_arg(args.get("angle"))?;
    let width = size_arg(args.get("width"), SNAPSHOT_WIDTH);
    let height = size_arg(args.get("height"), SNAPSHOT_HEIGHT);
    let preview = preview_of(ws, port).await?;
    let listed = preview.list().await?;
    // The preview's other tabs are not the 3D viewer's to render.
    let models: Vec<&OpenEntry> = listed.entries.iter().filter(|entry| entry.is_model()).collect();
    if models.is_empty() {
        return Err(format!(
            "No 3D model is open in \"{}\". Show one with model_show, then take the snapshot.",
            ws.name
        ));
    }
    let model = match text(args.get("path")) {
        Some(given) => {
            let canonical =
                canonical_path(port, &caller_folder(ws, caller), &given, Canon::Model).await;
            find_model(&models, &given, &canonical)?
        }
        None => models
            .iter()
            .find(|entry| listed.active_id.as_deref() == Some(entry.id.as_str()))
            .or(models.last())
            .copied()
            .ok_or("no model")?,
    };

    // In front in its workspace's preview, so the person sees what the agent
    // is looking at: there, without the preview coming over another workspace.
    preview.reveal(&model.id).await;
    // An agent iterating on a file snapshots right after writing it, before
    // the panel's own polling has noticed: load the new version first.
    let fresh = port.model_stat(&model.path).await.ok();
    let rewritten = fresh
        .as_ref()
        .is_some_and(|fresh| fresh.modified_ms > model.modified_ms);
    if let Some(fresh) = &fresh
        && rewritten
    {
        preview.reload(&model.id, fresh.modified_ms).await;
    }
    let ready = if rewritten || model.status == EntryStatus::Loading {
        preview.when_settled(&model.id, SNAPSHOT_SETTLE_MS).await
    } else {
        Some(model.clone())
    };
    let Some(ready) = ready else {
        return Err(format!(
            "{} is still loading after {} s. Try model_snapshot again in a moment.",
            model.title,
            SNAPSHOT_SETTLE_MS / 1000
        ));
    };
    if ready.status == EntryStatus::Error {
        return Err(format!(
            "{} failed to load: {}. Fix the file, then call model_show again.",
            model.title,
            ready.error.as_deref().unwrap_or("unknown error")
        ));
    }

    let png = match preview.snapshot(&model.id, angle, width, height).await {
        Ok(png) => png,
        Err(message) if message.to_lowercase().contains("not mounted") => {
            return Err(format!(
                "The 3D preview did not come up in the window that shows \"{}\", so {} cannot be \
                 rendered. Try model_snapshot again in a moment.",
                ws.name, model.title
            ));
        }
        Err(message) => return Err(format!("Could not render {}: {message}", model.title)),
    };
    let info = if ready.is_model() {
        ready.info().cloned()
    } else {
        model.info().cloned()
    };
    Ok(json!({
        "png": base64(&png),
        "mimeType": "image/png",
        "path": model.path,
        "angle": angle_name,
        "width": width,
        "height": height,
        "info": info,
    }))
}

pub async fn model_close(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let preview = preview_of(ws, port).await?;
    // Its models only: the preview's other tabs are preview_close's.
    let listed = preview.list().await?;
    let models: Vec<&OpenEntry> = listed.entries.iter().filter(|entry| entry.is_model()).collect();
    // The caller's own workspace's preview only: without a path, every model
    // in it.
    let closing: Vec<&OpenEntry> = match text(args.get("path")) {
        Some(given) => {
            let canonical =
                canonical_path(port, &caller_folder(ws, caller), &given, Canon::Model).await;
            vec![find_model(&models, &given, &canonical)?]
        }
        None => models.clone(),
    };
    close(ws, caller, port, &preview, models.len(), closing).await
}

/// Closes `closing` and says so; `open` is how many there were.
async fn close(
    ws: &Workspace,
    caller: &Caller,
    port: &Port,
    preview: &Arc<dyn PreviewHost>,
    open: usize,
    closing: Vec<&OpenEntry>,
) -> Result<Value, String> {
    if closing.is_empty() {
        return Ok(json!({ "closed": [], "open": open }));
    }
    preview
        .close(closing.iter().map(|entry| entry.id.clone()).collect())
        .await;
    let titles = closing
        .iter()
        .map(|entry| entry.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    announce(port, closed(port, ws, caller.pane_name.clone(), titles)).await;
    Ok(json!({
        "closed": closing.iter().map(|entry| entry.path.clone()).collect::<Vec<_>>(),
        "open": open - closing.len(),
    }))
}

/// Any file in the workspace's preview, one tab per path, as model_show
/// opens a model: core says what the file is, and a model goes through
/// model_show's own load, the rest straight to its viewer (`previewShow`).
pub async fn preview_show(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let Some(given) = text(args.get("path")) else {
        return Err(
            "Give the path of the file to show, absolute or relative to your folder.".into(),
        );
    };
    let path = resolve_path(&caller_folder(ws, caller), &given);
    let file = admit(ws, &path, "files", port, async |path: &str| {
        inspect_file(port, path).await
    })
    .await?;
    let mut answer = Map::new();
    answer.insert("kind".into(), json!(file.kind));
    answer.insert("name".into(), json!(file.name));
    answer.insert("size".into(), json!(file.size));
    if file.kind == "model" {
        // Core judged it with model_show's own rules: what it found is a
        // model's stat, and nothing is asked twice.
        let stat = ModelFileInfo {
            path: file.path.clone(),
            size: file.size,
            modified_ms: file.modified_ms,
        };
        answer.extend(open_model(ws, stat, &file.path, args, caller, port).await?);
        return Ok(Value::Object(answer));
    }

    let title = text(args.get("title")).unwrap_or_else(|| file.name.clone());
    let preview = preview_of(ws, port).await?;
    let woke = wake_for(ws, port).await;
    preview
        .show(Showing::File {
            kind: file.kind.clone(),
            path: file.path.clone(),
            title: title.clone(),
            mime: file.mime.clone(),
            size: file.size,
            opened_by: caller.pane_name.clone(),
            modified_ms: file.modified_ms,
        })
        .await?;
    announce(port, shown(port, ws, caller.pane_name.clone(), title.clone())).await;
    answer.insert("opened".into(), json!(true));
    answer.insert("path".into(), json!(file.path));
    answer.insert("mime".into(), json!(file.mime));
    answer.insert("title".into(), json!(title));
    answer.extend(sight(ws, &preview, woke).await.0);
    Ok(Value::Object(answer))
}

/// One file of the workspace's preview, whatever its kind, or all of them.
pub async fn preview_close(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let preview = preview_of(ws, port).await?;
    let listed = preview.list().await?;
    let open: Vec<&OpenEntry> = listed.entries.iter().collect();
    let closing: Vec<&OpenEntry> = match text(args.get("path")) {
        Some(given) => {
            let canonical =
                canonical_path(port, &caller_folder(ws, caller), &given, Canon::File).await;
            vec![find_open(
                &open,
                &given,
                &canonical,
                format!(
                    "Nothing is open in the preview of \"{}\", so nothing matches \"{given}\".",
                    ws.name
                ),
            )?]
        }
        None => open.clone(),
    };
    close(ws, caller, port, &preview, open.len(), closing).await
}

/// Standard base64, padded: the PNG of a snapshot as the agent receives it.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(ALPHABET[((triple >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
