use super::*;
use crate::testing::{Drive, simulate};
use iced::widget::text::Span;
use iced::widget::{column, span};

type Spans = Vec<Span<'static, (), Font>>;

fn plain(text: &str) -> Spans {
    vec![span(text.to_string())]
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Selected(String),
    Copied,
    Link(String),
    Pressed,
}

const PAINT: Color = Color::from_rgb(1.0, 0.0, 0.0);

/// Three blocks, one under another, and the document they share.
fn three() -> (Doc, impl Fn(&Doc) -> Element<'static, Msg>) {
    let doc = Doc::new(PAINT);
    let build = |doc: &Doc| {
        doc.begin();
        let page = column![
            doc.block::<(), Msg>(plain("alpha beta")).size(14),
            doc.block::<(), Msg>(plain("gamma")).size(14),
            doc.block::<(), Msg>(plain("delta epsilon")).size(14),
        ];
        selectable(doc, page)
            .on_select(Msg::Selected)
            .on_copy(|| Msg::Copied)
            .into()
    };
    (doc, build)
}

/// The middle of block `block`, as the last event left it.
fn middle(doc: &Doc, block: usize) -> Point {
    let inner = doc.lock();
    let bounds = inner.places[&block].bounds;
    Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
}

/// Where the characters `from..to` of a block are across, in its middle.
fn across(doc: &Doc, block: usize, from: usize, to: usize) -> (f32, f32, f32) {
    let inner = doc.lock();
    let place = &inner.places[&block];
    let x = |index: usize| {
        place.bounds.x
            + place
                .paragraph
                .grapheme_position(0, index)
                .map_or(0.0, |point| point.x)
    };
    (
        (x(from) + x(to)) / 2.0,
        place.bounds.y + place.bounds.height / 2.0,
        place.bounds.x,
    )
}

fn ctrl(character: &str) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character(character.into()),
        modified_key: Key::Character(character.into()),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::COMMAND,
        text: None,
        repeat: false,
    })
}

fn said(ui: iced_test::Simulator<'_, Msg>) -> Vec<Msg> {
    ui.into_messages().collect()
}

#[test]
fn a_selection_knows_which_part_of_each_block_it_takes() {
    let selection = Selection {
        anchor: Pos {
            block: 3,
            offset: 4,
        },
        focus: Pos {
            block: 1,
            offset: 2,
        },
    };
    assert_eq!(selection.ordered().0.block, 1, "either end may come first");
    assert_eq!(selection.within(0, 10), None);
    assert_eq!(selection.within(1, 10), Some(2..10));
    assert_eq!(selection.within(2, 10), Some(0..10));
    assert_eq!(selection.within(3, 10), Some(0..4));
    assert_eq!(selection.within(4, 10), None);
    // A block with nothing in it, between two others, has nothing to paint.
    assert_eq!(selection.within(2, 0), None);
    // Both ends in one block.
    let one = Selection {
        anchor: Pos {
            block: 2,
            offset: 7,
        },
        focus: Pos {
            block: 2,
            offset: 3,
        },
    };
    assert_eq!(one.within(2, 10), Some(3..7));
    // An end past the text is the end of the text.
    let past = Selection {
        anchor: Pos {
            block: 0,
            offset: 4,
        },
        focus: Pos {
            block: 0,
            offset: 99,
        },
    };
    assert_eq!(past.within(0, 6), Some(4..6));
    let none = Selection {
        anchor: Pos {
            block: 0,
            offset: 4,
        },
        focus: Pos {
            block: 0,
            offset: 4,
        },
    };
    assert!(none.is_empty());
    assert_eq!(none.within(0, 10), None);
}

#[test]
fn a_double_click_takes_the_word_a_space_or_a_run_of_signs() {
    let text = "let foo_bar = a + b;";
    let word = |at: usize| &text[word_bounds(text, at)];
    assert_eq!(word(0), "let");
    assert_eq!(word(1), "let");
    assert_eq!(word(3), "let", "the end of a word is the word's");
    assert_eq!(word(5), "foo_bar", "an underscore is part of a word");
    assert_eq!(word(11), "foo_bar");
    assert_eq!(word(12), "=");
    assert_eq!(word(13), " ", "a space between signs and words");
    assert_eq!(word(16), "+");
    assert_eq!(word(19), "b", "a letter next to a sign is the letter's");
    assert_eq!(word(20), ";", "the end of the text is its last character");
    assert_eq!(word_bounds("", 0), 0..0);
    // Multi-byte characters stay whole.
    let accents = "déjà vu";
    assert_eq!(&accents[word_bounds(accents, 2)], "déjà");
    assert_eq!(&accents[word_bounds(accents, 1)], "déjà");
}

#[test]
fn spans_are_split_where_the_selection_starts_and_ends_and_keep_their_links() {
    let mut spans: Vec<Span<'static, u8, Font>> = vec![span("abc"), span("defg"), span("hi")];
    spans[1].link = Some(7);
    let split = split_spans(&spans, &(2..6), PAINT);
    let texts: Vec<&str> = split.iter().map(|s| s.text.as_ref()).collect();
    assert_eq!(texts, ["ab", "c", "def", "g", "hi"]);
    let painted: Vec<bool> = split.iter().map(|s| s.highlight.is_some()).collect();
    assert_eq!(painted, [false, true, true, false, false]);
    assert_eq!(split[2].link, Some(7), "the link stays on what was a link");
    assert_eq!(split[3].link, Some(7));
    assert_eq!(split[4].link, None);
    // The text is what it was.
    let joined: String = split.iter().map(|s| s.text.as_ref()).collect();
    assert_eq!(joined, "abcdefghi");
    // Nothing selected in a span: it is left as it was.
    let untouched = split_spans(&spans, &(5..5), PAINT);
    assert_eq!(untouched.len(), 3);
    // A cut inside a character moves to its start.
    let accents: Vec<Span<'static, (), Font>> = vec![span("é")];
    let cut = split_spans(&accents, &(1..2), PAINT);
    assert!(cut.iter().all(|s| s.text.is_char_boundary(0)));
}

#[test]
fn a_drag_across_blocks_selects_from_where_it_went_down_to_where_it_let_go() {
    let (doc, build) = three();
    let mut ui = simulate(build(&doc));
    ui.move_to(1.0, 5.0);
    let top = middle(&doc, 0);
    let bottom = middle(&doc, 2);
    ui.move_to(1.0, top.y);
    ui.press(mouse::Button::Left);
    ui.move_to(500.0, bottom.y);
    ui.release(mouse::Button::Left);
    assert_eq!(
        said(ui),
        [Msg::Selected("alpha beta\ngamma\ndelta epsilon".into())]
    );
    assert_eq!(doc.selected_text(), "alpha beta\ngamma\ndelta epsilon");
}

#[test]
fn a_drag_up_the_page_selects_the_same_text() {
    let (doc, build) = three();
    let mut ui = simulate(build(&doc));
    ui.move_to(1.0, 5.0);
    let top = middle(&doc, 0);
    let bottom = middle(&doc, 2);
    // The right edge of the last line, inside the view.
    let edge = doc.lock().places[&2].bounds;
    ui.move_to(edge.x + edge.width - 0.5, bottom.y);
    ui.press(mouse::Button::Left);
    ui.move_to(1.0, top.y);
    ui.release(mouse::Button::Left);
    assert_eq!(doc.selected_text(), "alpha beta\ngamma\ndelta epsilon");
}

#[test]
fn a_drag_within_a_line_takes_the_part_of_it_between_the_two_points() {
    let (doc, build) = three();
    let mut ui = simulate(build(&doc));
    ui.move_to(1.0, 5.0);
    // From the middle of "alpha" to the middle of "beta".
    let (from, y, _) = across(&doc, 0, 0, 5);
    let (to, ..) = across(&doc, 0, 6, 10);
    ui.move_to(from, y);
    ui.press(mouse::Button::Left);
    ui.move_to(to, y);
    ui.release(mouse::Button::Left);
    let text = doc.selected_text();
    assert!(
        text.len() >= 4 && "alpha beta".contains(&text) && text.contains("a b"),
        "{text:?}"
    );
}

#[test]
fn two_clicks_take_a_word_and_three_take_the_block() {
    let (doc, build) = three();
    let mut ui = simulate(build(&doc));
    ui.move_to(1.0, 5.0);
    let (x, y, _) = across(&doc, 0, 6, 10);
    ui.move_to(x, y);
    for _ in 0..2 {
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
    }
    assert_eq!(doc.selected_text(), "beta");
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    assert_eq!(doc.selected_text(), "alpha beta");
    let messages = said(ui);
    assert_eq!(
        messages.last(),
        Some(&Msg::Selected("alpha beta".into())),
        "{messages:?}"
    );
}

#[test]
fn a_click_alone_selects_nothing_and_clears_what_was_selected() {
    let (doc, build) = three();
    let mut ui = simulate(build(&doc));
    ui.move_to(1.0, 5.0);
    let (x, y, _) = across(&doc, 1, 0, 5);
    ui.move_to(x, y);
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    assert_eq!(doc.selection(), None, "no characters are between its ends");
    doc.select_all();
    assert!(doc.selection().is_some());
    // Later than a double click's window would take it for one.
    std::thread::sleep(std::time::Duration::from_millis(600));
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    assert_eq!(doc.selected_text(), "");
    let messages = said(ui);
    assert_eq!(messages.last(), Some(&Msg::Selected(String::new())));
}

#[test]
fn select_all_and_copy_answer_the_keys_when_the_view_has_the_pointer_last() {
    let (doc, build) = three();
    let mut ui = simulate(build(&doc));
    // Keys do nothing before the pointer has been down in the view.
    ui.simulate([ctrl("a")]);
    assert_eq!(doc.selected_text(), "");
    ui.move_to(1.0, 5.0);
    let (x, y, _) = across(&doc, 1, 0, 5);
    ui.move_to(x, y);
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    let statuses = ui.simulate([ctrl("a")]);
    assert_eq!(statuses, [iced::event::Status::Captured]);
    assert_eq!(doc.selected_text(), "alpha beta\ngamma\ndelta epsilon");
    ui.simulate([ctrl("c")]);
    let escape = Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Escape),
        modified_key: Key::Named(Named::Escape),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    });
    ui.simulate([escape]);
    assert_eq!(doc.selected_text(), "");
    let messages = said(ui);
    assert!(messages.contains(&Msg::Copied), "{messages:?}");
    assert!(
        messages.contains(&Msg::Selected("alpha beta\ngamma\ndelta epsilon".into())),
        "{messages:?}"
    );
}

#[test]
fn a_press_elsewhere_gives_the_keys_back() {
    let (doc, build) = three();
    // The view fills the left of a row; the right is something else.
    let page = iced::widget::row![
        iced::widget::container(build(&doc)).width(300),
        iced::widget::Space::new().width(300).height(100)
    ];
    let mut ui = simulate(page);
    ui.move_to(1.0, 5.0);
    let (x, y, _) = across(&doc, 1, 0, 5);
    ui.move_to(x, y);
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    ui.move_to(500.0, 50.0);
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    ui.simulate([ctrl("a")]);
    assert_eq!(
        doc.selected_text(),
        "",
        "Ctrl+A is not for a view that was left"
    );
}

#[test]
fn a_press_on_a_link_follows_it_and_selects_nothing() {
    let doc = Doc::new(PAINT);
    doc.begin();
    let mut spans: Vec<Span<'static, String, Font>> = vec![span("go "), span("there")];
    spans[1].link = Some("here".into());
    let block = doc
        .block::<String, Msg>(spans)
        .size(14)
        .on_link_click(Msg::Link);
    let mut ui = simulate(selectable(&doc, block));
    ui.move_to(1.0, 5.0);
    let (x, y, _) = across(&doc, 0, 3, 8);
    ui.move_to(x, y);
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    assert_eq!(doc.selection(), None);
    assert_eq!(said(ui), [Msg::Link("here".into())]);
}

/// Ten lines, of which a view builds three.
struct Ten;

impl Source for Ten {
    fn count(&self) -> usize {
        10
    }

    fn text(&self, block: usize) -> String {
        format!("line {block}")
    }
}

#[test]
fn a_source_gives_the_text_of_the_blocks_that_are_not_built() {
    let doc = Doc::new(PAINT);
    doc.source(Some(Arc::new(Ten)));
    doc.begin();
    let page = column![
        doc.block_at::<(), Msg>(4, plain("line 4")).size(14),
        doc.block_at::<(), Msg>(5, plain("line 5")).size(14),
    ];
    let _ui = simulate(selectable(&doc, page));
    doc.select_all();
    let all = doc.selected_text();
    assert_eq!(all.lines().count(), 10);
    assert!(
        all.starts_with("line 0\n") && all.ends_with("\nline 9"),
        "{all}"
    );
    // A selection from a built line to one that is not.
    doc.set_selection(Some(Selection {
        anchor: Pos {
            block: 5,
            offset: 2,
        },
        focus: Pos {
            block: 8,
            offset: 4,
        },
    }));
    assert_eq!(doc.selected_text(), "ne 5\nline 6\nline 7\nline");
}

#[test]
fn the_selected_part_is_drawn_in_the_selection_colour_and_only_that() {
    let doc = Doc::new(PAINT);
    doc.begin();
    let page = column![
        doc.block::<(), Msg>(plain("alpha beta")).size(14),
        doc.block::<(), Msg>(plain("gamma")).size(14),
    ];
    let mut ui = simulate(selectable(&doc, page));
    ui.move_to(1.0, 5.0);
    let (x, y, _) = across(&doc, 1, 0, 5);
    ui.move_to(x, y);
    for _ in 0..3 {
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
    }
    assert_eq!(doc.selected_text(), "gamma");
    let (gamma, alpha) = (middle(&doc, 1), middle(&doc, 0));
    let snapshot = ui.snapshot(&iced::Theme::Dark).expect("it draws");
    let dir = crate::testdir::scratch("selectable");
    snapshot
        .matches_image(dir.join("selected"))
        .expect("written");
    let file = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .next()
        .expect("the png")
        .path();
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(file).unwrap()))
        .read_info()
        .unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    // The snapshot is drawn at twice the size.
    let reds_near = |point: Point| {
        let (cx, cy) = ((point.x * 2.0) as i32, (point.y * 2.0) as i32);
        let mut reds = 0;
        for y in cy - 12..cy + 12 {
            for x in cx - 40..cx + 40 {
                if x < 0 || y < 0 || x >= info.width as i32 || y >= info.height as i32 {
                    continue;
                }
                let at = (y as usize * info.width as usize + x as usize) * 4;
                if bytes[at] > 200 && bytes[at + 1] < 60 && bytes[at + 2] < 60 {
                    reds += 1;
                }
            }
        }
        reds
    };
    assert!(reds_near(gamma) > 100, "{}", reds_near(gamma));
    assert_eq!(reds_near(alpha), 0, "the block above is not painted");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_view_scrolled_down_selects_what_is_under_the_pointer_not_what_was_there() {
    use iced::widget::scrollable;
    let doc = Doc::new(PAINT);
    doc.begin();
    let lines: Vec<Element<'static, Msg>> = (0..60)
        .map(|n| {
            doc.block::<(), Msg>(plain(&format!("row {n}")))
                .size(14)
                .into()
        })
        .collect();
    let page = selectable(&doc, iced::widget::column(lines))
        .on_select(Msg::Selected)
        .on_copy(|| Msg::Copied);
    let mut ui = simulate(scrollable(page).height(200).width(iced::Length::Fill));
    ui.move_to(20.0, 20.0);
    // Scroll a good way: the rows in sight are not the first.
    ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -400.0 },
    })]);
    ui.move_to(20.0, 21.0);
    let seen = doc.lock().places.len();
    assert!(seen > 20, "every block says where it is");
    for _ in 0..3 {
        ui.click("row 24").expect("that row is in sight");
    }
    assert_eq!(doc.selected_text(), "row 24");
    let messages = said(ui);
    assert_eq!(messages.last(), Some(&Msg::Selected("row 24".into())));
}

#[test]
fn a_press_in_the_view_is_said_whatever_was_under_it_and_one_outside_is_not() {
    let doc = Doc::new(PAINT);
    doc.begin();
    let mut spans: Vec<Span<'static, String, Font>> = vec![span("go "), span("there")];
    spans[1].link = Some("here".into());
    let block = doc
        .block::<String, Msg>(spans)
        .size(14)
        .on_link_click(Msg::Link);
    let page = iced::widget::row![
        iced::widget::container(selectable(&doc, block).on_press(|| Msg::Pressed)).width(300),
        iced::widget::Space::new().width(300).height(100)
    ];
    let mut ui = simulate(page);
    ui.move_to(1.0, 5.0);
    let (on_text, y, _) = across(&doc, 0, 0, 2);
    let (on_link, ..) = across(&doc, 0, 3, 8);
    for x in [on_text, on_link, on_text + 3.0] {
        ui.move_to(x, y);
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
    }
    // Outside the view: nothing.
    ui.move_to(500.0, 50.0);
    ui.press(mouse::Button::Left);
    ui.release(mouse::Button::Left);
    let messages = said(ui);
    assert_eq!(
        messages.iter().filter(|m| **m == Msg::Pressed).count(),
        3,
        "{messages:?}"
    );
    assert!(messages.contains(&Msg::Link("here".into())));
}
