// SPDX-License-Identifier: GPL-3.0-only
//! A screen description from the strings the UI drew in one frame.

use super::protocol::DrawnText;
use serde::Serialize;

/// Bottom edge of the title bar.
const TITLE_BAR: i16 = 21;
/// Status icons sit right of this in the title bar.
const STATUS_X: i16 = 260;
/// A half-width list ends here; the preview pane starts.
const HALF_WIDTH: i16 = 160;
/// Strings whose tops are this close share a row.
const ROW_SLACK: i16 = 4;
/// OSOS 2.0.5 thumbnail lists: labels repeat every 55 px, details sit 29 px below.
const THUMBNAIL_HEIGHT: i16 = 55;
const THUMBNAIL_DETAIL: i16 = 29;
/// View class of the selected row's text in OSOS 2.0.5.
pub(crate) const SELECTED_TEXT_VIEW: u32 = 0x0899_3504;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Row {
    pub index: usize,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Screen {
    /// "list" when rows were found, "other" otherwise.
    pub state: &'static str,
    pub title: String,
    pub rows: Vec<Row>,
    pub selected: Option<usize>,
    /// Strings of the preview pane beside a half-width list.
    pub preview: Vec<String>,
    /// Every other string, top to bottom, for screens without a list.
    pub text: Vec<String>,
}

impl Screen {
    pub fn row(&self, label: &str) -> Option<&Row> {
        self.rows
            .iter()
            .find(|r| r.label.eq_ignore_ascii_case(label))
    }
}

pub(crate) fn describe(drawn: &[DrawnText]) -> Screen {
    let mut screen = Screen::default();
    let mut body: Vec<&DrawnText> = Vec::new();
    for t in drawn.iter().filter(|t| !t.text.trim().is_empty()) {
        if t.bottom <= TITLE_BAR + 2 && t.top < TITLE_BAR {
            if t.left < STATUS_X && screen.title.is_empty() {
                screen.title = t.text.clone();
            }
        } else {
            body.push(t);
        }
    }
    body.sort_by_key(|t| (t.top, t.left, t.view_class != SELECTED_TEXT_VIEW));
    body.dedup_by(|b, a| a.top == b.top && a.left == b.left && a.text == b.text);
    // Anchor the row grid to the selected label; an absent artist/detail
    // must not turn the next album's label into the preceding album's value.
    let selected_top = body
        .iter()
        .find(|t| t.view_class == SELECTED_TEXT_VIEW)
        .map(|t| t.top);
    if body.len() >= 2
        && body[0].left >= 20
        && body[0].left < HALF_WIDTH
        && body.iter().all(|t| t.left == body[0].left)
        && selected_top.is_some_and(|top| {
            body.iter().all(|t| {
                let offset = (t.top - top).rem_euclid(THUMBNAIL_HEIGHT);
                offset == 0 || (offset == THUMBNAIL_DETAIL && t.view_class != SELECTED_TEXT_VIEW)
            })
        })
    {
        screen.state = "list";
        let top = selected_top.unwrap();
        for label in body
            .iter()
            .filter(|t| (t.top - top).rem_euclid(THUMBNAIL_HEIGHT) == 0)
        {
            let index = screen.rows.len();
            let selected = label.view_class == SELECTED_TEXT_VIEW;
            if selected {
                screen.selected = Some(index);
            }
            screen.rows.push(Row {
                index,
                label: label.text.trim_end().to_string(),
                value: body
                    .iter()
                    .find(|t| t.top == label.top + THUMBNAIL_DETAIL)
                    .map(|t| t.text.trim_end().to_string()),
                selected,
            });
        }
        return screen;
    }
    // A list is two or more bands that each start at the left edge.
    let mut bands: Vec<Vec<&DrawnText>> = Vec::new();
    for t in body.iter().copied() {
        match bands.last_mut() {
            Some(band) if (t.top - band[0].top).abs() <= ROW_SLACK => band.push(t),
            _ => bands.push(vec![t]),
        }
    }
    let row_bands: Vec<&Vec<&DrawnText>> = bands
        .iter()
        .filter(|band| band.iter().any(|t| t.left < 20))
        .collect();
    let half = row_bands.iter().all(|band| {
        band.iter()
            .filter(|t| t.left < 20)
            .all(|t| t.right <= HALF_WIDTH)
    });
    if row_bands.len() >= 2 {
        screen.state = "list";
        for band in &row_bands {
            let mut in_list: Vec<&DrawnText> = band
                .iter()
                .copied()
                .filter(|t| !half || t.left < HALF_WIDTH)
                .collect();
            in_list.sort_by_key(|t| (t.left, t.view_class != SELECTED_TEXT_VIEW));
            // The selected row's label can be drawn twice in one frame.
            in_list.dedup_by(|b, a| b.text == a.text);
            let Some(label) = in_list.first() else {
                continue;
            };
            let index = screen.rows.len();
            let selected = in_list.iter().any(|t| t.view_class == SELECTED_TEXT_VIEW);
            if selected {
                screen.selected = Some(index);
            }
            screen.rows.push(Row {
                index,
                label: label.text.clone(),
                value: in_list.get(1).map(|t| t.text.clone()),
                selected,
            });
        }
        if half {
            screen.preview = body
                .iter()
                .filter(|t| t.left >= HALF_WIDTH)
                .map(|t| t.text.clone())
                .collect();
        }
    } else {
        screen.state = "other";
        screen.text = body.iter().map(|t| t.text.clone()).collect();
    }
    screen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str, top: i16, left: i16, right: i16, class: u32) -> DrawnText {
        DrawnText {
            text: text.into(),
            top,
            left,
            bottom: top + 21,
            right,
            color: 0,
            view_class: class,
        }
    }

    #[test]
    fn settings_list_with_preview_pane() {
        let drawn = [
            t("Settings", 2, 6, 113, 1),
            t("Repeat", 72, 5, 154, 1),
            t("Off", 72, 107, 154, 1),
            t("About", 23, 5, 154, 1),
            t("Shuffle", 48, 5, 154, SELECTED_TEXT_VIEW),
            t("Off", 48, 107, 154, 1),
            t("Synthetic", 20, 180, 300, 1),
            t("283 MB Free", 190, 170, 310, 1),
        ];
        let s = describe(&drawn);
        assert_eq!(s.state, "list");
        assert_eq!(s.title, "Settings");
        let labels: Vec<_> = s.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["About", "Shuffle", "Repeat"]);
        assert_eq!(s.rows[1].value.as_deref(), Some("Off"));
        assert_eq!(s.selected, Some(1));
        assert_eq!(s.preview, ["Synthetic", "283 MB Free"]);
        assert!(s.row("shuffle").is_some());
    }

    #[test]
    fn a_selected_label_drawn_twice_is_one_row() {
        let s = describe(&[
            t("Music", 23, 5, 154, 1),
            t("Podcasts", 48, 5, 154, 1),
            t("Podcasts", 48, 5, 154, SELECTED_TEXT_VIEW),
        ]);
        assert_eq!(s.rows[1].label, "Podcasts");
        assert_eq!(s.rows[1].value, None);
        assert_eq!(s.selected, Some(1));
    }

    #[test]
    fn thumbnail_album_list_keeps_artist_with_its_album() {
        let s = describe(&[
            t("Albums", 2, 6, 273, 1),
            t("All Songs", 20, 65, 293, 1),
            t("All Songs", 20, 65, 293, SELECTED_TEXT_VIEW),
            t("3491 Songs", 49, 65, 293, 1),
            t("666", 75, 65, 314, 1),
            t("Aphrodite’s Child     ", 104, 65, 314, 1),
            t("777", 130, 65, 314, 1),
            t("Latto     ", 159, 65, 314, 1),
        ]);
        assert_eq!(s.state, "list");
        assert_eq!(s.rows.len(), 3);
        assert_eq!(s.selected, Some(0));
        assert_eq!(
            s.row("666").unwrap().value.as_deref(),
            Some("Aphrodite’s Child")
        );
    }

    #[test]
    fn inset_text_without_a_selected_row_is_not_a_thumbnail_list() {
        let s = describe(&[
            t("Text", 2, 6, 273, 1),
            t("One", 40, 65, 314, 1),
            t("Two", 70, 65, 314, 1),
            t("Three", 100, 65, 314, 1),
            t("Four", 130, 65, 314, 1),
        ]);
        assert_eq!(s.state, "other");
        assert!(s.rows.is_empty());
    }

    #[test]
    fn missing_thumbnail_details_do_not_shift_labels() {
        for missing in [49, 104, 159] {
            let drawn = [
                t("Albums", 2, 6, 273, 1),
                t("All Songs", 20, 65, 293, 1),
                t("3491 Songs", 49, 65, 293, 1),
                t("666", 75, 65, 314, SELECTED_TEXT_VIEW),
                t("Aphrodite’s Child", 104, 65, 314, 1),
                t("777", 130, 65, 314, 1),
                t("Latto", 159, 65, 314, 1),
            ];
            let drawn: Vec<_> = drawn.into_iter().filter(|t| t.top != missing).collect();
            let s = describe(&drawn);
            assert_eq!(
                s.rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
                ["All Songs", "666", "777"]
            );
            assert_eq!(s.selected, Some(1));
            assert_eq!(s.rows[((missing - 49) / 55) as usize].value, None);
        }
    }

    #[test]
    fn thumbnail_labels_without_details_remain_separate_rows() {
        let s = describe(&[
            t("Albums", 2, 6, 273, 1),
            t("One", 20, 65, 314, SELECTED_TEXT_VIEW),
            t("Two", 75, 65, 314, 1),
        ]);
        assert_eq!(s.rows.len(), 2);
        assert!(s.rows.iter().all(|r| r.value.is_none()));
    }

    #[test]
    fn screens_without_rows_list_their_text() {
        let s = describe(&[t("Now Playing", 2, 6, 113, 1), t("Artist", 80, 160, 300, 1)]);
        assert_eq!((s.state, s.title.as_str()), ("other", "Now Playing"));
        assert_eq!(s.text, ["Artist"]);
    }
}
