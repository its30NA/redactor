//! The clipboard contract (`support/clipboard_contract.rs`) for the clipboard that works only
//! while the app's window is in front (GNOME, Weston, WSLg: no data-control), plus what only it
//! has: an inactive window neither reads nor writes, and a window that gets focus back sees the
//! copy made meanwhile.
//!
//! These tests own the clipboard of the compositor they run on, so they are `#[ignore]`d and
//! panic unless `REDACTOR_TEST_WAYLAND` names the throwaway compositor that
//! `tools/ci/wayland-tests.sh` started. Only that script sets it, and runs them one at a time.
//!
//! GTK starts once per process: on a thread of its own, which becomes its main thread and runs
//! its main loop for the rest of the run. Each test maps its own 200x200 window there and waits
//! until it is active (sway focuses new windows). Headless sway has no keyboard and so no
//! focus; the seat gets a virtual one first (`support/virtual_keyboard.rs`). The other app is
//! [`WaylandOther`]: data-control works on sway, so its copies are real and reach the window
//! like any other app's. In the contract it is [`BackHere`]: after each copy the user's next
//! input reaches our window, as on a desktop without data-control.
#![cfg(target_os = "linux")]

#[path = "support/clipboard_contract.rs"]
mod contract;
#[path = "support/virtual_keyboard.rs"]
mod virtual_keyboard;
// Its odd senders are for `wayland_clipboard.rs`.
#[allow(dead_code)]
#[path = "support/wayland_other.rs"]
mod wayland_other;
#[path = "support/wayland_session.rs"]
mod wayland_session;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use contract::{unique, OtherApp};
use gtk::glib;
use gtk::prelude::*;
use redactor_app::clipscrub::{ClipText, Clipboard};
use redactor_app::platform::linux::wayland::FocusedClipboard;
use redactor_app::platform::SystemClipboard;
use virtual_keyboard::VirtualKeyboard;
use wayland_other::WaylandOther;
use wayland_session::compositor;

/// How long a window gets to become active, or to lose focus.
const FOCUS_DEADLINE: Duration = Duration::from_secs(5);
/// How long the GTK thread gets to answer.
const GTK_DEADLINE: Duration = Duration::from_secs(10);

/// The seat's keyboard: there once GTK runs.
static KEYBOARD: OnceLock<VirtualKeyboard> = OnceLock::new();

fn keyboard() -> &'static VirtualKeyboard {
    KEYBOARD.get().expect("GTK is not started")
}

/// Starts GTK on its own thread, once per process, after the seat got its keyboard.
fn start_gtk() {
    KEYBOARD.get_or_init(|| {
        compositor();
        let keyboard = VirtualKeyboard::new();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("gtk-main".into())
            .spawn(move || {
                // A Wayland client of the throwaway compositor, never XWayland.
                gtk::gdk::set_allowed_backends("wayland");
                gtk::init().expect("start GTK on the test compositor");
                // Said from inside the main loop: from then on it owns the main context.
                glib::idle_add_local_once(move || {
                    let _ = tx.send(());
                });
                gtk::main();
            })
            .expect("start the GTK thread");
        rx.recv_timeout(GTK_DEADLINE)
            .expect("GTK did not start in 10 s");
        keyboard
    });
}

/// Runs `f` on the GTK thread and returns what it returned.
fn on_gtk<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    glib::MainContext::default().invoke(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(GTK_DEADLINE)
        .expect("the GTK thread did not answer in 10 s")
}

/// Our windows, as GTK knows them (on the GTK thread).
fn our_windows() -> Vec<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .collect()
}

fn window_active() -> bool {
    on_gtk(|| our_windows().iter().any(|w| w.is_active()))
}

/// Polls `done` until it is true or `FOCUS_DEADLINE` has passed; returns its last answer.
fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + FOCUS_DEADLINE;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A 200x200 window of ours, active once `open` returns; closed when dropped.
struct Window;

impl Window {
    fn open() -> Window {
        start_gtk();
        on_gtk(|| {
            let window = gtk::Window::new(gtk::WindowType::Toplevel);
            window.set_default_size(200, 200);
            window.show_all();
        });
        assert!(
            wait_until(window_active),
            "our window did not become active in 5 s"
        );
        Window
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // Nothing here may panic: this also runs while a failed test unwinds.
        let (tx, rx) = mpsc::channel();
        glib::MainContext::default().invoke(move || {
            for window in our_windows() {
                window.close();
            }
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(GTK_DEADLINE);
        let gone = || {
            let (tx, rx) = mpsc::channel();
            glib::MainContext::default().invoke(move || {
                let _ = tx.send(our_windows().iter().all(|w| !w.is_visible()));
            });
            rx.recv_timeout(GTK_DEADLINE).unwrap_or(true)
        };
        wait_until(gone);
    }
}

/// Another process with a window that takes the focus from ours: this test binary started
/// again as [`focus_thief`]. A second window of our own would still count as ours, and active.
/// Its window closes when this is dropped.
struct FocusThief(Child);

impl FocusThief {
    fn start() -> FocusThief {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["focus_thief", "--exact", "--ignored", "--nocapture"])
            .env("WAYLAND_FOCUSED_MODE", "focus-thief")
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the focus thief");
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            let n = out.read_line(&mut line).unwrap();
            assert!(n > 0, "the focus thief ended before its window was active");
            if line.trim_end() == "ready" {
                break;
            }
        }
        let thief = FocusThief(child);
        assert!(
            wait_until(|| !window_active()),
            "our window was still active 5 s after the thief's took the focus"
        );
        thief
    }
}

impl Drop for FocusThief {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The other app as it copies where there is no data-control: it needs the focus to copy, so
/// the user comes back to our window afterwards, and the next input there carries a serial
/// newer than the copy's. A compositor refuses a copy of ours with an older one (wlroots and
/// Weston do; `refused_copy_is_an_error_and_the_other_copy_stays`). A Shift tap stands in for
/// that input; the window has the focus throughout.
struct BackHere(WaylandOther);

impl OtherApp for BackHere {
    fn copy_text(&self, text: &str) {
        self.0.copy_text(text);
        keyboard().tap();
    }

    fn copy_protected(&self, text: &str) {
        self.0.copy_protected(text);
        keyboard().tap();
    }

    fn copy_image(&self) {
        self.0.copy_image();
        keyboard().tap();
    }

    fn clear(&self) {
        self.0.clear();
        keyboard().tap();
    }

    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
}

fn read_text(cb: &FocusedClipboard) -> Option<String> {
    match cb.read().unwrap() {
        ClipText::Text(t) => Some(t.to_string()),
        _ => None,
    }
}

/// One contract check per test, named as the contract names it, with our window in front.
macro_rules! contract_tests {
    ($($check:ident),* $(,)?) => {
        $(
            #[test]
            #[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
            fn $check() {
                compositor();
                let _window = Window::open();
                let cb = FocusedClipboard::new();
                let other = BackHere(WaylandOther::new());
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

/// The app is not in front: it reads nothing and writes nothing, and the other app's copy stays.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn inactive_window_refuses_read_and_write() {
    compositor();
    let _window = Window::open();
    let cb = FocusedClipboard::new();
    assert!(cb.needs_focus());
    let other = WaylandOther::new();
    let text = format!("another app's copy {}", unique());
    other.copy_text(&text);
    let seq = cb.sequence();

    let _thief = FocusThief::start();
    assert!(cb.read().is_err(), "an inactive window read the clipboard");
    assert_eq!(cb.write_if_unchanged("ours", seq), Ok(false));
    assert!(cb.write_text("ours").is_err());
    assert_eq!(other.paste().as_deref(), Some(text.as_str()));
}

/// A copy that came after the window's last input (here: through data-control, with no input
/// since): the compositor refuses our copy. The write says so, the other copy stays, and our
/// reads see it, not the text GTK had offered.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn refused_copy_is_an_error_and_the_other_copy_stays() {
    compositor();
    let _window = Window::open();
    let cb = FocusedClipboard::new();
    let other = WaylandOther::new();
    let text = format!("a newer copy {}", unique());
    other.copy_text(&text);
    let seq = cb.sequence();

    assert!(cb.write_if_unchanged("ours", seq).is_err());
    assert!(cb.write_text("ours").is_err());
    assert_eq!(other.paste().as_deref(), Some(text.as_str()));
    assert_eq!(read_text(&cb), Some(text));

    // The user's next input: now the copy is taken.
    keyboard().tap();
    let ours = format!("ours {}", unique());
    cb.write_text(&ours).unwrap();
    assert_eq!(other.paste().as_deref(), Some(ours.as_str()));
}

/// Our text reaches another app as written, line endings included (GTK's own text calls would
/// send CRLF), and reads back the same.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn our_text_reaches_wl_paste_byte_for_byte() {
    compositor();
    let _window = Window::open();
    let cb = FocusedClipboard::new();
    let other = WaylandOther::new();
    let text = format!("line one {}\nline two\r\n\tthree äöü 😀\n", unique());
    cb.write_text(&text).unwrap();
    assert_eq!(other.paste().as_deref(), Some(text.as_str()));
    assert_eq!(read_text(&cb), Some(text));
}

/// A sender that never writes and keeps the pipe open: the read gives up after 10 s. GTK's wait
/// stays open until the next copy ends it; until then every operation fails at once, and the
/// next copy reads normally.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn silent_sender_fails_until_the_next_copy() {
    compositor();
    let _window = Window::open();
    let cb = FocusedClipboard::new();
    let other = BackHere(WaylandOther::new());
    other.0.copy_silent();
    keyboard().tap();
    let started = Instant::now();
    assert!(cb.read().is_err());
    assert!(started.elapsed() < Duration::from_secs(12));

    let started = Instant::now();
    assert!(cb.read().is_err());
    assert!(cb.write_text("ours").is_err());
    assert!(started.elapsed() < Duration::from_secs(1));

    let text = format!("after the silence {}", unique());
    other.copy_text(&text);
    assert_eq!(read_text(&cb), Some(text));
}

/// A copy made while the app was not in front is what it reads once its window is active again,
/// and the sequence number has moved.
#[test]
#[ignore = "needs a throwaway Wayland compositor: tools/ci/wayland-tests.sh"]
fn focus_regained_sees_the_newer_copy() {
    compositor();
    let _window = Window::open();
    let cb = FocusedClipboard::new();
    let other = WaylandOther::new();
    let older = format!("older copy {}", unique());
    other.copy_text(&older);
    assert_eq!(read_text(&cb), Some(older));
    let before = cb.sequence();

    let thief = FocusThief::start();
    let newer = format!("newer copy {}", unique());
    other.copy_text(&newer);
    drop(thief);
    assert!(
        wait_until(window_active),
        "our window did not get the focus back in 5 s"
    );

    assert_eq!(read_text(&cb), Some(newer));
    assert_ne!(cb.sequence(), before, "the sequence did not move");
}

/// Not a test: the focus thief of [`FocusThief`], a window of another process. Returns at once
/// in a normal test run.
#[test]
#[ignore = "the focus thief of the focus tests; does nothing without WAYLAND_FOCUSED_MODE"]
fn focus_thief() {
    if std::env::var("WAYLAND_FOCUSED_MODE").as_deref() != Ok("focus-thief") {
        return;
    }
    compositor();
    gtk::gdk::set_allowed_backends("wayland");
    gtk::init().expect("start GTK on the test compositor");
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(200, 200);
    window.show_all();
    let deadline = Instant::now() + FOCUS_DEADLINE;
    while !window.is_active() {
        assert!(
            Instant::now() < deadline,
            "the thief's window did not become active in 5 s"
        );
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // The test harness may have printed on this line already.
    println!("\nready");
    // Until the test ends this process.
    gtk::main();
}
