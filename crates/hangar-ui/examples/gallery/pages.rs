//! The pages of the gallery: one function each, built only from hangar-ui.

use hangar_theme::tokens::space;
use hangar_theme::{Theme, style};
use hangar_ui::badge::{self, Tone};
use hangar_ui::button;
use hangar_ui::empty;
use hangar_ui::form;
use hangar_ui::glyph::{self, GlyphState};
use hangar_ui::keycap;
use hangar_ui::list;
use hangar_ui::panel;
use hangar_ui::section;
use hangar_ui::shortcut_field::shortcut_field;
use hangar_ui::split::{self, Arrange};
use hangar_ui::tabs;
use hangar_ui::{Icon, icon, input, menu, text as typography, tip};
use iced::widget::{Row, column, container, row, rule, space as iced_space, text};
use iced::{Alignment, Element, Length, Padding};

use crate::{GLYPHS, Gallery, Message, Mode, Page, demo_entries};

pub fn view(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;
    let body = match g.page {
        Page::Buttons => buttons(g),
        Page::Inputs => inputs(g),
        Page::Overlays => overlays(g),
        Page::Layout => layout(g),
        Page::Form => settings(g),
        Page::Icons => icons(g),
    };
    container(
        column![typography::title(t, g.page.name()).size(20.0), body]
            .spacing(space::XXXL)
            .max_width(900.0),
    )
    .padding(space::XXXL)
    .width(Length::Fill)
    .into()
}

/// A titled block of examples.
fn block<'a>(
    t: &'a Theme,
    title: &str,
    note: Option<&str>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut head = column![typography::heading(t, title.to_string()).size(12.0)].spacing(2);
    if let Some(note) = note {
        head = head.push(typography::hint(t, note.to_string()));
    }
    column![
        head,
        rule::horizontal(1).style(style::rule::hairline(t)),
        content.into()
    ]
    .spacing(space::LG)
    .into()
}

fn wrap<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    Row::with_children(items)
        .spacing(space::LG)
        .align_y(Alignment::Center)
        .wrap()
        .into()
}

// ---- Buttons -----------------------------------------------------------------------------------

fn buttons(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;
    let noop = Message::Noop;
    let kinds = wrap(vec![
        button::primary(t, "Save")
            .icon(Icon::Check)
            .on_press(noop.clone())
            .into(),
        button::secondary(t, "Restart")
            .icon(Icon::Restart)
            .on_press(noop.clone())
            .into(),
        button::ghost(t, "Cancel").on_press(noop.clone()).into(),
        button::danger(t, "Delete")
            .icon(Icon::Trash)
            .on_press(noop.clone())
            .into(),
        button::secondary(t, "Options")
            .icon_after(Icon::ChevronDown)
            .on_press(noop.clone())
            .into(),
    ]);
    let small = wrap(vec![
        button::primary(t, "Apply")
            .small()
            .on_press(noop.clone())
            .into(),
        button::secondary(t, "Edit")
            .small()
            .icon(Icon::Pencil)
            .on_press(noop.clone())
            .into(),
        button::ghost(t, "Skip")
            .small()
            .on_press(noop.clone())
            .into(),
        button::danger(t, "Remove")
            .small()
            .on_press(noop.clone())
            .into(),
    ]);
    let disabled = wrap(vec![
        button::primary(t, "Save").on_press_maybe(None).into(),
        button::secondary(t, "Restart").on_press_maybe(None).into(),
        button::ghost(t, "Cancel").on_press_maybe(None).into(),
        button::danger(t, "Delete").on_press_maybe(None).into(),
    ]);
    let icons_row = wrap(vec![
        button::icon_button(t, Icon::Gear)
            .tip("Settings")
            .keys("Ctrl+,")
            .on_press(noop.clone())
            .into(),
        button::icon_button(t, Icon::Plus)
            .tip("New terminal")
            .on_press(noop.clone())
            .into(),
        button::icon_button(t, Icon::Trash)
            .tip("Delete")
            .danger()
            .on_press(noop.clone())
            .into(),
        button::icon_button(t, Icon::More)
            .tip("More")
            .small()
            .on_press(noop.clone())
            .into(),
        button::icon_button(t, Icon::Search)
            .on_press_maybe(None)
            .into(),
    ]);
    let toggles = wrap(vec![
        button::toggle_button(t, Icon::SidebarLeft, g.switch)
            .tip("Navigator")
            .on_press(Message::Switch(!g.switch))
            .into(),
        button::toggle_button(t, Icon::SidebarRight, !g.switch)
            .tip("Inspector")
            .on_press(Message::Switch(!g.switch))
            .into(),
        button::toggle_button(t, Icon::Broadcast, false)
            .tip("Broadcast")
            .on_press(noop.clone())
            .into(),
    ]);
    let tabs_row = container(
        row![
            button::tab_button(t, "Terminals", g.mode == Mode::Dark)
                .icon(Icon::Terminal)
                .on_press(Message::Mode(Mode::Dark)),
            button::tab_button(t, "Board", g.mode == Mode::Light)
                .icon(Icon::Board)
                .on_press(Message::Mode(Mode::Light)),
            button::tab_button(t, "Memory", g.mode == Mode::System)
                .icon(Icon::Bookmark)
                .on_press(Message::Mode(Mode::System)),
        ]
        .height(30.0),
    )
    .style(style::container::pane_head(t, false));
    let links = wrap(vec![
        typography::ui(t, "Read the").into(),
        button::link(t, "documentation", noop.clone()),
        typography::ui(t, "for more.").into(),
    ]);
    column![
        block(
            t,
            "Kinds",
            Some("Primary is the one action of a dialog."),
            kinds
        ),
        block(
            t,
            "Small",
            Some("22 px, for a row of actions inside a list item."),
            small
        ),
        block(
            t,
            "Disabled",
            Some("A button without a message is faded and does nothing."),
            disabled
        ),
        block(t, "Icon buttons", Some("Hover for the tooltip."), icons_row),
        block(t, "Toggles", Some("Pressed in while on."), toggles),
        block(t, "Tabs", None, tabs_row),
        block(t, "Link", None, links),
    ]
    .spacing(space::XXXL)
    .into()
}

// ---- Inputs ------------------------------------------------------------------------------------

fn inputs(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;
    let fields = column![
        input::text_field(t, &g.text)
            .label("Name")
            .hint("Shown in the navigator.")
            .placeholder("Workspace name")
            .on_input(Message::Text),
        input::text_field(t, "")
            .label("Command")
            .error(Some("A command is required."))
            .mono()
            .on_input(Message::Text),
        input::text_field(t, "off")
            .label("Disabled")
            .hint("No handler, no edits."),
        input::search_field(t, &g.search, "Search settings").on_input(Message::Search),
        row![
            container(typography::label(t, "Scrollback (thousands)")).width(200.0),
            container(input::number_input(t, &g.number, Message::Number)).width(160.0),
        ]
        .align_y(Alignment::Center),
        input::textarea(t, &g.area, Message::Area),
    ]
    .spacing(space::XL)
    .max_width(420.0);

    let choices = column![
        row![
            container(typography::label(t, "Shell")).width(120.0),
            container(input::select(
                t,
                ["bash", "zsh", "fish"],
                Some(g.shell),
                Message::Shell
            ))
            .width(200.0),
        ]
        .align_y(Alignment::Center),
        input::checkbox(t, "Restore panes on start", g.check).on_toggle(Message::Check),
        input::checkbox(t, "Disabled box", true),
        input::switch(t, g.switch)
            .label("Notify me when an agent is waiting")
            .on_toggle(Message::Switch),
        input::switch(t, false).label("Disabled switch"),
        input::slider(t, 90.0..=130.0, g.slider, Message::Slider)
            .step(5.0)
            .display(format!("{:.0} %", g.slider)),
        input::segmented(
            t,
            &[
                (Mode::Dark, "Dark"),
                (Mode::Light, "Light"),
                (Mode::System, "System"),
            ],
            &g.mode,
            Message::Mode,
        ),
    ]
    .spacing(space::XL)
    .max_width(420.0);

    let keys = column![
        row![
            container(typography::label(t, "At rest")).width(120.0),
            shortcut_field(t, &g.bindings, None)
                .on_record(Message::Record)
                .on_remove(Message::RemoveBinding)
                .on_reset(Message::ResetBindings)
                .customized(g.bindings.len() != 2),
        ]
        .align_y(Alignment::Center),
        row![
            container(typography::label(t, "Recording")).width(120.0),
            shortcut_field(t, &g.bindings, g.recorder.as_ref())
                .on_record(Message::Record)
                .on_input(Message::RecorderInput)
                .on_confirm(Message::RecorderConfirm)
                .on_cancel(Message::RecorderCancel),
        ]
        .align_y(Alignment::Center),
        typography::hint(
            t,
            "Press the plus, then a chord. Enter confirms, Escape cancels."
        ),
    ]
    .spacing(space::XL);

    column![
        block(t, "Text", None, fields),
        block(t, "Choosing", None, choices),
        block(
            t,
            "Shortcut recorder",
            Some("Captures the whole keyboard while recording."),
            keys
        ),
    ]
    .spacing(space::XXXL)
    .into()
}

// ---- Menus and dialogs --------------------------------------------------------------------------

fn overlays(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;
    let anchor = button::secondary(t, "Pane menu")
        .icon_after(Icon::ChevronDown)
        .on_press(Message::ToggleMenu);
    let dropdown =
        menu::dropdown(t, anchor, g.menu_open.then(demo_entries)).on_dismiss(Message::CloseMenu);

    let area = container(
        column![
            icon(Icon::Folder, 20.0).color(t.colors.fg_muted),
            typography::label(t, "Right-click anywhere on this card"),
        ]
        .spacing(space::MD)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .width(320.0)
    .height(90.0)
    .style(style::container::card(t));
    let context = menu::context_menu(t, area, g.context, demo_entries)
        .on_open(Message::OpenContext)
        .on_dismiss(Message::CloseContext);

    let dialogs = wrap(vec![
        button::secondary(t, "Open a dialog")
            .icon(Icon::Window)
            .on_press(Message::OpenDialog)
            .into(),
        button::secondary(t, "Command palette")
            .icon(Icon::Search)
            .on_press(Message::OpenPalette)
            .into(),
    ]);
    let toasts = wrap(vec![
        button::ghost(t, "Info").on_press(Message::Toast(0)).into(),
        button::ghost(t, "Success")
            .on_press(Message::Toast(1))
            .into(),
        button::ghost(t, "Warning")
            .on_press(Message::Toast(2))
            .into(),
        button::ghost(t, "Error with an action")
            .on_press(Message::Toast(3))
            .into(),
    ]);
    let tips = wrap(vec![
        tip::bottom(
            t,
            button::secondary(t, "Under").on_press(Message::Noop),
            "A tip under the control",
        ),
        tip::over(
            t,
            button::secondary(t, "Over").on_press(Message::Noop),
            "A tip over the control",
        ),
        tip::right(
            t,
            button::secondary(t, "Beside").on_press(Message::Noop),
            "A tip beside it",
        ),
        tip::keys(
            t,
            button::secondary(t, "With keys").on_press(Message::Noop),
            "Open the palette",
            "Ctrl+K",
        ),
    ]);
    column![
        block(
            t,
            "Drop-down menu",
            Some(
                "Arrows, Home, End, Enter; Escape or a click outside closes. Greyed lines say why."
            ),
            dropdown,
        ),
        block(
            t,
            "Context menu",
            Some("At the pointer; pulled back inside the window near an edge."),
            context
        ),
        block(
            t,
            "Dialog and palette",
            Some("Escape closes; Tab stays inside; the first field has the keyboard."),
            dialogs
        ),
        block(
            t,
            "Toasts",
            Some("Gone after a few seconds, longer the more they matter."),
            toasts
        ),
        block(t, "Tooltips", None, tips),
        typography::muted(t, g.last.clone()),
    ]
    .spacing(space::XXXL)
    .into()
}

// ---- Layout pieces ---------------------------------------------------------------------------------

fn layout(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;

    let sections = container(column![
        section::section(t, "Changes", g.folded[0], Message::Fold(0))
            .meta("12")
            .action(button::link(t, "View diff", Message::Noop))
            .body(
                column![
                    typography::ui(t, "src/lib/paletteMatch.ts"),
                    typography::ui(t, "crates/hangar-ui/src/matching.rs"),
                ]
                .spacing(4)
            ),
        section::section(t, "Worktrees", g.folded[1], Message::Fold(1))
            .meta("3")
            .last()
            .body(typography::ui(t, "feat/native-rust")),
    ])
    .width(320.0)
    .style(style::container::panel(t));

    let header = panel::header(t, "Ben")
        .leading(glyph::state(t, GlyphState::Waiting, 8.0))
        .subtitle("Claude Code")
        .action(glyph::gauge(t, 72).tip("72 % of 200k tokens"))
        .action(
            button::icon_button(t, Icon::More)
                .small()
                .tip("More")
                .on_press(Message::Noop),
        )
        .focused(true);
    let panel_demo = container(
        panel::panel(
            t,
            "Worktrees",
            container(typography::muted(t, "Three worktrees, none dirty.")).padding(space::XL),
        )
        .icon(Icon::Branch)
        .subtitle("3")
        .action(
            button::icon_button(t, Icon::Plus)
                .small()
                .tip("New worktree")
                .on_press(Message::Noop),
        ),
    )
    .width(320.0)
    .height(120.0);

    let strip: Element<'_, Message> = if g.tabs_open.is_empty() {
        typography::muted(t, "All closed. Restart the gallery to bring them back.").into()
    } else {
        tabs::strip(t, &g.tabs_open, &g.tab, Message::Tab)
            .on_close(Message::CloseTab)
            .into()
    };

    let rows = container(
        column(
            ["storefront", "hangar", "docs-site", "kits"]
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    list::row(
                        t,
                        row![
                            glyph::state(
                                t,
                                match at {
                                    0 => GlyphState::Working,
                                    1 => GlyphState::Waiting,
                                    2 => GlyphState::Idle,
                                    _ => GlyphState::Yours,
                                },
                                7.0,
                            ),
                            typography::ui(t, *name),
                            iced_space::horizontal(),
                            badge::count(t, at + 2),
                        ]
                        .spacing(space::MD)
                        .align_y(Alignment::Center),
                        g.row == at,
                    )
                    .on_press(Message::Row(at))
                    .into()
                }),
        )
        .spacing(2),
    )
    .padding(6)
    .width(320.0)
    .style(style::container::sidebar(t));

    let badges = wrap(vec![
        badge::badge(t, "3 waiting", Tone::Warning),
        badge::badge(t, "merged", Tone::Success),
        badge::badge(t, "failed", Tone::Danger),
        badge::badge(t, "new", Tone::Accent),
        badge::badge(t, "12", Tone::Neutral),
        badge::count(t, 42),
        badge::tag(t, "custom"),
    ]);

    let glyphs = wrap(
        GLYPHS
            .iter()
            .map(|(state, name)| {
                row![glyph::state(t, *state, 10.0), typography::ui(t, *name)]
                    .spacing(space::MD)
                    .align_y(Alignment::Center)
                    .into()
            })
            .collect(),
    );
    let gauges = wrap(
        [10, 45, 72, 91, 100]
            .into_iter()
            .map(|pct| glyph::gauge(t, pct).into())
            .collect(),
    );
    let keys = wrap(vec![
        keycap::keycap(t, "Esc"),
        keycap::binding(t, "Ctrl+K").into(),
        keycap::binding(t, "Ctrl+K Ctrl+S").into(),
        keycap::binding(t, "Alt+Up").conflict().into(),
        keycap::held(t, &["Ctrl", "Shift"]),
    ]);

    let empty_state = container(
        empty::empty(t, Icon::Board, "No tasks yet")
            .description("Agents add tasks here as they plan the work.")
            .action(
                button::primary(t, "New task")
                    .icon(Icon::Plus)
                    .on_press(Message::Noop),
            ),
    )
    .width(360.0)
    .height(220.0)
    .style(style::container::card(t));

    let pane = |label: &'static str| -> Element<'_, Message> {
        container(typography::muted(t, label))
            .center(Length::Fill)
            .style(style::container::card(t))
            .into()
    };
    let splitter = container(
        split::split(t, pane("first"), pane("second"))
            .arrange(Arrange::Columns)
            .ratio(g.split)
            .min_first(80.0)
            .min_second(80.0)
            .on_resize(Message::Split)
            .on_reset(Message::SplitReset),
    )
    .width(Length::Fill)
    .height(110.0);
    let stacked = container(
        split::split(t, pane("top"), pane("bottom"))
            .arrange(Arrange::Rows)
            .ratio(0.4),
    )
    .width(Length::Fill)
    .height(140.0);

    column![
        block(
            t,
            "Sections",
            Some("The title row is one button; the action sits beside it."),
            sections
        ),
        block(
            t,
            "Panel headers",
            None,
            column![Element::from(header), Element::from(panel_demo)].spacing(space::LG)
        ),
        block(
            t,
            "Tabs",
            Some("The cross closes the tab and does not also select it."),
            strip
        ),
        block(t, "List rows", None, rows),
        block(t, "Badges and counters", None, badges),
        block(
            t,
            "Agent states",
            Some("Shape and colour both say the state."),
            glyphs
        ),
        block(
            t,
            "Context gauge",
            Some("Amber from 70 %, red from 90 %."),
            gauges
        ),
        block(t, "Key caps", None, keys),
        block(t, "Empty state", None, empty_state),
        block(
            t,
            "Resizable divider",
            Some("Drag; double-click to reset; click it, then the arrows."),
            column![splitter, stacked].spacing(space::LG)
        ),
    ]
    .spacing(space::XXXL)
    .into()
}

// ---- The settings form --------------------------------------------------------------------------

fn settings(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;
    let appearance = form::section(t, "Appearance")
        .hint("How the window looks.")
        .group(vec![
            form::row(
                t,
                "Interface size",
                input::slider(t, 90.0..=130.0, g.scale, Message::Scale)
                    .step(5.0)
                    .display(format!("{:.0} %", g.scale)),
            )
            .hint("Scales every size in the window.")
            .wide()
            .modified(g.scale != 100.0)
            .on_reset(Message::ResetScale)
            .into(),
            form::row(
                t,
                "Mode",
                input::segmented(
                    t,
                    &[
                        (Mode::Dark, "Dark"),
                        (Mode::Light, "Light"),
                        (Mode::System, "System"),
                    ],
                    &g.mode,
                    Message::Mode,
                ),
            )
            .into(),
            form::row(
                t,
                "Shell",
                input::select(t, ["bash", "zsh", "fish"], Some(g.shell), Message::Shell),
            )
            .wide()
            .into(),
        ]);
    let behaviour = form::section(t, "Behaviour").group(vec![
        form::row(
            t,
            "Notify when an agent is waiting",
            input::switch(t, g.switch).on_toggle(Message::Switch),
        )
        .hint("A desktop notification, with a click back to the pane.")
        .into(),
        form::row(
            t,
            "Restore panes on start",
            input::switch(t, g.check).on_toggle(Message::Check),
        )
        .into(),
        form::row(
            t,
            "Workspace name",
            input::text_field(t, &g.text).on_input(Message::Text),
        )
        .wide()
        .into(),
        form::row(t, "Version", typography::mono(t, "1.12.1")).into(),
        form::row(
            t,
            "Reset everything",
            button::danger(t, "Reset…").on_press(Message::Noop),
        )
        .hint("Puts every setting back to its default.")
        .into(),
    ]);
    let shortcuts = form::section(t, "Shortcuts")
        .hint("Click the plus to record another key.")
        .group(vec![
            form::row(
                t,
                "Open the settings",
                shortcut_field(t, &g.bindings, g.recorder.as_ref())
                    .on_record(Message::Record)
                    .on_input(Message::RecorderInput)
                    .on_confirm(Message::RecorderConfirm)
                    .on_cancel(Message::RecorderCancel)
                    .on_remove(Message::RemoveBinding)
                    .on_reset(Message::ResetBindings)
                    .customized(g.bindings.len() != 2)
                    .conflicts(if g.bindings.len() > 2 {
                        vec![2]
                    } else {
                        vec![]
                    }),
            )
            .into(),
            form::row(t, "Command palette", keycap::binding(t, "Ctrl+K")).into(),
            form::block(
                t,
                Some("Preview"),
                Some("A block takes the whole width of its group."),
                container(typography::mono(t, "$ claude --resume").size(12.0))
                    .padding(space::LG)
                    .width(Length::Fill)
                    .style(style::container::well(t)),
            ),
        ]);
    form::page(vec![appearance.into(), behaviour.into(), shortcuts.into()])
}

// ---- Icons ---------------------------------------------------------------------------------------------

fn icons(g: &Gallery) -> Element<'_, Message> {
    let t = g.theme;
    let sizes = wrap(
        [10.0, 12.0, 14.0, 16.0, 18.0, 24.0, 32.0]
            .into_iter()
            .map(|side| icon(Icon::Gear, side).into())
            .collect(),
    );
    let tints = wrap(vec![
        icon(Icon::Alert, 20.0).color(t.colors.warning).into(),
        icon(Icon::Check, 20.0).color(t.colors.success).into(),
        icon(Icon::Close, 20.0).color(t.colors.danger).into(),
        icon(Icon::Info, 20.0).color(t.colors.accent_hi).into(),
        icon(Icon::ChevronDown, 20.0)
            .rotate(std::f32::consts::FRAC_PI_2)
            .into(),
    ]);
    let cells: Vec<Element<'_, Message>> = Icon::ALL
        .iter()
        .map(|which| {
            container(
                column![
                    icon(*which, 20.0),
                    text(which.name()).size(10.0).style(style::text::muted(t)),
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .width(104.0)
            .padding(Padding::from([10.0, 4.0]))
            .center_x(Length::Shrink)
            .style(style::container::card(t))
            .into()
        })
        .collect();
    let grid = Row::with_children(cells).spacing(8).wrap();
    column![
        block(
            t,
            "Sizes",
            Some("They take the colour of the text around them."),
            sizes
        ),
        block(t, "Colours and turns", None, tints),
        block(
            t,
            &format!("All {} icons of the web app", Icon::ALL.len()),
            None,
            container(grid.vertical_spacing(8)),
        ),
    ]
    .spacing(space::XXXL)
    .into()
}
