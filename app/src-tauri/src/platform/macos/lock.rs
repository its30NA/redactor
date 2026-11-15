//! Wipe on lock and sleep on macOS.
//!
//! macOS has no documented "the screen was locked" signal. `com.apple.screenIsLocked` is the
//! only direct one and Apple calls it an implementation detail, so three documented signals
//! wipe as well: the displays went to sleep, the Mac is about to sleep, and the session was
//! switched away (fast user switching). A wipe too many is harmless.

use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSOperationQueue,
    NSString,
};

use crate::platform::LockWatcher;
use crate::wipe::WipeReason;

/// Where a signal is posted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Centre {
    /// Between the processes of the login session.
    Distributed,
    /// The workspace's own centre, inside this process.
    Workspace,
}

/// Every notification that wipes: its name, where it is posted, and the reason it stands for.
/// The workspace names are the values of AppKit's constants of the same name.
pub const SIGNALS: [(&str, Centre, WipeReason); 4] = [
    (
        "com.apple.screenIsLocked",
        Centre::Distributed,
        WipeReason::Lock,
    ),
    (
        "NSWorkspaceSessionDidResignActiveNotification",
        Centre::Workspace,
        WipeReason::Lock,
    ),
    (
        "NSWorkspaceScreensDidSleepNotification",
        Centre::Workspace,
        WipeReason::Lock,
    ),
    (
        "NSWorkspaceWillSleepNotification",
        Centre::Workspace,
        WipeReason::Sleep,
    ),
];

/// The reason a notification with this name stands for; `None` for every other name.
pub fn reason(name: &str) -> Option<WipeReason> {
    SIGNALS
        .iter()
        .find(|(signal, _, _)| *signal == name)
        .map(|&(_, _, why)| why)
}

/// Watches the lock, display-sleep, sleep and session-switch notifications.
pub struct MacLock;

impl LockWatcher for MacLock {
    /// Adds one observer per signal. `on_event` runs on the main thread, so the main thread
    /// must run its event loop. The observers stay for the life of the process.
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>) {
        let on_event: Arc<dyn Fn(WipeReason) + Send + Sync> = Arc::from(on_event);
        autoreleasepool(|_| {
            let main_queue = NSOperationQueue::mainQueue();
            let distributed = NSDistributedNotificationCenter::defaultCenter();
            let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
            for (name, centre, _) in SIGNALS {
                let centre: &NSNotificationCenter = match centre {
                    Centre::Distributed => &distributed,
                    Centre::Workspace => &workspace,
                };
                let on_event = Arc::clone(&on_event);
                let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
                    // SAFETY: the centre passes a valid notification that outlives the call.
                    let name = unsafe { notification.as_ref() }.name();
                    if let Some(why) = reason(&name.to_string()) {
                        on_event(why);
                    }
                });
                // SAFETY: no sender object is passed. The main queue takes work from any
                // thread. The block owns only the callback, which is `Send + Sync`, so it
                // may be run on, and released from, any thread.
                let observer = unsafe {
                    centre.addObserverForName_object_queue_usingBlock(
                        Some(&NSString::from_str(name)),
                        None,
                        Some(&main_queue),
                        &block,
                    )
                };
                // Never removed: the observer is kept for the life of the process.
                std::mem::forget(observer);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_maps_to_its_reason() {
        assert_eq!(reason("com.apple.screenIsLocked"), Some(WipeReason::Lock));
        assert_eq!(
            reason("NSWorkspaceSessionDidResignActiveNotification"),
            Some(WipeReason::Lock)
        );
        assert_eq!(
            reason("NSWorkspaceScreensDidSleepNotification"),
            Some(WipeReason::Lock)
        );
        assert_eq!(
            reason("NSWorkspaceWillSleepNotification"),
            Some(WipeReason::Sleep)
        );
        assert_eq!(SIGNALS.len(), 4, "a new signal needs a row above");
    }

    #[test]
    fn other_names_are_ignored() {
        for name in [
            "com.apple.screenIsUnlocked",
            "NSWorkspaceDidWakeNotification",
            "",
        ] {
            assert_eq!(reason(name), None, "{name:?}");
        }
    }
}
