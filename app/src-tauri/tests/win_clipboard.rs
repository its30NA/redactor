//! Windows clipboard and history tests. They use the real clipboard
//! of the desktop they run on, so they are `#[ignore]`d and run explicitly by the Windows build
//! loop (`tools/win-build.sh test`), one at a time.
//!
//! Safety for the person at that desktop: each test saves the current clipboard text first and
//! puts it back afterwards (kept out of the history). If the clipboard holds a password-manager
//! copy or non-text content that could not be restored, the test does nothing and says so.
//! Only history items the test itself created are deleted.
#![cfg(windows)]

use std::sync::Mutex;

use redactor_app::clipscrub::{self, ClipStatus, ClipText, Clipboard};
use redactor_app::win::clipboard::testing::{self, Saved};
use redactor_app::win::clipboard::WinClipboard;
use redactor_app::win::history;
use redactor_core::{Engine, EngineConfig, Sensitivity};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Runs `body` with the user's clipboard saved and restored. Returns false when skipped.
fn with_saved_clipboard(name: &str, body: impl FnOnce()) -> bool {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let saved = testing::save().expect("open the clipboard");
    match saved {
        Saved::Protected => {
            eprintln!("{name}: skipped, the clipboard holds a password-manager copy");
            return false;
        }
        Saved::Other => {
            eprintln!("{name}: skipped, the clipboard holds non-text content");
            return false;
        }
        _ => {}
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    testing::restore(&saved).expect("restore the clipboard");
    if let Err(p) = result {
        std::panic::resume_unwind(p);
    }
    true
}

/// Test text built at run time; `tag` keeps runs apart.
fn secret_text(tag: &str) -> (String, String) {
    let pw = ["Plum", "tree", "63", "-", "Kite", tag].concat();
    (format!("DB_PASSWORD={pw}\r\n"), pw)
}

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scrub() -> clipscrub::ClipboardScrub {
    clipscrub::scrub(&WinClipboard, &engine(), Sensitivity::Balanced, false, true)
}

fn read_text() -> Option<String> {
    match WinClipboard.read().unwrap() {
        ClipText::Text(t) => Some(t.to_string()),
        _ => None,
    }
}

fn unique() -> String {
    format!("{}", std::process::id())
}

#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn clipboard_roundtrip_text() {
    with_saved_clipboard("clipboard_roundtrip_text", || {
        let text = format!("😀 Grüße\r\n\tline two {}\r\n", unique());
        WinClipboard.write_private(&text).unwrap();
        assert_eq!(read_text().as_deref(), Some(text.as_str()));
        assert!(!WinClipboard.is_protected().unwrap());
    });
}

#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn protected_marker_respected() {
    with_saved_clipboard("protected_marker_respected", || {
        let (text, _) = secret_text(&unique());
        testing::write_marked(&text).unwrap();
        assert!(matches!(WinClipboard.read().unwrap(), ClipText::Protected));
        assert_eq!(
            redactor_app::win::clipboard::read_text_for_ui().unwrap(),
            None
        );
        let seq = WinClipboard.sequence();
        let r = scrub();
        assert_eq!(r.status, ClipStatus::Protected);
        assert_eq!(WinClipboard.sequence(), seq, "the clipboard was rewritten");
        assert_eq!(
            testing::raw_text().unwrap().as_deref().map(|s| s.as_str()),
            Some(text.as_str())
        );
    });
}

#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn empty_and_image_clipboard() {
    with_saved_clipboard("empty_and_image_clipboard", || {
        testing::clear().unwrap();
        assert!(matches!(WinClipboard.read().unwrap(), ClipText::Empty));
        assert_eq!(scrub().status, ClipStatus::Empty);

        testing::write_image().unwrap();
        let seq = WinClipboard.sequence();
        assert!(matches!(WinClipboard.read().unwrap(), ClipText::Empty));
        assert_eq!(scrub().status, ClipStatus::Empty);
        assert_eq!(WinClipboard.sequence(), seq, "the image was replaced");
        assert_eq!(
            redactor_app::win::clipboard::read_text_for_ui().unwrap(),
            None
        );
    });
}

#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn oversized_text_is_refused_without_reaching_the_ui() {
    with_saved_clipboard("oversized_text_is_refused_without_reaching_the_ui", || {
        let big = "a".repeat(redactor_app::bridge::MAX_INPUT_BYTES + 1);
        WinClipboard.write_private(&big).unwrap();
        assert!(matches!(WinClipboard.read().unwrap(), ClipText::TooLarge));
        assert!(redactor_app::win::clipboard::read_text_for_ui()
            .unwrap_err()
            .contains("20 MiB"));
        let seq = WinClipboard.sequence();
        assert_eq!(scrub().status, ClipStatus::TooLarge);
        assert_eq!(WinClipboard.sequence(), seq, "the clipboard was rewritten");
    });
}

#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn a_stale_sequence_number_blocks_the_write() {
    with_saved_clipboard("a_stale_sequence_number_blocks_the_write", || {
        let text = format!("first {}", unique());
        WinClipboard.write_private(&text).unwrap();
        let stale = WinClipboard.sequence();
        WinClipboard
            .write_private(&format!("second {}", unique()))
            .unwrap();
        assert!(!WinClipboard.write_if_unchanged("third", stale).unwrap());
        assert_eq!(read_text().unwrap(), format!("second {}", unique()));
        let now = WinClipboard.sequence();
        assert!(WinClipboard.write_if_unchanged("third", now).unwrap());
        assert_eq!(read_text().as_deref(), Some("third"));
    });
}

#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn scrub_in_place_rewrites_only_when_findings() {
    with_saved_clipboard("scrub_in_place_rewrites_only_when_findings", || {
        let (text, pw) = secret_text(&unique());
        // An ordinary copy, so it enters the Win+V history like a real one.
        WinClipboard.write_text(&text).unwrap();
        let history_on = history::enabled();
        if history_on {
            // The history picks new items up asynchronously.
            for _ in 0..20 {
                if history::contains(&text) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        let r = scrub();
        let now = read_text().unwrap();
        // Clean up the test's own history items whatever happens next.
        history::delete_matching(&now);
        let raw_left = history::contains(&text);
        history::delete_matching(&text);

        assert_eq!(r.status, ClipStatus::Cleaned);
        assert_eq!(r.removed, 1);
        assert!(!now.contains(&pw));
        assert!(now.starts_with("DB_PASSWORD=[REDACTED"));
        if history_on {
            assert!(
                r.history_cleaned,
                "the raw item was not removed from the history"
            );
            assert!(!raw_left);
        } else {
            eprintln!("clipboard history is off: history checks skipped");
        }

        // Clean text is left exactly as it is.
        let clean = format!("lunch at noon {}", unique());
        WinClipboard.write_private(&clean).unwrap();
        let seq = WinClipboard.sequence();
        let r = scrub();
        assert_eq!(r.status, ClipStatus::Clean);
        assert_eq!(WinClipboard.sequence(), seq);
        assert_eq!(read_text().as_deref(), Some(clean.as_str()));
    });
}
