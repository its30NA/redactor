//! The clipboard contract (`support/clipboard_contract.rs`) on the real macOS pasteboard, plus
//! what only macOS has: transient copies, UTF-16-only text, and the two races its write and
//! read rules exist for (macOS has no compare-and-set).
//!
//! These tests take over the clipboard of the desktop they run on, so they are `#[ignore]`d and
//! panic unless `REDACTOR_TEST_PASTEBOARD` is set. Only `tools/ci/macos-tests.sh` sets it, and
//! runs them one at a time.
#![cfg(target_os = "macos")]

#[path = "support/clipboard_contract.rs"]
mod contract;
#[path = "support/mac_other.rs"]
mod mac_other;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use contract::{secret_text, unique, OtherApp};
use mac_other::{MacOther, OtherProcess};
use redactor_app::clipscrub::{ClipStatus, ClipText, Clipboard};
use redactor_app::platform::macos::clipboard::MacClipboard;
use redactor_app::platform::SystemClipboard;

/// The name of the test below that is the other process of the race tests.
const OTHER_PROCESS_TEST: &str = "the_other_process_of_the_race_tests";

const ROUNDS: u32 = 2000;

/// Refuses to touch a clipboard nobody offered.
fn guard() {
    assert!(
        std::env::var_os("REDACTOR_TEST_PASTEBOARD").is_some(),
        "this test takes over the clipboard: run tools/ci/macos-tests.sh"
    );
}

fn read_text(cb: &MacClipboard) -> Option<String> {
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
            #[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
            fn $check() {
                guard();
                contract::$check(&MacClipboard, &MacOther);
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

/// A copy its app will take back in a moment is left alone like a password manager's.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn transient_copy_is_left_alone() {
    guard();
    let (cb, other) = (MacClipboard, MacOther);
    let (text, _) = secret_text(&unique());
    other.copy_transient(&text);
    assert!(matches!(cb.read().unwrap(), ClipText::Protected));
    let seq = cb.sequence();
    assert_eq!(contract::scrub(&cb).status, ClipStatus::Protected);
    assert_eq!(cb.sequence(), seq, "the clipboard was rewritten");
    assert_eq!(other.paste().as_deref(), Some(text.as_str()));
}

/// Text an app offers as UTF-16 only is still read.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn utf16_only_text_is_read() {
    guard();
    let text = format!("Grüße 😀 {}", unique());
    MacOther.copy_utf16_only(&text);
    assert_eq!(read_text(&MacClipboard).as_deref(), Some(text.as_str()));
}

/// What the guarded write relies on: taking the pasteboard over moves the count by exactly
/// one when nobody else touches it.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn clear_moves_the_count_by_one() {
    guard();
    let other = MacOther;
    for round in 0..20 {
        other.copy_raw(&format!("round {round} {}", unique()));
        let before = other.change_count();
        let reported = other.clear_and_count();
        assert_eq!(reported, before + 1);
        assert_eq!(other.change_count(), reported);
    }
}

#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn our_own_text_reads_back_and_bumps_the_sequence() {
    guard();
    let (cb, other) = (MacClipboard, MacOther);
    other.copy_raw(&format!("before {}", unique()));
    let before = cb.sequence();
    let text = format!("ours 😀 Grüße\r\n\t{}", unique());
    cb.write_text(&text).unwrap();
    assert_ne!(cb.sequence(), before);
    assert_eq!(read_text(&cb).as_deref(), Some(text.as_str()));
    assert_eq!(other.paste().as_deref(), Some(text.as_str()));
}

/// A large copy of ours reaches another program whole.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn large_text_reaches_pbpaste() {
    guard();
    let mut big = String::with_capacity((2 << 20) + 64);
    let mut line = 0u64;
    while big.len() < 2 << 20 {
        big.push_str(&format!("line {line} Grüße 😀\t{}\n", line * 7919));
        line += 1;
    }
    MacClipboard.write_text(&big).unwrap();
    // Not assert_eq: a failure must not print two megabytes.
    assert!(
        mac_other::pbpaste() == big.as_bytes(),
        "pbpaste got other bytes"
    );
}

/// Not a test of its own: started by the race tests as a second process (see
/// `mac_other::OtherProcess`). Run as a test it does nothing.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn the_other_process_of_the_race_tests() {
    guard();
    if std::env::var_os(mac_other::OTHER_PROCESS_ENV).is_some() {
        mac_other::serve();
    }
}

/// Review focus: a password manager's copy that lands between "which types" and "give me the
/// text" is never handed out. Another process alternates plain and concealed copies as fast
/// as it can while we read.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn a_concealed_copy_is_never_read_in_a_race() {
    guard();
    let mut other = OtherProcess::start(OTHER_PROCESS_TEST);
    let stop = AtomicBool::new(false);
    let (mut plain, mut protected, mut empty, mut gave_up, mut hidden) = (0, 0, 0, 0, 0);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut n = 0u32;
            while !stop.load(Ordering::SeqCst) {
                other.send(&format!("raw plain {n}"));
                other.done();
                other.send(&format!("concealed hidden {n}"));
                other.done();
                n += 1;
            }
        });
        for _ in 0..ROUNDS {
            match MacClipboard.read() {
                Ok(ClipText::Text(t)) if t.starts_with("plain") => plain += 1,
                // Counted, not asserted here: a panic would leave the other thread running.
                Ok(ClipText::Text(_) | ClipText::TooLarge) => hidden += 1,
                Ok(ClipText::Protected) => protected += 1,
                Ok(ClipText::Empty) => empty += 1,
                Err(_) => gave_up += 1,
            }
        }
        stop.store(true, Ordering::SeqCst);
    });
    println!(
        "OBSERVED concealed-race plain={plain} protected={protected} empty={empty} gave-up={gave_up} of {ROUNDS}"
    );
    assert_eq!(hidden, 0, "a concealed copy was read");
    assert!(
        plain > 0 && protected > 0,
        "the race never happened: both kinds of copy must have been seen"
    );
}

/// Review focus: a copy from another process that lands during our write is never replaced
/// by our text. Afterwards the clipboard holds the other copy, or it is empty and our write
/// reported an error.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn a_copy_during_the_write_is_never_replaced_by_ours() {
    guard();
    let (cb, start) = (MacClipboard, MacOther);
    let mut other = OtherProcess::start(OTHER_PROCESS_TEST);
    let (mut wrote, mut left_alone, mut errors, mut lost) = (0, 0, 0, 0);
    let (mut ours_won, mut unexplained) = (0, 0);
    for n in 0..ROUNDS {
        start.copy_raw(&format!("start {n}"));
        let seq = cb.sequence();
        let (theirs, ours) = (format!("theirs {n}"), format!("ours {n}"));
        // Both sides wait a little, each by its own pattern, so that the other copy lands
        // before, inside and after our write.
        other.send(&format!("raw-after {} {theirs}", n % 250));
        mac_other::spin(Duration::from_micros(u64::from(n / 250 % 8) * 50));
        let result = cb.write_if_unchanged(&ours, seq);
        other.done();

        match result {
            Ok(true) => wrote += 1,
            Ok(false) => left_alone += 1,
            Err(_) => errors += 1,
        }
        match mac_other::raw_text() {
            Some(now) if now == theirs => {}
            None if result.is_err() => lost += 1,
            Some(now) if now == ours => ours_won += 1,
            _ => unexplained += 1,
        }
    }
    println!("OBSERVED lost-copies={lost}/{ROUNDS}");
    println!(
        "OBSERVED write-race wrote={wrote} left-alone={left_alone} errors={errors} of {ROUNDS}"
    );
    assert_eq!(ours_won, 0, "our text replaced another app's newer copy");
    assert_eq!(
        unexplained, 0,
        "the clipboard held neither copy, without an error"
    );
}
