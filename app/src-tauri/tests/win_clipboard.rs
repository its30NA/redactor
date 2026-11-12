//! The clipboard contract (`support/clipboard_contract.rs`) on the real Windows clipboard, plus
//! the Win+V history check. They use the real clipboard of the desktop they run on, so they are `#[ignore]`d and run explicitly by the Windows build
//! loop (`tools/win-build.sh test`), one at a time.
//!
//! Safety for the person at that desktop: each test saves the current clipboard text first and
//! puts it back afterwards (kept out of the history). If the clipboard holds a password-manager
//! copy or non-text content that could not be restored, the test does nothing and says so.
//! Only history items the test itself created are deleted.
#![cfg(windows)]

#[path = "support/clipboard_contract.rs"]
mod contract;

use std::sync::Mutex;

use contract::{secret_text, unique, OtherApp};
use redactor_app::clipscrub::{ClipStatus, ClipText, Clipboard};
use redactor_app::platform::windows::clipboard::testing::{self, Saved};
use redactor_app::platform::windows::clipboard::WinClipboard;
use redactor_app::platform::windows::history;

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

/// Another app on this desktop. Its plain copies stay out of the Win+V history.
struct WinOther;

impl OtherApp for WinOther {
    fn copy_text(&self, text: &str) {
        WinClipboard.write_private(text).unwrap();
    }

    fn copy_protected(&self, text: &str) {
        testing::write_marked(text).unwrap();
    }

    fn copy_image(&self) {
        testing::write_image().unwrap();
    }

    fn clear(&self) {
        testing::clear().unwrap();
    }

    fn paste(&self) -> Option<String> {
        testing::raw_text().unwrap().map(|t| t.to_string())
    }
}

/// One contract check per test; the names are the Windows loop's, kept from before the
/// contract existed so its counts compare directly.
macro_rules! contract_tests {
    ($($test:ident => $check:ident),* $(,)?) => {
        $(
            #[test]
            #[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
            fn $test() {
                with_saved_clipboard(stringify!($test), || contract::$check(&WinClipboard, &WinOther));
            }
        )*
    };
}

contract_tests!(
    clipboard_roundtrip_text => text_round_trip,
    protected_marker_respected => protected_copy_is_left_alone,
    empty_and_image_clipboard => empty_and_image_read_as_empty,
    oversized_text_is_refused_without_reaching_the_ui => oversized_text_never_reaches_the_ui,
    a_stale_sequence_number_blocks_the_write => stale_sequence_blocks_the_write,
    scrub_in_place_rewrites_only_when_findings => scrub_rewrites_only_when_findings,
);

/// Windows only: an ordinary copy enters the Win+V history, and scrubbing it removes the raw
/// item from there too.
#[test]
#[ignore = "uses the desktop clipboard; run by tools/win-build.sh"]
fn scrub_removes_the_raw_copy_from_history() {
    with_saved_clipboard("scrub_removes_the_raw_copy_from_history", || {
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
        let r = contract::scrub(&WinClipboard);
        let now = match WinClipboard.read().unwrap() {
            ClipText::Text(t) => t.to_string(),
            _ => panic!("the scrubbed clipboard holds no text"),
        };
        // Clean up the test's own history items whatever happens next.
        history::delete_matching(&now);
        let raw_left = history::contains(&text);
        history::delete_matching(&text);

        assert_eq!(r.status, ClipStatus::Cleaned);
        assert!(!now.contains(&pw));
        if history_on {
            assert!(
                r.history_cleaned,
                "the raw item was not removed from the history"
            );
            assert!(!raw_left);
        } else {
            eprintln!("clipboard history is off: history checks skipped");
        }
    });
}
