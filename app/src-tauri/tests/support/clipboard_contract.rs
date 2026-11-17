//! The clipboard contract every `SystemClipboard` implementation must pass on its own OS
//! (`docs/superpowers/specs/2026-11-12-port-ci-test-strategy.md`, the six contract points).
//!
//! A runner per implementation includes this file with
//! `#[path = "support/clipboard_contract.rs"] mod contract;`, supplies the clipboard and an
//! [`OtherApp`] that acts like another application on the same desktop, and calls each check
//! from one `#[test]`. The in-memory fake runs it everywhere (`fake_clipboard_contract.rs`); real
//! clipboards run it on their desktop (`win_clipboard.rs`, later macOS and Linux).
//!
//! Test text is made up and built at run time.

use redactor_app::bridge::MAX_INPUT_BYTES;
use redactor_app::clipscrub::{self, ClipStatus, ClipText, ClipboardScrub};
use redactor_app::platform::SystemClipboard;
use redactor_core::{Engine, EngineConfig, Sensitivity};

/// Another application on the same desktop: the side the app must not trust or overwrite.
pub trait OtherApp {
    fn copy_text(&self, text: &str);
    /// Copies text the way this OS's password managers mark it (Windows
    /// ExcludeClipboardContentFromMonitorProcessing, macOS org.nspasteboard.ConcealedType,
    /// Linux x-kde-passwordManagerHint = secret).
    fn copy_protected(&self, text: &str);
    fn copy_image(&self);
    fn clear(&self);
    /// What a paste in another app would get now.
    fn paste(&self) -> Option<String>;
}

/// Test text with a password in it; `tag` keeps runs apart.
pub fn secret_text(tag: &str) -> (String, String) {
    let pw = ["Plum", "tree", "63", "-", "Kite", tag].concat();
    (format!("DB_PASSWORD={pw}\r\n"), pw)
}

pub fn unique() -> String {
    format!("{}", std::process::id())
}

pub fn scrub(cb: &dyn SystemClipboard) -> ClipboardScrub {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    clipscrub::scrub(cb, &engine, Sensitivity::Balanced, false, true)
}

fn read_text(cb: &dyn SystemClipboard) -> Option<String> {
    match cb.read().unwrap() {
        ClipText::Text(t) => Some(t.to_string()),
        _ => None,
    }
}

/// 1. Text another app copies reads back unchanged: emoji, umlauts, CRLF, tabs.
pub fn text_round_trip(cb: &dyn SystemClipboard, other: &dyn OtherApp) {
    let text = format!("😀 Grüße\r\n\tline two {}\r\n", unique());
    other.copy_text(&text);
    assert_eq!(read_text(cb).as_deref(), Some(text.as_str()));
    assert_eq!(
        clipscrub::read_text_for_ui(cb).unwrap().as_deref(),
        Some(text.as_str())
    );
}

/// 2. A password manager's copy is never read, scanned or rewritten.
pub fn protected_copy_is_left_alone(cb: &dyn SystemClipboard, other: &dyn OtherApp) {
    let (text, _) = secret_text(&unique());
    other.copy_protected(&text);
    assert!(matches!(cb.read().unwrap(), ClipText::Protected));
    assert_eq!(clipscrub::read_text_for_ui(cb).unwrap(), None);
    let seq = cb.sequence();
    let r = scrub(cb);
    assert_eq!(r.status, ClipStatus::Protected);
    assert_eq!(cb.sequence(), seq, "the clipboard was rewritten");
    assert_eq!(other.paste().as_deref(), Some(text.as_str()));
}

/// 3. An empty clipboard and an image both read as empty, and an image is never replaced.
pub fn empty_and_image_read_as_empty(cb: &dyn SystemClipboard, other: &dyn OtherApp) {
    other.clear();
    assert!(matches!(cb.read().unwrap(), ClipText::Empty));
    assert_eq!(scrub(cb).status, ClipStatus::Empty);

    other.copy_image();
    let seq = cb.sequence();
    assert!(matches!(cb.read().unwrap(), ClipText::Empty));
    assert_eq!(scrub(cb).status, ClipStatus::Empty);
    assert_eq!(cb.sequence(), seq, "the image was replaced");
    assert_eq!(clipscrub::read_text_for_ui(cb).unwrap(), None);
}

/// 4. Text over the input limit is refused before it is copied out, and never rewritten.
pub fn oversized_text_never_reaches_the_ui(cb: &dyn SystemClipboard, other: &dyn OtherApp) {
    let big = "a".repeat(MAX_INPUT_BYTES + 1);
    other.copy_text(&big);
    assert!(matches!(cb.read().unwrap(), ClipText::TooLarge));
    assert!(clipscrub::read_text_for_ui(cb)
        .unwrap_err()
        .contains("20 MiB"));
    let seq = cb.sequence();
    assert_eq!(scrub(cb).status, ClipStatus::TooLarge);
    assert_eq!(cb.sequence(), seq, "the clipboard was rewritten");
}

/// 5. A copy that lands between reading and writing wins: the stale write does nothing.
pub fn stale_sequence_blocks_the_write(cb: &dyn SystemClipboard, other: &dyn OtherApp) {
    other.copy_text(&format!("first {}", unique()));
    let stale = cb.sequence();
    let second = format!("second {}", unique());
    other.copy_text(&second);
    assert!(!cb.write_if_unchanged("third", stale).unwrap());
    assert_eq!(other.paste().as_deref(), Some(second.as_str()));
    let now = cb.sequence();
    assert!(cb.write_if_unchanged("third", now).unwrap());
    assert_eq!(other.paste().as_deref(), Some("third"));
}

/// 6. Scrubbing rewrites the clipboard only when something was found.
pub fn scrub_rewrites_only_when_findings(cb: &dyn SystemClipboard, other: &dyn OtherApp) {
    let (text, pw) = secret_text(&unique());
    other.copy_text(&text);
    let r = scrub(cb);
    assert_eq!(r.status, ClipStatus::Cleaned);
    assert_eq!(r.removed, 1);
    let now = other.paste().unwrap();
    assert!(!now.contains(&pw));
    assert!(now.starts_with("DB_PASSWORD=[REDACTED"));

    // Clean text is left exactly as it is.
    let clean = format!("lunch at noon {}", unique());
    other.copy_text(&clean);
    let seq = cb.sequence();
    let r = scrub(cb);
    assert_eq!(r.status, ClipStatus::Clean);
    assert_eq!(cb.sequence(), seq);
    assert_eq!(other.paste().as_deref(), Some(clean.as_str()));
}
