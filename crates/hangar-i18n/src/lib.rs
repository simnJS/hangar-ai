//! Hangar's interface strings, in English and French, and the functions that
//! turn a key into text. Ported from `src/i18n.ts`; see docs/native/PLAN.md,
//! "hangar-i18n".
//!
//! # Using it from a view
//!
//! The app keeps a [`Translator`] in its state, set from the saved language
//! (or [`Locale::system`] before the person has picked one), and a view asks it
//! for each label by [`Key`]:
//!
//! ```
//! use hangar_i18n::{args, Key, Locale, Translator};
//!
//! let tr = Translator::new(Locale::Fr);
//!
//! // A label: the compiler checks the key.
//! assert_eq!(tr.t(Key::SidebarSettings), "Réglages");
//!
//! // A sentence with a placeholder. Text is put in as it is.
//! let hint = tr.t_with(Key::SidebarVersion, &args!("version" => "1.12.1"));
//! assert_eq!(hint, "Hangar.AI 1.12.1");
//!
//! // A counted sentence: the language decides which form `n` takes. French
//! // reads 0 and 1 as singular, English only 1, and `{n}` is written the way
//! // the language writes a number.
//! assert_eq!(tr.tn(Key::SidebarActivityWorking, 0, &[]), "0 agent travaille");
//! assert_eq!(tr.tn(Key::SidebarActivityWorking, 12_000, &[]), "12\u{202f}000 agents travaillent");
//! ```
//!
//! A `Translator` is `Copy`, so it goes into a `Message` or a closure as it is.
//! What the functions return is a `Cow<'static, str>`: a label without
//! placeholders is borrowed from the dictionary and allocates nothing, which
//! matters for a view that is built again on every change. It derefs to `&str`
//! and iced's `text` takes it directly. With the locale at hand instead of a
//! translator, [`t`], [`t_with`] and [`tn`] are the same functions.
//!
//! ## Computed keys
//!
//! The web app builds some keys at run time (`` t(`pane.state.${state}`) ``).
//! Prefer a `match` that returns the [`Key`] of each case, which the compiler
//! checks. Where that would only repeat the dictionary, a string works wherever
//! a [`Key`] does: `tr.t(format!("pane.state.{state}"))`. A string that names no
//! key is shown as written, so a typo is visible on screen, and [`Key::from_id`]
//! lets a test ask for every id a program can build.
//!
//! # Adding or changing a string
//!
//! The strings live in the web app's files, still the source of truth while both
//! apps exist: `src/i18n.ts` (`en`, and `fr` which is typed against it, so a
//! missing French string fails there), `src/strings/*.ts`, and
//! `src/components/accounts/strings.ts`, whose keys get an `accounts.` prefix
//! here because they clash with the central dictionary's `settings.*`. Then:
//!
//! 1. Write both languages with plain spaces; the typesetter adds the no-break
//!    ones (`src/lib/typography.ts`: narrow before `? ! ; : » %` and after `«`
//!    in French, and between a number and its unit in both).
//! 2. Run `node crates/hangar-i18n/tools/generate.mjs` (Node 22.18 or later) and
//!    commit `src/generated.rs` with the change. `--check` fails when the file
//!    is stale. Nothing in `cargo build` needs Node.
//! 3. Use the new `Key` variant: `settings.catExperimental` is
//!    `Key::SettingsCatExperimental`. A counted message is one variant for its
//!    `x_one` and `x_other` pair.
//!
//! `tests/dictionary.rs` holds every French string to the glossary's typography
//! and both languages to the same placeholders, so a bad string fails
//! `cargo test`.
//!
//! # How the dictionaries are stored
//!
//! As Rust source (`static` arrays of `&'static str`), not JSON parsed at start:
//! the key enum has to be generated Rust anyway, a lookup by [`Key`] is an array
//! index with no parsing and no allocation, and a string that is not in the
//! dictionary cannot be missed at run time. The cost is a 400 kB source file
//! that compiles in about a second.

mod args;
mod dictionary;
// One entry per line, so a diff of a changed string is one line; rustfmt would
// spread each over several.
#[rustfmt::skip]
mod generated;
mod locale;
mod plural;
mod translate;

pub use args::{Arg, Number};
pub use dictionary::{AsKey, KeyRef};
pub use generated::{KEY_COUNT, Key};
pub use locale::{LOCALES, Locale, LocaleOption, UnknownLocale};
pub use plural::PluralCategory;
pub use translate::{Translator, t, t_with, tn};
