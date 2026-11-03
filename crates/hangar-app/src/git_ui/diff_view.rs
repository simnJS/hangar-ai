//! The "Changes" modal (`DiffView.tsx`): what an agent changed in a folder,
//! read-only: the files on the left, the unified diff of the selected one on
//! the right.
//!
//! A linked worktree is compared with where its branch left the main
//! checkout's branch by default — agents commit, and their commits are the
//! work as much as what is still uncommitted. Anywhere else, HEAD.
//!
//! The modal is opened from a pane (the inspector's Files section, the pane's
//! header), and by the worktree panel for one of its rows, where it takes the
//! panel's place and gives it back; both are this one state and view, wrapped
//! in the messages of whoever shows them.
//!
//! The diff is drawn as a list of fixed-height rows of which only the ones in
//! view are built: a diff can be thousands of lines long (the backend cuts it
//! at 512 KiB), and building a widget for each of them on every frame would
//! stall the window.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use hangar_core::git::{self, ChangedFile, Changes, FileDiff, RepoInfo};
use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{BORDER_WIDTH, font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::button::{self, icon_button};
use hangar_ui::input::segmented_labels;
use hangar_ui::keys::on_key;
use hangar_ui::tail_text::tail_text;
use hangar_ui::{Icon, layer, list, tip};
use iced::advanced::text::{self as adv_text, Paragraph as _};
use iced::keyboard::{Key as Keyboard, key::Named};
use iced::widget::{
    Id, column, container, operation, row, rule, scrollable, space as filler, text,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Pixels, Size, Task};

use super::diff_parse::{DiffRow, DiffRowKind, parse_unified_diff};
use crate::ctx::Ctx;
use crate::inspector::files_model::split_path;
use crate::message::Message;

/// How the owner turns the modal's messages into its own.
pub type Wrap = fn(DiffMsg) -> Message;

/// The height of a row of the diff: 12 px of mono at the stylesheet's 1.55.
const ROW: f32 = 19.0;
/// Rows built above and below what is in view, so a fast scroll never shows
/// a hole.
const OVERSCAN: usize = 12;
/// What a viewport is taken to be until the first scroll says.
const FIRST_VIEWPORT: f32 = 900.0;
/// The distance between tab stops: the browser's default `tab-size`, which
/// the web app's diff table (`white-space: pre`) never overrode.
const TAB_STOP: usize = 8;
/// The height of a file in the list (`.diff__file`).
const FILE_ROW: f32 = 26.0;

// ---- State ----------------------------------------------------------------------------------

#[derive(Debug)]
enum Setup {
    Loading,
    Ready {
        info: RepoInfo,
        /// The main checkout's branch when this folder is a linked worktree
        /// on another one: what "Since" compares with.
        main_branch: Option<String>,
    },
}

/// What the diff pane shows of the selected file: the rows that are not the
/// file's header, and what sizes the table.
#[derive(Debug)]
pub struct Shown {
    rows: Vec<DiffRow>,
    binary: bool,
    truncated: bool,
    /// The longest line of code, in characters, tabs expanded.
    widest: usize,
    /// The largest line number, which sets the width of the gutters.
    last_line: u32,
}

/// `text` with each tab replaced by the spaces that reach the next tab stop,
/// as a browser lays a tab out: a tab after three characters is five spaces,
/// not a fixed number, so indentation lines up with what an editor shows.
fn expand_tabs(text: &str, stop: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let spaces = stop - column % stop;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

impl Shown {
    fn of(diff: &FileDiff) -> Shown {
        let rows: Vec<DiffRow> = parse_unified_diff(&diff.text)
            .into_iter()
            .filter(|row| row.kind != DiffRowKind::Meta)
            .map(|mut row| {
                if row.text.contains('\t') {
                    row.text = expand_tabs(&row.text, TAB_STOP);
                }
                row
            })
            .collect();
        let widest = rows
            .iter()
            .map(|row| row.text.chars().count())
            .max()
            .unwrap_or(0);
        let last_line = rows
            .iter()
            .flat_map(|row| [row.old_line, row.new_line])
            .flatten()
            .max()
            .unwrap_or(0);
        Shown {
            rows,
            binary: diff.binary,
            truncated: diff.truncated,
            widest,
            last_line,
        }
    }

    /// There is something to read: a diff of nothing but its header (a mode
    /// change) has no lines.
    fn has_lines(&self) -> bool {
        !self.rows.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Scroll {
    y: f32,
    height: f32,
    /// The viewport's width, known once a scroll has said it.
    width: f32,
}

#[derive(Debug)]
pub struct DiffState {
    pub cwd: String,
    /// Who the changes belong to — a pane or a branch name — shown in the title.
    pub label: Option<String>,
    /// Which opening this is: an answer for another is not this one's.
    generation: u64,
    setup: Setup,
    /// `None` compares with HEAD; a branch name, with where this branch forked.
    base: Option<String>,
    changes: Option<Arc<Changes>>,
    error: Option<String>,
    loading: bool,
    /// Which listing is the latest: an older answer arriving late is dropped.
    pub(super) load_seq: u64,
    selected: Option<String>,
    /// Diffs already read for this listing, by base and file. A reload empties
    /// it, and answers still in flight from before it are dropped by their
    /// listing's number.
    cache: HashMap<String, Arc<Shown>>,
    shown: Option<(String, Arc<Shown>)>,
    diff_error: Option<(String, String)>,
    list_scroll: Scroll,
    diff_scroll: Scroll,
    /// The width of one character of code, measured once.
    char_width: Cell<Option<f32>>,
}

#[derive(Debug, Clone)]
pub enum DiffMsg {
    /// `git_repo_info` and, for a linked worktree, the main branch.
    Setup(u64, Box<(RepoInfo, Option<String>)>),
    /// The listing, for this opening and this listing's number.
    Listed(u64, u64, Result<Arc<Changes>, String>),
    /// One file's diff, with the key it is cached under.
    FileRead(u64, u64, String, Result<Arc<FileDiff>, String>),
    SetBase(Option<String>),
    Reload,
    Select(String),
    /// Up and down walk the list, as a file list in an editor does.
    Step(i32),
    ListScrolled {
        y: f32,
        height: f32,
    },
    DiffScrolled {
        y: f32,
        height: f32,
        width: f32,
    },
}

/// The key of a file in the list and in the cache: a rename is told from the
/// file it replaced.
fn file_key(file: &ChangedFile) -> String {
    format!(
        "{}\u{0}{}",
        file.old_path.as_deref().unwrap_or(""),
        file.path
    )
}

/// Letters in the list: untracked reads U, as in VS Code; git's own U — a
/// conflict — is shown as "!" so the two never share a letter.
fn status_mark(status: &str) -> &str {
    match status {
        "?" => "U",
        "U" => "!",
        other => other,
    }
}

/// The room a diff needs to be drawn: how many rows and how wide.
fn visible_range(scroll_y: f32, viewport: f32, rows: usize) -> (usize, usize) {
    let viewport = if viewport > 0.0 {
        viewport
    } else {
        FIRST_VIEWPORT
    };
    let first = ((scroll_y.max(0.0) / ROW).floor() as usize)
        .saturating_sub(OVERSCAN)
        .min(rows);
    let last = (((scroll_y.max(0.0) + viewport) / ROW).ceil() as usize + OVERSCAN).min(rows);
    (first, last.max(first))
}

impl DiffState {
    /// A modal for the changes of `cwd`, and the first thing it asks.
    pub fn open(cwd: String, label: Option<String>, generation: u64) -> (DiffState, Task<DiffMsg>) {
        let state = DiffState {
            cwd: cwd.clone(),
            label,
            generation,
            setup: Setup::Loading,
            base: None,
            changes: None,
            error: None,
            loading: false,
            load_seq: 0,
            selected: None,
            cache: HashMap::new(),
            shown: None,
            diff_error: None,
            list_scroll: Scroll::default(),
            diff_scroll: Scroll::default(),
            char_width: Cell::new(None),
        };
        let asked = Task::perform(setup(cwd), move |found| {
            DiffMsg::Setup(generation, Box::new(found))
        });
        (state, asked)
    }

    fn ready(&self) -> bool {
        matches!(&self.setup, Setup::Ready { info, .. } if info.installed && info.is_repo)
    }

    fn files(&self) -> &[ChangedFile] {
        self.changes
            .as_ref()
            .map(|changes| changes.files.as_slice())
            .unwrap_or_default()
    }

    fn current(&self) -> Option<&ChangedFile> {
        let wanted = self.selected.as_deref()?;
        self.files().iter().find(|file| file_key(file) == wanted)
    }

    /// The key the selected file's diff is cached under: `base␁file`.
    pub(super) fn wanted_key(&self) -> Option<String> {
        let file = self.current()?;
        Some(format!(
            "{}\u{1}{}",
            self.base.as_deref().unwrap_or(""),
            file_key(file)
        ))
    }

    /// Reads the listing again, from scratch.
    fn load(&mut self) -> Task<DiffMsg> {
        self.load_seq += 1;
        self.loading = true;
        self.error = None;
        self.cache.clear();
        let (generation, seq) = (self.generation, self.load_seq);
        let (cwd, base) = (self.cwd.clone(), self.base.clone());
        Task::perform(
            async move { git::git_changes(cwd, base).await.map(Arc::new) },
            move |answer| DiffMsg::Listed(generation, seq, answer),
        )
    }

    /// Makes the pane show the selected file's diff: from the cache, or by
    /// reading it.
    fn want_file(&mut self) -> Task<DiffMsg> {
        // Back to the top; the pane is as wide as it was.
        self.diff_scroll = Scroll {
            width: self.diff_scroll.width,
            ..Scroll::default()
        };
        let scrolled_back = operation::scroll_to(
            diff_id(),
            scrollable::AbsoluteOffset {
                x: Some(0.0),
                y: Some(0.0),
            },
        );
        let (Some(key), Some(file)) = (self.wanted_key(), self.current().cloned()) else {
            self.shown = None;
            self.diff_error = None;
            return scrolled_back;
        };
        self.diff_error = None;
        if let Some(known) = self.cache.get(&key) {
            self.shown = Some((key, known.clone()));
            return scrolled_back;
        }
        self.shown = None;
        let (generation, seq) = (self.generation, self.load_seq);
        let (cwd, base) = (self.cwd.clone(), self.base.clone());
        let asked = Task::perform(
            async move {
                git::git_file_diff(
                    cwd,
                    file.path.clone(),
                    file.old_path.clone(),
                    file.status == "?",
                    base,
                )
                .await
                .map(Arc::new)
            },
            move |answer| DiffMsg::FileRead(generation, seq, key.clone(), answer),
        );
        Task::batch([scrolled_back, asked])
    }

    fn select(&mut self, key: String) -> Task<DiffMsg> {
        if self.selected.as_ref() == Some(&key) {
            return Task::none();
        }
        self.selected = Some(key);
        Task::batch([self.want_file(), self.keep_selection_in_view()])
    }

    /// Scrolls the list so the selected file is on screen.
    fn keep_selection_in_view(&self) -> Task<DiffMsg> {
        let Some(at) = self
            .current()
            .and_then(|current| self.files().iter().position(|file| file == current))
        else {
            return Task::none();
        };
        let top = at as f32 * (FILE_ROW + 1.0);
        let bottom = top + FILE_ROW;
        let view = self.list_scroll;
        let height = if view.height > 0.0 {
            view.height
        } else {
            FIRST_VIEWPORT
        };
        let to = if top < view.y {
            top
        } else if bottom > view.y + height {
            bottom - height
        } else {
            return Task::none();
        };
        operation::scroll_to(
            list_id(),
            scrollable::AbsoluteOffset {
                x: Some(0.0),
                y: Some(to),
            },
        )
    }

    pub fn update(&mut self, msg: DiffMsg) -> Task<DiffMsg> {
        match msg {
            DiffMsg::Setup(generation, found) if generation == self.generation => {
                let (info, main_branch) = *found;
                self.base = main_branch.clone();
                self.setup = Setup::Ready { info, main_branch };
                if self.ready() {
                    return self.load();
                }
                Task::none()
            }
            DiffMsg::Listed(generation, seq, answer)
                if generation == self.generation && seq == self.load_seq =>
            {
                self.loading = false;
                match answer {
                    Ok(next) => {
                        // The selection stays while its file is still listed;
                        // otherwise the first takes it.
                        let keep = self
                            .selected
                            .as_ref()
                            .filter(|key| next.files.iter().any(|file| &file_key(file) == *key))
                            .cloned();
                        self.selected = keep.or_else(|| next.files.first().map(file_key));
                        self.changes = Some(next);
                        self.error = None;
                    }
                    Err(error) => {
                        self.changes = None;
                        self.error = Some(error);
                    }
                }
                self.want_file()
            }
            DiffMsg::FileRead(generation, seq, key, answer)
                if generation == self.generation && seq == self.load_seq =>
            {
                let wanted = self.wanted_key().as_ref() == Some(&key);
                match answer {
                    Ok(read) => {
                        let shown = Arc::new(Shown::of(&read));
                        self.cache.insert(key.clone(), shown.clone());
                        if wanted {
                            self.shown = Some((key, shown));
                        }
                    }
                    Err(error) if wanted => self.diff_error = Some((key, error)),
                    Err(_) => {}
                }
                Task::none()
            }
            DiffMsg::SetBase(base) => {
                if self.base == base {
                    return Task::none();
                }
                self.base = base;
                self.load()
            }
            DiffMsg::Reload => {
                if self.ready() && !self.loading {
                    self.load()
                } else {
                    Task::none()
                }
            }
            DiffMsg::Select(key) => self.select(key),
            DiffMsg::Step(delta) => {
                let files = self.files();
                if files.is_empty() {
                    return Task::none();
                }
                let at = self
                    .selected
                    .as_deref()
                    .and_then(|key| files.iter().position(|file| file_key(file) == key))
                    .map_or(-1, |at| at as i32);
                let to = (at + delta).clamp(0, files.len() as i32 - 1) as usize;
                let key = file_key(&files[to]);
                self.select(key)
            }
            DiffMsg::ListScrolled { y, height } => {
                self.list_scroll = Scroll {
                    y,
                    height,
                    width: 0.0,
                };
                Task::none()
            }
            DiffMsg::DiffScrolled { y, height, width } => {
                self.diff_scroll = Scroll { y, height, width };
                Task::none()
            }
            // An answer for an earlier opening or listing.
            DiffMsg::Setup(..) | DiffMsg::Listed(..) | DiffMsg::FileRead(..) => Task::none(),
        }
    }

    fn char_width(&self) -> f32 {
        if let Some(width) = self.char_width.get() {
            return width;
        }
        let width = text_width("0000000000", font_size::UI, fonts::mono()) / 10.0;
        self.char_width.set(Some(width));
        width
    }
}

/// What the setup learns: the folder's repository and, for a linked worktree,
/// the branch to compare with.
async fn setup(cwd: String) -> (RepoInfo, Option<String>) {
    let info = git::git_repo_info(cwd.clone()).await;
    let mut comparable = None;
    if info.is_repo && info.is_linked_worktree {
        let worktrees = git::git_worktree_list(cwd).await.unwrap_or_default();
        let main_branch = worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .and_then(|worktree| worktree.branch.clone());
        // Comparing a branch with itself is HEAD; offer it only when it differs.
        comparable = main_branch.filter(|main| Some(main) != info.branch.as_ref());
    }
    (info, comparable)
}

/// The width of `content` set in this size and face.
fn text_width(content: &str, size: f32, font: iced::Font) -> f32 {
    type Paragraph = <iced::Renderer as adv_text::Renderer>::Paragraph;
    Paragraph::with_text(adv_text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: adv_text::LineHeight::default(),
        font,
        align_x: adv_text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: adv_text::Shaping::Advanced,
        wrapping: adv_text::Wrapping::None,
    })
    .min_bounds()
    .width
}

fn diff_id() -> Id {
    Id::new("git-diff-table")
}

fn list_id() -> Id {
    Id::new("git-diff-files")
}

// ---- View -----------------------------------------------------------------------------------

/// The whole overlay: the veil and the modal. `wrap` makes the modal's
/// messages the owner's; `close` is what Escape, the veil and the cross send.
pub fn view<'a>(
    ds: &'a DiffState,
    ctx: Ctx<'a>,
    wrap: Wrap,
    close: Message,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let modal = modal(ds, ctx, wrap, close.clone());
    layer::modal(
        iced::widget::space::horizontal().height(Length::Fill),
        Some(modal),
    )
    .veil(style::container::scrim(t))
    .on_dismiss(close)
    .margin(40.0)
    .into()
}

/// The modal itself, without the veil: the worktree panel puts it where it
/// stood.
pub fn modal<'a>(
    ds: &'a DiffState,
    ctx: Ctx<'a>,
    wrap: Wrap,
    close: Message,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let tr = ctx.i18n;
    let (chrome, line, shadow) = (c.chrome, c.line_strong, c.shadow);

    let mut title = row![
        text(tr.t(Key::DiffTitle).into_owned())
            .size(font_size::BODY)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
    ]
    .spacing(space::LG)
    .align_y(Alignment::Center);
    if let Some(label) = &ds.label {
        title = title.push(
            tail_text(label.clone())
                .cut_end()
                .size(font_size::CAPTION_LG)
                .font(fonts::mono())
                .color(c.fg_subtle)
                .width(Length::Shrink),
        );
    }
    let head = container(
        row![
            container(title).width(Length::Fill),
            icon_button(t, Icon::Close)
                .side(28.0)
                .tip(format!("{} · Esc", tr.t(Key::DiffClose)))
                .on_press(close),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center),
    )
    .height(44.0)
    .padding(Padding {
        right: 8.0,
        left: 16.0,
        ..Padding::ZERO
    })
    .align_y(Alignment::Center);

    let body = column![
        head,
        rule::horizontal(BORDER_WIDTH).style(style::rule::hairline(t)),
        bar(ds, ctx, wrap),
        rule::horizontal(BORDER_WIDTH).style(style::rule::sep(t)),
        content(ds, ctx, wrap),
    ];
    container(body)
        .width(Length::Fill)
        .max_width(1240.0)
        .height(Length::Fill)
        .clip(true)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(chrome)),
            border: Border {
                color: line,
                width: 1.0,
                radius: radius::XL.into(),
            },
            shadow: iced::Shadow {
                color: shadow,
                offset: iced::Vector::new(0.0, 24.0),
                blur_radius: 60.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// The bar under the title: where, on which branch, against what, and the
/// way to read again.
fn bar<'a>(ds: &'a DiffState, ctx: Ctx<'a>, wrap: Wrap) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let tr = ctx.i18n;
    let info = match &ds.setup {
        Setup::Ready { info, .. } => Some(info),
        Setup::Loading => None,
    };
    let root = ds
        .changes
        .as_ref()
        .map(|changes| changes.root.clone())
        .or_else(|| info.and_then(|info| info.root.clone()))
        .unwrap_or_else(|| ds.cwd.clone());

    let mut line = row![
        container(tip::bottom(
            t,
            tail_text(root.clone())
                .size(font_size::CAPTION)
                .font(fonts::mono())
                .color(c.fg_subtle),
            root,
        ))
        .max_width(520.0)
    ]
    .spacing(space::LG)
    .align_y(Alignment::Center);

    if let Some(branch) = info.and_then(|info| info.branch.clone()) {
        let fill = c.line;
        line = line.push(
            container(
                text(branch)
                    .size(font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::muted(t)),
            )
            .padding(Padding::from([1.0, 6.0]))
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(fill)),
                border: Border {
                    radius: radius::XS.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
        );
    }

    // Two ways to compare, when the folder is a worktree on its own branch.
    if let Setup::Ready {
        main_branch: Some(main),
        ..
    } = &ds.setup
    {
        let since = tr
            .t_with(Key::DiffSinceBranch, &args!("branch" => main.as_str()))
            .into_owned();
        let hint = if ds.base.is_some() {
            tr.t_with(Key::DiffSinceBranchHint, &args!("branch" => main.as_str()))
                .into_owned()
        } else {
            tr.t(Key::DiffUncommittedHint).into_owned()
        };
        let main = main.clone();
        line = line.push(tip::bottom(
            t,
            segmented_labels(
                t,
                vec![
                    (false, tr.t(Key::DiffUncommitted).into_owned()),
                    (true, since),
                ],
                &ds.base.is_some(),
                move |since| wrap(DiffMsg::SetBase(since.then(|| main.clone()))),
            ),
            hint,
        ));
    }
    line = line.push(filler::horizontal());

    if let Some(changes) = &ds.changes
        && !changes.files.is_empty()
    {
        let (additions, deletions) = totals(&changes.files);
        let mut summary = row![
            text(tr.tn(Key::DiffFiles, changes.files.len(), &[]).into_owned())
                .size(font_size::CAPTION_LG)
                .style(style::text::muted(t)),
            text(format!("+{additions}"))
                .size(font_size::CAPTION_LG)
                .font(fonts::mono())
                .style(style::text::success(t)),
            text(format!("−{deletions}"))
                .size(font_size::CAPTION_LG)
                .font(fonts::mono())
                .style(style::text::danger(t)),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        if let Some(base) = &changes.base {
            summary = summary.push(
                text(
                    tr.t_with(Key::DiffAgainst, &args!("base" => base.as_str()))
                        .into_owned(),
                )
                .size(font_size::CAPTION_LG)
                .style(style::text::subtle(t)),
            );
        }
        line = line.push(summary);
    }

    if ds.ready() {
        line = line.push(
            button::ghost(t, tr.t(Key::DiffRefresh).into_owned())
                .small()
                .on_press_maybe((!ds.loading).then(|| wrap(DiffMsg::Reload))),
        );
    }

    let fill = c.pane_head;
    container(line)
        .padding(Padding::from([5.0, 12.0]).left(16.0))
        .height(38.0)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(fill)),
            ..container::Style::default()
        })
        .into()
}

/// The added and removed lines of a listing.
fn totals(files: &[ChangedFile]) -> (u32, u32) {
    files.iter().fold((0, 0), |(additions, deletions), file| {
        (
            additions + file.additions.unwrap_or(0),
            deletions + file.deletions.unwrap_or(0),
        )
    })
}

/// A centred line of the quiet colour: what the body says when it has no
/// files to show (`.diff__empty`).
fn empty<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    container(
        text(words)
            .size(font_size::UI_LG)
            .style(style::text::subtle(t))
            .align_x(iced::alignment::Horizontal::Center),
    )
    .padding(Padding::from([56.0, 16.0]))
    .width(Length::Fill)
    .align_x(Alignment::Center)
    .into()
}

/// An error in the danger colour over a faint wash of it (`.notice--error`).
pub(super) fn error_note<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    let c = &t.colors;
    let (edge, wash, ink) = (
        c.danger.scale_alpha(0.4),
        c.danger.scale_alpha(0.08),
        c.danger,
    );
    container(
        text(words)
            .size(font_size::UI)
            .style(move |_: &iced::Theme| text::Style { color: Some(ink) }),
    )
    .padding(Padding::from([6.0, 10.0]))
    .width(Length::Fill)
    .style(move |_: &iced::Theme| container::Style {
        background: Some(Background::Color(wash)),
        border: Border {
            color: edge,
            width: 1.0,
            radius: radius::MD.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// Below the bar: one of the states, or the files and their diff.
fn content<'a>(ds: &'a DiffState, ctx: Ctx<'a>, wrap: Wrap) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let state = match &ds.setup {
        Setup::Loading => Some(empty(t, tr.t(Key::DiffLoading).into_owned())),
        Setup::Ready { info, .. } if !info.installed => {
            Some(empty(t, tr.t(Key::DiffNoGit).into_owned()))
        }
        Setup::Ready { info, .. } if !info.is_repo => {
            Some(empty(t, tr.t(Key::DiffNotRepo).into_owned()))
        }
        Setup::Ready { .. } => match (&ds.error, &ds.changes) {
            (Some(error), _) => Some(container(error_note(t, error.clone())).padding(12).into()),
            (None, None) => Some(empty(t, tr.t(Key::DiffLoading).into_owned())),
            (None, Some(changes)) if changes.files.is_empty() => Some(empty(
                t,
                match &changes.base {
                    Some(base) => tr
                        .t_with(Key::DiffClean, &args!("base" => base.as_str()))
                        .into_owned(),
                    None => tr.t(Key::DiffCleanUnborn).into_owned(),
                },
            )),
            (None, Some(_)) => None,
        },
    };
    if let Some(state) = state {
        return container(state).height(Length::Fill).into();
    }

    let split = row![files(ds, ctx, wrap), diff_pane(ds, ctx, wrap)]
        .width(Length::Fill)
        .height(Length::Fill);
    // Up and down walk the list, wherever the pointer is in the modal.
    on_key(split, move |key, modifiers| {
        if !modifiers.is_empty() {
            return None;
        }
        match key {
            Keyboard::Named(Named::ArrowDown) => Some(wrap(DiffMsg::Step(1))),
            Keyboard::Named(Named::ArrowUp) => Some(wrap(DiffMsg::Step(-1))),
            _ => None,
        }
    })
}

/// The color of a status letter (`.diff__status--*`).
fn status_color(t: &Theme, status: &str) -> Color {
    let c = &t.colors;
    match status {
        "M" => c.warning,
        "A" | "?" => c.success,
        "D" | "U" => c.danger,
        "R" | "C" => c.accent_hi,
        _ => c.fg_subtle,
    }
}

/// The word for a status in the tooltip, or the letter itself when git said
/// something this version has no word for.
fn status_word(tr: Translator, status: &str) -> String {
    let key = match status {
        "M" => Key::DiffStatusM,
        "A" => Key::DiffStatusA,
        "D" => Key::DiffStatusD,
        "R" => Key::DiffStatusR,
        "C" => Key::DiffStatusC,
        "T" => Key::DiffStatusT,
        "U" => Key::DiffStatusU,
        "?" => Key::DiffStatus,
        other => return other.to_string(),
    };
    tr.t(key).into_owned()
}

/// The list of files (`.diff__files`).
fn files<'a>(ds: &'a DiffState, ctx: Ctx<'a>, wrap: Wrap) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let c = &t.colors;
    let listed = ds.files();
    let mut rows = column![].spacing(1);
    for file in listed {
        let key = file_key(file);
        let (dir, name) = split_path(&file.path);
        let selected = ds.selected.as_ref() == Some(&key);

        let mark = container(
            text(status_mark(&file.status).to_string())
                .size(font_size::LABEL)
                .font(fonts::mono())
                .style({
                    let ink = status_color(t, &file.status);
                    move |_: &iced::Theme| text::Style { color: Some(ink) }
                }),
        )
        .width(18.0)
        .align_x(Alignment::Center);
        // The folder gives way first, from its start; the name stays whole.
        let path = tail_text(dir)
            .suffix(name, c.fg)
            .size(font_size::UI)
            .color(c.fg_subtle);
        let mut line = row![tip::bottom(t, mark, status_word(tr, &file.status)), path,]
            .spacing(space::MD)
            .align_y(Alignment::Center);
        let counted = !file.binary && (file.additions.is_some() || file.deletions.is_some());
        if counted {
            let mut counts = row![].spacing(6);
            if let Some(added) = file.additions.filter(|n| *n > 0) {
                counts = counts.push(
                    text(format!("+{added}"))
                        .size(font_size::LABEL)
                        .font(fonts::mono())
                        .style(style::text::success(t)),
                );
            }
            if let Some(removed) = file.deletions.filter(|n| *n > 0) {
                counts = counts.push(
                    text(format!("−{removed}"))
                        .size(font_size::LABEL)
                        .font(fonts::mono())
                        .style(style::text::danger(t)),
                );
            }
            line = line.push(counts);
        }
        let hint = match &file.old_path {
            Some(old) => format!(
                "{} — {}",
                file.path,
                tr.t_with(Key::DiffRenamedFrom, &args!("path" => old.as_str()))
            ),
            None => file.path.clone(),
        };
        rows = rows.push(tip::bottom(
            t,
            list::row(t, line, selected)
                .height(FILE_ROW)
                .padding(Padding::from([0.0, 8.0]))
                .on_press(wrap(DiffMsg::Select(key))),
            hint,
        ));
    }
    if ds.changes.as_ref().is_some_and(|changes| changes.truncated) {
        rows = rows.push(
            container(
                text(
                    tr.t_with(Key::DiffTruncated, &args!("n" => listed.len()))
                        .into_owned(),
                )
                .size(font_size::LABEL)
                .style(style::text::warning(t)),
            )
            .padding(Padding::from([6.0, 8.0])),
        );
    }

    let rail = c.sep;
    let list = scrollable(container(rows).padding(6).width(Length::Fill))
        .id(list_id())
        .on_scroll(move |viewport| {
            wrap(DiffMsg::ListScrolled {
                y: viewport.absolute_offset().y,
                height: viewport.bounds().height,
            })
        })
        .style(style::scrollable::default(t))
        .height(Length::Fill);
    container(row![
        container(list).width(Length::Fill),
        container(filler::horizontal())
            .width(1.0)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(rail)),
                ..container::Style::default()
            }),
    ])
    .width(Length::FillPortion(3))
    .into()
}

/// The pane of the selected file's diff (`.diff__view`).
fn diff_pane<'a>(ds: &'a DiffState, ctx: Ctx<'a>, wrap: Wrap) -> Element<'a, Message> {
    let (t, tr) = (ctx.theme, ctx.i18n);
    let c = &t.colors;
    let page = c.bg;
    let inner: Element<'a, Message> = if ds.current().is_none() {
        empty(t, tr.t(Key::DiffPick).into_owned())
    } else if let Some((_, error)) = &ds.diff_error {
        container(error_note(t, error.clone())).padding(12).into()
    } else if let Some((_, shown)) = &ds.shown {
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if shown.truncated {
            parts.push(note(t, tr.t(Key::DiffDiffTruncated).into_owned()));
        }
        if shown.binary {
            parts.push(empty(t, tr.t(Key::DiffBinary).into_owned()));
        } else if !shown.has_lines() {
            parts.push(empty(t, tr.t(Key::DiffNoText).into_owned()));
        } else {
            parts.push(table(ds, ctx, wrap, shown));
        }
        column(parts).height(Length::Fill).into()
    } else {
        empty(t, tr.t(Key::DiffLoading).into_owned())
    };
    container(inner)
        .width(Length::FillPortion(7))
        .height(Length::Fill)
        .clip(true)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(page)),
            ..container::Style::default()
        })
        .into()
}

/// A line over the diff saying it is not whole (`.diff__note`).
fn note<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    let c = &t.colors;
    let (wash, ink) = (c.warning.scale_alpha(0.08), c.warning);
    container(
        text(words)
            .size(font_size::CAPTION)
            .style(move |_: &iced::Theme| text::Style { color: Some(ink) }),
    )
    .padding(Padding::from([6.0, 12.0]))
    .width(Length::Fill)
    .style(move |_: &iced::Theme| container::Style {
        background: Some(Background::Color(wash)),
        ..container::Style::default()
    })
    .into()
}

/// The diff's lines, as the rows in view between two spacers that stand for
/// the rest.
fn table<'a>(
    ds: &'a DiffState,
    ctx: Ctx<'a>,
    wrap: Wrap,
    shown: &'a Shown,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let char_w = ds.char_width();
    // The gutters hold the widest line number the diff has, and at least three.
    let digits = shown.last_line.max(1).ilog10() as usize + 1;
    let number_w = digits.max(3) as f32 * char_w * (font_size::CAPTION / font_size::UI) + 20.0;
    let sign_w = 4.0 + char_w + 8.0;
    // The rows reach the pane's right edge, so that the tint of an added line
    // is a band and not a block: the pane's width, which a scroll has told or
    // else what the modal gives it (seven tenths of it, less a scrollbar).
    let pane = if ds.diff_scroll.width > 0.0 {
        ds.diff_scroll.width
    } else {
        ((ctx.metrics.body_width - 80.0).min(1240.0) * 0.7 - 14.0).max(300.0)
    };
    let total = (2.0 * number_w + sign_w + 10.0 + shown.widest as f32 * char_w + 10.0).max(pane);

    let (first, last) = visible_range(ds.diff_scroll.y, ds.diff_scroll.height, shown.rows.len());
    let mut lines = column![filler::vertical().height(first as f32 * ROW)].width(total);
    for at in first..last {
        lines = lines.push(diff_row(t, &shown.rows[at], number_w, sign_w, total));
    }
    lines = lines.push(filler::vertical().height((shown.rows.len() - last) as f32 * ROW));

    scrollable(lines)
        .id(diff_id())
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .on_scroll(move |viewport| {
            wrap(DiffMsg::DiffScrolled {
                y: viewport.absolute_offset().y,
                height: viewport.bounds().height,
                width: viewport.bounds().width,
            })
        })
        .style(style::scrollable::default(t))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One line of the diff: old number, new number, sign, code.
fn diff_row<'a>(
    t: &'a Theme,
    row_of: &'a DiffRow,
    number_w: f32,
    sign_w: f32,
    total: f32,
) -> Element<'a, Message> {
    let c = &t.colors;
    let (wash, sign, sign_ink) = match row_of.kind {
        DiffRowKind::Add => (Some(c.success.scale_alpha(0.11)), "+", c.success),
        DiffRowKind::Del => (Some(c.danger.scale_alpha(0.11)), "−", c.danger),
        DiffRowKind::Hunk => (Some(c.accent.scale_alpha(0.08)), "", c.fg_subtle),
        DiffRowKind::Ctx | DiffRowKind::Note | DiffRowKind::Meta => (None, "", c.fg_subtle),
    };
    let code_ink = match row_of.kind {
        DiffRowKind::Hunk | DiffRowKind::Note => c.fg_subtle,
        _ => c.fg,
    };
    let number = |n: Option<u32>| -> Element<'a, Message> {
        container(
            text(n.map(|n| n.to_string()).unwrap_or_default())
                .size(font_size::CAPTION)
                .font(fonts::mono())
                .style(style::text::subtle(t))
                .wrapping(text::Wrapping::None),
        )
        .width(number_w)
        .padding(Padding::ZERO.right(10.0))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    };
    let line = row![
        number(row_of.old_line),
        number(row_of.new_line),
        container(text(sign).size(font_size::UI).font(fonts::mono()).style(
            move |_: &iced::Theme| text::Style {
                color: Some(sign_ink)
            }
        ))
        .width(sign_w)
        .padding(Padding::ZERO.left(8.0)),
        text(row_of.text.as_str())
            .size(font_size::UI)
            .font(fonts::mono())
            .style(move |_: &iced::Theme| text::Style {
                color: Some(code_ink)
            })
            .wrapping(text::Wrapping::None),
    ]
    .align_y(Alignment::Center);
    container(line)
        .width(total)
        .height(ROW)
        .align_y(Alignment::Center)
        .style(move |_: &iced::Theme| container::Style {
            background: wash.map(Background::Color),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests;
