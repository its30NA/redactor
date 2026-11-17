//! The pipes of a clipboard transfer, bounded in size and in time.
//!
//! The other end belongs to another program and is not trusted: it may send more than the input
//! limit, never write and keep the pipe open, or stop half way. A read waits a limited time for
//! each piece and for the whole, and stops one byte over its limit; a write gives up at its
//! deadline. Waiting is `poll` on the pipe, so a read runs on the thread that asked for it and
//! nothing is left blocked behind.

use std::fs::File;
use std::io::{ErrorKind, PipeReader, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::time::{Duration, Instant};

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
use zeroize::Zeroizing;

use crate::clipscrub::ClipError;

/// The largest piece read at a time.
const PIECE: usize = 64 * 1024;

/// Reads `reader` to its end: at most `max` bytes, waiting at most `step` for each piece and
/// `total` for the whole. `Ok(None)`: the sender had more than `max` bytes (the rest is never
/// read; dropping the pipe tells the sender to stop). An error: a deadline passed or the pipe
/// broke.
pub(super) fn read_bounded(
    mut reader: PipeReader,
    max: usize,
    total: Duration,
    step: Duration,
) -> Result<Option<Zeroizing<Vec<u8>>>, ClipError> {
    let deadline = Instant::now() + total;
    // The pieces are kept as they arrive and joined once, so no partial copy of the text is
    // left behind in memory that a growing buffer gave back.
    let mut pieces: Vec<Zeroizing<Vec<u8>>> = Vec::new();
    let mut held = 0usize;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if !ready(&reader, PollFlags::IN, left.min(step))? {
            return Err(ClipError);
        }
        // One byte more than the limit allows tells "too large" from "exactly at the limit".
        let mut piece = Zeroizing::new(vec![0u8; (max - held).saturating_add(1).min(PIECE)]);
        let n = match reader.read(&mut piece) {
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(ClipError),
        };
        if n == 0 {
            break;
        }
        if n > max - held {
            return Ok(None);
        }
        piece.truncate(n);
        held += n;
        pieces.push(piece);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(held));
    for piece in &pieces {
        bytes.extend_from_slice(piece);
    }
    Ok(Some(bytes))
}

/// Writes all of `bytes` to the pipe `fd`, giving up when `total` has passed. A reader that
/// closes its end early is an error too.
pub(super) fn write_bounded(fd: OwnedFd, bytes: &[u8], total: Duration) -> Result<(), ClipError> {
    // Non-blocking, so a reader that never reads cannot hold a write past the deadline.
    let flags = fcntl_getfl(&fd).map_err(|_| ClipError)?;
    fcntl_setfl(&fd, flags | OFlags::NONBLOCK).map_err(|_| ClipError)?;
    let mut pipe = File::from(fd);
    let deadline = Instant::now() + total;
    let mut rest = bytes;
    while !rest.is_empty() {
        match pipe.write(rest) {
            Ok(0) => return Err(ClipError),
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(Instant::now());
                if !ready(&pipe, PollFlags::OUT, left)? {
                    return Err(ClipError);
                }
            }
            Err(_) => return Err(ClipError),
        }
    }
    Ok(())
}

/// Waits up to `timeout` until `fd` is ready for `flags`, or closed at the other end. False:
/// the time ran out.
fn ready(fd: &impl AsFd, flags: PollFlags, timeout: Duration) -> Result<bool, ClipError> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        let wait = Timespec::try_from(left).map_err(|_| ClipError)?;
        let mut fds = [PollFd::new(fd, flags)];
        match poll(&mut fds, Some(&wait)) {
            Ok(0) => {}
            Ok(_) => return Ok(true),
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return Err(ClipError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::pipe;

    const STEP: Duration = Duration::from_millis(200);

    #[test]
    fn reads_to_the_end() {
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(b"some made-up text").unwrap();
        drop(writer);
        let bytes = read_bounded(reader, 64, STEP, STEP).unwrap().unwrap();
        assert_eq!(bytes.as_slice(), b"some made-up text");
    }

    #[test]
    fn exactly_the_limit_is_fine_one_more_is_too_large() {
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(&[b'a'; 64]).unwrap();
        drop(writer);
        assert_eq!(
            read_bounded(reader, 64, STEP, STEP).unwrap().unwrap().len(),
            64
        );

        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(&[b'a'; 65]).unwrap();
        drop(writer);
        assert!(read_bounded(reader, 64, STEP, STEP).unwrap().is_none());
    }

    #[test]
    fn a_silent_writer_times_out() {
        let (reader, writer) = pipe().unwrap();
        let started = Instant::now();
        assert!(read_bounded(reader, 64, Duration::from_secs(5), STEP).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(writer);
    }

    #[test]
    fn a_reader_that_never_reads_ends_the_write_at_its_deadline() {
        let (reader, writer) = pipe().unwrap();
        let started = Instant::now();
        // More than a pipe holds.
        let big = vec![b'a'; 4 * 1024 * 1024];
        assert!(write_bounded(writer.into(), &big, STEP).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(reader);
    }

    #[test]
    fn a_reader_that_goes_away_ends_the_write() {
        let (reader, writer) = pipe().unwrap();
        drop(reader);
        assert!(write_bounded(writer.into(), b"text", STEP).is_err());
    }

    #[test]
    fn a_reader_gets_everything_that_is_written() {
        let (reader, writer) = pipe().unwrap();
        let big: Vec<u8> = (0..3 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
        let sent = big.clone();
        let writing =
            std::thread::spawn(move || write_bounded(writer.into(), &sent, Duration::from_secs(5)));
        let got = read_bounded(reader, big.len(), Duration::from_secs(5), STEP)
            .unwrap()
            .unwrap();
        assert!(writing.join().unwrap().is_ok());
        assert!(got.as_slice() == big.as_slice());
    }
}
