//! Prints, as JSON, every chrome token of the themes the kits' stylesheet is
//! written in: by default the two `crates/hangar-core/kit-sdk/tokens.json`
//! names (Graphite and GitHub Light), or the ids given as arguments.
//!
//! ```text
//! cargo run -q -p hangar-theme --example kit-tokens
//! {"graphite":{"--chrome":"#1b1c1f",...},"github-light":{...}}
//! ```
//!
//! `scripts/gen-hangar-ui.mjs` reads this to write the colour blocks of
//! `kit.css`: the themes' data in this crate is where those colours come from,
//! as it is for the window, so a theme changed here reaches the kits once the
//! script runs again.

use std::process::ExitCode;

use hangar_theme::{by_id, css, kit_default_themes};
use serde_json::{Map, Value};

fn main() -> ExitCode {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        let (dark, light) = kit_default_themes();
        ids = vec![dark.to_owned(), light.to_owned()];
    }
    let mut out = Map::new();
    for id in &ids {
        let Some(theme) = by_id(id) else {
            eprintln!("no bundled theme has the id {id:?}");
            return ExitCode::FAILURE;
        };
        let tokens: Map<String, Value> = theme
            .colors
            .iter()
            .map(|(name, color)| (name.to_owned(), Value::String(css(color))))
            .collect();
        out.insert(id.clone(), Value::Object(tokens));
    }
    println!("{}", Value::Object(out));
    ExitCode::SUCCESS
}
