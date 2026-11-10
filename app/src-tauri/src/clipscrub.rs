//! Scrubbing the clipboard in place, independent of the platform clipboard.
//!
//! Rules: a password-manager copy is never read or rewritten (`protected`);
//! no text (nothing, an image, files) is `empty`; text over 20 MiB is `tooLarge` and is never
//! copied out of the clipboard; the clipboard is rewritten only when the scan redacted
//! something, and only if nobody changed the clipboard meanwhile (checked inside the same open
//! clipboard session that writes). After a rewrite the raw item is removed from the clipboard
//! history (best effort, `historyCleaned`). When the engine could not verify its output (the
//! verify loop hit its cap), the redacted text is still written, as it is safer than the
//! original, but the result is `unverified` and never claims the text is safe to paste.

use redactor_core::{Engine, Sensitivity};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::bridge::{self, LabelCount};

/// What the clipboard holds, as far as scrubbing is concerned.
pub enum ClipText {
    Text(Zeroizing<String>),
    /// No text: nothing at all, an image, files.
    Empty,
    /// Marked by a password manager (`ExcludeClipboardContentFromMonitorProcessing`).
    Protected,
    /// More than [`bridge::MAX_INPUT_BYTES`] of UTF-8: not copied out of the clipboard.
    TooLarge,
}

/// The UI-facing message for clipboard text over the limit.
pub const TOO_LARGE_MESSAGE: &str = "The clipboard text is larger than 20 MiB.";

/// How much of a NUL-terminated UTF-16 buffer is text, if that text fits in `max_bytes` of
/// UTF-8: the number of code units before the first NUL (or all of them). `None` when the
/// text would be larger. Looks at no more units than the limit needs and allocates nothing,
/// so an oversized clipboard is refused before any copy is made.
pub fn utf16_text_len(units: &[u16], max_bytes: usize) -> Option<usize> {
    let mut bytes = 0usize;
    for (i, &u) in units.iter().enumerate() {
        bytes += match u {
            0 => return Some(i),
            0x0001..=0x007F => 1,
            0x0080..=0x07FF => 2,
            // Each half of a surrogate pair stands for 2 of the 4 bytes.
            0xD800..=0xDFFF => 2,
            _ => 3,
        };
        if bytes > max_bytes {
            return None;
        }
    }
    Some(units.len())
}

/// The platform clipboard. Errors carry no content.
pub trait Clipboard {
    /// Changes whenever the clipboard content changes.
    fn sequence(&self) -> u32;
    fn read(&self) -> Result<ClipText, ClipError>;
    /// Replaces the clipboard with `text` only if its sequence number is still `seq`. The
    /// number is compared after the clipboard is opened for writing and before it is emptied,
    /// in the same open session, so no other copy can land in between. `Ok(false)`: the
    /// clipboard changed, nothing was written.
    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError>;
    /// Removes history items whose text equals `raw`. True when one was removed.
    fn delete_from_history(&self, raw: &str) -> bool;
}

/// A clipboard operation failed (busy, out of memory). Never carries text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipError;

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The clipboard is busy or unavailable.")
    }
}

impl std::error::Error for ClipError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipStatus {
    /// Rewritten, and the rescan of the output came back clean.
    Cleaned,
    /// Rewritten, but the engine could not verify the output: check before pasting.
    Unverified,
    Clean,
    Empty,
    Protected,
    TooLarge,
    Error,
}

/// The `scrubClipboard` result and `clipboard-scrubbed` event payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardScrub {
    pub status: ClipStatus,
    pub removed: usize,
    pub by_label: Vec<LabelCount>,
    pub history_cleaned: bool,
}

impl ClipboardScrub {
    fn status(status: ClipStatus) -> ClipboardScrub {
        ClipboardScrub {
            status,
            removed: 0,
            by_label: Vec::new(),
            history_cleaned: false,
        }
    }

    /// The notification text after a hotkey scrub.
    pub fn notice(&self) -> String {
        match self.status {
            ClipStatus::Cleaned => format!(
                "{} {} removed, safe to paste",
                self.removed,
                if self.removed == 1 {
                    "secret"
                } else {
                    "secrets"
                }
            ),
            ClipStatus::Unverified if self.removed > 0 => format!(
                "{} {} removed, check before pasting",
                self.removed,
                if self.removed == 1 {
                    "secret"
                } else {
                    "secrets"
                }
            ),
            ClipStatus::Unverified => "Check before pasting".to_string(),
            ClipStatus::Clean => "Nothing sensitive found".to_string(),
            ClipStatus::Empty => "The clipboard holds no text".to_string(),
            ClipStatus::Protected => {
                "Left alone: a password manager marked that copy private".to_string()
            }
            ClipStatus::TooLarge => {
                "Left alone: the clipboard text is larger than 20 MiB".to_string()
            }
            ClipStatus::Error => "The clipboard could not be scrubbed".to_string(),
        }
    }
}

/// Scans the clipboard text and rewrites it when something was redacted.
pub fn scrub(
    cb: &impl Clipboard,
    engine: &Engine,
    sensitivity: Sensitivity,
    pii: bool,
    strip_invisible: bool,
) -> ClipboardScrub {
    scrub_with(cb, sensitivity, pii, strip_invisible, |req| {
        bridge::scan(engine, req)
    })
}

/// [`scrub`] with the scan passed in, so tests can hand back any engine result.
fn scrub_with(
    cb: &impl Clipboard,
    sensitivity: Sensitivity,
    pii: bool,
    strip_invisible: bool,
    scan: impl FnOnce(&bridge::ScanRequestIn) -> bridge::ScanResponseOut,
) -> ClipboardScrub {
    let seq = cb.sequence();
    let mut text = match cb.read() {
        Ok(ClipText::Text(t)) => t,
        Ok(ClipText::Empty) => return ClipboardScrub::status(ClipStatus::Empty),
        Ok(ClipText::Protected) => return ClipboardScrub::status(ClipStatus::Protected),
        Ok(ClipText::TooLarge) => return ClipboardScrub::status(ClipStatus::TooLarge),
        Err(_) => return ClipboardScrub::status(ClipStatus::Error),
    };
    if text.is_empty() {
        return ClipboardScrub::status(ClipStatus::Empty);
    }
    // The request takes the text over (no copy) and zeroizes it when dropped.
    let req = bridge::plain_request(
        std::mem::take(&mut *text),
        sensitivity,
        pii,
        strip_invisible,
    );
    if req.text.len() > bridge::MAX_INPUT_BYTES {
        return ClipboardScrub::status(ClipStatus::TooLarge);
    }
    let resp = scan(&req);
    let removed = resp.removed();
    if removed == 0 {
        return ClipboardScrub::status(if resp.verified {
            ClipStatus::Clean
        } else {
            ClipStatus::Unverified
        });
    }
    // Someone copied something else while we scanned: leave their copy alone.
    match cb.write_if_unchanged(&resp.output, seq) {
        Ok(true) => {}
        Ok(false) | Err(_) => return ClipboardScrub::status(ClipStatus::Error),
    }
    let history_cleaned = cb.delete_from_history(&req.text);
    ClipboardScrub {
        status: if resp.verified {
            ClipStatus::Cleaned
        } else {
            ClipStatus::Unverified
        },
        removed,
        by_label: resp.stats.by_label,
        history_cleaned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redactor_core::EngineConfig;
    use std::cell::{Cell, RefCell};

    enum Fake {
        Text(String),
        Empty,
        Protected,
    }

    struct FakeClipboard {
        content: RefCell<Fake>,
        seq: Cell<u32>,
        writes: Cell<u32>,
        history: RefCell<Vec<String>>,
        /// Simulates another app copying during the scan.
        bump_on_read: bool,
        /// Simulates another app copying while the write waits to open the clipboard.
        bump_before_write: bool,
    }

    impl FakeClipboard {
        fn new(content: Fake) -> FakeClipboard {
            let history = match &content {
                Fake::Text(t) => vec![t.clone()],
                _ => vec![],
            };
            FakeClipboard {
                content: RefCell::new(content),
                seq: Cell::new(1),
                writes: Cell::new(0),
                history: RefCell::new(history),
                bump_on_read: false,
                bump_before_write: false,
            }
        }
        fn text(&self) -> Option<String> {
            match &*self.content.borrow() {
                Fake::Text(t) => Some(t.clone()),
                _ => None,
            }
        }
    }

    impl Clipboard for FakeClipboard {
        fn sequence(&self) -> u32 {
            self.seq.get()
        }
        fn read(&self) -> Result<ClipText, ClipError> {
            if self.bump_on_read {
                self.seq.set(self.seq.get() + 1);
            }
            Ok(match &*self.content.borrow() {
                Fake::Text(t) => ClipText::Text(Zeroizing::new(t.clone())),
                Fake::Empty => ClipText::Empty,
                Fake::Protected => ClipText::Protected,
            })
        }
        fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
            if self.bump_before_write {
                self.seq.set(self.seq.get() + 1);
            }
            if self.seq.get() != seq {
                return Ok(false);
            }
            self.writes.set(self.writes.get() + 1);
            self.seq.set(self.seq.get() + 1);
            *self.content.borrow_mut() = Fake::Text(text.to_string());
            self.history.borrow_mut().push(text.to_string());
            Ok(true)
        }
        fn delete_from_history(&self, raw: &str) -> bool {
            let mut h = self.history.borrow_mut();
            let before = h.len();
            h.retain(|t| t != raw);
            h.len() < before
        }
    }

    fn engine() -> Engine {
        Engine::new(EngineConfig::default()).unwrap()
    }

    fn secret_text() -> (String, String) {
        let pw = ["Plum", "tree", "51", "-", "Kite"].concat();
        (format!("DB_PASSWORD={pw}\n"), pw)
    }

    fn run(cb: &FakeClipboard) -> ClipboardScrub {
        scrub(cb, &engine(), Sensitivity::Balanced, false, true)
    }

    #[test]
    fn scrub_in_place_rewrites_only_when_findings() {
        let (text, pw) = secret_text();
        let cb = FakeClipboard::new(Fake::Text(text.clone()));
        let r = run(&cb);
        assert_eq!(r.status, ClipStatus::Cleaned);
        assert_eq!(r.removed, 1);
        assert!(r.history_cleaned);
        assert_eq!(r.by_label.len(), 1);
        let now = cb.text().unwrap();
        assert!(!now.contains(&pw));
        assert!(now.starts_with("DB_PASSWORD=[REDACTED"));
        assert!(
            !cb.history.borrow().contains(&text),
            "raw item left in history"
        );

        let clean = FakeClipboard::new(Fake::Text("just a note about lunch".into()));
        let r = run(&clean);
        assert_eq!(r.status, ClipStatus::Clean);
        assert_eq!(clean.writes.get(), 0);
        assert!(!r.history_cleaned);

        // Scrubbing the scrubbed text again finds nothing and writes nothing.
        let again = FakeClipboard::new(Fake::Text(now));
        assert_eq!(run(&again).status, ClipStatus::Clean);
        assert_eq!(again.writes.get(), 0);
    }

    #[test]
    fn protected_and_empty_are_never_rewritten() {
        let p = FakeClipboard::new(Fake::Protected);
        assert_eq!(run(&p).status, ClipStatus::Protected);
        assert_eq!(p.writes.get(), 0);
        let e = FakeClipboard::new(Fake::Empty);
        assert_eq!(run(&e).status, ClipStatus::Empty);
        assert_eq!(e.writes.get(), 0);
        let blank = FakeClipboard::new(Fake::Text(String::new()));
        assert_eq!(run(&blank).status, ClipStatus::Empty);
    }

    #[test]
    fn changed_clipboard_is_left_alone() {
        let (text, _) = secret_text();
        let mut cb = FakeClipboard::new(Fake::Text(text.clone()));
        cb.bump_on_read = true;
        assert_eq!(run(&cb).status, ClipStatus::Error);
        assert_eq!(cb.writes.get(), 0);
        assert_eq!(cb.text().unwrap(), text);
    }

    #[test]
    fn a_copy_landing_just_before_the_write_is_left_alone() {
        let (text, _) = secret_text();
        let mut cb = FakeClipboard::new(Fake::Text(text.clone()));
        cb.bump_before_write = true;
        assert_eq!(run(&cb).status, ClipStatus::Error);
        assert_eq!(cb.writes.get(), 0);
        assert_eq!(cb.text().unwrap(), text);
    }

    #[test]
    fn utf16_text_len_stops_at_nul_and_at_the_limit() {
        let units: Vec<u16> = "ab\u{e9}\u{20ac}\u{1F600}".encode_utf16().collect();
        // 1 + 1 + 2 + 3 + 4 bytes.
        assert_eq!(utf16_text_len(&units, 11), Some(units.len()));
        assert_eq!(utf16_text_len(&units, 10), None);
        let mut nul = units.clone();
        nul.insert(2, 0);
        nul.extend([0x41; 100]);
        assert_eq!(utf16_text_len(&nul, 2), Some(2));
        assert_eq!(utf16_text_len(&[], 0), Some(0));
        // A huge buffer is judged after looking at only `max_bytes + 1` units.
        let huge = vec![0x41u16; 1 << 20];
        assert_eq!(utf16_text_len(&huge, 1000), None);
    }

    #[test]
    fn too_large_is_its_own_status() {
        struct Big;
        impl Clipboard for Big {
            fn sequence(&self) -> u32 {
                1
            }
            fn read(&self) -> Result<ClipText, ClipError> {
                Ok(ClipText::TooLarge)
            }
            fn write_if_unchanged(&self, _: &str, _: u32) -> Result<bool, ClipError> {
                panic!("must not write")
            }
            fn delete_from_history(&self, _: &str) -> bool {
                false
            }
        }
        let r = scrub(&Big, &engine(), Sensitivity::Balanced, false, true);
        assert_eq!(r.status, ClipStatus::TooLarge);
        assert_eq!(serde_json::to_value(&r).unwrap()["status"], "tooLarge");
        assert!(r.notice().contains("20 MiB"));
    }

    #[test]
    fn unverified_output_is_written_but_never_called_safe() {
        let (text, pw) = secret_text();
        let cb = FakeClipboard::new(Fake::Text(text.clone()));
        // The engine's verify loop hit its cap: the output is redacted but not verified.
        let r = scrub_with(&cb, Sensitivity::Balanced, false, true, |req| {
            let mut resp = bridge::scan(&engine(), req);
            resp.verified = false;
            resp.warnings.push(bridge::WarningOut {
                kind: bridge::WarningKindOut::VerifyCapHit,
                count: 1,
            });
            resp
        });
        assert_eq!(r.status, ClipStatus::Unverified);
        assert_eq!(r.removed, 1);
        assert!(r.history_cleaned);
        // Still written: the redacted text is safer than the original.
        assert_eq!(cb.writes.get(), 1);
        assert!(!cb.text().unwrap().contains(&pw));
        assert_eq!(r.notice(), "1 secret removed, check before pasting");
        assert!(!r.notice().contains("safe"));
        assert_eq!(serde_json::to_value(&r).unwrap()["status"], "unverified");
    }

    #[test]
    fn notices_and_json() {
        let mut r = ClipboardScrub::status(ClipStatus::Cleaned);
        r.removed = 2;
        assert_eq!(r.notice(), "2 secrets removed, safe to paste");
        assert_eq!(
            ClipboardScrub::status(ClipStatus::Clean).notice(),
            "Nothing sensitive found"
        );
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "cleaned");
        assert_eq!(v["historyCleaned"], false);
        assert!(v["byLabel"].is_array());
    }
}
