//! The clipboard contract (`support/clipboard_contract.rs`) on a Wayland compositor with
//! data-control, plus what only Wayland has: the password-manager hint as a mime type, senders
//! that send too much, nothing, or stop early, the bounded write race, and what a quit leaves.
//!
//! These tests own the clipboard of the compositor they run on, so they are `#[ignore]`d and
//! panic unless `REDACTOR_TEST_WAYLAND` names the throwaway compositor that
//! `tools/ci/wayland-tests.sh` started (headless sway; `kwin_wayland --virtual` in the Plasma
//! container). Only that script sets it, and runs them one at a time.
#![cfg(target_os = "linux")]

#[path = "support/clipboard_contract.rs"]
mod contract;
#[path = "support/wayland_other.rs"]
mod wayland_other;
#[path = "support/wayland_session.rs"]
mod wayland_session;

use std::process::Command;
use std::time::{Duration, Instant};

use contract::{unique, OtherApp};
use redactor_app::bridge::MAX_INPUT_BYTES;
use redactor_app::clipscrub::{ClipText, Clipboard};
use redactor_app::platform::linux::wayland::{self, DataControlClipboard, DataControlKind};
use redactor_app::platform::SystemClipboard;
use wayland_other::{WaylandOther, HINT, TEXT_MIMES};
use wayland_session::compositor;

/// The app's clipboard and another app, both on the test compositor.
fn desktop() -> (DataControlClipboard, WaylandOther) {
    let cb = DataControlClipboard::connect().expect("connect to the test compositor");
    (cb, WaylandOther::new())
}

fn read_text(cb: &DataControlClipboard) -> Option<String> {
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
            #[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
            fn $check() {
                compositor();
                let (cb, other) = desktop();
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

/// The probe names the protocol the clipboard will use: `ext` where offered (Plasma 6), else
/// `zwlr` (sway, KWin 5.27).
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn ext_or_wlr_is_reported() {
    compositor();
    let expected = match std::env::var("REDACTOR_WAYLAND_COMPOSITOR").as_deref() {
        Ok("kwin") => DataControlKind::Ext,
        _ => DataControlKind::Wlr,
    };
    let globals = wayland::globals().expect("probe the test compositor");
    assert_eq!(globals.data_control, Some(expected));
    assert!(globals.seat, "no seat reported");
}

/// A sender that never stops: the read stops at the input limit and returns.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn endless_sender_is_refused_at_the_cap() {
    compositor();
    let (cb, other) = desktop();
    other.copy_endless();
    let started = Instant::now();
    assert!(matches!(cb.read().unwrap(), ClipText::TooLarge));
    assert!(started.elapsed() < Duration::from_secs(12));
}

/// A sender that never writes and keeps the pipe open: the read gives up, it does not hang,
/// and the next copy reads normally.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn silent_sender_times_out() {
    compositor();
    let (cb, other) = desktop();
    other.copy_silent();
    let started = Instant::now();
    assert!(cb.read().is_err());
    assert!(started.elapsed() < Duration::from_secs(12));

    let text = format!("after the silence {}", unique());
    other.copy_text(&text);
    assert_eq!(read_text(&cb), Some(text));
}

/// A sender that closes the pipe after a few bytes: what came is the text.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn sender_closing_early_is_text_so_far_or_error() {
    compositor();
    let (cb, other) = desktop();
    let short = ["sh", "ort"].concat();
    other.copy_short_then_close(short.as_bytes());
    assert_eq!(read_text(&cb), Some(short));
}

/// A sender that writes one byte at a time: the read collects the whole text (and holds no more
/// memory than the text needs; `pipe.rs` pins that).
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn trickling_sender_reads_whole() {
    compositor();
    let (cb, other) = desktop();
    let mut text = String::new();
    while text.len() < 3000 {
        text.push_str(&format!("a slow sender {} ", unique()));
    }
    other.copy_trickle(&text);
    assert_eq!(read_text(&cb), Some(text));
}

/// Only the value `secret` marks a password-manager copy.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn hint_with_another_value_is_ordinary_text() {
    compositor();
    let (cb, other) = desktop();
    let text = format!("ordinary text {}", unique());
    let mut mimes: Vec<(&str, &[u8])> = TEXT_MIMES.iter().map(|m| (*m, text.as_bytes())).collect();
    mimes.push((HINT, b"public"));
    other.copy_with(&mimes);
    assert_eq!(read_text(&cb), Some(text));
}

/// The sequence number moves with every copy and stays put in between, whatever we read.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn sequence_moves_once_per_copy() {
    compositor();
    let (cb, other) = desktop();
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

/// Our own write counts as a change like any other, and reads back as written.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn our_own_text_reads_back_and_bumps_the_sequence() {
    compositor();
    let (cb, other) = desktop();
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

/// A large text we write reaches another app whole, byte for byte.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn large_cleaned_text_reaches_wl_paste() {
    compositor();
    let (cb, other) = desktop();
    let text = varied_text(3 * 1024 * 1024);
    cb.write_text(&text).unwrap();
    let pasted = other.paste().expect("wl-paste got nothing");
    assert_eq!(pasted.len(), text.len());
    assert!(pasted == text, "the pasted text differs");
}

/// A measurement of the write race. Wayland has no server grab: `write_if_unchanged` compares
/// at a round trip and then sets, and a copy that reaches the compositor inside that round trip
/// is overwritten (a Known limit). Here another app's copy is sent right before each write,
/// based on a sequence number from before that copy. Always checked: no write fails, and after
/// `Ok(false)` ours never lands. Measured (the OBSERVED lines): how many writes went through
/// (`wrote`) and how often ours ended on top of the newer copy after `Ok(true)` (`lost`).
///
/// Whether writes go through at all depends on the compositor's scheduling (whether it handles
/// our sync before the copy that was sent first). In CI the race window was not reached: sway 1.9
/// and KWin 6.6 on the GitHub runners gave wrote=0 left-alone=2000, so the 10 % bound on `lost`
/// did not bite there. It bites only where writes land: local sway runs saw up to 2.25 % lost
/// with the compare-and-set on the worker, while comparing and setting from the calling thread
/// (the plain order) lost 14-19 %, and no comparison at all would lose nearly every round.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn write_race_is_bounded() {
    const ROUNDS: u32 = 2000;
    compositor();
    let (cb, other) = desktop();
    let (mut wrote, mut left_alone, mut lost) = (0u32, 0u32, 0u32);
    for n in 0..ROUNDS {
        let seq = cb.sequence();
        other.start_copy(&format!("theirs {n}"));
        // 0 to 49 microseconds: the other copy is handled before our write arrives, or while
        // it arrives.
        let started = Instant::now();
        while started.elapsed() < Duration::from_micros(u64::from(n % 50)) {
            std::hint::spin_loop();
        }
        let result = cb.write_if_unchanged(&format!("ours {n}"), seq);
        let theirs_on_top = other.copy_is_in_place();
        match result {
            Ok(true) => {
                wrote += 1;
                if !theirs_on_top {
                    lost += 1;
                }
            }
            Ok(false) => {
                left_alone += 1;
                assert!(
                    theirs_on_top,
                    "round {n}: our text landed after the write said it did nothing"
                );
            }
            Err(e) => panic!("round {n}: the write failed: {e:?}"),
        }
    }
    println!("OBSERVED write_race lost={lost}/{ROUNDS}");
    println!("OBSERVED write_race wrote={wrote} left-alone={left_alone} of {ROUNDS}");
    assert!(
        lost <= ROUNDS / 10,
        "our text replaced another app's newer copy in more than 10 % of the rounds"
    );
}

/// sway has no clipboard manager: once Redactor quits, its text is gone from the clipboard. A
/// Known limit, recorded here so the install guide says what really happens.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn quit_drops_our_selection_on_sway() {
    compositor();
    let other = WaylandOther::new();
    other.copy_text(&format!("before the app {}", unique()));
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["quit_helper", "--exact", "--ignored", "--nocapture"])
        .env("WAYLAND_QUIT_HELPER", "1")
        .output()
        .expect("start the helper process");
    assert!(out.status.success(), "the helper process failed");
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l == "written"),
        "the helper process did not write"
    );
    // The compositor notices the closed connection a moment later.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut kept = other.has_selection();
    while kept && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        kept = other.has_selection();
    }
    let compositor_name = std::env::var("REDACTOR_WAYLAND_COMPOSITOR").unwrap_or_default();
    println!(
        "OBSERVED quit_drops_our_selection_on_sway compositor={compositor_name} selection-after-quit={}",
        if kept { "kept" } else { "gone" }
    );
    if compositor_name != "kwin" {
        assert!(
            !kept,
            "the clipboard still holds a selection after the quit"
        );
    }
}

/// Not a test: the helper process of `quit_drops_our_selection_on_sway`, an app that writes
/// and quits. Returns at once in a normal test run.
#[test]
#[ignore = "the helper process of quit_drops_our_selection_on_sway; does nothing without WAYLAND_QUIT_HELPER"]
fn quit_helper() {
    if std::env::var_os("WAYLAND_QUIT_HELPER").is_none() {
        return;
    }
    compositor();
    let cb = DataControlClipboard::connect().expect("connect to the test compositor");
    cb.write_text(&format!("gone with the app {}", unique()))
        .unwrap();
    cb.keep_after_exit();
    // The test harness may have printed on this line already.
    println!("\nwritten");
    // Out at once, as a quitting app is: nothing of ours runs after this.
    std::process::exit(0);
}
