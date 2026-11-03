//! The engine every tab runs in, and a kit's module checked before any of
//! it runs.
//!
//! A module is held to the ABI before it is instantiated: it imports only
//! what Hangar gives, from `hangar`, and exports what Hangar calls, with the
//! right types. No WASI is linked: a module built for it does not load, with
//! a message saying how to build it instead.
//!
//! Time is bounded by epoch interruption: a thread advances the engine's
//! epoch every 10 ms, and each call into a module gets a deadline in those
//! ticks. Fuel would count instructions, the same on every machine, but
//! costs more on every one of them and measures nothing a person waits on;
//! what matters here is that the window is never held for long.

use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use wasmtime::{Config, Engine, ExternType, FuncType, Module, ValType};

use hangar_kit_ui::abi::{export, import};

/// How often the epoch advances: the grain of every deadline.
pub(crate) const TICK: Duration = Duration::from_millis(10);

/// What a tab may use. One set for the whole runtime: a kit cannot ask for
/// more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The most linear memory an instance may grow to.
    pub memory_bytes: usize,
    /// How long `hangar_init` may take.
    pub init_deadline: Duration,
    /// How long any other call may take: an update, a view. The window waits
    /// for it, so it is short.
    pub call_deadline: Duration,
    /// The largest module a tab may load.
    pub module_bytes: usize,
    /// How many log lines a tab keeps, the oldest dropped first.
    pub log_lines: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            memory_bytes: 128 * 1024 * 1024,
            init_deadline: Duration::from_secs(1),
            call_deadline: Duration::from_millis(250),
            module_bytes: 64 * 1024 * 1024,
            log_lines: 200,
        }
    }
}

/// Why a module could not become a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Larger than [`Limits::module_bytes`].
    TooLarge { bytes: usize, max: usize },
    /// Not WebAssembly, or not valid.
    Invalid(String),
    /// It imports what Hangar does not give.
    Import { module: String, name: String },
    /// It does not export what Hangar calls, or not with that type.
    Export { name: &'static str, why: String },
    /// It speaks another ABI than those this Hangar reads.
    Abi { module: u32, host: u32 },
    /// The engine could not be made.
    Engine(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::TooLarge { bytes, max } => {
                write!(
                    f,
                    "the tab's module is {} MB, more than the {} MB a tab may load",
                    bytes >> 20,
                    max >> 20
                )
            }
            LoadError::Invalid(why) => write!(
                f,
                "the tab's file is not a WebAssembly module Hangar can run: {why}"
            ),
            LoadError::Import { module, name }
                if module == "wasi_snapshot_preview1" || module.starts_with("wasi") =>
            {
                write!(
                    f,
                    "the tab imports {module}::{name}: it was built for WASI. Build it for wasm32-unknown-unknown, where Hangar gives what a tab needs"
                )
            }
            LoadError::Import { module, name }
                if module.starts_with("__wbindgen") || module == "wbg" =>
            {
                write!(
                    f,
                    "the tab imports {module}::{name}: it was built with wasm-bindgen, for a browser. A native tab needs no JavaScript glue"
                )
            }
            LoadError::Import { module, name } => {
                write!(
                    f,
                    "the tab imports {module}::{name}, which Hangar does not give: only hangar::log and hangar::now"
                )
            }
            LoadError::Export { name, why } => {
                write!(
                    f,
                    "the tab does not export {name} as Hangar calls it ({why}): was it built with hangar_kit_sdk::export_kit!?"
                )
            }
            LoadError::Abi { module, host } if module > host => {
                write!(
                    f,
                    "the tab speaks the ABI {module} and this Hangar reads {host}: it needs a newer Hangar"
                )
            }
            LoadError::Abi { module, host } => {
                write!(
                    f,
                    "the tab speaks the ABI {module}, which this Hangar no longer reads ({host}): it needs a newer SDK"
                )
            }
            LoadError::Engine(why) => write!(f, "Hangar could not start WebAssembly: {why}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// The engine all tabs share, and its clock. Cheap to clone: one per app.
#[derive(Clone)]
pub struct KitRuntime {
    pub(crate) engine: Engine,
    pub(crate) limits: Arc<Limits>,
}

impl KitRuntime {
    pub fn new(limits: Limits) -> Result<KitRuntime, LoadError> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        // A module of one memory and one thread: what wasm32-unknown-unknown
        // builds, and nothing that could share memory with another tab.
        config.wasm_threads(false);
        config.wasm_memory64(false);
        config.wasm_multi_memory(false);
        let engine = Engine::new(&config).map_err(|why| LoadError::Engine(why.to_string()))?;
        let clock = engine.weak();
        thread::Builder::new()
            .name("hangar-kit-epoch".into())
            .spawn(move || {
                // Ends once every tab and the runtime are gone.
                while let Some(engine) = clock.upgrade() {
                    engine.increment_epoch();
                    drop(engine);
                    thread::sleep(TICK);
                }
            })
            .map_err(|why| LoadError::Engine(why.to_string()))?;
        Ok(KitRuntime {
            engine,
            limits: Arc::new(limits),
        })
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Compiles a tab's module and holds it to the ABI. Keep the result for
    /// as long as the file does not change: compiling is the slow part, and
    /// every tab opened from it shares it.
    pub fn compile(&self, wasm: &[u8]) -> Result<KitModule, LoadError> {
        if wasm.len() > self.limits.module_bytes {
            return Err(LoadError::TooLarge {
                bytes: wasm.len(),
                max: self.limits.module_bytes,
            });
        }
        let module = Module::new(&self.engine, wasm)
            .map_err(|why| LoadError::Invalid(format!("{why:#}")))?;
        check_imports(&module)?;
        check_exports(&module)?;
        Ok(KitModule { module })
    }
}

/// A module that passed [`KitRuntime::compile`].
#[derive(Clone)]
pub struct KitModule {
    pub(crate) module: Module,
}

/// A function type by its numbers alone: all the ABI uses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Num {
    I32,
    I64,
}

fn num(ty: &ValType) -> Option<Num> {
    match ty {
        ValType::I32 => Some(Num::I32),
        ValType::I64 => Some(Num::I64),
        _ => None,
    }
}

fn fits(ty: &FuncType, params: &[Num], results: &[Num]) -> bool {
    ty.params()
        .map(|p| num(&p))
        .eq(params.iter().map(|p| Some(*p)))
        && ty
            .results()
            .map(|r| num(&r))
            .eq(results.iter().map(|r| Some(*r)))
}

fn describe(ty: &FuncType) -> String {
    let name = |ty: ValType| match num(&ty) {
        Some(Num::I32) => "i32".to_owned(),
        Some(Num::I64) => "i64".to_owned(),
        None => format!("{ty:?}"),
    };
    let params: Vec<String> = ty.params().map(name).collect();
    let results: Vec<String> = ty.results().map(name).collect();
    format!("({}) -> ({})", params.join(", "), results.join(", "))
}

fn check_imports(module: &Module) -> Result<(), LoadError> {
    use Num::*;
    for wanted in module.imports() {
        let given = wanted.module() == import::MODULE
            && match (wanted.name(), wanted.ty()) {
                (import::LOG, ExternType::Func(ty)) => fits(&ty, &[I32, I32, I32], &[]),
                (import::NOW, ExternType::Func(ty)) => fits(&ty, &[], &[I64]),
                _ => false,
            };
        if !given {
            return Err(LoadError::Import {
                module: wanted.module().to_owned(),
                name: wanted.name().to_owned(),
            });
        }
    }
    Ok(())
}

fn check_exports(module: &Module) -> Result<(), LoadError> {
    use Num::*;
    let wanted: [(&'static str, &[Num], &[Num], &str); 5] = [
        (export::ABI, &[], &[I32], "() -> (i32)"),
        (export::ALLOC, &[I32], &[I32], "(i32) -> (i32)"),
        (export::INIT, &[I32, I32], &[I64], "(i32, i32) -> (i64)"),
        (export::UPDATE, &[I32, I32], &[I64], "(i32, i32) -> (i64)"),
        (export::VIEW, &[], &[I64], "() -> (i64)"),
    ];
    for (name, params, results, signature) in wanted {
        match module.get_export(name) {
            Some(ExternType::Func(ty)) if fits(&ty, params, results) => {}
            Some(ExternType::Func(ty)) => {
                return Err(LoadError::Export {
                    name,
                    why: format!("it is {}, not {signature}", describe(&ty)),
                });
            }
            Some(_) => {
                return Err(LoadError::Export {
                    name,
                    why: "it is not a function".into(),
                });
            }
            None => {
                return Err(LoadError::Export {
                    name,
                    why: "it is missing".into(),
                });
            }
        }
    }
    match module.get_export(export::MEMORY) {
        Some(ExternType::Memory(_)) => Ok(()),
        _ => Err(LoadError::Export {
            name: export::MEMORY,
            why: "no memory is exported".into(),
        }),
    }
}
