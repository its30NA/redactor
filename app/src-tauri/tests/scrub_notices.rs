//! What a scrub result says, from the table the UI's tests read too
//! (`tests/fixtures/scrub_notices.json`): the window's toast and the desktop notification
//! use the same words for the same result.

use redactor_app::clipscrub::{expired, ClipStatus, ClipboardScrub};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    status: String,
    removed: usize,
    history_cleaned: bool,
    title: String,
    detail: String,
}

const STATUSES: [ClipStatus; 8] = [
    ClipStatus::Cleaned,
    ClipStatus::Unverified,
    ClipStatus::Stripped,
    ClipStatus::Clean,
    ClipStatus::Empty,
    ClipStatus::Protected,
    ClipStatus::TooLarge,
    ClipStatus::Error,
];

fn status(name: &str) -> Option<ClipStatus> {
    STATUSES
        .into_iter()
        .find(|s| serde_json::to_value(s).unwrap() == name)
}

#[test]
fn every_result_in_the_shared_table() {
    let raw = include_str!("fixtures/scrub_notices.json");
    let cases: Vec<Case> = serde_json::from_str(raw).expect("valid case table");
    let mut seen = Vec::new();
    for c in &cases {
        let words = if c.status == "expired" {
            expired()
        } else {
            let status =
                status(&c.status).unwrap_or_else(|| panic!("unknown status {:?}", c.status));
            seen.push(status);
            ClipboardScrub {
                status,
                removed: c.removed,
                by_label: Vec::new(),
                history_cleaned: c.history_cleaned,
            }
            .words()
        };
        assert_eq!(
            (words.title, words.detail.as_str()),
            (c.title.as_str(), c.detail.as_str()),
            "{} removed {}",
            c.status,
            c.removed
        );
    }
    for s in STATUSES {
        assert!(seen.contains(&s), "{s:?} is not in the table");
    }
    assert!(
        cases.iter().any(|c| c.status == "expired"),
        "the expiry is not in the table"
    );
}

#[test]
fn the_notification_is_the_title_then_the_detail() {
    let r = ClipboardScrub {
        status: ClipStatus::Cleaned,
        removed: 2,
        by_label: Vec::new(),
        history_cleaned: true,
    };
    assert_eq!(
        r.notice(),
        "Clipboard scrubbed. 2 secrets removed, safe to paste · history cleaned."
    );
    let clean = ClipboardScrub {
        status: ClipStatus::Clean,
        removed: 0,
        by_label: Vec::new(),
        history_cleaned: false,
    };
    assert_eq!(
        clean.notice(),
        "Clipboard is clean. Nothing sensitive found."
    );
    assert_eq!(
        expired().notice(),
        "Not scrubbed. Redactor's window did not come to the front in time."
    );
}
