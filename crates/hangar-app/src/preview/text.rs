//! A text file as the viewers read it: its head, cut to whole lines, laid out
//! if it is JSON on one line, coloured once the highlighter has run
//! (`useTextHead.ts`, `TextParts.tsx`, `TextView.tsx`).
//!
//! The TS fetched the head over the scheme with a `Range`; the native viewer
//! reads the file itself, at most `TEXT_HEAD_BYTES` of it, on a blocking task.
//! What the view needs of it is `Ready`.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use iced::Element;

use hangar_i18n::{Key, args};

use super::glyph::Glyph;
use super::highlight::{Class, Lines, Run};
use super::model::{self, TEXT_HEAD_BYTES, TEXT_HEAD_LINES};
use super::parts::{self, Feel, Mark};
use crate::ctx::Ctx;

/// The first bytes of a file, and its whole size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub bytes: Vec<u8>,
    pub total: u64,
}

/// Reads at most `TEXT_HEAD_BYTES` of a file.
pub fn read_head(path: &Path) -> Result<Head, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let total = file
        .metadata()
        .map_err(|err| format!("{}: {err}", path.display()))?
        .len();
    let mut bytes = Vec::with_capacity((total as usize).min(TEXT_HEAD_BYTES));
    file.take(TEXT_HEAD_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(Head { bytes, total })
}

/// What a viewer holds of a text it is reading.
#[derive(Debug, Clone)]
pub enum Load {
    Loading,
    Failed(String),
    Ready(Box<Ready>),
}

#[derive(Debug, Clone)]
pub struct Ready {
    /// The text as drawn: JSON written on one line, laid out.
    pub text: String,
    /// The file goes on past what is drawn.
    pub truncated: bool,
    /// Bytes read, and the whole file's.
    pub read: u64,
    pub total: u64,
    /// The layout of one-line JSON was applied, which the bar says.
    pub pretty: bool,
    /// The text, line by line, in runs of one colour.
    pub lines: Arc<Lines>,
    /// Which text `lines` colours: a colouring that arrives for another one
    /// (the file was read again meanwhile) is dropped.
    pub generation: u64,
}

/// The lines of a text with no colour.
pub fn plain(text: &str) -> Lines {
    model::text_lines(text)
        .into_iter()
        .map(|line| {
            vec![Run {
                text: line.to_string(),
                class: Class::Plain,
            }]
        })
        .collect()
}

impl Ready {
    /// A head decoded and cut to what is drawn. JSON written on one line is
    /// laid out for reading (`allow_pretty`, for a file the JSON language
    /// names), only when the whole file was read.
    pub fn of(head: &Head, allow_pretty: bool, generation: u64) -> Ready {
        let decoded = model::decode_head(&head.bytes);
        let read = head.bytes.len() as u64;
        let cut = model::text_head(&decoded, read < head.total, TEXT_HEAD_LINES);
        let pretty = (allow_pretty && !cut.truncated)
            .then(|| model::pretty_json(&cut.text))
            .flatten();
        let laid_out = pretty.is_some();
        let text = pretty.unwrap_or(cut.text);
        Ready {
            lines: Arc::new(plain(&text)),
            text,
            truncated: cut.truncated,
            read,
            total: head.total,
            pretty: laid_out,
            generation,
        }
    }
}

/// The stage of a text viewer (`TextStage`): while the head loads or fails,
/// the word that says only its start is shown, and `body` once there is text
/// to draw.
pub fn stage<'a, M: Clone + 'a>(
    ctx: Ctx<'a>,
    load: &'a Load,
    title: &'a str,
    retry: M,
    body: impl FnOnce(&'a Ready) -> Element<'a, M>,
) -> Element<'a, M> {
    let t = ctx.theme;
    match load {
        Load::Loading => parts::loading(ctx, title, None),
        Load::Failed(reason) => parts::stage_message(
            t,
            Mark::Icon(hangar_ui::Icon::Alert),
            Feel::Danger,
            ctx.t(Key::PreviewLoadFailed),
            Some(reason.clone()),
            Some((ctx.t(Key::PreviewRetry).into_owned(), retry)),
        ),
        Load::Ready(ready) if ready.text.trim().is_empty() && !ready.truncated => {
            parts::stage_message(
                t,
                Mark::Glyph(Glyph::Text),
                Feel::Calm,
                ctx.t(Key::PreviewEmptyFile),
                None,
                None,
            )
        }
        Load::Ready(ready) => {
            let mut stage = iced::widget::column![];
            if ready.truncated {
                let words = if ready.read < ready.total {
                    ctx.i18n.t_with(
                        Key::PreviewHead,
                        &args!(
                            "shown" => parts::byte_size(ctx, ready.read),
                            "total" => parts::byte_size(ctx, ready.total),
                        ),
                    )
                } else {
                    ctx.i18n
                        .t_with(Key::PreviewHeadLines, &args!("n" => TEXT_HEAD_LINES))
                };
                stage = stage.push(parts::note(t, words.into_owned(), false));
            }
            stage.push(body(ready)).into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(text: &str, total: u64) -> Head {
        Head {
            bytes: text.as_bytes().to_vec(),
            total,
        }
    }

    #[test]
    fn reads_a_whole_short_file_without_saying_it_is_cut() {
        let ready = Ready::of(&head("one\ntwo\n", 8), false, 1);
        assert_eq!(ready.text, "one\ntwo\n");
        assert!(!ready.truncated);
        assert_eq!(ready.lines.len(), 2);
        assert_eq!((ready.read, ready.total), (8, 8));
    }

    #[test]
    fn a_file_read_short_of_its_end_loses_the_piece_of_a_line_and_says_it_goes_on() {
        let ready = Ready::of(&head("one\ntwo\nthr", 4000), false, 1);
        assert_eq!(ready.text, "one\ntwo");
        assert!(ready.truncated);
        assert!(ready.read < ready.total);
    }

    #[test]
    fn lays_out_one_line_json_only_for_json_and_only_when_whole() {
        let minified = "{\"a\":[1,2]}";
        let whole = Ready::of(&head(minified, minified.len() as u64), true, 1);
        assert!(whole.pretty);
        assert!(whole.text.starts_with("{\n  \"a\": ["));
        let other_language = Ready::of(&head(minified, minified.len() as u64), false, 1);
        assert!(!other_language.pretty);
        let cut = Ready::of(&head(minified, 9999), true, 1);
        assert!(!cut.pretty);
    }

    #[test]
    fn an_empty_file_has_one_empty_row_and_no_note() {
        let ready = Ready::of(&head("", 0), false, 1);
        assert!(!ready.truncated);
        assert!(ready.text.is_empty());
    }

    #[test]
    fn reads_the_head_of_a_file_and_no_more() {
        let dir = std::env::temp_dir().join(format!("hangar-text-head-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.log");
        std::fs::write(&path, vec![b'x'; TEXT_HEAD_BYTES + 5000]).unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!(head.bytes.len(), TEXT_HEAD_BYTES);
        assert_eq!(head.total, (TEXT_HEAD_BYTES + 5000) as u64);
        assert!(read_head(&dir.join("missing.log")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
