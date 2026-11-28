//! Another application on the macOS desktop, for the clipboard tests.
//!
//! Independent code on purpose: it never uses the app's `MacClipboard`. Plain copies and
//! pastes go through `pbcopy` and `pbpaste`, which are real other programs; what they cannot
//! do (a password manager's marker, an image, UTF-16 only) are `NSPasteboard` calls of its
//! own, made the old two-step way (`declareTypes`, then the data), not the way the app writes.
//!
//! The race tests need a copy from a second process at a chosen moment, faster than starting
//! `pbcopy`: [`OtherProcess`] is this test binary started again, copying on command.
//!
//! Test text is made up and built at run time.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSArray, NSData, NSString};

use crate::contract::OtherApp;

const UTF8: &str = "public.utf8-plain-text";
const UTF16: &str = "public.utf16-plain-text";
const CONCEALED: &str = "org.nspasteboard.ConcealedType";
const TRANSIENT: &str = "org.nspasteboard.TransientType";
const PNG: &str = "public.png";

/// A 1x1 transparent PNG.
const ONE_PIXEL: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// The environment variable that turns this test binary into the other process.
pub const OTHER_PROCESS_ENV: &str = "REDACTOR_TEST_OTHER_PROCESS";

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

/// Takes the pasteboard over for `types`. Returns the new change count.
fn declare(pasteboard: &NSPasteboard, types: &[&str]) -> isize {
    let names: Vec<Retained<NSString>> = types.iter().map(|name| ns(name)).collect();
    let names = NSArray::from_retained_slice(&names);
    // SAFETY: the type names are strings, and there is no owner object: the data for every
    // type is set right after.
    unsafe { pasteboard.declareTypes_owner(&names, None) }
}

/// Copies `text` with a marker type next to it. False when a step was refused (another copy
/// took the pasteboard meanwhile).
fn copy_marked(text: &str, marker: &str) -> bool {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        declare(&pasteboard, &[UTF8, marker]);
        let text_set = pasteboard.setString_forType(&ns(text), &ns(UTF8));
        let marker_set = pasteboard.setString_forType(&ns(""), &ns(marker));
        text_set && marker_set
    })
}

/// Copies `text` the way most apps do. False when the text was refused (another copy took
/// the pasteboard meanwhile).
fn copy_plain(text: &str) -> bool {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        pasteboard.setString_forType(&ns(text), &ns(UTF8))
    })
}

/// The pasteboard's text as AppKit hands it to any app, or `None` without text.
pub fn raw_text() -> Option<String> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        // AppKit wants the types asked for before the data.
        pasteboard.types()?;
        pasteboard
            .stringForType(&ns(UTF8))
            .map(|text| text.to_string())
            .filter(|text| !text.is_empty())
    })
}

/// What `pbpaste` prints: the bytes another program gets.
pub fn pbpaste() -> Vec<u8> {
    let out = Command::new("pbpaste")
        .env("LC_ALL", "en_US.UTF-8")
        .output()
        .expect("run pbpaste");
    assert!(out.status.success(), "pbpaste failed");
    out.stdout
}

fn unix_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Waits without giving up the processor: sleeping is far too coarse for microseconds.
pub fn spin(time: Duration) {
    let until = Instant::now() + time;
    while Instant::now() < until {
        std::hint::spin_loop();
    }
}

pub struct MacOther;

impl MacOther {
    /// A plain copy without starting a program.
    pub fn copy_raw(&self, text: &str) {
        assert!(copy_plain(text), "the pasteboard refused a plain copy");
    }

    /// A copy an app will take back in a moment (`org.nspasteboard.TransientType`).
    pub fn copy_transient(&self, text: &str) {
        assert!(
            copy_marked(text, TRANSIENT),
            "the pasteboard refused a transient copy"
        );
    }

    /// Text offered as UTF-16 only, in this machine's byte order and without a byte-order
    /// mark, as the type is defined.
    pub fn copy_utf16_only(&self, text: &str) {
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_ne_bytes).collect();
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            declare(&pasteboard, &[UTF16]);
            assert!(
                pasteboard.setData_forType(Some(&NSData::with_bytes(&bytes)), &ns(UTF16)),
                "the pasteboard refused UTF-16 text"
            );
        });
    }

    /// Lists plain text but hands out no data for it: what a copy from an app whose lazy
    /// provider gives nothing looks like, or a read macOS refuses. There is no owner, so AppKit
    /// has nobody to ask for the data.
    pub fn list_text_without_data(&self) {
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            declare(&pasteboard, &[UTF8]);
        });
    }

    pub fn change_count(&self) -> isize {
        autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount())
    }

    /// Empties the pasteboard with the call the app's write uses. Returns the count the call
    /// reports.
    pub fn clear_and_count(&self) -> isize {
        autoreleasepool(|_| NSPasteboard::generalPasteboard().clearContents())
    }
}

impl OtherApp for MacOther {
    fn copy_text(&self, text: &str) {
        let mut pbcopy = Command::new("pbcopy")
            .env("LC_ALL", "en_US.UTF-8")
            .stdin(Stdio::piped())
            .spawn()
            .expect("start pbcopy");
        // The handle is dropped at the end of the statement: pbcopy sees the end of its input.
        pbcopy
            .stdin
            .take()
            .expect("pbcopy's input")
            .write_all(text.as_bytes())
            .expect("write to pbcopy");
        assert!(pbcopy.wait().expect("wait for pbcopy").success());
        // The checks need exactly this text on the pasteboard. If pbcopy changed it, say so
        // and copy it directly: what is under test is the app, not pbcopy.
        if raw_text().as_deref() != Some(text) && !text.is_empty() {
            println!("OBSERVED pbcopy=changed-the-text");
            self.copy_raw(text);
        }
    }

    fn copy_protected(&self, text: &str) {
        assert!(
            copy_marked(text, CONCEALED),
            "the pasteboard refused a concealed copy"
        );
    }

    fn copy_image(&self) {
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            declare(&pasteboard, &[PNG]);
            assert!(
                pasteboard.setData_forType(Some(&NSData::with_bytes(&ONE_PIXEL)), &ns(PNG)),
                "the pasteboard refused an image"
            );
        });
    }

    fn clear(&self) {
        self.clear_and_count();
    }

    /// What `pbpaste` gets. Checked against the pasteboard itself: only line endings may
    /// differ (pbpaste may turn CR into LF), and then the pasteboard's text is the answer.
    fn paste(&self) -> Option<String> {
        let printed = String::from_utf8(pbpaste()).expect("pbpaste printed UTF-8");
        let held = raw_text().unwrap_or_default();
        if printed != held {
            assert!(
                unix_lines(&printed) == unix_lines(&held),
                "pbpaste and the pasteboard disagree"
            );
            println!("OBSERVED pbpaste=changed-line-endings");
        }
        Some(held).filter(|text| !text.is_empty())
    }
}

/// This test binary started again as a second process that copies on command.
///
/// Commands, one per line: `raw <text>`, `raw-after <microseconds> <text>` (waits first) and
/// `concealed <text>`. Each is answered with `done` once the copy was made or refused.
pub struct OtherProcess {
    child: Child,
    commands: ChildStdin,
    answers: BufReader<ChildStdout>,
}

impl OtherProcess {
    /// Starts `test_name` of this binary (a test that calls [`serve`]) and waits until it
    /// listens.
    pub fn start(test_name: &str) -> OtherProcess {
        let mut child = Command::new(std::env::current_exe().expect("this test binary"))
            .args(["--ignored", "--exact", test_name, "--nocapture"])
            .env(OTHER_PROCESS_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the other process");
        let commands = child.stdin.take().expect("the other process's input");
        let answers = BufReader::new(child.stdout.take().expect("the other process's output"));
        let mut other = OtherProcess {
            child,
            commands,
            answers,
        };
        other.wait_for("ready");
        other
    }

    /// Sends a command and returns at once.
    pub fn send(&mut self, command: &str) {
        writeln!(self.commands, "{command}").expect("write to the other process");
        self.commands.flush().expect("write to the other process");
    }

    /// Waits until the last command was carried out.
    pub fn done(&mut self) {
        self.wait_for("done");
    }

    /// Reads the other process's output up to the line `answer`. The test harness prints
    /// lines of its own there too; they are skipped.
    fn wait_for(&mut self, answer: &str) {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self
                .answers
                .read_line(&mut line)
                .expect("read from the other process");
            assert!(read != 0, "the other process ended early");
            if line.trim_end().ends_with(answer) {
                return;
            }
        }
    }
}

impl Drop for OtherProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The other process's side: carries out commands from its input until the input ends.
pub fn serve() {
    println!("ready");
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("read a command");
        match line.split_once(' ') {
            Some(("raw", text)) => {
                copy_plain(text);
            }
            Some(("raw-after", rest)) => {
                let (micros, text) = rest.split_once(' ').expect("raw-after <micros> <text>");
                spin(Duration::from_micros(micros.parse().expect("microseconds")));
                copy_plain(text);
            }
            Some(("concealed", text)) => {
                copy_marked(text, CONCEALED);
            }
            _ => panic!("unknown command"),
        }
        println!("done");
    }
}
