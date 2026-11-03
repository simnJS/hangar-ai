use super::*;

fn theme() -> &'static Theme {
    hangar_theme::by_id_or_default("graphite")
}

/// The text of the paragraphs of a page, as the page draws them.
fn paragraphs(rendered: &Rendered) -> Vec<String> {
    let style = settings(theme()).style;
    rendered
        .content
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Paragraph(words) => Some(
                words
                    .spans(style)
                    .iter()
                    .map(|part| part.text.as_ref())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn never_turns_the_files_own_html_into_markup() {
    let source = "<script>alert(1)</script>\n\n<img src=x onerror=\"alert(1)\">\n\n[click](javascript:alert(1))";
    let rendered = Rendered::parse(source, "/dev/docs/README.md");
    let drawn = paragraphs(&rendered);
    // The tags are text, as they were typed.
    assert_eq!(drawn[0], "<script>alert(1)</script>");
    assert_eq!(drawn[1], "<img src=x onerror=\"alert(1)\">");
    // No image was made of the second paragraph, and the link goes nowhere.
    assert!(rendered.content.images().is_empty());
    assert_eq!(drawn[2], "click");
    assert_eq!(
        rendered.follow("javascript:alert(1)"),
        Follow::Inert("javascript:alert(1)".into())
    );
}

#[test]
fn escapes_tags_but_not_the_comparisons_and_arrows_of_prose() {
    assert_eq!(prepare("<b>bold</b>\n"), "\\<b>bold\\</b>\n");
    assert_eq!(prepare("<!-- note -->\n"), "\\<!-- note -->\n");
    assert_eq!(
        prepare("a < b and c <= d, x<y\n"),
        "a < b and c <= d, x<y\n"
    );
    assert_eq!(prepare("a -> b <- c\n"), "a -> b <- c\n");
}

#[test]
fn leaves_code_alone() {
    let source = "```html\n<script>x</script>\nhttps://example.com\n```\n\n`<b>` and `https://a.b`\n\n    <i>indented</i>\n";
    assert_eq!(prepare(source), source);
    let tilde = "~~~\n<b>\n~~~\n";
    assert_eq!(prepare(tilde), tilde);
    // A longer fence is closed by one at least as long.
    let long = "````\n```\n<b>\n````\n<b>\n";
    assert_eq!(prepare(long), "````\n```\n<b>\n````\n\\<b>\n");
}

#[test]
fn leaves_the_links_the_parser_already_makes() {
    let source = "<https://example.com> <me@example.com> [text](https://example.com/a_(b)) ![x](https://e.co/i.png)\n[ref]: https://example.com/ref\n";
    assert_eq!(prepare(source), source);
}

#[test]
fn links_a_bare_address_without_its_punctuation() {
    assert_eq!(
        prepare("See https://example.com/a?b=1, or (http://x.org/y). Done.\n"),
        "See <https://example.com/a?b=1>, or (<http://x.org/y>). Done.\n"
    );
    assert_eq!(prepare("https://a.b/c_(d)\n"), "<https://a.b/c_(d)>\n");
    // Not inside a word, an attribute or an angle bracket.
    assert_eq!(prepare("xhttps://a.b\n"), "xhttps://a.b\n");
    assert_eq!(
        prepare("src=\"https://a.b/c.png\"\n"),
        "src=\"https://a.b/c.png\"\n"
    );
    // A scheme alone is not an address.
    assert_eq!(prepare("https:// and http://\n"), "https:// and http://\n");
}

#[test]
fn a_bare_address_becomes_a_link_in_the_page() {
    let rendered = Rendered::parse("See https://example.com/docs now.\n", "/d/README.md");
    let style = settings(theme()).style;
    let Item::Paragraph(words) = &rendered.content.items()[0] else {
        panic!("a paragraph");
    };
    let linked: Vec<String> = words
        .spans(style)
        .iter()
        .filter(|part| part.link.is_some())
        .map(|part| part.text.to_string())
        .collect();
    assert_eq!(linked, vec!["https://example.com/docs"]);
}

#[test]
fn resolves_images_numbers_headings_and_colours_fenced_code_as_it_renders() {
    let source = "# Intro\n\n![shot](shots/one.png)\n\n```ts\nconst a = 1;\n```\n\n## Intro\n";
    let rendered = Rendered::parse(source, "/dev/docs/README.md");
    assert_eq!(
        heading_ids(&rendered.content, theme()),
        ["intro", "intro-1"]
    );
    match rendered.images.get("shots/one.png") {
        Some(Resolved::Handle(_)) => {}
        other => panic!("the image should load from beside the file: {other:?}"),
    }
    let Some(Item::CodeBlock { language, code, .. }) = rendered
        .content
        .items()
        .iter()
        .find(|item| matches!(item, Item::CodeBlock { .. }))
    else {
        panic!("a code block");
    };
    let lines = highlight::highlight_block(code, language.as_deref().unwrap()).unwrap();
    assert!(
        lines[0]
            .iter()
            .any(|run| run.text == "const" && run.class == highlight::Class::Keyword)
    );
}

#[test]
fn shows_an_image_it_may_not_load_as_its_alt_text() {
    let rendered = Rendered::parse(
        "![the <b>logo</b>](file:///C:/logo.png)",
        "/dev/docs/README.md",
    );
    assert!(
        rendered.images.is_empty(),
        "nothing loads from another scheme: {:?}",
        rendered.images
    );
}

#[test]
fn a_web_image_is_a_link_not_a_download() {
    let rendered = Rendered::parse("![badge](https://example.com/badge.svg)", "/d/README.md");
    assert!(matches!(
        rendered.images.get("https://example.com/badge.svg"),
        Some(Resolved::Web(address)) if address == "https://example.com/badge.svg"
    ));
}

#[test]
fn an_inline_image_is_read_from_its_bytes() {
    // One transparent pixel.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let rendered = Rendered::parse(
        &format!("![dot](data:image/png;base64,{png})"),
        "/d/README.md",
    );
    assert!(matches!(
        rendered.images.values().next(),
        Some(Resolved::Handle(_))
    ));
}

#[test]
fn scrolls_to_a_heading_opens_the_web_outside_and_goes_nowhere_for_anything_else() {
    let rendered = Rendered::parse("# Setting up\n", "/d/README.md");
    assert_eq!(
        Rendered::anchor("#setting-up").as_deref(),
        Some("setting-up")
    );
    assert_eq!(Rendered::anchor("https://hangar.simnjs.fr"), None);
    assert_eq!(
        rendered.follow("https://hangar.simnjs.fr"),
        Follow::Open("https://hangar.simnjs.fr".into())
    );
    assert_eq!(
        rendered.follow("mailto:me@example.com"),
        Follow::Open("mailto:me@example.com".into())
    );
    assert_eq!(rendered.follow("#setting-up"), Follow::Nothing);
    assert_eq!(
        rendered.follow("docs/other.md"),
        Follow::Inert("docs/other.md".into())
    );
}

#[test]
fn a_headings_widget_is_named_by_its_anchor() {
    assert_eq!(heading_widget("setting-up"), heading_widget("setting-up"));
    assert_ne!(heading_widget("setting-up"), heading_widget("other"));
}

#[test]
fn headings_are_sized_in_ems_of_the_text() {
    assert!((heading_size(HeadingLevel::H1) - 13.5 * 1.75).abs() < 1e-4);
    assert!(heading_size(HeadingLevel::H1) > heading_size(HeadingLevel::H2));
    assert!(heading_size(HeadingLevel::H4) > heading_size(HeadingLevel::H5));
}
