//! Keeping the PC awake while Hangar Mobile is on, so the phone can reach it
//! during a week away.
//!
//! The system is kept from sleeping, never the display: the screen goes off
//! as it always did. Opt-in, in the Mobile settings. Held only while both the
//! option and Hangar Mobile are on; released the moment either goes off, and
//! when Hangar quits (the held request dies with the process, and on macOS
//! `caffeinate` is told to follow Hangar's pid).
//!
//! - Windows: `SetThreadExecutionState(ES_SYSTEM_REQUIRED | ES_CONTINUOUS)`,
//!   which belongs to the thread that calls it: a thread of its own holds it.
//! - macOS: `caffeinate -i`, an IOKit "prevent idle sleep" assertion.
//! - Linux: not supported.

/// The platform's way of holding the PC awake.
pub trait Holder: Send {
    fn supported(&self) -> bool;
    fn acquire(&mut self) -> Result<(), String>;
    fn release(&mut self);
}

/// When to hold: the option wanted, Hangar Mobile on.
pub struct KeepAwake {
    holder: Box<dyn Holder>,
    wanted: bool,
    mobile_on: bool,
    held: bool,
}

pub const UNSUPPORTED: &str = "Keeping the PC awake is not supported on this system.";

impl KeepAwake {
    pub fn new(holder: Box<dyn Holder>, wanted: bool) -> Self {
        Self {
            holder,
            wanted,
            mobile_on: false,
            held: false,
        }
    }

    pub fn supported(&self) -> bool {
        self.holder.supported()
    }

    #[cfg(test)]
    pub fn held(&self) -> bool {
        self.held
    }

    /// The option switched. Turning it on where it cannot work is refused;
    /// a hold that fails leaves the option as it was.
    pub fn set_wanted(&mut self, wanted: bool) -> Result<(), String> {
        if wanted && !self.holder.supported() {
            return Err(UNSUPPORTED.to_string());
        }
        let before = self.wanted;
        self.wanted = wanted;
        self.apply().inspect_err(|_| self.wanted = before)
    }

    /// Hangar Mobile started or stopped listening.
    pub fn set_mobile_on(&mut self, on: bool) -> Result<(), String> {
        self.mobile_on = on;
        self.apply()
    }

    fn apply(&mut self) -> Result<(), String> {
        let target = self.wanted && self.mobile_on && self.holder.supported();
        if target && !self.held {
            self.holder.acquire()?;
            self.held = true;
        } else if !target && self.held {
            self.holder.release();
            self.held = false;
        }
        Ok(())
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if self.held {
            self.holder.release();
        }
    }
}

/// This system's way, or the one that says it cannot.
pub fn system() -> Box<dyn Holder> {
    Box::new(System::default())
}

#[cfg(windows)]
#[derive(Default)]
struct System {
    hold: Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)>,
}

#[cfg(windows)]
impl Holder for System {
    fn supported(&self) -> bool {
        true
    }

    fn acquire(&mut self) -> Result<(), String> {
        use windows_sys::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
        };
        if self.hold.is_some() {
            return Ok(());
        }
        let (stop, wait) = std::sync::mpsc::channel::<()>();
        let (ready, ready_rx) = std::sync::mpsc::channel::<bool>();
        let thread = std::thread::Builder::new()
            .name("hangar-keep-awake".into())
            .spawn(move || {
                // SAFETY: a plain call with flags; it changes this thread's state only.
                let before = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                let _ = ready.send(before != 0);
                if before != 0 {
                    // Until released: the sender dropped is a release too.
                    let _ = wait.recv();
                    // SAFETY: as above; ES_CONTINUOUS alone clears the request.
                    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
                }
            })
            .map_err(|err| format!("could not keep the PC awake: {err}"))?;
        match ready_rx.recv() {
            Ok(true) => {
                self.hold = Some((stop, thread));
                Ok(())
            }
            _ => {
                let _ = thread.join();
                Err("Windows refused to keep the PC awake.".to_string())
            }
        }
    }

    fn release(&mut self) {
        if let Some((stop, thread)) = self.hold.take() {
            let _ = stop.send(());
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct System {
    child: Option<std::process::Child>,
}

#[cfg(target_os = "macos")]
impl Holder for System {
    fn supported(&self) -> bool {
        true
    }

    fn acquire(&mut self) -> Result<(), String> {
        if self.child.is_some() {
            return Ok(());
        }
        // -i: no idle sleep, the display left alone. -w: ends with Hangar,
        // however Hangar ends.
        let child = std::process::Command::new("caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|err| format!("could not run caffeinate: {err}"))?;
        self.child = Some(child);
        Ok(())
    }

    fn release(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Default)]
struct System;

#[cfg(not(any(windows, target_os = "macos")))]
impl Holder for System {
    fn supported(&self) -> bool {
        false
    }

    fn acquire(&mut self) -> Result<(), String> {
        Err(UNSUPPORTED.to_string())
    }

    fn release(&mut self) {}
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A holder that writes down what it was asked, and can refuse.
    #[derive(Clone)]
    pub struct Fake {
        pub calls: Arc<Mutex<Vec<&'static str>>>,
        supported: bool,
        refuse: bool,
    }

    impl Fake {
        pub fn new() -> Self {
            Self {
                calls: Default::default(),
                supported: true,
                refuse: false,
            }
        }

        fn unsupported() -> Self {
            Self {
                supported: false,
                ..Self::new()
            }
        }

        fn refusing() -> Self {
            Self {
                refuse: true,
                ..Self::new()
            }
        }

        pub fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Holder for Fake {
        fn supported(&self) -> bool {
            self.supported
        }

        fn acquire(&mut self) -> Result<(), String> {
            if self.refuse {
                return Err("refused".into());
            }
            self.calls.lock().unwrap().push("acquire");
            Ok(())
        }

        fn release(&mut self) {
            self.calls.lock().unwrap().push("release");
        }
    }

    #[test]
    fn it_holds_only_while_the_option_and_hangar_mobile_are_both_on() {
        let fake = Fake::new();
        let mut awake = KeepAwake::new(Box::new(fake.clone()), false);
        awake.set_mobile_on(true).unwrap();
        assert!(!awake.held(), "Mobile on alone holds nothing");
        awake.set_wanted(true).unwrap();
        assert!(awake.held());
        awake.set_mobile_on(false).unwrap();
        assert!(!awake.held(), "Mobile off lets the PC sleep");
        awake.set_mobile_on(true).unwrap();
        assert!(awake.held());
        awake.set_wanted(false).unwrap();
        assert!(!awake.held(), "the option off lets the PC sleep");
        assert_eq!(fake.calls(), ["acquire", "release", "acquire", "release"]);
    }

    #[test]
    fn an_option_left_on_holds_as_soon_as_mobile_starts() {
        let fake = Fake::new();
        let mut awake = KeepAwake::new(Box::new(fake.clone()), true);
        assert!(!awake.held());
        awake.set_mobile_on(true).unwrap();
        assert!(awake.held());
    }

    #[test]
    fn saying_the_same_thing_twice_holds_once() {
        let fake = Fake::new();
        let mut awake = KeepAwake::new(Box::new(fake.clone()), true);
        awake.set_mobile_on(true).unwrap();
        awake.set_mobile_on(true).unwrap();
        awake.set_wanted(true).unwrap();
        assert_eq!(fake.calls(), ["acquire"]);
        awake.set_mobile_on(false).unwrap();
        awake.set_mobile_on(false).unwrap();
        assert_eq!(fake.calls(), ["acquire", "release"]);
    }

    #[test]
    fn quitting_releases_what_is_held() {
        let fake = Fake::new();
        let mut awake = KeepAwake::new(Box::new(fake.clone()), true);
        awake.set_mobile_on(true).unwrap();
        drop(awake);
        assert_eq!(fake.calls(), ["acquire", "release"]);

        let idle = Fake::new();
        drop(KeepAwake::new(Box::new(idle.clone()), true));
        assert!(idle.calls().is_empty(), "nothing held, nothing to release");
    }

    #[test]
    fn where_it_cannot_work_the_option_is_refused() {
        let fake = Fake::unsupported();
        let mut awake = KeepAwake::new(Box::new(fake.clone()), false);
        assert!(!awake.supported());
        assert_eq!(awake.set_wanted(true), Err(UNSUPPORTED.to_string()));
        awake.set_mobile_on(true).unwrap();
        assert!(!awake.held());
        assert!(fake.calls().is_empty());
        // A saved "on" from elsewhere holds nothing either.
        let mut saved = KeepAwake::new(Box::new(fake.clone()), true);
        saved.set_mobile_on(true).unwrap();
        assert!(!saved.held());
    }

    #[test]
    fn a_hold_that_fails_leaves_the_option_as_it_was() {
        let mut awake = KeepAwake::new(Box::new(Fake::refusing()), false);
        awake.set_mobile_on(true).unwrap();
        assert_eq!(awake.set_wanted(true), Err("refused".to_string()));
        assert!(!awake.held());
        // Still off: Mobile going on and off again asks nothing of the system.
        awake.set_mobile_on(false).unwrap();
        awake.set_mobile_on(true).unwrap();
        assert!(!awake.held());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn the_system_holds_and_lets_go() {
        let mut system = System::default();
        assert!(system.supported());
        system.acquire().unwrap();
        system.acquire().unwrap();
        system.release();
        system.release();
    }
}
