//! The exports [`export_kit!`](crate::export_kit) writes, in their bodies:
//! bytes in, JSON read, the kit driven, JSON out. Only for WebAssembly.
//!
//! Who owns what: Hangar writes a message into a buffer `hangar_alloc` gave
//! it, and passes it to an export, which takes it back and frees it once
//! read. An export answers with a buffer kept here until the next call,
//! which Hangar copies before making one.

use std::cell::RefCell;

use serde::Serialize;
use serde::de::DeserializeOwned;

use hangar_kit_ui::abi::{self, Input, LogLevel, Output, Start};

use crate::Kit;
use crate::runtime::Runtime;
use crate::sys;

thread_local! {
    /// The last answer, alive until the next call.
    static ANSWER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn alloc(len: u32) -> u32 {
    let buffer = vec![0u8; len as usize].into_boxed_slice();
    Box::into_raw(buffer) as *mut u8 as usize as u32
}

/// The buffer `alloc` gave, back.
///
/// # Safety
/// `ptr` and `len` must be what `alloc` gave and Hangar filled, once.
unsafe fn take(ptr: u32, len: u32) -> Box<[u8]> {
    // SAFETY: the caller's.
    unsafe {
        Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            ptr as usize as *mut u8,
            len as usize,
        ))
    }
}

fn answer(value: &impl Serialize) -> u64 {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|error| {
        sys::log(
            LogLevel::Error,
            &format!("the answer did not write as JSON: {error}"),
        );
        b"{}".to_vec()
    });
    ANSWER.with(|slot| {
        let mut slot = slot.borrow_mut();
        *slot = bytes;
        abi::pack(slot.as_ptr() as usize as u32, slot.len() as u32)
    })
}

fn read<T: DeserializeOwned>(ptr: u32, len: u32, what: &str) -> Option<T> {
    // SAFETY: Hangar passes what `hangar_alloc` gave it, filled, once.
    let bytes = unsafe { take(ptr, len) };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            // A newer Hangar's input kind reads as an error here: it is
            // not one, the kit just does not know it.
            sys::log(LogLevel::Debug, &format!("{what} not read: {error}"));
            None
        }
    }
}

/// Says a panic's message before the module traps, for Hangar to show it.
fn hook_panics() {
    std::panic::set_hook(Box::new(|info| {
        sys::log(LogLevel::Panic, &info.to_string())
    }));
}

/// # Safety
/// `ptr` and `len` must be a buffer of `hangar_alloc`, filled by Hangar.
pub unsafe fn init<K: Kit>(runtime: &RefCell<Runtime<K>>, ptr: u32, len: u32) -> u64 {
    hook_panics();
    let output = match read::<Start>(ptr, len, "the start") {
        Some(start) => runtime.borrow_mut().init(start),
        None => panic!("Hangar's start did not read: this SDK is older than the Hangar it runs in"),
    };
    answer(&output)
}

/// # Safety
/// `ptr` and `len` must be a buffer of `hangar_alloc`, filled by Hangar.
pub unsafe fn update<K: Kit>(runtime: &RefCell<Runtime<K>>, ptr: u32, len: u32) -> u64 {
    let output = match read::<Input>(ptr, len, "an input") {
        Some(input) => runtime.borrow_mut().update(input),
        None => Output::default(),
    };
    answer(&output)
}

pub fn view<K: Kit>(runtime: &RefCell<Runtime<K>>) -> u64 {
    let view = runtime.borrow_mut().view();
    answer(&view)
}
