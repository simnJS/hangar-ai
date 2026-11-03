//! The commands shortcuts and the palette run (inventory §1.10): their ids,
//! and the table of what is live and what each does.

pub mod id;
pub mod run;

// For the keymap and the palette (P7), which have no code yet.
#[allow(unused_imports)]
pub use id::{CommandId, Section, UnknownCommand};
#[allow(unused_imports)]
pub use run::{available, run};
