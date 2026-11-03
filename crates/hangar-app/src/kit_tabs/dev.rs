//! A development aid: a debug build opens kit tabs in the active workspace's
//! preview once the state is read, so a tab can be looked at without a click
//! or an agent. With `HANGAR_DEV_KIT_TABS=<kit>/<tab>,<kit>/<tab>…` the tabs
//! open in that order, the last in front:
//!
//! ```sh
//! HANGAR_DEV_KIT_TABS=hello-native/hello \
//!   HANGAR_DEV_SCREENSHOT=/tmp/kit.png scripts/native-dev.sh
//! ```
//!
//! A release build never reads it.

/// The tabs asked for, as `(kit, tab)`.
pub fn requested() -> Vec<(String, String)> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    parse(&std::env::var("HANGAR_DEV_KIT_TABS").unwrap_or_default())
}

fn parse(spec: &str) -> Vec<(String, String)> {
    spec.split(',')
        .filter_map(|item| {
            let (kit, tab) = item.trim().split_once('/')?;
            (!kit.is_empty() && !tab.is_empty()).then(|| (kit.to_owned(), tab.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_kits_and_tabs_and_skips_what_is_not_one() {
        assert_eq!(
            parse("hello-native/hello, html-demo/page,broken,/x"),
            vec![
                ("hello-native".to_string(), "hello".to_string()),
                ("html-demo".to_string(), "page".to_string()),
            ]
        );
        assert!(parse("").is_empty());
    }
}
