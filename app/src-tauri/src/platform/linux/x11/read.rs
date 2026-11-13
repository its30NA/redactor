//! Reading the clipboard: the requestor side of the X11 selection protocol.
//!
//! The owner of the selection is another program and is not trusted: it may announce a size and
//! send more, never answer, or exit half way. Every wait has a deadline, the size is known
//! before a byte is copied, and each read uses its own window, so that destroying it ends
//! whatever the owner was still doing.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, GetPropertyReply, Property,
    Timestamp, Window, WindowClass,
};
use zeroize::{Zeroize, Zeroizing};

use super::{lost, Op, Reply, Shared, READ_TIMEOUT, STEP_TIMEOUT};
use crate::bridge::MAX_INPUT_BYTES;
use crate::clipscrub::{ClipError, ClipText};

/// How many targets of an owner are looked at.
const MAX_TARGETS: u32 = 1024;
/// A password-manager hint is a short word; this many bytes of it are read.
const HINT_BYTES: u32 = 64;

/// What the clipboard holds. `time` is a recent server time (from a ping).
pub(super) fn read(shared: &Shared, op: &Op, time: Timestamp) -> Result<ClipText, ClipError> {
    let a = &shared.atoms;
    let owner = shared.owner()?;
    if owner == x11rb::NONE {
        return Ok(ClipText::Empty);
    }
    if owner == shared.window {
        return Ok(own_text(shared));
    }
    let reading = Reading::start(shared, op, time)?;

    // An owner that does not list its targets is asked for plain text.
    let listed = reading.convert(a.TARGETS)?;
    let targets = if listed {
        reading.targets()?
    } else {
        vec![a.UTF8_STRING]
    };
    if targets.contains(&a.PASSWORD_HINT) && reading.hint_is_secret() {
        return Ok(ClipText::Protected);
    }
    let string: Atom = AtomEnum::STRING.into();
    let Some(target) = [a.UTF8_STRING, a.TEXT_PLAIN_UTF8, string]
        .into_iter()
        .find(|t| targets.contains(t))
    else {
        // An image, files: no text.
        return Ok(ClipText::Empty);
    };
    if !reading.convert(target)? {
        // Refusing a target it listed is a failure; refusing our guess means "no text".
        return if listed {
            Err(ClipError)
        } else {
            Ok(ClipText::Empty)
        };
    }
    Ok(match reading.fetch()? {
        Some(bytes) => decode(bytes, target == string),
        None => ClipText::TooLarge,
    })
}

/// Our own text, read as another app would get it: up to the first NUL, within the limit.
fn own_text(shared: &Shared) -> ClipText {
    let Some(text) = shared.owned_text() else {
        return ClipText::Empty;
    };
    let end = memchr::memchr(0, text.as_bytes()).unwrap_or(text.len());
    if end == 0 {
        ClipText::Empty
    } else if end > MAX_INPUT_BYTES {
        ClipText::TooLarge
    } else {
        ClipText::Text(Zeroizing::new(text[..end].to_owned()))
    }
}

/// One read: its own unmapped window, and the deadline for the whole of it.
struct Reading<'a> {
    shared: &'a Shared,
    op: &'a Op,
    window: Window,
    time: Timestamp,
    deadline: Instant,
}

impl Drop for Reading<'_> {
    /// Destroying the window also ends a transfer the owner is still sending.
    fn drop(&mut self) {
        self.shared.listening.store(x11rb::NONE, Ordering::SeqCst);
        let _ = self.shared.conn.destroy_window(self.window);
        let _ = self.shared.conn.flush();
    }
}

impl<'a> Reading<'a> {
    fn start(shared: &'a Shared, op: &'a Op, time: Timestamp) -> Result<Self, ClipError> {
        let window = shared.conn.generate_id().map_err(lost)?;
        // From here on the worker hands this window's events to us.
        shared.listening.store(window, Ordering::SeqCst);
        let reading = Reading {
            shared,
            op,
            window,
            time,
            deadline: Instant::now() + READ_TIMEOUT,
        };
        shared
            .conn
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                shared.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map_err(lost)?;
        Ok(reading)
    }

    /// Waits for the next answer that `wanted` accepts, for one step at most.
    fn wait<T>(&self, wanted: impl Fn(Reply) -> Option<T>) -> Result<T, ClipError> {
        let until = self.deadline.min(Instant::now() + STEP_TIMEOUT);
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left == Duration::ZERO {
                return Err(ClipError);
            }
            let reply = self.op.replies.recv_timeout(left).map_err(lost)?;
            if let Some(found) = wanted(reply) {
                return Ok(found);
            }
        }
    }

    /// Asks the owner for `target`. True: the answer is in our property. False: refused.
    fn convert(&self, target: Atom) -> Result<bool, ClipError> {
        let a = &self.shared.atoms;
        let conn = &self.shared.conn;
        conn.convert_selection(self.window, a.CLIPBOARD, target, a.TRANSFER, self.time)
            .map_err(lost)?;
        conn.flush().map_err(lost)?;
        self.wait(|reply| match reply {
            Reply::Selection(e)
                if e.requestor == self.window
                    && e.selection == a.CLIPBOARD
                    && e.target == target =>
            {
                Some(e.property != x11rb::NONE)
            }
            _ => None,
        })
    }

    /// Our property: up to `words` 32-bit units of it, deleted afterwards if `delete`.
    fn property(&self, delete: bool, words: u32) -> Result<GetPropertyReply, ClipError> {
        self.shared
            .conn
            .get_property(
                delete,
                self.window,
                self.shared.atoms.TRANSFER,
                AtomEnum::ANY,
                0,
                words,
            )
            .map_err(lost)?
            .reply()
            .map_err(lost)
    }

    /// The owner's answer to `TARGETS`. An answer that is not a list of atoms counts as "plain
    /// text only", like a refusal.
    fn targets(&self) -> Result<Vec<Atom>, ClipError> {
        let reply = self.property(true, MAX_TARGETS)?;
        let targets = match reply.value32() {
            Some(atoms) => atoms.collect(),
            None => vec![self.shared.atoms.UTF8_STRING],
        };
        Ok(targets)
    }

    /// True when the password-manager hint says `secret`, and also when it cannot be read: in
    /// doubt the copy is left alone.
    fn hint_is_secret(&self) -> bool {
        if !matches!(self.convert(self.shared.atoms.PASSWORD_HINT), Ok(true)) {
            return true;
        }
        let Ok(reply) = self.property(true, HINT_BYTES / 4) else {
            return true;
        };
        if reply.format != 8 {
            return true;
        }
        let value = reply.value.as_slice();
        let blank = |b: &u8| b.is_ascii_whitespace() || *b == 0;
        let start = value.iter().position(|b| !blank(b)).unwrap_or(value.len());
        let end = value
            .iter()
            .rposition(|b| !blank(b))
            .map_or(start, |i| i + 1);
        &value[start..end] == b"secret"
    }

    /// The converted text as raw bytes, or `None` when it is larger than the input limit. The
    /// size of a property is asked for first, so nothing over the limit is ever copied.
    fn fetch(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClipError> {
        let head = self.property(false, 0)?;
        if head.type_ == self.shared.atoms.INCR {
            return self.fetch_pieces();
        }
        if head.format != 8 {
            return Err(ClipError);
        }
        self.take(head.bytes_after, 0)
    }

    /// Takes our property of `size` bytes out of the server, unless `held + size` is over the
    /// limit.
    fn take(&self, size: u32, held: usize) -> Result<Option<Zeroizing<Vec<u8>>>, ClipError> {
        if held + size as usize > MAX_INPUT_BYTES {
            return Ok(None);
        }
        let reply = self.property(true, size.div_ceil(4))?;
        let bytes = Zeroizing::new(reply.value);
        // The owner may have replaced the property between the two requests.
        if reply.format != 8 || reply.bytes_after != 0 || bytes.len() > size as usize {
            return Err(ClipError);
        }
        Ok(Some(bytes))
    }

    /// A transfer in pieces (INCR): the owner sends the next piece each time we delete the
    /// property, and an empty piece at the end.
    ///
    /// A text that turns out too large is not abandoned half way: an owner that serves one
    /// transfer at a time (`xclip`) would answer nobody until it ends, and the text could no
    /// longer be pasted anywhere. Its remaining pieces are deleted on the server, unread, until
    /// the owner is done or the deadline of the read passes.
    fn fetch_pieces(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClipError> {
        // The owner's lower bound for the size. It only serves to refuse early: what counts is
        // what arrives.
        let announced = self
            .property(false, 1)?
            .value32()
            .and_then(|mut v| v.next())
            .unwrap_or(0);
        let mut too_large = announced as usize > MAX_INPUT_BYTES;
        // Deleting the announcement starts the transfer.
        self.discard()?;

        // The pieces are kept as they arrive and joined once, so no partial copy of the text is
        // left behind in memory that a growing buffer gave back.
        let mut pieces: Vec<Zeroizing<Vec<u8>>> = Vec::new();
        let mut held = 0usize;
        loop {
            let piece = self.next_piece(too_large, held);
            if too_large {
                match piece {
                    Ok(Piece::Discarded) => continue,
                    // The end, or an owner that stopped: either way it was too large.
                    _ => return Ok(None),
                }
            }
            match piece? {
                Piece::Bytes(bytes) => {
                    held += bytes.len();
                    pieces.push(bytes);
                }
                Piece::Discarded => {
                    too_large = true;
                    pieces.clear();
                }
                Piece::End => break,
            }
        }
        let mut text = Zeroizing::new(Vec::with_capacity(held));
        for piece in &pieces {
            text.extend_from_slice(piece);
        }
        Ok(Some(text))
    }

    /// Waits for the owner's next piece and takes it; with `discard`, or when `held` plus the
    /// piece is over the limit, the piece is deleted on the server without being read.
    fn next_piece(&self, discard: bool, held: usize) -> Result<Piece, ClipError> {
        let a = &self.shared.atoms;
        self.wait(|reply| match reply {
            Reply::Property(e)
                if e.window == self.window
                    && e.atom == a.TRANSFER
                    && e.state == Property::NEW_VALUE =>
            {
                Some(())
            }
            _ => None,
        })?;
        let head = self.property(false, 0)?;
        if head.bytes_after == 0 {
            // The empty piece. Deleting it tells the owner we saw it.
            self.discard()?;
            return Ok(Piece::End);
        }
        if !discard {
            if head.format != 8 {
                return Err(ClipError);
            }
            if let Some(bytes) = self.take(head.bytes_after, held)? {
                return Ok(Piece::Bytes(bytes));
            }
        }
        self.discard()?;
        Ok(Piece::Discarded)
    }

    /// Deletes our property without reading it; the owner of a transfer in pieces sends the
    /// next one in answer.
    fn discard(&self) -> Result<(), ClipError> {
        self.shared
            .conn
            .delete_property(self.window, self.shared.atoms.TRANSFER)
            .map_err(lost)?;
        self.shared.conn.flush().map_err(lost)
    }
}

/// One step of a transfer in pieces.
enum Piece {
    Bytes(Zeroizing<Vec<u8>>),
    /// Over the limit: deleted on the server, never copied.
    Discarded,
    End,
}

/// The text in `bytes`: it ends at the first NUL (as on Windows), invalid UTF-8 becomes U+FFFD,
/// and `STRING` is Latin-1.
fn decode(mut bytes: Zeroizing<Vec<u8>>, latin1: bool) -> ClipText {
    if let Some(nul) = memchr::memchr(0, &bytes) {
        bytes.truncate(nul);
    }
    // Every buffer is sized once, for the worst case, so no copy of the text is left behind in
    // memory that a growing buffer gave back.
    let text = if latin1 {
        let mut text = String::with_capacity(bytes.len() * 2);
        text.extend(bytes.iter().map(|&b| char::from(b)));
        text
    } else {
        match String::from_utf8(std::mem::take(&mut *bytes)) {
            // The usual case: the bytes become the string, nothing is copied.
            Ok(text) => text,
            Err(invalid) => {
                let mut raw = invalid.into_bytes();
                let mut text = String::with_capacity(raw.len() * 3);
                for chunk in raw.utf8_chunks() {
                    text.push_str(chunk.valid());
                    if !chunk.invalid().is_empty() {
                        text.push(char::REPLACEMENT_CHARACTER);
                    }
                }
                raw.zeroize();
                text
            }
        }
    };
    let text = Zeroizing::new(text);
    if text.is_empty() {
        ClipText::Empty
    } else if text.len() > MAX_INPUT_BYTES {
        ClipText::TooLarge
    } else {
        ClipText::Text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8], latin1: bool) -> Option<String> {
        match decode(Zeroizing::new(bytes.to_vec()), latin1) {
            ClipText::Text(t) => Some(t.to_string()),
            _ => None,
        }
    }

    #[test]
    fn utf8_passes_through_unchanged() {
        let sample = "😀 Grüße\r\n\tline two";
        assert_eq!(text(sample.as_bytes(), false).as_deref(), Some(sample));
    }

    #[test]
    fn text_ends_at_the_first_nul() {
        assert_eq!(text(b"one\0two", false).as_deref(), Some("one"));
        assert!(matches!(
            decode(Zeroizing::new(b"\0two".to_vec()), false),
            ClipText::Empty
        ));
        assert!(matches!(
            decode(Zeroizing::new(Vec::new()), false),
            ClipText::Empty
        ));
    }

    #[test]
    fn invalid_utf8_becomes_the_replacement_character() {
        assert_eq!(
            text(b"a\xFFb\xC3", false).as_deref(),
            Some("a\u{FFFD}b\u{FFFD}")
        );
    }

    #[test]
    fn latin1_is_one_character_per_byte() {
        assert_eq!(text(b"Gr\xFC\xDFe", true).as_deref(), Some("Grüße"));
    }

    #[test]
    fn text_that_grows_over_the_limit_when_decoded_is_too_large() {
        // Latin-1 bytes above 0x7F take two bytes each as UTF-8.
        let raw = vec![0xE9u8; MAX_INPUT_BYTES / 2 + 1];
        assert!(matches!(
            decode(Zeroizing::new(raw), true),
            ClipText::TooLarge
        ));
    }
}
