//! The macOS clipboard: the general pasteboard.
//!
//! A pasteboard has no "open" session and no compare-and-set. Its change count moves each time
//! an app takes it over, so the count is the sequence number, and every rule is built on it:
//!
//! - A read is thrown away and started again when the count moved while it ran: what was read
//!   may belong to a newer copy than the types that were checked (a password manager's, say).
//! - A write checks the count, takes the pasteboard over, and checks the count that step
//!   returns. A copy that landed in between is lost (the pasteboard stays empty and the write
//!   reports an error), but it is never replaced by our text.
//!
//! Every call fetches the pasteboard itself and may come from any thread; one lock keeps this
//! process's call sequences apart.

use std::sync::{Mutex, MutexGuard};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSString;
use zeroize::Zeroizing;

use crate::bridge::MAX_INPUT_BYTES;
use crate::clipscrub::{self, ClipError, ClipText, Clipboard};
use crate::platform::SystemClipboard;

/// Set by password managers on what they copy (nspasteboard.org).
pub const CONCEALED: &str = "org.nspasteboard.ConcealedType";
/// Set by apps on a copy they will take back in a moment (nspasteboard.org).
pub const TRANSIENT: &str = "org.nspasteboard.TransientType";
/// Plain text, UTF-8: what almost every app offers.
pub const UTF8: &str = "public.utf8-plain-text";
/// Plain text, UTF-16: all a few older apps offer.
pub const UTF16: &str = "public.utf16-plain-text";

/// How often a read starts again because another copy landed while it ran.
const READ_ATTEMPTS: usize = 3;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

/// The general pasteboard. Holds nothing: every call asks AppKit for the pasteboard again.
pub struct MacClipboard;

/// The change count as the sequence number (it only ever needs to differ).
fn count(pasteboard: &NSPasteboard) -> u32 {
    pasteboard.changeCount() as u32
}

/// One look at the pasteboard. The caller checks that the change count stood still.
fn look(pasteboard: &NSPasteboard) -> ClipText {
    let Some(types) = pasteboard.types() else {
        return ClipText::Empty;
    };
    let (mut utf8, mut utf16) = (false, false);
    for name in types.to_vec() {
        match name.to_string().as_str() {
            CONCEALED | TRANSIENT => return ClipText::Protected,
            UTF8 => utf8 = true,
            UTF16 => utf16 = true,
            _ => {}
        }
    }
    // The length is known before a byte is copied into Rust.
    if utf8 {
        match pasteboard.dataForType(&NSString::from_str(UTF8)) {
            None => ClipText::Empty,
            Some(data) if data.len() > MAX_INPUT_BYTES => ClipText::TooLarge,
            Some(data) => clipscrub::text_from_utf8(Zeroizing::new(data.to_vec())),
        }
    } else if utf16 {
        match pasteboard.dataForType(&NSString::from_str(UTF16)) {
            None => ClipText::Empty,
            Some(data) if data.len() > 2 * MAX_INPUT_BYTES + 2 => ClipText::TooLarge,
            Some(data) => clipscrub::text_from_utf16(&Zeroizing::new(data.to_vec())),
        }
    } else {
        // An image, files: no text.
        ClipText::Empty
    }
}

impl Clipboard for MacClipboard {
    fn sequence(&self) -> u32 {
        let _lock = one_at_a_time();
        autoreleasepool(|_| count(&NSPasteboard::generalPasteboard()))
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        let _lock = one_at_a_time();
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            for _ in 0..READ_ATTEMPTS {
                let before = pasteboard.changeCount();
                let seen = look(&pasteboard);
                if pasteboard.changeCount() == before {
                    return Ok(seen);
                }
                // Another copy landed meanwhile: `seen` is dropped (text zeroized) unseen.
            }
            Err(ClipError)
        })
    }

    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
        let _lock = one_at_a_time();
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            if count(&pasteboard) != seq {
                return Ok(false);
            }
            // Taking the pasteboard over moves the count by one. Any other number means a
            // copy landed after the check: it is gone now, and our text does not replace it.
            let ours = pasteboard.clearContents() as u32;
            if ours != seq.wrapping_add(1) {
                return Err(ClipError);
            }
            let written =
                pasteboard.setString_forType(&NSString::from_str(text), &NSString::from_str(UTF8));
            if !written || count(&pasteboard) != ours {
                return Err(ClipError);
            }
            Ok(true)
        })
    }

    /// macOS has no clipboard history an app can reach.
    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for MacClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipError> {
        let _lock = one_at_a_time();
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            if pasteboard.setString_forType(&NSString::from_str(text), &NSString::from_str(UTF8)) {
                Ok(())
            } else {
                Err(ClipError)
            }
        })
    }
}
