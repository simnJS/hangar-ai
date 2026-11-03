//! A development aid: a debug build opens kit tabs in the active workspace's
//! preview once the state is read, so a tab can be looked at without a click
//! or an agent. With `HANGAR_DEV_KIT_TABS=<kit>/<tab>,<kit>/<tab>…` the tabs
//! open in that order, the last in front. A tab can be given params as an
//! agent's `kit_tab_open` would, after a `?`: `editor/editor?path=.env&line=3`
//! (a value cannot hold a `,` or a `&`):
//!
//! ```sh
//! HANGAR_DEV_KIT_TABS=hello-native/hello \
//!   HANGAR_DEV_SCREENSHOT=/tmp/kit.png scripts/native-dev.sh
//! ```
//!
//! A release build never reads it.

use crate::preview::store::KitTabParams;

/// One tab asked for.
#[derive(Debug, PartialEq)]
pub struct Asked {
    pub kit: String,
    pub tab: String,
    pub params: KitTabParams,
}

/// The tabs asked for, in order.
pub fn requested() -> Vec<Asked> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    parse(&std::env::var("HANGAR_DEV_KIT_TABS").unwrap_or_default())
}

fn parse(spec: &str) -> Vec<Asked> {
    spec.split(',')
        .filter_map(|item| {
            let (name, query) = item.trim().split_once('?').unwrap_or((item.trim(), ""));
            let (kit, tab) = name.split_once('/')?;
            let params = query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .filter(|(key, _)| !key.is_empty())
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect();
            (!kit.is_empty() && !tab.is_empty()).then(|| Asked {
                kit: kit.to_owned(),
                tab: tab.to_owned(),
                params,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(kit: &str, tab: &str, params: &[(&str, &str)]) -> Asked {
        Asked {
            kit: kit.to_owned(),
            tab: tab.to_owned(),
            params: params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }

    #[test]
    fn reads_kits_and_tabs_and_skips_what_is_not_one() {
        assert_eq!(
            parse("hello-native/hello, html-demo/page,broken,/x"),
            vec![
                asked("hello-native", "hello", &[]),
                asked("html-demo", "page", &[]),
            ]
        );
        assert!(parse("").is_empty());
    }

    #[test]
    fn reads_the_params_after_a_question_mark() {
        assert_eq!(
            parse("editor/editor?path=notes.txt&line=3,checklist/checklist?path=a.json&=x&bare"),
            vec![
                asked("editor", "editor", &[("path", "notes.txt"), ("line", "3")]),
                asked("checklist", "checklist", &[("path", "a.json")]),
            ]
        );
        assert_eq!(parse("/x?path=y"), vec![]);
    }
}
