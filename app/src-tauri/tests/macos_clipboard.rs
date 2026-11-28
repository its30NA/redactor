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
use std::time::{Duration, Instant};

use contract::{secret_text, unique, OtherApp};
use mac_other::{MacOther, OtherProcess};
use redactor_app::clipscrub::{ClipStatus, ClipText, Clipboard};
use redactor_app::platform::macos::clipboard::MacClipboard;
use redactor_app::platform::SystemClipboard;

/// The name of the test below that is the other process of the race tests.
const OTHER_PROCESS_TEST: &str = "the_other_process_of_the_race_tests";

const ROUNDS: u32 = 2000;

/// How long the concealed-copy race reads on, past `ROUNDS`, until both kinds of copy were seen.
const DEADLINE: Duration = Duration::from_secs(10);

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

/// Text the pasteboard lists but does not hand out (a lazy provider that gives nothing, or a
/// read macOS refuses) is a failed read, never "no text": the text may still be pasteable.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn listed_text_without_data_is_an_error() {
    guard();
    let (cb, other) = (MacClipboard, MacOther);
    other.list_text_without_data();
    assert!(
        cb.read().is_err(),
        "a listed text type without data read as text or empty"
    );
    let seq = cb.sequence();
    assert_eq!(contract::scrub(&cb).status, ClipStatus::Error);
    assert_eq!(cb.sequence(), seq, "the clipboard was rewritten");
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
/// text" is never handed out. Another process alternates plain and concealed copies while we
/// read, each left for 1 to 4 ms: macOS hands a copy to the pasteboard server a moment after
/// the call returns, so copies made back to back are never seen (CI round 3: no plain copy in
/// 181,830 reads).
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn a_concealed_copy_is_never_read_in_a_race() {
    guard();
    let mut other = OtherProcess::start(OTHER_PROCESS_TEST);
    // CI round 1: 2000 reads ran before the other process's first copy landed. So its first
    // copy is waited for, and the reads go on until both kinds were seen (or a deadline).
    other.send("raw plain start");
    other.done();
    let stop = AtomicBool::new(false);
    let (mut plain, mut protected, mut empty, mut gave_up, mut hidden) = (0, 0, 0, 0, 0);
    let (mut reads, mut first) = (0u32, 0);
    let started = Instant::now();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut n = 0u32;
            while !stop.load(Ordering::SeqCst) {
                let hold = Duration::from_millis(u64::from(n % 4) + 1);
                other.send(&format!("raw plain {n}"));
                other.done();
                mac_other::spin(hold);
                other.send(&format!("concealed hidden {n}"));
                other.done();
                mac_other::spin(hold);
                n += 1;
            }
        });
        while reads < ROUNDS || ((plain == 0 || protected == 0) && started.elapsed() < DEADLINE) {
            reads += 1;
            match MacClipboard.read() {
                // The first copy, made before the race, does not count as the race.
                Ok(ClipText::Text(t)) if t.as_str() == "plain start" => first += 1,
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
        "OBSERVED concealed-race first-copy={first} plain={plain} protected={protected} empty={empty} gave-up={gave_up} of {reads} reads in {} ms",
        started.elapsed().as_millis()
    );
    assert_eq!(hidden, 0, "a concealed copy was read");
    assert!(
        plain > 0 && protected > 0,
        "the race never happened: both kinds of copy must have been seen"
    );
}

/// Review focus: a copy from another process that lands during our write. Afterwards the
/// clipboard holds the other copy, or it is empty and our write reported an error, or (the
/// Known limit) our safe text, after a write whose every check passed: macOS hands a copy to
/// the pasteboard server a moment after the call returns, so nothing in our process can see
/// that the other copy came first. That last case is measured, not hidden: at most 2 % of the
/// forced races (CI rounds 2-3: 9 and 2 of 2000), never after an error.
#[test]
#[ignore = "takes over the clipboard: tools/ci/macos-tests.sh"]
fn a_copy_during_the_write_is_never_covered_after_an_error() {
    guard();
    let (cb, start) = (MacClipboard, MacOther);
    let mut other = OtherProcess::start(OTHER_PROCESS_TEST);
    let (mut wrote, mut left_alone, mut errors, mut lost) = (0, 0, 0, 0);
    // Which write result left our text on top, and how often the clipboard still changed
    // after the other process said it was done (CI round 2 could not tell).
    let (mut ours_after_ok, mut ours_after_err, mut unexplained, mut unsettled) = (0, 0, 0, 0);
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
        let first_look = mac_other::raw_text();
        mac_other::spin(Duration::from_millis(2));
        let settled = mac_other::raw_text();
        if settled != first_look {
            unsettled += 1;
        }
        match settled {
            Some(now) if now == theirs => {}
            None if result.is_err() => lost += 1,
            Some(now) if now == ours && result.is_ok() => ours_after_ok += 1,
            Some(now) if now == ours => ours_after_err += 1,
            _ => unexplained += 1,
        }
    }
    println!("OBSERVED lost-copies={lost}/{ROUNDS}");
    println!(
        "OBSERVED write-race wrote={wrote} left-alone={left_alone} errors={errors} of {ROUNDS}"
    );
    println!(
        "OBSERVED write-race-ours-on-top after-ok={ours_after_ok} after-error={ours_after_err} changed-after-done={unsettled}"
    );
    assert_eq!(
        ours_after_err, 0,
        "our text stayed on top of another app's newer copy after our write reported an error"
    );
    assert!(
        ours_after_ok <= ROUNDS / 50,
        "our text replaced another app's newer copy in more than 2 % of the forced races"
    );
    assert_eq!(
        unexplained, 0,
        "the clipboard held neither copy, without an error"
    );
}
