//! What the module imports from Hangar, and what stands for it on the host,
//! where a kit runs its tests.

use hangar_kit_ui::abi::LogLevel;

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "hangar")]
unsafe extern "C" {
    #[link_name = "log"]
    fn hangar_log(level: u32, ptr: *const u8, len: usize);
    #[link_name = "now"]
    fn hangar_now() -> i64;
}

/// A line for the tab's log, which Hangar keeps and shows beside a failure.
pub fn log(level: LogLevel, message: &str) {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: the pointer and length are a live `&str`'s, which Hangar reads
    // during the call and does not keep.
    unsafe {
        hangar_log(level as u32, message.as_ptr(), message.len())
    }
    #[cfg(not(target_arch = "wasm32"))]
    host::log(level, message);
}

/// Milliseconds since the Unix epoch, UTC: wasm32-unknown-unknown has no
/// clock of its own, so it is Hangar's.
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: takes nothing, returns a number.
    unsafe {
        hangar_now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    host::now_ms()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod host {
    use std::cell::{Cell, RefCell};
    use std::time::{SystemTime, UNIX_EPOCH};

    use hangar_kit_ui::abi::LogLevel;

    thread_local! {
        /// The clock a test set, in place of the system's.
        pub(crate) static NOW: Cell<Option<i64>> = const { Cell::new(None) };
        /// What was logged, for a test to read.
        pub(crate) static LOGGED: RefCell<Vec<(LogLevel, String)>> = const { RefCell::new(Vec::new()) };
    }

    pub fn log(level: LogLevel, message: &str) {
        LOGGED.with(|logged| logged.borrow_mut().push((level, message.to_owned())));
        if level >= LogLevel::Warn {
            eprintln!("[kit {level:?}] {message}");
        }
    }

    pub fn now_ms() -> i64 {
        NOW.with(Cell::get).unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|since| since.as_millis() as i64)
                .unwrap_or(0)
        })
    }
}

/// Logs at a level: `console::info("read 4 runs")`.
pub mod console {
    use hangar_kit_ui::abi::LogLevel;

    pub fn debug(message: impl AsRef<str>) {
        super::log(LogLevel::Debug, message.as_ref());
    }

    pub fn info(message: impl AsRef<str>) {
        super::log(LogLevel::Info, message.as_ref());
    }

    pub fn warn(message: impl AsRef<str>) {
        super::log(LogLevel::Warn, message.as_ref());
    }

    pub fn error(message: impl AsRef<str>) {
        super::log(LogLevel::Error, message.as_ref());
    }
}
