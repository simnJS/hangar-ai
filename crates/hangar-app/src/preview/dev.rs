//! A way to see the preview in a running app, for whoever builds it without
//! an agent to open files in it: with `HANGAR_DEV_PREVIEW=<file>,<file>…` a
//! debug build shows those files in the active workspace's preview as soon as
//! the state is loaded, as an agent's `preview_show` would. The paths are
//! absolute, or relative to the workspace's folder, and must be inside it.
//!
//! ```text
//! HANGAR_DEV_PREVIEW=/work/app/README.md,/work/app/shot.png \
//!   HANGAR_DEV_SCREENSHOT=/tmp/preview.png scripts/native-dev.sh
//! ```
//!
//! A release build ignores the variable.

use std::sync::Arc;

use iced::Task;

use super::access::Shared;
use crate::message::{Intent, Message};
use crate::services::preview::PreviewRequest;
use crate::shell::Notice;
use crate::state::WsId;

/// The files asked for, in order.
pub fn requested() -> Vec<String> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    parse(&std::env::var("HANGAR_DEV_PREVIEW").unwrap_or_default())
}

fn parse(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// Shows each file, one after the other, as the person's own request (an
/// agent's would name a pane).
pub fn show(shared: Arc<Shared>, workspace: WsId, paths: Vec<String>) -> Task<Message> {
    Task::perform(
        async move {
            for path in paths {
                shared
                    .show_request(PreviewRequest {
                        ws: workspace.clone(),
                        path,
                        title: None,
                        opened_by: None,
                        reveal: true,
                    })
                    .await?;
            }
            Ok::<(), String>(())
        },
        |shown| match shown {
            Ok(()) => Message::Noop,
            Err(why) => Message::Intent(Intent::Notice(Notice::Problem(why))),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_of_paths_is_split_on_commas_and_trimmed() {
        assert_eq!(parse("/a/b.png, /a/c.md ,,"), ["/a/b.png", "/a/c.md"]);
        assert!(parse("").is_empty());
    }
}
