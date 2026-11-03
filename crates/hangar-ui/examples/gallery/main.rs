//! Every widget of hangar-ui on one window, with a theme picker over all 33
//! themes, so that a person can look at them.
//!
//!     cargo run -p hangar-ui --example gallery
//!
//! Ctrl+K opens the command palette, which lists the pages and the themes.
//!
//! To look without a window (what the screenshots of the review are made of):
//!
//!     cargo run -p hangar-ui --example gallery -- --snapshot out/ [--theme id] [--page name] [--open menu]
//!
//! writes a PNG per page (or the one asked for), drawn headless by `iced_test`.

mod pages;
mod snapshot;

use std::time::Instant;

use hangar_theme::{AgentState, Theme, by_id_or_default, fonts, themes, to_iced};
use hangar_ui::button;
use hangar_ui::dialog::{self, dialog as make_dialog};
use hangar_ui::glyph::GlyphState;
use hangar_ui::input::NumberField;
use hangar_ui::list;
use hangar_ui::palette::{self, Detail, Item, Labels, Palette};
use hangar_ui::shortcut::{Input, Outcome, Recorder, write_binding};
use hangar_ui::split::{self, Arrange};
use hangar_ui::tabs::Tab;
use hangar_ui::toast::{self, Toast, Toasts};
use hangar_ui::{Icon, icon, input, menu, text as typography};
use iced::widget::{column, container, row, scrollable, text_editor};
use iced::{Alignment, Element, Length, Padding, Size, Subscription, Task, Vector, keyboard};

/// The pages of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Buttons,
    Inputs,
    Overlays,
    Layout,
    Form,
    Icons,
}

impl Page {
    pub const ALL: [Page; 6] = [
        Page::Buttons,
        Page::Inputs,
        Page::Overlays,
        Page::Layout,
        Page::Form,
        Page::Icons,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Page::Buttons => "Buttons",
            Page::Inputs => "Inputs",
            Page::Overlays => "Menus and dialogs",
            Page::Layout => "Layout pieces",
            Page::Form => "Settings form",
            Page::Icons => "Icons",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Page::Buttons => "buttons",
            Page::Inputs => "inputs",
            Page::Overlays => "overlays",
            Page::Layout => "layout",
            Page::Form => "form",
            Page::Icons => "icons",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Page::Buttons => Icon::Bolt,
            Page::Inputs => Icon::Pencil,
            Page::Overlays => Icon::Window,
            Page::Layout => Icon::Layout4,
            Page::Form => Icon::Sliders,
            Page::Icons => Icon::Cube,
        }
    }
}

/// What the palette runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Page(Page),
    Theme(&'static str),
    Dialog,
    Toast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(Page),
    Theme(&'static str),
    ThemeByName(String),
    Noop,
    // Inputs
    Text(String),
    Search(String),
    Number(hangar_ui::input::NumberMsg),
    Area(text_editor::Action),
    Shell(&'static str),
    Check(bool),
    Switch(bool),
    Slider(f32),
    Mode(Mode),
    // Shortcuts
    Record,
    RecorderInput(Input),
    RecorderConfirm,
    RecorderCancel,
    RemoveBinding(usize),
    ResetBindings,
    // Menus and dialogs
    ToggleMenu,
    CloseMenu,
    Picked(&'static str),
    OpenContext(Vector),
    CloseContext,
    OpenDialog,
    CloseDialog,
    DialogName(String),
    DialogSave,
    OpenPalette,
    Palette(palette::Msg),
    Toast(u8),
    ToastTick(Instant),
    ToastDismiss(u64),
    // Layout
    Fold(usize),
    Tab(u8),
    CloseTab(u8),
    Row(usize),
    Split(f32),
    SplitReset,
    // Form
    Scale(f32),
    ResetScale,
}

pub struct Gallery {
    pub theme: &'static Theme,
    pub page: Page,
    pub text: String,
    pub search: String,
    pub number: NumberField,
    pub area: text_editor::Content,
    pub shell: &'static str,
    pub check: bool,
    pub switch: bool,
    pub slider: f32,
    pub mode: Mode,
    pub bindings: Vec<String>,
    pub recorder: Option<Recorder>,
    pub menu_open: bool,
    pub context: Option<Vector>,
    pub dialog: bool,
    pub dialog_name: String,
    pub palette: Option<Palette<Target>>,
    pub toasts: Toasts<Message>,
    pub last: String,
    pub folded: [bool; 2],
    pub tab: u8,
    pub tabs_open: Vec<Tab<u8>>,
    pub row: usize,
    pub split: f32,
    pub scale: f32,
    pub labels: Labels,
}

impl Default for Gallery {
    fn default() -> Self {
        Gallery::new()
    }
}

impl Gallery {
    pub fn new() -> Gallery {
        Gallery {
            theme: by_id_or_default(hangar_theme::DEFAULT_ID),
            page: Page::Buttons,
            text: String::from("storefront"),
            search: String::new(),
            number: NumberField::new(16.0, 1.0, 64.0, 1.0),
            area: text_editor::Content::with_text(
                "Review the diff before merging.\nKeep the commit messages in the imperative.",
            ),
            shell: "bash",
            check: true,
            switch: true,
            slider: 100.0,
            mode: Mode::System,
            bindings: vec!["Ctrl+K Ctrl+S".to_string(), "F6".to_string()],
            recorder: None,
            menu_open: false,
            context: None,
            dialog: false,
            dialog_name: String::from("storefront"),
            palette: None,
            toasts: Toasts::new(),
            last: String::from("Nothing picked yet."),
            folded: [false, true],
            tab: 1,
            tabs_open: vec![
                Tab::new(1, "scene.glb").icon(Icon::Cube).closable(),
                Tab::new(2, "report.pdf").icon(Icon::Table).closable(),
                Tab::new(3, "notes").closable(),
            ],
            row: 1,
            split: 0.35,
            scale: 100.0,
            labels: Labels {
                anywhere: Some(String::from("Ctrl+K from anywhere")),
                ..Labels::default()
            },
        }
    }

    fn set_theme(&mut self, id: &str) {
        self.theme = by_id_or_default(id);
    }

    pub fn open_palette(&mut self) {
        let mut items = vec![
            Item::new(Target::Dialog, "Actions", "Open the rename dialog")
                .icon(Icon::Pencil)
                .keywords(["modal", "form"]),
            Item::new(Target::Toast, "Actions", "Show a toast")
                .icon(Icon::Bell)
                .keys("Ctrl+T"),
        ];
        for page in Page::ALL {
            items.push(
                Item::new(
                    Target::Page(page),
                    "Pages",
                    format!("Go to {}", page.name()),
                )
                .icon(page.icon()),
            );
        }
        let states = [
            ("Ben", "Claude Code", AgentState::Waiting, "waiting"),
            ("Ava", "Claude Code", AgentState::Working, "working"),
            ("Zoe", "Shell", AgentState::Idle, "idle"),
        ];
        for (name, agent, state, word) in states {
            items.push(
                Item::new(Target::Page(Page::Layout), "Panes", format!("Go to {name}"))
                    .state(state.into())
                    .detail(
                        Detail::new()
                            .plain(format!("{agent} · "))
                            .state(state, word),
                    )
                    .keywords([agent]),
            );
        }
        for theme in themes() {
            items.push(
                Item::new(
                    Target::Theme(theme.id),
                    "Themes",
                    format!("Theme: {}", theme.name),
                )
                .icon(Icon::Appearance),
            );
        }
        self.palette = Some(Palette::new(items));
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Page(page) => self.page = page,
            Message::Theme(id) => self.set_theme(id),
            Message::ThemeByName(name) => {
                if let Some(theme) = themes().iter().find(|theme| theme.name == name) {
                    self.theme = theme;
                }
            }
            Message::Noop => {}
            Message::Text(text) => self.text = text,
            Message::Search(search) => self.search = search,
            Message::Number(message) => {
                self.number.update(message);
            }
            Message::Area(action) => self.area.perform(action),
            Message::Shell(shell) => self.shell = shell,
            Message::Check(on) => self.check = on,
            Message::Switch(on) => self.switch = on,
            Message::Slider(value) => self.slider = value,
            Message::Mode(mode) => self.mode = mode,
            Message::Record => self.recorder = Some(Recorder::new()),
            Message::RecorderInput(input) => {
                if let Some(recorder) = &mut self.recorder {
                    match recorder.input(input) {
                        Outcome::Done(chords) => {
                            self.bindings.push(write_binding(&chords));
                            self.recorder = None;
                        }
                        Outcome::Cancelled => self.recorder = None,
                        Outcome::Pending => {}
                    }
                }
            }
            Message::RecorderConfirm => {
                if let Some(chords) = self.recorder.take().and_then(|r| r.confirm()) {
                    self.bindings.push(write_binding(&chords));
                }
            }
            Message::RecorderCancel => self.recorder = None,
            Message::RemoveBinding(at) => {
                if at < self.bindings.len() {
                    self.bindings.remove(at);
                }
            }
            Message::ResetBindings => {
                self.bindings = vec!["Ctrl+K Ctrl+S".to_string(), "F6".to_string()];
            }
            Message::ToggleMenu => self.menu_open = !self.menu_open,
            Message::CloseMenu => self.menu_open = false,
            Message::Picked(what) => {
                self.last = format!("Picked “{what}”.");
                self.menu_open = false;
                self.context = None;
            }
            Message::OpenContext(at) => self.context = Some(at),
            Message::CloseContext => self.context = None,
            Message::OpenDialog => self.dialog = true,
            Message::CloseDialog => self.dialog = false,
            Message::DialogName(name) => self.dialog_name = name,
            Message::DialogSave => {
                self.dialog = false;
                let name = self.dialog_name.clone();
                self.toasts.push(
                    Toast::success("Workspace renamed").detail(name),
                    Instant::now(),
                );
            }
            Message::OpenPalette => {
                self.open_palette();
                // The palette takes the keyboard by itself when it opens.
            }
            Message::Palette(message) => {
                let Some(open) = &mut self.palette else {
                    return Task::none();
                };
                let (event, task) = open.update(message);
                match event {
                    palette::Event::Run(target) => {
                        self.palette = None;
                        match target {
                            Target::Page(page) => self.page = page,
                            Target::Theme(id) => self.set_theme(id),
                            Target::Dialog => self.dialog = true,
                            Target::Toast => {
                                self.toasts.push(
                                    Toast::info("Hello from the palette")
                                        .detail("It ran a command."),
                                    Instant::now(),
                                );
                            }
                        }
                    }
                    palette::Event::Dismiss => self.palette = None,
                    palette::Event::None => {}
                }
                return task.map(Message::Palette);
            }
            Message::Toast(kind) => {
                let now = Instant::now();
                let toast = match kind {
                    0 => Toast::info("Pane restarted").detail("Ben is back."),
                    1 => Toast::success("Delivery merged"),
                    2 => Toast::warning("The context is 92 % full"),
                    _ => Toast::error("Could not start the agent")
                        .detail("claude: command not found")
                        .action("Details", Message::Noop),
                };
                self.toasts.push(toast, now);
            }
            Message::ToastTick(now) => self.toasts.tick(now),
            Message::ToastDismiss(id) => self.toasts.dismiss(id),
            Message::Fold(at) => self.folded[at] = !self.folded[at],
            Message::Tab(id) => self.tab = id,
            Message::CloseTab(id) => {
                self.tabs_open.retain(|open| open.id != id);
                if self.tab == id {
                    self.tab = self.tabs_open.first().map_or(0, |open| open.id);
                }
            }
            Message::Row(at) => self.row = at,
            Message::Split(ratio) => self.split = ratio,
            Message::SplitReset => self.split = 0.35,
            Message::Scale(value) => self.scale = value,
            Message::ResetScale => self.scale = 100.0,
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let t = self.theme;

        // The bar: the title, and the theme picker over all 33 themes.
        let names: Vec<String> = themes()
            .iter()
            .map(|theme| theme.name.to_string())
            .collect();
        let picker = container(input::select(
            t,
            names,
            Some(t.name.to_string()),
            Message::ThemeByName,
        ))
        .width(220.0);
        let bar = container(
            row![
                icon(Icon::Layout4, 16.0).color(t.colors.accent_hi),
                typography::title(t, "Hangar UI"),
                typography::muted(t, "gallery"),
                iced::widget::space::horizontal(),
                typography::muted(t, if t.is_dark() { "dark" } else { "light" }),
                picker,
                button::icon_button(t, Icon::Search)
                    .tip("Command palette")
                    .keys("Ctrl+K")
                    .on_press(Message::OpenPalette),
            ]
            .spacing(hangar_theme::tokens::space::LG)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0.0, 12.0]))
        .height(hangar_theme::tokens::size::TOOLBAR_HEIGHT)
        .align_y(Alignment::Center)
        .style(hangar_theme::style::container::toolbar(t));

        let sidebar = container(
            column(Page::ALL.iter().map(|page| {
                list::row(
                    t,
                    row![icon(page.icon(), 14.0), typography::ui(t, page.name())]
                        .spacing(hangar_theme::tokens::space::MD)
                        .align_y(Alignment::Center),
                    *page == self.page,
                )
                .on_press(Message::Page(*page))
                .into()
            }))
            .spacing(2),
        )
        .padding(8)
        .width(hangar_theme::tokens::size::SIDEBAR_WIDTH)
        .height(Length::Fill)
        .style(hangar_theme::style::container::sidebar(t));

        let content = scrollable(pages::view(self))
            .style(hangar_theme::style::scrollable::default(t))
            .width(Length::Fill)
            .height(Length::Fill);

        let body = split::split(t, sidebar, content)
            .arrange(Arrange::Columns)
            .ratio(0.2)
            .min_first(180.0)
            .min_second(500.0);
        let page: Element<'_, Message> = container(column![
            bar,
            iced::widget::rule::horizontal(1).style(hangar_theme::style::rule::sep(t)),
            Element::from(body),
        ])
        .style(hangar_theme::style::container::window(t))
        .into();

        let with_palette = palette::over(
            t,
            page,
            self.palette
                .as_ref()
                .map(|open| open.view(t, &self.labels).map(Message::Palette)),
            Message::Palette(palette::Msg::Dismiss),
        );
        let with_dialog = dialog::over(t, with_palette, self.dialog.then(|| self.rename_dialog()));
        toast::over(
            t,
            Element::from(with_dialog),
            &self.toasts,
            Message::ToastDismiss,
        )
    }

    fn rename_dialog(&self) -> dialog::Dialog<'_, Message> {
        let t = self.theme;
        make_dialog(
            t,
            "Rename workspace",
            column![
                input::text_field(t, &self.dialog_name)
                    .label("Name")
                    .hint("Shown in the navigator.")
                    .error(
                        self.dialog_name
                            .trim()
                            .is_empty()
                            .then_some("A name is required.")
                    )
                    .on_input(Message::DialogName)
                    .on_submit(Message::DialogSave),
                input::select(t, ["bash", "zsh", "fish"], Some(self.shell), Message::Shell),
            ]
            .spacing(hangar_theme::tokens::space::XL),
        )
        .subtitle("~/dev/storefront")
        .action(button::ghost(t, "Cancel").on_press(Message::CloseDialog))
        .action(
            button::primary(t, "Save").icon(Icon::Check).on_press_maybe(
                (!self.dialog_name.trim().is_empty()).then_some(Message::DialogSave),
            ),
        )
        .on_close(Message::CloseDialog)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            keyboard::listen().filter_map(|event| match event {
                keyboard::Event::KeyPressed {
                    key: keyboard::Key::Character(c),
                    modifiers,
                    ..
                } if modifiers.control() && c.as_str().eq_ignore_ascii_case("k") => {
                    Some(Message::OpenPalette)
                }
                _ => None,
            }),
            self.toasts.subscription().map(Message::ToastTick),
        ])
    }
}

fn main() -> iced::Result {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--snapshot") {
        let rest: Vec<String> = args.collect();
        return snapshot::run(&rest);
    }
    iced::application(Gallery::new, Gallery::update, Gallery::view)
        .title("Hangar UI gallery")
        .theme(|gallery: &Gallery| to_iced(gallery.theme))
        .subscription(Gallery::subscription)
        .font(fonts::INTER_VARIABLE)
        .default_font(fonts::ui())
        .window_size(Size::new(1180.0, 800.0))
        .run()
}

// The glyph states, for the pages.
pub const GLYPHS: [(GlyphState, &str); 6] = [
    (GlyphState::Working, "working"),
    (GlyphState::Waiting, "waiting"),
    (GlyphState::Yours, "your turn"),
    (GlyphState::Idle, "idle"),
    (GlyphState::Starting, "starting"),
    (GlyphState::Exited, "exited"),
];

/// What the menu pages show.
pub fn demo_entries() -> Vec<menu::Entry<Message>> {
    vec![
        menu::heading("Pane"),
        menu::item("Split right")
            .icon(Icon::SplitRight)
            .shortcut("Ctrl+D")
            .on_select(Message::Picked("Split right"))
            .into(),
        menu::item("Split down")
            .icon(Icon::SplitDown)
            .shortcut("Ctrl+Shift+D")
            .on_select(Message::Picked("Split down"))
            .into(),
        menu::item("Restart")
            .icon(Icon::Restart)
            .on_select(Message::Picked("Restart"))
            .into(),
        menu::separator(),
        menu::item("Sleep")
            .icon(Icon::Moon)
            .disabled_because("Needs a running agent")
            .on_select(Message::Picked("Sleep"))
            .into(),
        menu::item("Compact view")
            .checked(true)
            .on_select(Message::Picked("Compact view"))
            .into(),
        menu::item("Waiting")
            .glyph(GlyphState::Waiting)
            .on_select(Message::Picked("Waiting"))
            .into(),
        menu::separator(),
        menu::item("Close pane")
            .icon(Icon::Close)
            .danger()
            .shortcut("Ctrl+W")
            .on_select(Message::Picked("Close pane"))
            .action(Icon::Pencil, "Rename", Message::Picked("Rename"))
            .into(),
    ]
}
