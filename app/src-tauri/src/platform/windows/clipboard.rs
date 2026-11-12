//! The Win32 clipboard, our own code instead of a clipboard plugin.
//!
//! Content marked `ExcludeClipboardContentFromMonitorProcessing` (password-manager copies) is
//! never read. Text we read is held in `Zeroizing` buffers. Text over the 20 MiB input limit is
//! measured in place (`GlobalSize`, then the UTF-16 units up to the limit) and refused before
//! anything is copied out of the clipboard.

use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use zeroize::Zeroizing;

use crate::bridge::MAX_INPUT_BYTES;
use crate::clipscrub::{utf16_text_len, ClipError, ClipText, Clipboard};

/// `CF_UNICODETEXT`.
pub const CF_UNICODETEXT: u32 = 13;

/// Format names with a documented meaning for clipboard monitors and the history.
pub const EXCLUDE_FROM_MONITORS: PCWSTR = w!("ExcludeClipboardContentFromMonitorProcessing");
pub const CAN_INCLUDE_IN_HISTORY: PCWSTR = w!("CanIncludeInClipboardHistory");
pub const CAN_UPLOAD_TO_CLOUD: PCWSTR = w!("CanUploadToCloudClipboard");

/// Registers (or looks up) a named clipboard format.
pub fn format_id(name: PCWSTR) -> u32 {
    // SAFETY: `name` is a static NUL-terminated wide string from `w!`.
    unsafe { RegisterClipboardFormatW(name) }
}

/// The clipboard's Unicode text, as far as reading it goes.
enum Unicode {
    Text(Zeroizing<String>),
    None,
    TooLarge,
}

/// The clipboard is opened for the lifetime of this guard.
struct Open;

impl Open {
    /// Another app may hold the clipboard for a moment: retry for about half a second.
    fn new() -> Result<Open, ClipError> {
        for _ in 0..25 {
            // SAFETY: no owner window; paired with CloseClipboard in Drop.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Open);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(ClipError)
    }

    fn has(&self, format: u32) -> bool {
        // SAFETY: the clipboard is open.
        unsafe { IsClipboardFormatAvailable(format) }.is_ok()
    }

    /// The clipboard's Unicode text. Text longer than [`MAX_INPUT_BYTES`] of UTF-8 is
    /// `TooLarge`: measured on the locked block without copying it.
    fn text(&self) -> Unicode {
        // SAFETY: the clipboard is open; the handle stays owned by the clipboard and is only
        // locked for the measurement and copy below.
        unsafe {
            let Ok(h) = GetClipboardData(CF_UNICODETEXT) else {
                return Unicode::None;
            };
            let mem = HGLOBAL(h.0);
            let p = GlobalLock(mem) as *const u16;
            if p.is_null() {
                return Unicode::None;
            }
            let max = GlobalSize(mem) / 2;
            // SAFETY: `p` points to `max` u16 units of locked global memory.
            let units = std::slice::from_raw_parts(p, max);
            // At most MAX_INPUT_BYTES + 1 units are looked at: the size is known before any
            // allocation.
            let text = match utf16_text_len(units, MAX_INPUT_BYTES) {
                Some(len) => Unicode::Text(Zeroizing::new(String::from_utf16_lossy(&units[..len]))),
                None => Unicode::TooLarge,
            };
            let _ = GlobalUnlock(mem);
            text
        }
    }

    fn sequence(&self) -> u32 {
        // SAFETY: no arguments, no preconditions.
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Replaces the clipboard with `text` plus the given DWORD-valued formats.
    fn set_text(&self, text: &str, dword_formats: &[(u32, u32)]) -> Result<(), ClipError> {
        let units: Zeroizing<Vec<u16>> =
            Zeroizing::new(text.encode_utf16().chain(std::iter::once(0)).collect());
        // SAFETY: the clipboard is open. Each global block is filled while locked and then
        // handed to the clipboard, which owns it from then on; on failure we free it.
        unsafe {
            EmptyClipboard().map_err(|_| ClipError)?;
            put(CF_UNICODETEXT, bytes_of(&units))?;
            for &(fmt, value) in dword_formats {
                put(fmt, &value.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

fn bytes_of(units: &[u16]) -> &[u8] {
    // SAFETY: u16 has no padding; the byte view covers exactly the same memory.
    unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len() * 2) }
}

/// Copies `data` into a movable global block and gives it to the clipboard.
///
/// # Safety
/// The clipboard must be open and emptied by this process.
unsafe fn put(format: u32, data: &[u8]) -> Result<(), ClipError> {
    // SAFETY: plain allocation of `data.len()` bytes (at least 1).
    let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)) }.map_err(|_| ClipError)?;
    // SAFETY: `mem` is a fresh block of at least `data.len()` bytes; locked for the copy only.
    unsafe {
        let p = GlobalLock(mem) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(mem));
            return Err(ClipError);
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        let _ = GlobalUnlock(mem);
        if SetClipboardData(format, Some(HANDLE(mem.0))).is_err() {
            let _ = GlobalFree(Some(mem));
            return Err(ClipError);
        }
    }
    Ok(())
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: we opened the clipboard in `Open::new`.
        let _ = unsafe { CloseClipboard() };
    }
}

/// The system clipboard.
pub struct WinClipboard;

impl WinClipboard {
    /// Writes plain text (Copy safe text): it may enter the clipboard history; it is safe text.
    pub fn write_text(&self, text: &str) -> Result<(), ClipError> {
        Open::new()?.set_text(text, &[])
    }

    /// Writes text that must stay out of the clipboard history and cloud clipboard.
    pub fn write_private(&self, text: &str) -> Result<(), ClipError> {
        let formats = [
            (format_id(CAN_INCLUDE_IN_HISTORY), 0u32),
            (format_id(CAN_UPLOAD_TO_CLOUD), 0u32),
        ];
        Open::new()?.set_text(text, &formats)
    }

    /// True when the current content carries the password-manager marker.
    pub fn is_protected(&self) -> Result<bool, ClipError> {
        let open = Open::new()?;
        Ok(open.has(format_id(EXCLUDE_FROM_MONITORS)))
    }
}

impl Clipboard for WinClipboard {
    fn sequence(&self) -> u32 {
        // SAFETY: no arguments, no preconditions.
        unsafe { GetClipboardSequenceNumber() }
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        let open = Open::new()?;
        if open.has(format_id(EXCLUDE_FROM_MONITORS)) {
            return Ok(ClipText::Protected);
        }
        if !open.has(CF_UNICODETEXT) {
            return Ok(ClipText::Empty);
        }
        Ok(match open.text() {
            Unicode::Text(t) => ClipText::Text(t),
            Unicode::None => ClipText::Empty,
            Unicode::TooLarge => ClipText::TooLarge,
        })
    }

    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
        // Opening may wait up to half a second for another app; the number is compared only
        // once the clipboard is ours, and `set_text` empties it right after.
        let open = Open::new()?;
        if open.sequence() != seq {
            return Ok(false);
        }
        open.set_text(text, &[])?;
        Ok(true)
    }

    fn delete_from_history(&self, raw: &str) -> bool {
        super::history::delete_matching(raw)
    }
}

/// Helpers for the Windows clipboard tests (`tests/win_clipboard.rs`), which run on a real
/// desktop: they save the user's clipboard text first and put it back afterwards.
#[doc(hidden)]
pub mod testing {
    use super::*;

    /// What the clipboard held before a test.
    pub enum Saved {
        Text(Zeroizing<String>),
        Empty,
        /// A password-manager copy: tests must not touch the clipboard at all.
        Protected,
        /// Non-text content (an image, files) that a test could not put back.
        Other,
    }

    pub fn save() -> Result<Saved, ClipError> {
        let open = Open::new()?;
        if open.has(format_id(EXCLUDE_FROM_MONITORS)) {
            return Ok(Saved::Protected);
        }
        if open.has(CF_UNICODETEXT) {
            return Ok(match open.text() {
                Unicode::Text(t) => Saved::Text(t),
                Unicode::None | Unicode::TooLarge => Saved::Other,
            });
        }
        // SAFETY: the clipboard is open; 0 starts the enumeration.
        let any = unsafe { windows::Win32::System::DataExchange::EnumClipboardFormats(0) } != 0;
        Ok(if any { Saved::Other } else { Saved::Empty })
    }

    /// Puts the saved text back, kept out of the history so it does not appear twice there.
    pub fn restore(saved: &Saved) -> Result<(), ClipError> {
        match saved {
            Saved::Text(t) => WinClipboard.write_private(t),
            Saved::Empty => clear(),
            Saved::Protected | Saved::Other => Ok(()),
        }
    }

    pub fn clear() -> Result<(), ClipError> {
        let _open = Open::new()?;
        // SAFETY: the clipboard is open.
        unsafe { EmptyClipboard() }.map_err(|_| ClipError)
    }

    /// Text carrying the password-manager marker, as a password manager would copy it.
    pub fn write_marked(text: &str) -> Result<(), ClipError> {
        let formats = [
            (format_id(EXCLUDE_FROM_MONITORS), 0u32),
            (format_id(CAN_INCLUDE_IN_HISTORY), 0u32),
        ];
        Open::new()?.set_text(text, &formats)
    }

    /// A 1x1 bitmap (CF_DIB) and nothing else.
    pub fn write_image() -> Result<(), ClipError> {
        const CF_DIB: u32 = 8;
        let mut dib = Vec::new();
        for v in [40u32, 1, 1] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes()); // planes
        dib.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        dib.extend_from_slice(&[0u8; 24]); // compression .. important colours
        dib.extend_from_slice(&[0x40, 0x80, 0xC0, 0xFF]); // one pixel
        let _open = Open::new()?;
        // SAFETY: the clipboard is open and emptied by us before `put`.
        unsafe {
            EmptyClipboard().map_err(|_| ClipError)?;
            put(CF_DIB, &dib)
        }
    }

    /// The raw clipboard text, even when marked (tests check it was left unchanged).
    pub fn raw_text() -> Result<Option<Zeroizing<String>>, ClipError> {
        Ok(match Open::new()?.text() {
            Unicode::Text(t) => Some(t),
            Unicode::None | Unicode::TooLarge => None,
        })
    }
}
