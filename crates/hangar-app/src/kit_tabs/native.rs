//! Native tabs: a kit's WebAssembly module, run by hangar-kit-host in its
//! sandbox and drawn with Hangar's own widgets (docs/native/kits-native.md,
//! "For Hangar: opening a native tab").
//!
//! One runtime for the app's life; a module compiled once per file, again
//! when the file changes (a linked folder rebuilt); one instance per open tab,
//! started when its page would load (`kit_tab_loads`) and dropped when the tab
//! closes, its kit stops being usable there, or its workspace is no longer
//! mounted. What the instance asks goes through its own gate and this
//! package's `TabApi`, the same an HTML tab's calls go through.
//!
//! The `ui.*` calls are the window's: a toast at the foot of the tab, a
//! question in place of its page, the clipboard. Their answers wait in here
//! until the person (or the toast's time) gives them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};

use hangar_kit_host::ui::Notice;
use hangar_kit_host::ui::abi::{KitInfo as StartKit, TabInfo};
use hangar_kit_host::ui::api::{self as kit_api, KitError, Scheme};
use hangar_kit_host::{KitModule, KitRuntime, KitTab, Limits, TabStart};
use hangar_ui::toast::{Toast, Toasts};

use super::api::TabApi;
use super::deps::UiAsk;
use crate::message::Message;
use crate::preview::store::WatchId;
use crate::services::inbox::Answer;

/// How long a toast stays that is not an error (`hangar.ui.toast`).
const TOAST_LIFETIME: Duration = Duration::from_secs(4);

type Reply = Answer<Result<Value, KitError>>;

/// The runtime and the modules compiled for it.
#[derive(Default)]
pub struct Runtime {
    runtime: Option<Result<KitRuntime, String>>,
    /// By file: its modification time when compiled, and what came of it.
    modules: HashMap<PathBuf, (Option<SystemTime>, Result<KitModule, String>)>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("modules", &self.modules.len())
            .finish()
    }
}

/// When a file was last written, to compile a module again once it was.
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

impl Runtime {
    pub fn runtime(&mut self) -> Result<KitRuntime, String> {
        self.runtime
            .get_or_insert_with(|| {
                KitRuntime::new(Limits::default()).map_err(|why| why.to_string())
            })
            .clone()
    }

    /// The module of `path`, compiled once for as long as the file is not
    /// written again. Why not, in the runtime's words, when it does not load.
    pub fn module(&mut self, path: &Path) -> Result<KitModule, String> {
        let modified = modified(path);
        if let Some((held, compiled)) = self.modules.get(path)
            && *held == modified
        {
            return compiled.clone();
        }
        let compiled = self.runtime().and_then(|runtime| {
            let bytes = std::fs::read(path).map_err(|why| format!("{}: {why}", path.display()))?;
            runtime.compile(&bytes).map_err(|why| why.to_string())
        });
        self.modules
            .insert(path.to_path_buf(), (modified, compiled.clone()));
        compiled
    }
}

/// A toast a tab raised, and the answer it waits for.
struct Raised {
    /// Its identity among the tab's toasts on screen.
    shown: u64,
    reply: Reply,
}

/// One open native tab.
pub struct NativeTab {
    pub tab: KitTab,
    pub api: Arc<TabApi>,
    /// What it was started from: the entry's revision (a reload starts it
    /// anew) and its module as it was then.
    pub revision: u64,
    pub module: PathBuf,
    pub modified: Option<SystemTime>,
    /// The store's watch of its params, to stop when it goes.
    pub watch: Option<WatchId>,
    pub toasts: Toasts<Message>,
    raised: HashMap<u64, Raised>,
    next_toast: u64,
    /// The question in place of its page, and who waits for the answer.
    pub confirm: Option<(kit_api::Confirm, Reply)>,
}

impl std::fmt::Debug for NativeTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTab")
            .field("revision", &self.revision)
            .field("module", &self.module)
            .field("failed", &self.tab.failure().is_some())
            .finish()
    }
}

/// What a tab is told it is, from the app's record of it.
pub struct Started<'a> {
    pub kit_id: &'a str,
    pub kit_name: &'a str,
    pub kit_version: &'a str,
    pub tab_id: &'a str,
    pub params: std::collections::BTreeMap<String, String>,
    pub caller: Option<String>,
    pub theme: kit_api::Theme,
    pub visible: bool,
}

impl NativeTab {
    /// A new instance of `module` for a tab, started: `init`, then its first
    /// page. The effects are its first calls.
    pub fn start(
        runtime: &KitRuntime,
        module: &KitModule,
        started: Started<'_>,
        api: Arc<TabApi>,
        revision: u64,
        path: PathBuf,
    ) -> (NativeTab, hangar_kit_host::Effects) {
        let start = TabStart {
            hangar: hangar_core::kits::HANGAR_VERSION.into(),
            kit: StartKit {
                id: started.kit_id.into(),
                name: started.kit_name.into(),
                version: started.kit_version.into(),
            },
            tab: TabInfo {
                id: started.tab_id.into(),
                params: started.params,
                caller: started.caller.map(|pane| kit_api::Caller { pane }),
            },
            theme: started.theme,
            visible: started.visible,
        };
        let (tab, effects) = KitTab::start(runtime, module, start, api.clone());
        let modified = modified(&path);
        (
            NativeTab {
                tab,
                api,
                revision,
                module: path,
                modified,
                watch: None,
                toasts: Toasts::new(),
                raised: HashMap::new(),
                next_toast: 0,
                confirm: None,
            },
            effects,
        )
    }

    /// The tab asks the window for a toast or a question; the clipboard is
    /// the caller's to write.
    pub fn ask(&mut self, entry: &str, ask: UiAsk, now: Instant) {
        let UiAsk { call, answer } = ask;
        match call {
            super::api::UiCall::Toast(asked) => {
                self.next_toast += 1;
                let seq = self.next_toast;
                let mut toast = match asked.kind {
                    Notice::Info => Toast::info(asked.message),
                    Notice::Success => Toast::success(asked.message),
                    Notice::Warning => Toast::warning(asked.message),
                    // An error stays until it is closed: it is to be read.
                    Notice::Error => Toast::error(asked.message).sticky(),
                };
                if asked.kind != Notice::Error {
                    toast = toast.lifetime(TOAST_LIFETIME);
                }
                if let Some(label) = asked.action {
                    toast = toast.action(
                        label,
                        Message::KitTabs(super::Msg::ToastAction(entry.to_owned(), seq)),
                    );
                }
                let shown = self.toasts.push(toast, now);
                self.raised.insert(
                    seq,
                    Raised {
                        shown,
                        reply: answer,
                    },
                );
                self.answer_gone();
            }
            super::api::UiCall::Confirm(question) => {
                // The gate lets one through at a time; a second waits for no
                // one, and is answered no.
                if let Some((_, waiting)) = self.confirm.replace((question, answer)) {
                    waiting.send(Ok(json!(false)));
                }
            }
            super::api::UiCall::Copy(_) => {
                answer.send(Ok(json!({})));
            }
        }
    }

    /// The toast's action was pressed: it goes, and says so.
    pub fn toast_action(&mut self, seq: u64) {
        if let Some(raised) = self.raised.remove(&seq) {
            self.toasts.dismiss(raised.shown);
            raised.reply.send(Ok(json!({ "action": true })));
        }
    }

    /// The person closed a toast.
    pub fn dismiss_toast(&mut self, shown: u64) {
        self.toasts.dismiss(shown);
        self.answer_gone();
    }

    /// The toasts whose time has come go.
    pub fn tick(&mut self, now: Instant) {
        self.toasts.tick(now);
        self.answer_gone();
    }

    /// Every toast no longer on screen is answered: its action was not
    /// pressed.
    fn answer_gone(&mut self) {
        let shown: Vec<u64> = self.toasts.iter().map(|toast| toast.id).collect();
        self.raised.retain(|_, raised| {
            let still = shown.contains(&raised.shown);
            if !still {
                raised.reply.send(Ok(json!({ "action": false })));
            }
            still
        });
    }

    /// The question answered.
    pub fn confirmed(&mut self, yes: bool) {
        if let Some((_, reply)) = self.confirm.take() {
            reply.send(Ok(json!(yes)));
        }
    }

    /// The tab goes: whoever waits for the window is told no.
    pub fn close(&mut self) {
        self.confirmed(false);
        for (_, raised) in self.raised.drain() {
            raised.reply.send(Ok(json!({ "action": false })));
        }
    }
}

/// The scheme and language a tab is drawn in.
pub fn theme_of(theme: &hangar_theme::Theme, locale: hangar_i18n::Locale) -> kit_api::Theme {
    kit_api::Theme {
        scheme: if theme.is_dark() {
            Scheme::Dark
        } else {
            Scheme::Light
        },
        locale: locale.id().into(),
    }
}
