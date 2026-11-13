//! The clipboard contract (`support/clipboard_contract.rs`) on a real X server, plus what only
//! X11 has: the password-manager hint, Latin-1 owners, INCR transfers, and owners that lie, hang
//! or die.
//!
//! These tests own the clipboard of the display they run on, so they are `#[ignore]`d and panic
//! unless `REDACTOR_TEST_DISPLAY` names a throwaway display. Only `tools/ci/x11-tests.sh` sets
//! it, on its own Xvfb, and runs them one at a time.
#![cfg(target_os = "linux")]

#[path = "support/clipboard_contract.rs"]
mod contract;
#[path = "support/x11_other.rs"]
mod x11_other;

use std::time::{Duration, Instant};

use contract::{unique, OtherApp};
use redactor_app::bridge::MAX_INPUT_BYTES;
use redactor_app::clipscrub::{ClipText, Clipboard};
use redactor_app::platform::linux::x11::X11Clipboard;
use redactor_app::platform::SystemClipboard;
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
    stale_sequence_blocks_the_write,
    scrub_rewrites_only_when_findings,
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

/// Made-up text of at least `bytes` bytes that differs from line to line.
fn varied_text(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes + 64);
    let mut line = 0u32;
    while text.len() < bytes {
        text.push_str(&format!("line {line} of the test text, äöü € 😀\n"));
        line += 1;
    }
    text
}

/// Text larger than one X request reaches another app whole (we serve it in pieces).
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn large_text_reaches_xclip() {
    let display = display();
    let (cb, other) = desktop(&display);
    let text = varied_text(2 * 1024 * 1024);
    cb.write_text(&text).unwrap();
    let pasted = other.paste().expect("xclip got nothing");
    assert_eq!(pasted.len(), text.len());
    assert!(pasted == text, "the pasted text differs");
}

/// The clipboard can change without a new owner: the owner exits or clears it. The sequence
/// moves, nothing is read, and a write based on the old state does nothing.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn owner_exit_moves_the_sequence_and_reads_empty() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.copy_text(&format!("about to vanish {}", unique()));
    let old = cb.sequence();
    other.clear();
    assert_ne!(cb.sequence(), old);
    assert!(matches!(cb.read().unwrap(), ClipText::Empty));
    assert!(!cb.write_if_unchanged("stale", old).unwrap());
    assert_eq!(other.paste(), None);
}

#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn our_own_text_reads_back_and_bumps_the_sequence() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.clear();
    let before = cb.sequence();
    let text = format!("our own text {}\r\n", unique());
    cb.write_text(&text).unwrap();
    assert_eq!(cb.sequence(), before.wrapping_add(1));
    assert_eq!(read_text(&cb), Some(text));
    assert_eq!(cb.sequence(), before.wrapping_add(1));

    // The limit holds for our own text too.
    cb.write_text(&"c".repeat(MAX_INPUT_BYTES + 1)).unwrap();
    assert!(matches!(cb.read().unwrap(), ClipText::TooLarge));
}

/// When another app copies, we stop being the source: its text is what everybody gets, and
/// ours is dropped. A client that still asks our window directly is refused.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn losing_ownership_forgets_the_text() {
    let display = display();
    let (cb, other) = desktop(&display);
    cb.write_text(&format!("ours {}", unique())).unwrap();
    let our_window = other.owner();
    assert!(
        other.asks_for_text(our_window),
        "the owner hands its text out"
    );

    let theirs = format!("theirs {}", unique());
    other.copy_text(&theirs);
    assert!(
        !other.asks_for_text(our_window),
        "the old text is still handed out"
    );
    assert_eq!(read_text(&cb), Some(theirs.clone()));
    assert_eq!(other.paste(), Some(theirs));
}

/// The app that holds the clipboard crashes: no new owner, but the clipboard is empty now.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn a_crashed_owner_moves_the_sequence_and_reads_empty() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.helper("hint-other", &format!("about to crash {}", unique()));
    let old = cb.sequence();
    other.kill_helpers();
    // The server notices the closed connection a moment later.
    let deadline = Instant::now() + Duration::from_secs(2);
    while cb.sequence() == old {
        assert!(Instant::now() < deadline, "the sequence did not move");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(cb.read().unwrap(), ClipText::Empty));
    assert!(!cb.write_if_unchanged("stale", old).unwrap());
    assert_eq!(other.paste(), None);
}

/// Two apps paste a large text at the same moment: two transfers in pieces side by side.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn two_readers_at_once_both_get_the_text() {
    let display = display();
    let (cb, other) = desktop(&display);
    let text = varied_text(1024 * 1024);
    cb.write_text(&text).unwrap();
    let (one, two) = std::thread::scope(|s| {
        let one = s.spawn(|| other.paste());
        let two = s.spawn(|| other.paste());
        (one.join().unwrap(), two.join().unwrap())
    });
    assert!(one.as_deref() == Some(text.as_str()), "the first reader");
    assert!(two.as_deref() == Some(text.as_str()), "the second reader");
}

/// The compare-and-set is one step for the X server. Another app's copy is already on its way
/// when our write starts, based on a sequence number from before that copy. Whichever of the
/// two the server handles first, the other app's copy must be what the clipboard holds in the
/// end: either our write saw it and did nothing, or it came after our write and replaced it.
/// A write that compares first and takes the clipboard later overwrites the newer copy.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn a_copy_in_flight_is_never_overwritten() {
    let display = display();
    let (cb, other) = desktop(&display);
    for round in 0..200 {
        let stale = cb.sequence();
        other.start_copy();
        let wrote = cb
            .write_if_unchanged("a write based on old news", stale)
            .unwrap();
        assert!(
            other.copy_is_in_place(),
            "round {round}: the newer copy was overwritten (the write returned {wrote})"
        );
    }
}

/// The same promise when the other app's copy arrives while our write is under way, at every
/// point of it: before the comparison, after the write, and in the gap between the two that
/// only the server grab closes. A second thread copies a little later each round.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn a_copy_during_the_write_is_never_overwritten() {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    const ROUNDS: u32 = 2000;
    let display = display();
    let (cb, other) = desktop(&display);
    let (go, copied, stop) = (AtomicU32::new(0), AtomicU32::new(0), AtomicBool::new(false));
    let mut failure = None;
    std::thread::scope(|s| {
        s.spawn(|| {
            for round in 1..=ROUNDS {
                while go.load(Ordering::SeqCst) < round {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    std::hint::spin_loop();
                }
                // 0 to 249 microseconds into the write.
                let started = Instant::now();
                while started.elapsed() < Duration::from_micros(u64::from(round % 250)) {
                    std::hint::spin_loop();
                }
                other.start_copy();
                copied.store(round, Ordering::SeqCst);
            }
        });
        for round in 1..=ROUNDS {
            let stale = cb.sequence();
            go.store(round, Ordering::SeqCst);
            let wrote = cb.write_if_unchanged("a write based on old news", stale);
            while copied.load(Ordering::SeqCst) < round {
                std::hint::spin_loop();
            }
            if wrote.is_err() || !other.copy_is_in_place() {
                failure = Some(format!("round {round}: the write returned {wrote:?}"));
                break;
            }
        }
        stop.store(true, Ordering::SeqCst);
    });
    assert_eq!(failure, None, "the newer copy was overwritten");
}

/// On X11 the clipboard is empty once its owner exits, unless a clipboard manager took the
/// text over. We ask it to before we go.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn the_manager_keeps_our_text() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.helper("manager", "");
    let text = format!("still here after the exit {}", unique());
    cb.write_text(&text).unwrap();
    cb.keep_after_exit();
    drop(cb);
    assert_eq!(other.paste(), Some(text));
}

/// A desktop without a clipboard manager does not hold the exit up.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn without_a_manager_nothing_waits() {
    let display = display();
    let (cb, _other) = desktop(&display);
    cb.write_text(&format!("nobody keeps this {}", unique()))
        .unwrap();
    let started = Instant::now();
    cb.keep_after_exit();
    assert!(started.elapsed() < Duration::from_millis(100));
}

/// A clipboard manager that never answers holds the exit up for one second, not longer.
#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn a_silent_manager_costs_one_second() {
    let display = display();
    let (cb, other) = desktop(&display);
    other.helper("manager-silent", "");
    cb.write_text(&format!("nobody saves this {}", unique()))
        .unwrap();
    let started = Instant::now();
    cb.keep_after_exit();
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_secs(2),
        "the hand-over took {took:?}"
    );
}
