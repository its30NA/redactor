//! Desktop checks on macOS that need the main thread's event loop: the lock watcher
//! (`MacLock`) against real notifications, and the global hotkey through `global_hotkey`, the
//! layer the global-shortcut plugin (`PluginHotkey`) hands the hotkey to.
//!
//! This binary has no test harness (`harness = false`): the harness never runs a main event
//! loop, and both the observers and Carbon hotkeys are served by it. The main thread sets up
//! and runs the application loop; a second thread runs the checks in order, prints `ok <name>`,
//! `FAILED <name>: <why>` or `not checked <name>: <why>`, then one `test result:` line, and
//! ends the process (exit 1 if a check failed).
//!
//! The checks press Ctrl+Alt+J and Shift+Cmd+J on the desktop they run on and post lock and
//! sleep notifications, so the binary does nothing unless `REDACTOR_TEST_DESKTOP` is set. Only
//! `tools/ci/macos-tests.sh` sets it.
//!
//! Started again with `MAC_HELPER=<mode>` it is the other process of a check: `post-lock`,
//! `post-lock-queued`, `post-unlock` (distributed notifications) and `hold-hotkey` (holds
//! Ctrl+Alt+K for 5 s).

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    mac::main();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode, KeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSWorkspace,
        NSWorkspaceScreensDidSleepNotification, NSWorkspaceSessionDidResignActiveNotification,
        NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::{
        NSDate, NSDistributedNotificationCenter, NSNotificationCenter, NSNotificationName,
        NSRunLoop, NSString,
    };
    use redactor_app::platform::macos::lock::MacLock;
    use redactor_app::platform::LockWatcher;
    use redactor_app::wipe::WipeReason;

    const LOCKED: &str = "com.apple.screenIsLocked";
    const UNLOCKED: &str = "com.apple.screenIsUnlocked";

    /// Everything must be over long before this; a hang is a failure, not a stuck CI job.
    const WATCHDOG: Duration = Duration::from_secs(120);

    pub fn main() {
        if let Ok(mode) = std::env::var("MAC_HELPER") {
            helper(&mode);
            return;
        }
        if std::env::var_os("REDACTOR_TEST_DESKTOP").is_none() {
            println!("skipped: run tools/ci/macos-tests.sh");
            return;
        }

        let mtm = MainThreadMarker::new().expect("main() runs on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        // No Dock icon, and never the active app.
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        let (wipes_tx, wipes) = mpsc::channel();
        MacLock.spawn(Box::new(move |why| {
            let _ = wipes_tx.send(why);
        }));

        // Carbon wants the manager made and the keys registered on the main thread. The
        // manager lives until the process ends: dropping it would give the keys back.
        let manager = GlobalHotKeyManager::new();
        let hotkeys = Hotkeys::register(manager.as_ref().map_err(ToString::to_string));
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();

        std::thread::spawn(|| {
            std::thread::sleep(WATCHDOG);
            println!("FAILED watchdog: the checks did not end within {WATCHDOG:?}");
            println!("test result: FAILED. the checks did not end");
            std::process::exit(1);
        });
        std::thread::spawn(move || {
            let report = run_checks(&wipes, &workspace, &hotkeys);
            std::process::exit(report.finish());
        });
        app.run();
        println!("FAILED application-loop: it ended before the checks did");
        std::process::exit(1);
    }

    // ----- the other process -----

    fn helper(mode: &str) {
        match mode {
            "post-lock" => post_distributed(LOCKED, true),
            "post-lock-queued" => post_distributed(LOCKED, false),
            "post-unlock" => post_distributed(UNLOCKED, true),
            "hold-hotkey" => hold_hotkey(),
            other => {
                eprintln!("unknown MAC_HELPER mode: {other}");
                std::process::exit(2);
            }
        }
    }

    /// Posts `name` to every process of this login session. `at_once` asks for delivery even
    /// to apps whose delivery is suspended (apps that are not active).
    fn post_distributed(name: &str, at_once: bool) {
        let centre = NSDistributedNotificationCenter::defaultCenter();
        // SAFETY: neither an object nor a user-info dictionary is passed.
        unsafe {
            centre.postNotificationName_object_userInfo_deliverImmediately(
                &NSString::from_str(name),
                None,
                None,
                at_once,
            );
        }
        stay(Duration::from_millis(200));
    }

    /// Holds Ctrl+Alt+K as another app would, says `ready`, and stays for 5 s.
    fn hold_hotkey() {
        let manager = GlobalHotKeyManager::new().expect("the hotkey manager starts");
        match manager.register(ctrl_alt(Code::KeyK)) {
            Ok(()) => println!("ready"),
            Err(e) => {
                println!("failed: {e}");
                std::process::exit(1);
            }
        }
        stay(Duration::from_secs(5));
    }

    /// Serves this thread's run loop for `time`. A run loop with nothing attached returns at
    /// once, so each turn also sleeps.
    fn stay(time: Duration) {
        let end = Instant::now() + time;
        while Instant::now() < end {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Runs this binary again as the other process, to its end.
    fn run_helper(mode: &str) -> Result<(), String> {
        let status = helper_command(mode)
            .status()
            .map_err(|e| format!("helper {mode} did not start: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("helper {mode} ended with {status}"))
        }
    }

    fn helper_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("this test binary"));
        command.env("MAC_HELPER", mode).stdin(Stdio::null());
        command
    }

    /// The other process holding Ctrl+Alt+K; killed on drop.
    struct HoldsHotkey(Child);

    impl HoldsHotkey {
        /// Starts it and waits (up to 5 s) until it says that it holds the key.
        fn start() -> Result<HoldsHotkey, String> {
            let mut child = helper_command("hold-hotkey")
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| format!("helper hold-hotkey did not start: {e}"))?;
            let stdout = child.stdout.take().expect("stdout is piped");
            let held = HoldsHotkey(child);
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = BufReader::new(stdout).read_line(&mut line);
                let _ = tx.send(line);
            });
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(line) if line.trim() == "ready" => Ok(held),
                Ok(line) => Err(format!("helper hold-hotkey said {:?}", line.trim())),
                Err(_) => Err("helper hold-hotkey was not ready within 5 s".into()),
            }
        }
    }

    impl Drop for HoldsHotkey {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // ----- hotkeys -----

    fn ctrl_alt(key: Code) -> HotKey {
        HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), key)
    }

    fn shift_cmd(key: Code) -> HotKey {
        HotKey::new(Some(Modifiers::SHIFT | Modifiers::SUPER), key)
    }

    /// What registering said, taken on the main thread before the loop runs.
    struct Hotkeys {
        /// Ctrl+Alt+J.
        free: Result<HotKey, String>,
        /// Shift+Cmd+J.
        cmd: Result<HotKey, String>,
        /// `Ok` when the second registration of Ctrl+Alt+J was refused.
        twice: Result<(), String>,
        /// `refused`, `accepted`, or why it is unknown: Ctrl+Alt+K while another app holds it.
        held: String,
    }

    impl Hotkeys {
        fn register(manager: Result<&GlobalHotKeyManager, String>) -> Hotkeys {
            let manager = match manager {
                Ok(m) => m,
                Err(e) => {
                    let e = format!("the hotkey manager did not start: {e}");
                    return Hotkeys {
                        free: Err(e.clone()),
                        cmd: Err(e.clone()),
                        twice: Err(e.clone()),
                        held: format!("unknown ({e})"),
                    };
                }
            };
            let register =
                |hk: HotKey| manager.register(hk).map(|()| hk).map_err(|e| e.to_string());
            let free = register(ctrl_alt(Code::KeyJ));
            let twice = match &free {
                Err(e) => Err(format!("the first registration already failed: {e}")),
                Ok(hk) => match manager.register(*hk) {
                    Err(_) => Ok(()),
                    Ok(()) => Err("a second registration of Ctrl+Alt+J was accepted".into()),
                },
            };
            let cmd = register(shift_cmd(Code::KeyJ));
            let held = match HoldsHotkey::start() {
                Err(e) => format!("unknown ({e})"),
                Ok(_other) => {
                    let hk = ctrl_alt(Code::KeyK);
                    match manager.register(hk) {
                        Err(_) => "refused".into(),
                        Ok(()) => {
                            let _ = manager.unregister(hk);
                            "accepted".into()
                        }
                    }
                }
            };
            Hotkeys {
                free,
                cmd,
                twice,
                held,
            }
        }
    }

    /// Forgets hotkey events left over from an earlier check (key releases arrive late).
    fn drain_hotkey_events() {
        while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
    }

    /// Waits for a press of `hk` until `deadline` passes.
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

    /// Presses and releases `key` with `flags` held, as a keyboard would.
    fn post_key(key: CGKeyCode, flags: CGEventFlags) -> Result<(), String> {
        for down in [true, false] {
            let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .map_err(|()| "no event source".to_string())?;
            let event = CGEvent::new_keyboard_event(source, key, down)
                .map_err(|()| "no key event".to_string())?;
            event.set_flags(flags);
            event.post(CGEventTapLocation::HID);
            std::thread::sleep(Duration::from_millis(30));
        }
        Ok(())
    }

    /// A registered key fires on a posted key press. Not every macOS hands posted keys to the
    /// hotkey matcher (it depends on the permissions of the process that posts): then nothing
    /// is known about the hotkey, which is `NotChecked` with an `OBSERVED` line, not a failure.
    fn fires(registered: &Result<HotKey, String>, label: &str, flags: CGEventFlags) -> Outcome {
        let hk = match registered {
            Ok(hk) => hk,
            Err(e) => return Outcome::Failed(format!("{label} was not registered: {e}")),
        };
        drain_hotkey_events();
        if let Err(e) = post_key(KeyCode::ANSI_J, flags) {
            println!("OBSERVED posted-key {label}=not-posted");
            return Outcome::NotChecked(format!("the key could not be posted: {e}"));
        }
        if pressed_within(hk, Duration::from_secs(2)) {
            println!("OBSERVED posted-key {label}=fired-the-hotkey");
            Outcome::Passed
        } else {
            println!("OBSERVED posted-key {label}=no-hotkey-event");
            Outcome::NotChecked("no hotkey event within 2 s of the posted key".into())
        }
    }

    // ----- the checks -----

    enum Outcome {
        Passed,
        Failed(String),
        NotChecked(String),
    }

    #[derive(Default)]
    struct Report {
        passed: u32,
        failed: u32,
        not_checked: u32,
    }

    impl Report {
        fn check(&mut self, name: &str, outcome: Outcome) {
            match outcome {
                Outcome::Passed => {
                    self.passed += 1;
                    println!("ok {name}");
                }
                Outcome::Failed(why) => {
                    self.failed += 1;
                    println!("FAILED {name}: {why}");
                }
                Outcome::NotChecked(why) => {
                    self.not_checked += 1;
                    println!("not checked {name}: {why}");
                }
            }
        }

        /// Prints the result line and returns the exit code.
        fn finish(&self) -> i32 {
            let Report {
                passed,
                failed,
                not_checked,
            } = self;
            let verdict = if *failed == 0 { "ok" } else { "FAILED" };
            println!(
                "test result: {verdict}. {passed} passed; {failed} failed; {not_checked} not checked"
            );
            i32::from(*failed != 0)
        }
    }

    fn drain_wipes(wipes: &mpsc::Receiver<WipeReason>) {
        while wipes.try_recv().is_ok() {}
    }

    /// The next callback is `want`, within `deadline`.
    fn expect_wipe(
        wipes: &mpsc::Receiver<WipeReason>,
        want: WipeReason,
        deadline: Duration,
    ) -> Result<(), String> {
        match wipes.recv_timeout(deadline) {
            Ok(got) if got == want => Ok(()),
            Ok(got) => Err(format!("the callback got {got:?}, not {want:?}")),
            Err(_) => Err(format!("no callback within {deadline:?}")),
        }
    }

    fn outcome(result: Result<(), String>) -> Outcome {
        match result {
            Ok(()) => Outcome::Passed,
            Err(why) => Outcome::Failed(why),
        }
    }

    fn a_lock_from_another_process_reaches_the_callback(
        wipes: &mpsc::Receiver<WipeReason>,
    ) -> Result<(), String> {
        drain_wipes(wipes);
        run_helper("post-lock")?;
        expect_wipe(wipes, WipeReason::Lock, Duration::from_secs(5))
    }

    /// The three documented signals, posted as the system posts them: on the workspace's
    /// centre, under the names AppKit exports.
    fn every_signal_reaches_the_callback(
        wipes: &mpsc::Receiver<WipeReason>,
        workspace: &NSNotificationCenter,
    ) -> Result<(), String> {
        // SAFETY: the three statics are AppKit's constant notification names.
        let signals: [(&str, &NSNotificationName, WipeReason); 3] = unsafe {
            [
                (
                    "will sleep",
                    NSWorkspaceWillSleepNotification,
                    WipeReason::Sleep,
                ),
                (
                    "screens did sleep",
                    NSWorkspaceScreensDidSleepNotification,
                    WipeReason::Lock,
                ),
                (
                    "session did resign active",
                    NSWorkspaceSessionDidResignActiveNotification,
                    WipeReason::Lock,
                ),
            ]
        };
        for (label, name, want) in signals {
            drain_wipes(wipes);
            // SAFETY: no object is passed.
            unsafe { workspace.postNotificationName_object(name, None) };
            expect_wipe(wipes, want, Duration::from_secs(2))
                .map_err(|e| format!("{label}: {e}"))?;
        }
        Ok(())
    }

    fn an_unlock_is_ignored(wipes: &mpsc::Receiver<WipeReason>) -> Result<(), String> {
        drain_wipes(wipes);
        run_helper("post-unlock")?;
        match wipes.recv_timeout(Duration::from_millis(500)) {
            Ok(got) => Err(format!("the callback got {got:?}")),
            Err(_) => Ok(()),
        }
    }

    /// Not a check: whether a lock notification posted without "deliver at once" reaches an
    /// app that is not the active one (this process never is) while it is in the background.
    fn observe_a_queued_lock(wipes: &mpsc::Receiver<WipeReason>) {
        drain_wipes(wipes);
        let seen = match run_helper("post-lock-queued") {
            Err(e) => format!("unknown ({e})"),
            Ok(()) => match wipes.recv_timeout(Duration::from_secs(2)) {
                Ok(_) => "arrived".into(),
                Err(_) => "held-back".into(),
            },
        };
        println!("OBSERVED lock-posted-without-deliver-at-once={seen}");
        drain_wipes(wipes);
    }

    fn run_checks(
        wipes: &mpsc::Receiver<WipeReason>,
        workspace: &Retained<NSNotificationCenter>,
        hotkeys: &Hotkeys,
    ) -> Report {
        let mut report = Report::default();
        report.check(
            "a_lock_from_another_process_reaches_the_callback",
            outcome(a_lock_from_another_process_reaches_the_callback(wipes)),
        );
        report.check(
            "every_signal_reaches_the_callback",
            outcome(every_signal_reaches_the_callback(wipes, workspace)),
        );
        report.check("an_unlock_is_ignored", outcome(an_unlock_is_ignored(wipes)));
        observe_a_queued_lock(wipes);
        report.check(
            "a_free_combo_fires",
            fires(
                &hotkeys.free,
                "ctrl-alt-j",
                CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagAlternate,
            ),
        );
        report.check(
            "a_cmd_combo_fires",
            fires(
                &hotkeys.cmd,
                "shift-cmd-j",
                CGEventFlags::CGEventFlagShift | CGEventFlags::CGEventFlagCommand,
            ),
        );
        report.check(
            "the_same_combo_twice_is_refused",
            outcome(hotkeys.twice.clone()),
        );
        println!("OBSERVED hotkey-held-by-another-app={}", hotkeys.held);
        report
    }
}
