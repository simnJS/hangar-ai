//! The two modals drawn headless, as the inspector's column is (see
//! `inspector::snapshot` for how to ask for the pictures).

use std::sync::Arc;
use std::time::Instant;

use hangar_core::git::{
    Branch, ChangedFile, Changes, FileDiff, MergedWorktree, RepoInfo, Worktree,
};
use hangar_i18n::{Locale, Translator};
use hangar_theme::{Theme, by_id_or_default};
use iced::widget::container;
use iced::{Element, Length, Size, window};

use super::diff_view::{self, DiffMsg, DiffState};
use super::worktrees::{self, After, Loaded, Panel, WtMsg};
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::inspector::snapshot::render;
use crate::message::Message;
use crate::state::Model;
use crate::state::types::{AppState, Pane, Workspace};

fn model(locale: Locale, theme: &str) -> Model {
    let mut m = Model::default();
    let mut state = AppState {
        workspaces: vec![
            Workspace {
                id: "w".into(),
                name: "storefront".into(),
                cwd: "/home/ada/dev/storefront".into(),
                panes: vec![Pane {
                    id: "a".into(),
                    name: "Ava".into(),
                    cwd: Some("/home/ada/dev/storefront-worktrees/feat-search".into()),
                    ..Pane::default()
                }],
                ..Workspace::default()
            },
            Workspace {
                id: "x".into(),
                name: "pay".into(),
                cwd: "/home/ada/dev/storefront-worktrees/feat-pay".into(),
                ..Workspace::default()
            },
        ],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    };
    state.settings.locale = Some(locale);
    state.settings.theme_id = theme.into();
    m.hydrate(state);
    m
}

fn context<'a>(
    m: &'a Model,
    packages: &'a Packages,
    theme: &'static Theme,
    locale: Locale,
) -> Ctx<'a> {
    Ctx {
        model: m,
        pkg: packages,
        i18n: Translator::new(locale),
        theme,
        metrics: Metrics {
            body_width: 1100.0,
            ..Metrics::default()
        },
        keymap: &m.live.keymap.index,
        window: window::Id::unique(),
        now: Instant::now(),
    }
}

// ---- The diff ---------------------------------------------------------------------------------

const DIFF: &str =
    "diff --git a/packages/checkout-ui/src/Cart.tsx b/packages/checkout-ui/src/Cart.tsx
index 3f2a9c1..9d1f7a2 100644
--- a/packages/checkout-ui/src/Cart.tsx
+++ b/packages/checkout-ui/src/Cart.tsx
@@ -12,9 +12,14 @@ import { Discount } from \"./pricing/discount\";
 export function Cart({ items }: Props) {
   const total = sum(items);
-  const discounted = applyDiscount(total, coupon);
+  const discounted = stackDiscounts(total, coupons);
+
+  // Coupons now stack: the best one first, then the rest.
+  if (coupons.length > 1) {
+    track(\"coupons.stacked\", { count: coupons.length });
+  }

   return (
\t<section className=\"cart\">
-      <Total value={discounted} />
+      <Total value={discounted} saved={total - discounted} />
     </section>
   );
 }
@@ -40,4 +45,6 @@ export function sum(items: Item[]) {
   return items.reduce((a, b) => a + b.price, 0);
 }
+
+export const STACK_LIMIT = 3;
\\ No newline at end of file
";

fn linked_info() -> RepoInfo {
    RepoInfo {
        installed: true,
        is_repo: true,
        root: Some("/home/ada/dev/storefront-worktrees/feat-search".into()),
        main_root: Some("/home/ada/dev/storefront".into()),
        is_linked_worktree: true,
        branch: Some("feat/search".into()),
        head: Some("9d1f7a2".into()),
    }
}

fn changed(path: &str, status: &str, additions: u32, deletions: u32) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        old_path: None,
        status: status.into(),
        additions: Some(additions),
        deletions: Some(deletions),
        binary: false,
    }
}

/// A diff view that has read a worktree on its own branch: five files, the
/// first one's diff shown.
fn diff_state() -> DiffState {
    let (mut state, _first) = DiffState::open(
        "/home/ada/dev/storefront-worktrees/feat-search".into(),
        Some("Ava".into()),
        1,
    );
    let _ = state.update(DiffMsg::Setup(
        1,
        Box::new((linked_info(), Some("main".to_string()))),
    ));
    let files = vec![
        changed("packages/checkout-ui/src/Cart.tsx", "M", 8, 2),
        changed("packages/checkout-ui/src/pricing/stack.ts", "?", 64, 0),
        ChangedFile {
            old_path: Some("packages/checkout-ui/src/old-total.ts".into()),
            ..changed("packages/checkout-ui/src/Total.tsx", "R", 3, 3)
        },
        ChangedFile {
            binary: true,
            additions: None,
            deletions: None,
            ..changed("packages/checkout-ui/logo.png", "M", 0, 0)
        },
        changed("README.md", "D", 0, 12),
    ];
    let seq = state.load_seq;
    let _ = state.update(DiffMsg::Listed(
        1,
        seq,
        Ok(Arc::new(Changes {
            root: "/home/ada/dev/storefront-worktrees/feat-search".into(),
            base: Some("a1b2c3d".into()),
            files,
            truncated: false,
        })),
    ));
    let key = state.wanted_key().expect("a file is selected");
    let _ = state.update(DiffMsg::FileRead(
        1,
        seq,
        key,
        Ok(Arc::new(FileDiff {
            text: DIFF.into(),
            binary: false,
            truncated: false,
        })),
    ));
    state
}

fn draw_diff(theme_id: &str, locale: Locale) {
    let (m, packages) = (model(locale, theme_id), Packages::default());
    let theme = by_id_or_default(theme_id);
    let ctx = context(&m, &packages, theme, locale);
    let state = diff_state();
    let modal = diff_view::modal(
        &state,
        ctx,
        |msg| Message::GitUi(super::Msg::Diff(msg)),
        Message::Noop,
    );
    let page: Element<'_, Message> = container(modal)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(40)
        .center_x(Length::Fill)
        .style(hangar_theme::style::container::scrim(theme))
        .into();
    render("diff", theme, locale, Size::new(1100.0, 760.0), page);
}

#[test]
fn the_diff_of_a_worktree_is_drawn_in_both_languages() {
    for locale in [Locale::En, Locale::Fr] {
        draw_diff("graphite", locale);
    }
}

#[test]
fn the_diff_is_drawn_on_a_light_theme() {
    let light = hangar_theme::themes()
        .iter()
        .find(|theme| !theme.is_dark())
        .expect("a light theme exists")
        .id;
    draw_diff(light, Locale::En);
}

// ---- The worktrees ----------------------------------------------------------------------------

fn branch(
    kind: &str,
    name: &str,
    held: Option<&str>,
    upstream: Option<&str>,
    track: Option<&str>,
) -> Branch {
    Branch {
        name: name.into(),
        kind: kind.into(),
        is_current: false,
        worktree_path: held.map(str::to_string),
        head: Some("9d1f7a2".into()),
        committer_date: Some(1_700_000_000),
        author_name: Some("Ada Lovelace".into()),
        subject: Some("Stack the coupons".into()),
        upstream: upstream.map(str::to_string),
        track: track.map(str::to_string),
    }
}

fn merged(path: &str, branch: &str, dirty: Option<u32>) -> MergedWorktree {
    MergedWorktree {
        path: path.into(),
        branch: branch.into(),
        locked: None,
        dirty,
        no_own_commit: false,
        held: false,
        preselected: dirty == Some(0),
    }
}

/// A panel that has read a repository with four worktrees, one of each kind.
fn panel(m: &mut Model) -> Panel {
    let (mut panel, _first) = Panel::open("w".into(), "/home/ada/dev/storefront".into(), 1);
    let main = "/home/ada/dev/storefront";
    let search = "/home/ada/dev/storefront-worktrees/feat-search";
    let pay = "/home/ada/dev/storefront-worktrees/feat-pay";
    let old = "/home/ada/dev/storefront-worktrees/old-experiment";
    let loaded = Loaded::Ready {
        info: RepoInfo {
            installed: true,
            is_repo: true,
            root: Some(main.into()),
            main_root: Some(main.into()),
            is_linked_worktree: false,
            branch: Some("main".into()),
            head: Some("a1b2c3d".into()),
        },
        list: Ok(vec![
            Worktree {
                is_main: true,
                is_current: true,
                ..Worktree {
                    path: main.into(),
                    branch: Some("main".into()),
                    head: Some("a1b2c3d".into()),
                    ..Worktree::default()
                }
            },
            Worktree {
                path: search.into(),
                branch: Some("feat/search".into()),
                head: Some("9d1f7a2".into()),
                ..Worktree::default()
            },
            Worktree {
                path: pay.into(),
                branch: Some("feat/pay".into()),
                head: Some("3f2a9c1".into()),
                locked: Some(String::new()),
                ..Worktree::default()
            },
            Worktree {
                path: old.into(),
                branch: Some("old-experiment".into()),
                head: Some("0b7d4c5".into()),
                prunable: Some("gitdir file points to non-existent location".into()),
                ..Worktree::default()
            },
        ]),
        refs: Ok(vec![
            branch("local", "main", Some(main), Some("origin/main"), None),
            branch(
                "local",
                "feat/search",
                Some(search),
                Some("origin/feat/search"),
                Some("[ahead 2, behind 1]"),
            ),
            branch(
                "local",
                "feat/pay",
                Some(pay),
                Some("origin/feat/pay"),
                Some("[gone]"),
            ),
            branch("local", "old-experiment", Some(old), None, None),
            branch("local", "dev", None, Some("origin/dev"), None),
            branch("remote", "origin/main", None, None, None),
            branch("remote", "origin/dev", None, None, None),
            branch("remote", "origin/feat/refund", None, None, None),
        ]),
    };
    let _ = worktrees::update(
        &mut panel,
        m,
        WtMsg::Loaded(1, Box::new(loaded), After::Nothing),
    );
    panel
}

fn draw_worktrees(
    name: &str,
    theme_id: &str,
    locale: Locale,
    setup: impl FnOnce(&mut Panel, &mut Model),
) {
    let (mut m, packages) = (model(locale, theme_id), Packages::default());
    let theme = by_id_or_default(theme_id);
    let mut panel = panel(&mut m);
    setup(&mut panel, &mut m);
    let ctx = context(&m, &packages, theme, locale);
    let dialog: Element<'_, Message> = worktrees::modal(&panel, ctx).into();
    let page: Element<'_, Message> = container(dialog)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(20)
        .center_x(Length::Fill)
        .style(hangar_theme::style::container::scrim(theme))
        .into();
    render(name, theme, locale, Size::new(760.0, 1100.0), page);
}

fn send(panel: &mut Panel, m: &mut Model, msg: WtMsg) {
    let _task = worktrees::update(panel, m, msg);
}

#[test]
fn the_worktree_list_and_the_form_are_drawn_in_both_languages() {
    for locale in [Locale::En, Locale::Fr] {
        draw_worktrees("worktrees", "graphite", locale, |panel, m| {
            send(panel, m, WtMsg::NewBranch("feat/discount rules".into()));
        });
    }
}

#[test]
fn a_removal_waiting_for_its_second_click_a_block_and_a_question_are_drawn() {
    draw_worktrees("worktrees-armed", "graphite", Locale::En, |panel, m| {
        let search = "/home/ada/dev/storefront-worktrees/feat-search".to_string();
        send(panel, m, WtMsg::RemovePressed(search.clone()));
        send(panel, m, WtMsg::MigrateAsk(search));
        panel.orphan = Some(worktrees::Orphan {
            name: "feat/billing".into(),
            needs_force: false,
            message: None,
        });
    });
}

#[test]
fn the_cleanup_is_drawn() {
    draw_worktrees("worktrees-cleanup", "graphite", Locale::En, |panel, m| {
        send(panel, m, WtMsg::OpenCleanup);
        let mut held = merged(
            "/home/ada/dev/storefront-worktrees/old-experiment",
            "old-experiment",
            Some(4),
        );
        held.held = true;
        let items = vec![
            merged(
                "/home/ada/dev/storefront-worktrees/feat-search",
                "feat/search",
                Some(0),
            ),
            held,
            merged(
                "/home/ada/dev/storefront-worktrees/feat-pay",
                "feat/pay",
                None,
            ),
        ];
        send(panel, m, WtMsg::CleanupListed(1, Ok(items)));
        send(panel, m, WtMsg::CleanupRemovePressed);
    });
}

#[test]
fn the_existing_branch_picker_is_drawn_open() {
    draw_worktrees("worktrees-picker", "graphite", Locale::En, |panel, m| {
        send(panel, m, WtMsg::SetMode(worktrees::create::Mode::Existing));
        send(panel, m, WtMsg::TogglePicker(worktrees::Picker::Existing));
    });
}

#[test]
fn the_panel_is_drawn_on_a_light_theme_and_for_a_folder_that_is_not_a_repository() {
    let light = hangar_theme::themes()
        .iter()
        .find(|theme| !theme.is_dark())
        .expect("a light theme exists")
        .id;
    draw_worktrees("worktrees", light, Locale::En, |_, _| {});
    draw_worktrees("worktrees-plain", "graphite", Locale::En, |panel, m| {
        let plain = RepoInfo {
            installed: true,
            ..RepoInfo::default()
        };
        send(
            panel,
            m,
            WtMsg::Loaded(1, Box::new(Loaded::Unavailable(plain)), After::Nothing),
        );
    });
}
