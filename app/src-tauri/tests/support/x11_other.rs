//! Another application on the test X display, for the clipboard contract on X11.
//!
//! Independent code on purpose: it never uses `X11Clipboard`. Ordinary copies and pastes go
//! through `xclip`, a real third-party client. What `xclip` cannot do (a password-manager hint,
//! a Latin-1-only owner, owners that lie, hang or die) runs in a child process: this test binary
//! started again as [`x11_other_helper`], with the mode in `X11_OTHER_MODE`, the text on stdin,
//! and a line `ready` on stdout once it owns the selection.
//!
//! The including test file must name this module `x11_other`.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use redactor_app::bridge::MAX_INPUT_BYTES;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt, CreateWindowAux, EventMask, PropMode,
    Property, SelectionNotifyEvent, SelectionRequestEvent, Window, WindowClass,
    SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::contract::OtherApp;

/// The throwaway X display. Panics when it is not set: these tests never touch a real desktop.
pub fn display() -> String {
    std::env::var("REDACTOR_TEST_DISPLAY")
        .expect("REDACTOR_TEST_DISPLAY is not set: run tools/ci/x11-tests.sh")
}

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        INCR,
        UTF8_STRING,
        HINT: b"x-kde-passwordManagerHint",
        CLIPBOARD_MANAGER,
        SAVE_TARGETS,
        FETCHED: b"_X11_OTHER_FETCHED",
    }
}

/// A 1x1 transparent PNG.
const PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// How long another app gets to take the selection over.
const OWNER_DEADLINE: Duration = Duration::from_secs(2);

pub struct X11Other {
    display: String,
    conn: RustConnection,
    clipboard: Atom,
    /// A window of this app's own, for [`X11Other::start_copy`].
    window: Window,
    /// Helper processes still running; killed when this is dropped.
    helpers: Mutex<Vec<(Child, ChildStdout)>>,
}

impl X11Other {
    pub fn new(display: &str) -> X11Other {
        let (conn, screen) =
            RustConnection::connect(Some(display)).expect("connect to the test display");
        let clipboard = Atoms::new(&conn).unwrap().reply().unwrap().CLIPBOARD;
        let window = plain_window(&conn, screen);
        X11Other {
            display: display.to_string(),
            conn,
            clipboard,
            window,
            helpers: Mutex::new(Vec::new()),
        }
    }

    /// Takes the clipboard and returns without waiting for the server: the copy is still on
    /// its way when the caller goes on. (Nobody is served; the tests only look at who owns.)
    pub fn start_copy(&self) {
        self.conn
            .set_selection_owner(self.window, self.clipboard, x11rb::CURRENT_TIME)
            .unwrap();
        self.conn.flush().unwrap();
    }

    /// True when the copy made with [`X11Other::start_copy`] is what the clipboard holds.
    pub fn copy_is_in_place(&self) -> bool {
        self.owner() == self.window
    }

    fn owner(&self) -> Window {
        self.conn
            .get_selection_owner(self.clipboard)
            .unwrap()
            .reply()
            .unwrap()
            .owner
    }

    /// `xclip -i` with `data` as the given target. Returns once the new owner is in place.
    fn xclip_copy(&self, target: &str, data: &[u8]) {
        let before = self.owner();
        let mut child = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", target, "-i"])
            .env("DISPLAY", &self.display)
            .stdin(Stdio::piped())
            // xclip keeps serving in a forked child that inherits these.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start xclip");
        child.stdin.take().unwrap().write_all(data).unwrap();
        assert!(child.wait().unwrap().success(), "xclip -i failed");
        // xclip takes the selection in its forked child, after the parent has exited.
        let deadline = Instant::now() + OWNER_DEADLINE;
        loop {
            let now = self.owner();
            if now != x11rb::NONE && now != before {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "xclip did not take the clipboard"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Starts a helper process in `mode` with `text` and waits until it says `ready`.
    pub fn helper(&self, mode: &str, text: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "x11_other::x11_other_helper",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env("X11_OTHER_MODE", mode)
            .env("REDACTOR_TEST_DISPLAY", &self.display)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the helper process");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            let n = out.read_line(&mut line).unwrap();
            assert!(n > 0, "the {mode} helper ended before it was ready");
            if line.trim_end() == "ready" {
                break;
            }
        }
        self.helpers.lock().unwrap().push((child, out.into_inner()));
    }
}

impl Drop for X11Other {
    fn drop(&mut self) {
        for (child, _) in self.helpers.get_mut().unwrap().iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl OtherApp for X11Other {
    fn copy_text(&self, text: &str) {
        self.xclip_copy("UTF8_STRING", text.as_bytes());
    }

    fn copy_protected(&self, text: &str) {
        self.helper("protected", text);
    }

    fn copy_image(&self) {
        self.xclip_copy("image/png", &PNG);
    }

    fn clear(&self) {
        self.helper("clear", "");
        assert_eq!(
            self.owner(),
            x11rb::NONE,
            "the clipboard still has an owner"
        );
    }

    fn paste(&self) -> Option<String> {
        let out = Command::new("timeout")
            .args([
                "10",
                "xclip",
                "-selection",
                "clipboard",
                "-t",
                "UTF8_STRING",
                "-o",
            ])
            .env("DISPLAY", &self.display)
            .stderr(Stdio::null())
            .output()
            .expect("start xclip");
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).expect("xclip returned invalid UTF-8"))
    }
}

/// An unmapped window to own a selection with.
fn plain_window(conn: &RustConnection, screen: usize) -> Window {
    let window = conn.generate_id().unwrap();
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        conn.setup().roots[screen].root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .unwrap();
    window
}

/// Not a test: the helper process of [`X11Other`]. Returns at once in a normal test run.
#[test]
#[ignore = "the helper process of X11Other; does nothing without X11_OTHER_MODE"]
fn x11_other_helper() {
    let Ok(mode) = std::env::var("X11_OTHER_MODE") else {
        return;
    };
    let mut text = Vec::new();
    std::io::stdin().read_to_end(&mut text).unwrap();
    Helper::start(&mode, text).serve();
}

/// How much an INCR transfer sends at a time.
const CHUNK: usize = 256 * 1024;
/// What the `die-mid-incr` owner announces (and never delivers).
const DYING_TOTAL: usize = 1024 * 1024;

/// An INCR transfer this owner is sending.
struct Sending {
    requestor: Window,
    property: Atom,
    sent: usize,
    total: usize,
}

/// A selection owner with one odd habit, chosen by `mode`.
struct Helper {
    mode: String,
    text: Vec<u8>,
    conn: RustConnection,
    atoms: Atoms,
    /// The window that owns the selection (0 in mode `clear`).
    window: Window,
    sending: Option<Sending>,
}

impl Helper {
    /// Connects, takes the clipboard (or clears it, in mode `clear`) and prints `ready`.
    fn start(mode: &str, text: Vec<u8>) -> Helper {
        let (conn, screen) =
            RustConnection::connect(Some(&display())).expect("connect to the test display");
        let atoms = Atoms::new(&conn).unwrap().reply().unwrap();
        let owner = if mode == "clear" {
            x11rb::NONE
        } else {
            plain_window(&conn, screen)
        };
        // A clipboard manager owns its own selection and takes the clipboard only when asked.
        let selection = if mode == "manager" {
            atoms.CLIPBOARD_MANAGER
        } else {
            atoms.CLIPBOARD
        };
        conn.set_selection_owner(owner, selection, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let now = conn
            .get_selection_owner(selection)
            .unwrap()
            .reply()
            .unwrap()
            .owner;
        assert_eq!(now, owner, "the helper could not set the selection owner");
        // The test harness may have printed on this line already.
        println!("\nready");
        std::io::stdout().flush().unwrap();
        Helper {
            mode: mode.to_string(),
            text,
            conn,
            atoms,
            window: owner,
            sending: None,
        }
    }

    /// Answers requests until the selection is lost or the process is killed.
    fn serve(mut self) {
        if self.mode == "clear" {
            return;
        }
        loop {
            match self.conn.wait_for_event().expect("the X connection broke") {
                Event::SelectionClear(_) => return,
                Event::SelectionRequest(req) if self.mode != "silent" => self.answer(&req),
                Event::PropertyNotify(e) if e.state == Property::DELETE => {
                    self.send_next(e.window, e.atom);
                }
                // X errors (a requestor that went away) and everything else.
                _ => {}
            }
            let _ = self.conn.flush();
        }
    }

    fn answer(&mut self, req: &SelectionRequestEvent) {
        let a = self.atoms;
        let string: Atom = AtomEnum::STRING.into();
        let property = if req.property == x11rb::NONE {
            req.target
        } else {
            req.property
        };
        let hinted = matches!(self.mode.as_str(), "protected" | "hint-other");
        let text_target = if self.mode == "string-only" {
            string
        } else {
            a.UTF8_STRING
        };
        let done = if req.selection == a.CLIPBOARD_MANAGER {
            req.target == a.SAVE_TARGETS && self.take_over()
        } else if req.target == a.TARGETS {
            let mut targets = vec![a.TARGETS, text_target];
            if hinted {
                targets.push(a.HINT);
            }
            let data: Vec<u8> = targets.iter().flat_map(|t| t.to_ne_bytes()).collect();
            self.put(req.requestor, property, AtomEnum::ATOM.into(), 32, &data)
        } else if req.target == a.HINT && hinted {
            let value: &[u8] = if self.mode == "protected" {
                b"secret"
            } else {
                b"public"
            };
            self.put(req.requestor, property, a.HINT, 8, value)
        } else if req.target == text_target {
            match self.mode.as_str() {
                // Announces one byte, then sends more than the app accepts.
                "understate" => self.announce(req.requestor, property, 1, MAX_INPUT_BYTES + 1),
                "die-mid-incr" => self.announce(req.requestor, property, DYING_TOTAL, DYING_TOTAL),
                "string-only" => {
                    // Latin-1: one byte per character.
                    let latin1: Vec<u8> = String::from_utf8_lossy(&self.text)
                        .chars()
                        .map(|c| u8::try_from(u32::from(c)).expect("Latin-1 text"))
                        .collect();
                    self.put(req.requestor, property, string, 8, &latin1)
                }
                _ => self.put(req.requestor, property, a.UTF8_STRING, 8, &self.text),
            }
        } else {
            false
        };
        let event = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: req.time,
            requestor: req.requestor,
            selection: req.selection,
            target: req.target,
            property: if done { property } else { x11rb::NONE },
        };
        let _ = self
            .conn
            .send_event(false, req.requestor, EventMask::NO_EVENT, event);
    }

    /// What a clipboard manager does when an app says it is about to exit (`SAVE_TARGETS`):
    /// fetches the text from the app that owns the clipboard and becomes the owner itself.
    /// Small texts only (no transfer in pieces).
    fn take_over(&mut self) -> bool {
        let a = self.atoms;
        let _ = self.conn.convert_selection(
            self.window,
            a.CLIPBOARD,
            a.UTF8_STRING,
            a.FETCHED,
            x11rb::CURRENT_TIME,
        );
        let _ = self.conn.flush();
        loop {
            match self.conn.wait_for_event().expect("the X connection broke") {
                Event::SelectionNotify(e) if e.requestor == self.window => {
                    if e.property == x11rb::NONE {
                        return false;
                    }
                    break;
                }
                _ => {}
            }
        }
        let fetched = self
            .conn
            .get_property(true, self.window, a.FETCHED, a.UTF8_STRING, 0, u32::MAX / 4)
            .unwrap()
            .reply()
            .unwrap();
        if fetched.type_ != a.UTF8_STRING || fetched.format != 8 {
            return false;
        }
        self.text = fetched.value;
        self.conn
            .set_selection_owner(self.window, a.CLIPBOARD, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .is_ok()
    }

    fn put(&self, window: Window, property: Atom, kind: Atom, format: u8, data: &[u8]) -> bool {
        let len = u32::try_from(data.len() / usize::from(format / 8)).unwrap();
        self.conn
            .change_property(PropMode::REPLACE, window, property, kind, format, len, data)
            .is_ok()
    }

    /// Starts an INCR transfer that says `declared` bytes and sends `total`.
    fn announce(
        &mut self,
        requestor: Window,
        property: Atom,
        declared: usize,
        total: usize,
    ) -> bool {
        let _ = self.conn.change_window_attributes(
            requestor,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        );
        self.sending = Some(Sending {
            requestor,
            property,
            sent: 0,
            total,
        });
        let declared = u32::try_from(declared).unwrap().to_ne_bytes();
        self.put(requestor, property, self.atoms.INCR, 32, &declared)
    }

    /// The requestor deleted the property: send the next piece (an empty one ends it).
    fn send_next(&mut self, window: Window, property: Atom) {
        let Some(s) = self
            .sending
            .as_mut()
            .filter(|s| s.requestor == window && s.property == property)
        else {
            return;
        };
        let n = CHUNK.min(s.total - s.sent);
        s.sent += n;
        let done = n == 0;
        self.put(window, property, self.atoms.UTF8_STRING, 8, &vec![b'a'; n]);
        if self.mode == "die-mid-incr" {
            // The first piece is on its way; the rest never comes.
            let _ = self.conn.flush();
            std::process::exit(0);
        }
        if done {
            self.sending = None;
        }
    }
}
