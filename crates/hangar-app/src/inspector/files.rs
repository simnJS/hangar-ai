//! The Files section (`InspectorFiles.tsx`): the files the agent edited,
//! with git's counts where git knows them, or, for a pane whose agent
//! reported nothing, what changed in its folder.

use hangar_core::git::Changes;
use hangar_i18n::{Key, args};
use hangar_theme::tokens::font_size;
use hangar_theme::{fonts, style};
use hangar_ui::section::Section;
use hangar_ui::tail_text::tail_text;
use hangar_ui::{Icon, icon, list};
use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Padding};

use super::files_model::{FileRow, diff_segments, diff_totals, edited_rows, folder_rows};
use super::sections::{Scene, empty_note, ink};
use super::widgets::{diff_bar, stacked};
use super::{Msg, SectionId};
use crate::message::Message;
use crate::state::live::PaneInsight;

/// Edited files listed before "+N more": the diff view has the rest.
const EDITED_SHOWN: usize = 10;
const FOLDER_SHOWN: usize = 8;
/// Narrower than this, the "View diff" action gives up its words.
const NARROW: f32 = 280.0;

/// What the section lists.
#[derive(Debug, PartialEq)]
enum Listing {
    /// The agent reported edits.
    Edited {
        rows: Vec<FileRow>,
        more: usize,
        totals: (u32, u32),
        /// Some row has git's counts, so the totals mean something.
        counted: bool,
        count: usize,
    },
    /// The agent reported nothing, and git knows the folder.
    Folder { rows: Vec<FileRow>, more: usize },
    /// Neither: nothing to list yet.
    Nothing,
}

fn listing(edited: &[String], changes: Option<&Changes>, folder: &str) -> Listing {
    if !edited.is_empty() {
        let rows = edited_rows(edited, changes, Some(folder));
        let count = rows.len();
        let counted = rows.iter().any(|row| row.additions.is_some());
        let totals = diff_totals(&rows);
        return Listing::Edited {
            more: count.saturating_sub(EDITED_SHOWN),
            rows: rows.into_iter().take(EDITED_SHOWN).collect(),
            totals,
            counted,
            count,
        };
    }
    if let Some(changes) = changes {
        let (rows, more) = folder_rows(changes, folder, FOLDER_SHOWN);
        return Listing::Folder { rows, more };
    }
    Listing::Nothing
}

/// What the row's tooltip says about the file's state, when anything is known.
fn status_name(s: &Scene<'_>, row: &FileRow) -> Option<String> {
    let tr = s.tr();
    let key = match row.status.as_deref() {
        Some("M") => Key::InspectorStatusM,
        Some("A") => Key::InspectorStatusA,
        Some("D") => Key::InspectorStatusD,
        Some("R") => Key::InspectorStatusR,
        Some("C") => Key::InspectorStatusC,
        Some("U") => Key::InspectorStatusU,
        _ => {
            return row
                .settled
                .then(|| tr.t(Key::InspectorFileGone).into_owned());
        }
    };
    Some(tr.t(key).into_owned())
}

/// "+3 −1" in green and red; a side that did not move is quiet.
fn counts<'a>(s: &Scene<'a>, additions: u32, deletions: u32) -> Element<'a, Message> {
    let c = &s.theme().colors;
    let side = |sign: &str, n: u32, lit| {
        ink(
            format!("{sign}{n}"),
            font_size::LABEL,
            if n > 0 { lit } else { c.fg_subtle },
        )
        .font(fonts::mono())
    };
    row![
        side("+", additions, c.success),
        side("−", deletions, c.danger)
    ]
    .spacing(4)
    .into()
}

fn file_line<'a>(s: &Scene<'a>, line: &FileRow, open: Msg) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let status = status_name(s, line);
    let hint = match &status {
        Some(status) => tr
            .t_with(
                Key::InspectorFileRow,
                &args!("path" => line.path.as_str(), "status" => status.as_str()),
            )
            .into_owned(),
        None => line.path.clone(),
    };

    let letter_color = match line.status.as_deref() {
        Some("A") => c.success,
        Some("D") => c.danger,
        Some("U") => c.warning,
        _ => c.fg_subtle,
    };
    let letter = match (&line.status, line.settled) {
        (Some(letter), _) => letter.clone(),
        (None, true) => "·".to_string(),
        (None, false) => String::new(),
    };
    let name_color = if line.settled { c.fg_muted } else { c.fg };
    // The directory gives way first, from its start, so the file name always
    // shows; only a name longer than the whole row is cut.
    let path: Element<'a, Message> = tail_text(line.dir.clone())
        .suffix(line.name.clone(), name_color)
        .size(font_size::CAPTION)
        .font(fonts::mono())
        .color(c.fg_subtle)
        .into();

    let mut cells = row![
        container(ink(letter, font_size::MICRO, letter_color).font(fonts::ui_semibold()))
            .width(12.0)
            .align_x(iced::alignment::Horizontal::Center),
        path,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    match (line.additions, line.deletions) {
        (Some(a), Some(d)) => {
            cells = cells
                .push(counts(s, a, d))
                .push(diff_bar(t, &diff_segments(a, d, 5)));
        }
        _ if line.binary => {
            cells = cells.push(ink(
                tr.t(Key::InspectorFileBinary).into_owned(),
                font_size::LABEL,
                c.fg_subtle,
            ));
        }
        _ => {}
    }
    // Every row opens the same diff view.
    s.tip(
        list::row(t, cells, false)
            .height(22.0)
            .padding(Padding::from([0.0, 4.0]))
            .on_press(Message::Inspector(open)),
        hint,
    )
}

pub(super) fn section<'a>(
    s: &Scene<'a>,
    insight: &'a PaneInsight,
    folder: &'a str,
    changes: Option<&Changes>,
) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let open = Msg::OpenDiff(s.ws.id.clone(), s.pane.id.clone());
    let listing = listing(&insight.edited_files, changes, folder);
    let mut section = s.section(SectionId::Files, Key::InspectorSectionFiles);

    let (rows, more, folder_mode) = match &listing {
        Listing::Edited {
            rows,
            more,
            totals,
            counted,
            count,
        } => {
            let meta: Element<'a, Message> = if *counted {
                let (additions, deletions) = *totals;
                s.tip(
                    row![
                        ink(
                            format!("+{additions}"),
                            font_size::LABEL,
                            if additions > 0 {
                                c.success
                            } else {
                                c.fg_subtle
                            }
                        )
                        .font(fonts::mono()),
                        ink(
                            format!("−{deletions}"),
                            font_size::LABEL,
                            if deletions > 0 { c.danger } else { c.fg_subtle }
                        )
                        .font(fonts::mono()),
                    ]
                    .spacing(4),
                    tr.t_with(
                        Key::InspectorFilesTotals,
                        &args!("add" => additions, "del" => deletions),
                    )
                    .into_owned(),
                )
            } else {
                ink(count.to_string(), font_size::LABEL, c.fg_subtle)
                    .font(fonts::mono())
                    .into()
            };
            section = section.meta_element(meta);
            (rows.as_slice(), *more, false)
        }
        Listing::Folder { rows, more } => (rows.as_slice(), *more, true),
        Listing::Nothing => (&[][..], 0, false),
    };

    if !rows.is_empty() {
        let label = if s.width <= NARROW {
            None
        } else {
            Some(tr.t(Key::InspectorFilesDiff).into_owned())
        };
        let mut words = row![icon(Icon::Diff, 11.0)]
            .spacing(4)
            .align_y(Alignment::Center);
        if let Some(label) = label {
            words = words.push(
                text::Text::new(label)
                    .size(font_size::CAPTION)
                    .font(fonts::ui_medium())
                    .wrapping(text::Wrapping::None),
            );
        }
        let action = iced::widget::button(words)
            .padding(Padding::from([0.0, 2.0]))
            .style(style::button::link(t))
            .on_press(Message::Inspector(open.clone()));
        section = section.action(s.tip(action, tr.t(Key::InspectorFilesDiffHint).into_owned()));
    }

    let mut body: Vec<Element<'a, Message>> = Vec::new();
    if folder_mode && !rows.is_empty() {
        body.push(
            ink(
                tr.t(Key::InspectorFilesFolder).into_owned(),
                font_size::LABEL,
                c.fg_subtle,
            )
            .into(),
        );
    }
    if rows.is_empty() {
        body.push(empty_note(
            t,
            tr.t(
                if folder_mode || matches!(listing, Listing::Folder { .. }) {
                    Key::InspectorFilesClean
                } else {
                    Key::InspectorFilesEmpty
                },
            )
            .into_owned(),
        ));
    } else {
        body.push(column(rows.iter().map(|line| file_line(s, line, open.clone()))).into());
    }
    if more > 0 {
        body.push(
            container(ink(
                tr.tn(Key::InspectorFilesMore, more, &[]).into_owned(),
                font_size::CAPTION,
                c.fg_subtle,
            ))
            .padding(Padding {
                left: 16.0,
                ..Padding::ZERO
            })
            .into(),
        );
    }
    section.body(stacked(3.0, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_core::git::ChangedFile;

    fn file(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status: "M".into(),
            additions: Some(2),
            deletions: Some(1),
            ..ChangedFile::default()
        }
    }

    fn changes(files: Vec<ChangedFile>) -> Changes {
        Changes {
            root: "/work".into(),
            base: Some("abc1234".into()),
            files,
            truncated: false,
        }
    }

    #[test]
    fn edits_the_agent_reported_come_first_with_totals() {
        let edited = vec!["/work/a.rs".to_string(), "/work/b.rs".to_string()];
        let found = changes(vec![file("a.rs"), file("c.rs")]);
        let Listing::Edited {
            rows,
            more,
            totals,
            counted,
            count,
        } = listing(&edited, Some(&found), "/work")
        else {
            panic!("a listing of edits");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!((more, totals, counted, count), (0, (2, 1), true, 2));
    }

    #[test]
    fn without_git_the_edits_are_a_count_not_totals() {
        let edited = vec!["/work/a.rs".to_string()];
        let Listing::Edited {
            counted,
            totals,
            count,
            ..
        } = listing(&edited, None, "/work")
        else {
            panic!("a listing of edits");
        };
        assert_eq!((counted, totals, count), (false, (0, 0), 1));
    }

    #[test]
    fn many_edits_are_cut_to_ten_and_the_rest_counted() {
        let edited: Vec<String> = (0..14).map(|i| format!("/work/f{i}.rs")).collect();
        let Listing::Edited {
            rows, more, count, ..
        } = listing(&edited, None, "/work")
        else {
            panic!("a listing of edits");
        };
        assert_eq!((rows.len(), more, count), (10, 4, 14));
    }

    #[test]
    fn a_pane_with_no_reported_edits_lists_its_folder() {
        let found = changes((0..11).map(|i| file(&format!("f{i}.rs"))).collect());
        let Listing::Folder { rows, more } = listing(&[], Some(&found), "/work") else {
            panic!("a listing of the folder");
        };
        assert_eq!((rows.len(), more), (8, 3));
    }

    #[test]
    fn with_nothing_known_nothing_is_listed() {
        assert_eq!(listing(&[], None, "/work"), Listing::Nothing);
    }
}
