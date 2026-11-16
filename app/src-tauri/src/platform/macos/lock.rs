//! Wipe on lock and sleep on macOS.
//!
//! macOS has no documented "the screen was locked" signal. `com.apple.screenIsLocked` is the
//! only direct one and Apple calls it an implementation detail, so three documented signals
//! wipe as well: the displays went to sleep, the Mac is about to sleep, and the session was
//! switched away (fast user switching). A wipe too many is harmless.

use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationSuspensionBehavior,
    NSOperationQueue, NSString,
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

type OnEvent = Arc<dyn Fn(WipeReason) + Send + Sync>;

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class does not implement Drop.
    #[unsafe(super(NSObject))]
    #[ivars = OnEvent]
    /// Receives the distributed signals. A block observer cannot say how it wants to be
    /// treated while the centre is suspended, and AppKit suspends the distributed centre
    /// whenever the app is not the active one, which a menu-bar app hardly ever is: the lock
    /// would wait until the app is activated again. This observer is registered with
    /// "deliver immediately", which a suspension does not hold back.
    struct LockObserver;

    impl LockObserver {
        #[unsafe(method(signal:))]
        fn signal(&self, notification: &NSNotification) {
            if let Some(why) = reason(&notification.name().to_string()) {
                (self.ivars())(why);
            }
        }
    }
);

impl LockObserver {
    fn new(on_event: OnEvent) -> Retained<Self> {
        let this = Self::alloc().set_ivars(on_event);
        // SAFETY: NSObject's designated initialiser, called on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// Watches the lock, display-sleep, sleep and session-switch notifications.
pub struct MacLock;

impl LockWatcher for MacLock {
    /// Adds one observer per signal. `on_event` runs on the main thread, so the main thread
    /// must run its event loop. The observers stay for the life of the process.
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>) {
        let on_event: OnEvent = Arc::from(on_event);
        autoreleasepool(|_| {
            let main_queue = NSOperationQueue::mainQueue();
            let distributed = NSDistributedNotificationCenter::defaultCenter();
            let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
            let receiver = LockObserver::new(Arc::clone(&on_event));
            for (name, centre, _) in SIGNALS {
                if centre == Centre::Distributed {
                    // SAFETY: the observer is a LockObserver, which implements `signal:` with
                    // the one notification argument the centre passes; no sender object is
                    // passed. The centre does not retain observers: `receiver` is kept for
                    // the life of the process below.
                    unsafe {
                        distributed.addObserver_selector_name_object_suspensionBehavior(
                            &receiver,
                            sel!(signal:),
                            Some(&NSString::from_str(name)),
                            None,
                            NSNotificationSuspensionBehavior::DeliverImmediately,
                        );
                    }
                    continue;
                }
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
                    workspace.addObserverForName_object_queue_usingBlock(
                        Some(&NSString::from_str(name)),
                        None,
                        Some(&main_queue),
                        &block,
                    )
                };
                // Never removed: the observer is kept for the life of the process.
                std::mem::forget(observer);
            }
            // Never released: the centre holds it unretained for the life of the process.
            std::mem::forget(receiver);
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
