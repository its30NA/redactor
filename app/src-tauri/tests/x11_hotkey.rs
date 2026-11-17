//! The global hotkey on a real X server, through `global_hotkey`: the layer the global-shortcut
//! plugin (`PluginHotkey`) hands the hotkey to. Keys are pressed with `xdotool`; another app
//! that holds a key is `xbindkeys`.
//!
//! These tests grab keys on the display they run on, so they are `#[ignore]`d and panic unless
//! `REDACTOR_TEST_DISPLAY` names a throwaway display. Only `tools/ci/x11-tests.sh` sets it, on
//! its own Xvfb, and runs them one at a time. Each test uses its own key, so a grab that a
//! previous test's hotkey thread has not let go of yet never gets in the way.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// The throwaway X display. Panics when it is not set: these tests never touch a real desktop.
/// `global_hotkey` connects to `DISPLAY`, so that must be the same display.
fn display() -> String {
    let display = std::env::var("REDACTOR_TEST_DISPLAY")
        .expect("REDACTOR_TEST_DISPLAY is not set: run tools/ci/x11-tests.sh");
    assert_eq!(
        std::env::var("DISPLAY").ok().as_deref(),
        Some(display.as_str()),
        "DISPLAY must be the test display: run tools/ci/x11-tests.sh"
    );
    display
}

/// Runs `xdotool` on `display` with `args`.
fn xdotool(display: &str, args: &[&str]) {
    let status = Command::new("xdotool")
        .env("DISPLAY", display)
        .args(args)
        .status()
        .expect("xdotool is installed");
    assert!(status.success(), "xdotool {args:?} failed");
}

fn ctrl_alt(key: Code) -> HotKey {
    HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), key)
}

/// Forgets events left over from earlier tests (key releases arrive after a test ends).
fn drain_events() {
    while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
}

/// Polls for a press of `hk` until `deadline` passes.
fn pressed_within(hk: &HotKey, deadline: Duration) -> bool {
    let end = Instant::now() + deadline;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        match GlobalHotKeyEvent::receiver().recv_timeout(left) {
            Ok(e) if e.id() == hk.id() && e.state() == HotKeyState::Pressed => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn a_free_combo_fires() {
    let display = display();
    drain_events();
    let manager = GlobalHotKeyManager::new().unwrap();
    let hk = ctrl_alt(Code::KeyJ);
    manager.register(hk).expect("Ctrl+Alt+J is free");

    xdotool(&display, &["key", "ctrl+alt+j"]);
    let fired = pressed_within(&hk, Duration::from_secs(2));

    manager.unregister(hk).unwrap();
    assert!(fired, "no hotkey event within 2 s");
}

/// A scratch folder that is removed again.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("redactor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `xbindkeys` holding Ctrl+Alt+R on the test display; killed on drop.
struct Xbindkeys(Child);

impl Xbindkeys {
    /// Starts it and waits until it really holds the key: a press runs its command, which
    /// creates `scratch/pressed`.
    fn hold_ctrl_alt_r(display: &str, scratch: &Scratch) -> Xbindkeys {
        let pressed = scratch.0.join("pressed");
        let rc = scratch.0.join("xbindkeysrc");
        std::fs::write(
            &rc,
            format!("\"touch '{}'\"\n  control+alt + r\n", pressed.display()),
        )
        .unwrap();
        // Line-buffered, so "starting loop..." arrives as soon as the keys are grabbed.
        let mut child = Command::new("stdbuf")
            .args(["-oL", "xbindkeys", "-n", "-v", "-f"])
            .arg(&rc)
            .env("DISPLAY", display)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("xbindkeys is installed");
        let stdout = child.stdout.take().unwrap();
        let held = Xbindkeys(child);

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.starts_with("starting loop") {
                    let _ = tx.send(());
                }
            }
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("xbindkeys did not start within 5 s");

        xdotool(display, &["key", "ctrl+alt+r"]);
        let end = Instant::now() + Duration::from_secs(3);
        while !pressed.exists() {
            assert!(
                Instant::now() < end,
                "xbindkeys did not react to Ctrl+Alt+R within 3 s"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        held
    }
}

impl Drop for Xbindkeys {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn a_taken_combo_is_refused() {
    let display = display();
    let scratch = Scratch::new("hotkey");
    let _other = Xbindkeys::hold_ctrl_alt_r(&display, &scratch);

    let manager = GlobalHotKeyManager::new().unwrap();
    let hk = ctrl_alt(Code::KeyR);
    let r = manager.register(hk);
    if r.is_ok() {
        manager.unregister(hk).unwrap();
    }
    assert!(r.is_err(), "Ctrl+Alt+R is held by xbindkeys");
}

/// NumLock switched on for the test, and back off on drop.
struct NumLockOn<'a>(&'a str);

fn num_lock_is_on(display: &str) -> bool {
    let out = Command::new("xset")
        .env("DISPLAY", display)
        .arg("q")
        .output()
        .expect("xset is installed");
    assert!(out.status.success(), "xset q failed");
    let text = String::from_utf8_lossy(&out.stdout);
    let after = text
        .split("Num Lock:")
        .nth(1)
        .expect("xset q shows Num Lock");
    after.trim_start().starts_with("on")
}

/// Polls until NumLock is `on` (or off), for up to 2 s.
fn num_lock_becomes(display: &str, on: bool) -> bool {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if num_lock_is_on(display) == on {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl<'a> NumLockOn<'a> {
    fn new(display: &'a str) -> NumLockOn<'a> {
        assert!(!num_lock_is_on(display), "NumLock starts off");
        xdotool(display, &["key", "Num_Lock"]);
        let on = NumLockOn(display);
        assert!(
            num_lock_becomes(display, true),
            "NumLock is on after the press"
        );
        on
    }
}

impl Drop for NumLockOn<'_> {
    fn drop(&mut self) {
        let _ = Command::new("xdotool")
            .env("DISPLAY", self.0)
            .args(["key", "Num_Lock"])
            .status();
    }
}

#[test]
#[ignore = "needs a throwaway X display: tools/ci/x11-tests.sh"]
fn numlock_does_not_hide_the_hotkey() {
    let display = display();
    drain_events();
    let manager = GlobalHotKeyManager::new().unwrap();
    let hk = ctrl_alt(Code::KeyK);
    manager.register(hk).expect("Ctrl+Alt+K is free");

    let fired = {
        let _num_lock = NumLockOn::new(&display);
        xdotool(&display, &["key", "ctrl+alt+k"]);
        pressed_within(&hk, Duration::from_secs(2))
    };

    manager.unregister(hk).unwrap();
    assert!(num_lock_becomes(&display, false), "NumLock is off again");
    assert!(fired, "no hotkey event within 2 s with NumLock on");
}
