//! The clipboard contract (`support/clipboard_contract.rs`) on a real X server, plus what only
//! X11 has: the password-manager hint, Latin-1 owners, INCR transfers, and owners that lie, hang
//! or die.
//!
//! These tests own the clipboard of the display they run on, so they are `#[ignore]`d and panic
//! unless `REDACTOR_TEST_DISPLAY` names a throwaway display. Only `tools/ci/x11-tests.sh` sets
//! it, on its own Xvfb, and runs them one at a time.
#![cfg(target_os = "linux")]

// Contract points 5 and 6 (writing) are not called yet.
#[allow(dead_code)]
#[path = "support/clipboard_contract.rs"]
mod contract;
#[path = "support/x11_other.rs"]
mod x11_other;

use std::time::{Duration, Instant};

use contract::{unique, OtherApp};
use redactor_app::bridge::MAX_INPUT_BYTES;
use redactor_app::clipscrub::{ClipText, Clipboard};
use redactor_app::platform::linux::x11::X11Clipboard;
use x11_other::{display, X11Other};

/// The app's clipboard and another app, both on the test display.
fn desktop(display: &str) -> (X11Clipboard, X11Other) {
    let cb = X11Clipboard::connect(Some(display)).expect("connect to the test display");
    (cb, X11Other::new(display))
}

fn read_text(cb: &X11Clipboard) -> Option<String> {
    match cb.read().unwrap() {
        ClipText::Text(t) => Some(t.to_string()),
        _ => None,
    }
}

/// One contract check per test, named as the contract names it.
macro_rules! contract_tests {
    ($($check:ident),* $(,)?) => {
        $(
            #[test]
            #[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
            fn $check() {
                let display = display();
                let (cb, other) = desktop(&display);
                contract::$check(&cb, &other);
            }
        )*
    };
}

contract_tests!(
    text_round_trip,
    protected_copy_is_left_alone,
    empty_and_image_read_as_empty,
    oversized_text_never_reaches_the_ui,
);

/// Only the value `secret` marks a password-manager copy.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn hint_with_another_value_is_ordinary_text() {
    let display = display();
    let (cb, other) = desktop(&display);
    let text = format!("ordinary text {}", unique());
    other.helper("hint-other", &text);
    assert_eq!(read_text(&cb), Some(text));
}

/// An old app that offers only `STRING` (Latin-1).
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn string_only_owner_reads_latin1() {
    let display = display();
    let (cb, other) = desktop(&display);
    let text = format!("Grüße {}", unique());
    other.helper("string-only", &text);
    assert_eq!(read_text(&cb), Some(text));
}

/// An owner that announces one byte and sends more than 20 MiB.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn understated_incr_is_still_refused() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.helper("understate", "");
    assert!(matches!(cb.read().unwrap(), ClipText::TooLarge));
}

/// Refusing a text that is too large must not break the clipboard for everybody else: `xclip`
/// serves one transfer at a time, and one that is abandoned half way blocks it for good.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn refused_large_text_can_still_be_pasted_elsewhere() {
    let display = display();
    let (cb, other) = desktop(&display);
    let big = "b".repeat(MAX_INPUT_BYTES + 1);
    other.copy_text(&big);
    assert!(matches!(cb.read().unwrap(), ClipText::TooLarge));
    assert_eq!(other.paste().map(|t| t.len()), Some(big.len()));
}

/// An owner that never answers: the read gives up, it does not hang.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn silent_owner_times_out() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.helper("silent", "");
    let started = Instant::now();
    assert!(cb.read().is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// An owner that exits in the middle of a large transfer; the next copy reads normally.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn owner_dying_mid_transfer_is_an_error() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.helper("die-mid-incr", "");
    assert!(cb.read().is_err());

    let text = format!("after the crash {}", unique());
    other.copy_text(&text);
    assert_eq!(read_text(&cb), Some(text));
}

/// The sequence number moves with every copy and stays put in between, whatever we read.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn sequence_moves_once_per_copy() {
    let display = display();
    let (cb, other) = desktop(&display);
    let first = format!("first {}", unique());
    other.copy_text(&first);
    let one = cb.sequence();
    assert_eq!(read_text(&cb), Some(first));
    assert_eq!(cb.sequence(), one);

    other.copy_text(&format!("second {}", unique()));
    let two = cb.sequence();
    assert_eq!(two, one.wrapping_add(1));
    assert_eq!(cb.sequence(), two);
}
