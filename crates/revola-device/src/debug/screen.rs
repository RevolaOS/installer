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
    body.sort_by_key(|t| (t.top, t.left));
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
    fn screens_without_rows_list_their_text() {
        let s = describe(&[t("Now Playing", 2, 6, 113, 1), t("Artist", 80, 160, 300, 1)]);
        assert_eq!((s.state, s.title.as_str()), ("other", "Now Playing"));
        assert_eq!(s.text, ["Artist"]);
    }
}
