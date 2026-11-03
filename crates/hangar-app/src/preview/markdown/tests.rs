use super::*;
use std::collections::HashMap;

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
    // The image is a file to be read, beside the page, by its checks.
    assert_eq!(
        rendered.wanted(),
        [(
            "shots/one.png".to_string(),
            PathBuf::from("/dev/docs/shots/one.png")
        )]
    );
    assert!(rendered.images.is_empty());
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
        Some(Resolved::Raster(_))
    ));
    assert!(rendered.wanted().is_empty());
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

/* ── the images of a page ──────────────────────────────────────── */

/// A folder the preview may read from, for as long as the test lives. The
/// list of folders is the process's, so the tests that set it take turns.
struct Granted {
    dir: PathBuf,
    outside: PathBuf,
    _turn: tokio::sync::MutexGuard<'static, ()>,
}

impl Granted {
    async fn new(name: &str) -> Granted {
        let turn = crate::preview::tests::ROOTS.lock().await;
        let base = std::env::temp_dir().join(format!("hangar-md-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("docs");
        let outside = base.join("elsewhere");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        hangar_core::model::model_allow_roots(vec![dir.to_string_lossy().into_owned()])
            .await
            .unwrap();
        // The roots are canonical (`/tmp` may be a link), so the paths the
        // tests write are too.
        Granted {
            dir: dir.canonicalize().unwrap(),
            outside: outside.canonicalize().unwrap(),
            _turn: turn,
        }
    }
}

impl Drop for Granted {
    fn drop(&mut self) {
        if let Some(base) = self.dir.parent() {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

fn png_file(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(file, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[255u8; 16]).unwrap();
}

fn gif_file(path: &Path) {
    use ::image::codecs::gif::GifEncoder;
    use ::image::{Delay, Frame, Rgba, RgbaImage};
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut out);
        for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let image = RgbaImage::from_pixel(4, 4, Rgba(colour));
            let delay = Delay::from_numer_denom_ms(100, 1);
            encoder
                .encode_frame(Frame::from_parts(image, 0, 0, delay))
                .unwrap();
        }
    }
    std::fs::write(path, out).unwrap();
}

/// What a page ends up with for the images it names, by address.
async fn loaded(text: &str, page: &Path) -> HashMap<Uri, Resolved> {
    let rendered = Rendered::parse(text, &page.to_string_lossy());
    load_images(rendered.wanted()).await.into_iter().collect()
}

#[tokio::test]
async fn an_image_beside_the_page_loads_whatever_it_is_made_of() {
    let grant = Granted::new("beside").await;
    png_file(&grant.dir.join("one.png"));
    gif_file(&grant.dir.join("move.gif"));
    std::fs::write(
        grant.dir.join("logo.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'/>",
    )
    .unwrap();
    std::fs::create_dir_all(grant.dir.join("shots")).unwrap();
    png_file(&grant.dir.join("shots/two.png"));
    let page = grant.dir.join("README.md");
    let found = loaded(
        "![a](one.png) ![b](shots/two.png) ![c](./shots/../one.png) ![d](move.gif) ![e](logo.svg)",
        &page,
    )
    .await;
    assert!(matches!(found.get("one.png"), Some(Resolved::Raster(_))));
    assert!(matches!(
        found.get("shots/two.png"),
        Some(Resolved::Raster(_))
    ));
    assert!(matches!(
        found.get("./shots/../one.png"),
        Some(Resolved::Raster(_))
    ));
    assert!(
        matches!(found.get("move.gif"), Some(Resolved::Animated(a)) if a.frames.len() == 2),
        "a GIF of two frames plays"
    );
    assert!(
        matches!(found.get("logo.svg"), Some(Resolved::Vector(_))),
        "an SVG is drawn as a vector, not handed to the raster decoder"
    );
}

#[tokio::test]
async fn an_image_outside_the_allowed_folders_is_never_read() {
    let grant = Granted::new("outside").await;
    png_file(&grant.outside.join("secret.png"));
    png_file(&grant.dir.join("fine.png"));
    let page = grant.dir.join("README.md");
    let text = format!(
        "![up](../elsewhere/secret.png) ![abs]({}) ![fine](fine.png)",
        grant.outside.join("secret.png").display()
    );
    let found = loaded(&text, &page).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found.contains_key("fine.png"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_inside_the_folder_to_a_file_outside_it_is_refused() {
    let grant = Granted::new("link").await;
    png_file(&grant.outside.join("secret.png"));
    std::os::unix::fs::symlink(
        grant.outside.join("secret.png"),
        grant.dir.join("sneak.png"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&grant.outside, grant.dir.join("out")).unwrap();
    let page = grant.dir.join("README.md");
    let found = loaded("![a](sneak.png) ![b](out/secret.png)", &page).await;
    assert!(found.is_empty(), "{found:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_device_is_not_an_image() {
    let grant = Granted::new("device").await;
    std::os::unix::fs::symlink("/dev/zero", grant.dir.join("zero.png")).unwrap();
    let page = grant.dir.join("README.md");
    let found = loaded("![z](zero.png) ![z2](/dev/zero)", &page).await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn a_file_that_is_not_what_its_name_says_or_too_large_is_refused() {
    let grant = Granted::new("kinds").await;
    std::fs::write(grant.dir.join("page.png"), "<html>an error page</html>").unwrap();
    std::fs::create_dir_all(grant.dir.join("folder.png")).unwrap();
    // A PNG that is larger than the 64 MB the preview reads: only a sparse
    // file, which costs no disk.
    {
        use std::io::Write;
        let mut file = std::fs::File::create(grant.dir.join("huge.png")).unwrap();
        file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        file.set_len(64 * 1024 * 1024 + 1).unwrap();
    }
    png_file(&grant.dir.join("ok.png"));
    let page = grant.dir.join("README.md");
    let found = loaded(
        "![a](page.png) ![b](folder.png) ![c](huge.png) ![d](ok.png) ![e](missing.png)",
        &page,
    )
    .await;
    assert_eq!(found.keys().collect::<Vec<_>>(), ["ok.png"], "{found:?}");
}

#[tokio::test]
async fn a_picture_with_a_header_the_decoder_cannot_read_is_left_out() {
    let grant = Granted::new("broken").await;
    // The right first bytes, then nothing a PNG has.
    std::fs::write(
        grant.dir.join("broken.png"),
        b"\x89PNG\r\n\x1a\nnot a header",
    )
    .unwrap();
    let page = grant.dir.join("README.md");
    let found = loaded("![b](broken.png)", &page).await;
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn the_page_resolves_what_was_found_and_keeps_the_rest_as_alt_text() {
    let mut rendered = Rendered::parse("![a](one.png) ![b](two.png)", "/dev/docs/README.md");
    assert_eq!(rendered.wanted().len(), 2);
    rendered.resolve(vec![(
        "one.png".to_string(),
        Resolved::Raster(image::Handle::from_bytes(vec![1])),
    )]);
    assert!(matches!(
        rendered.images.get("one.png"),
        Some(Resolved::Raster(_))
    ));
    assert!(!rendered.images.contains_key("two.png"));
}

/* ── front matter ──────────────────────────────────────────────── */

#[test]
fn front_matter_is_shown_as_a_code_block_instead_of_dropped() {
    let source = "---\ntitle: Notes\ntags: [a, b]\n---\n\n# Heading\n\ntext\n";
    assert_eq!(
        prepare(source),
        "```yaml\ntitle: Notes\ntags: [a, b]\n```\n\n\n# Heading\n\ntext\n"
    );
    let rendered = Rendered::parse(source, "/d/README.md");
    let items = rendered.content.items();
    assert!(
        matches!(&items[0], Item::CodeBlock { language: Some(l), code, .. }
            if l == "yaml" && code.starts_with("title: Notes")),
        "{:?}",
        items.first()
    );
    assert!(matches!(&items[1], Item::Heading(..)));
}

#[test]
fn a_toml_block_and_a_dotted_end_are_front_matter_too() {
    assert_eq!(
        prepare("+++\ntitle = \"x\"\n+++\ntext\n"),
        "```toml\ntitle = \"x\"\n```\n\ntext\n"
    );
    assert_eq!(
        prepare("---\na: 1\n...\ntext\n"),
        "```yaml\na: 1\n```\n\ntext\n"
    );
}

#[test]
fn a_rule_at_the_top_that_is_never_closed_is_not_front_matter() {
    assert_eq!(prepare("---\ntext\n"), "---\ntext\n");
    // Nothing inside: dropped, as there is nothing to read.
    assert_eq!(prepare("---\n---\ntext\n"), "text\n");
}

#[test]
fn the_fence_of_front_matter_is_longer_than_any_backticks_inside_it() {
    let out = prepare("---\nnote: ```code```\n---\n");
    assert!(out.starts_with("````yaml\n"), "{out}");
    assert!(out.contains("\n````\n"));
}

#[test]
fn headings_are_sized_in_ems_of_the_text() {
    assert!((heading_size(HeadingLevel::H1) - 13.5 * 1.75).abs() < 1e-4);
    assert!(heading_size(HeadingLevel::H1) > heading_size(HeadingLevel::H2));
    assert!(heading_size(HeadingLevel::H4) > heading_size(HeadingLevel::H5));
}
