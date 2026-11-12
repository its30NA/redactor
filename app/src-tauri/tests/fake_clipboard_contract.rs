//! The clipboard contract on an in-memory fake, on every OS. It proves the contract code itself
//! is sound; each real clipboard runs the same checks on its own desktop.

#[path = "support/clipboard_contract.rs"]
mod contract;

use std::sync::{Arc, Mutex};

use contract::OtherApp;
use redactor_app::bridge::MAX_INPUT_BYTES;
use redactor_app::clipscrub::{ClipError, ClipText, Clipboard};
use redactor_app::platform::SystemClipboard;
use zeroize::Zeroizing;

#[derive(Clone)]
enum Content {
    Nothing,
    Text(String),
    Protected(String),
    Image,
}

/// One desk both sides share: what is on the clipboard and its sequence number.
struct Desk {
    content: Content,
    seq: u32,
}

impl Desk {
    fn put(&mut self, c: Content) {
        self.content = c;
        self.seq += 1;
    }
}

type Shared = Arc<Mutex<Desk>>;

struct FakeClipboard(Shared);
struct FakeOther(Shared);

fn desk() -> (FakeClipboard, FakeOther) {
    let d = Arc::new(Mutex::new(Desk {
        content: Content::Nothing,
        seq: 0,
    }));
    (FakeClipboard(d.clone()), FakeOther(d))
}

impl Clipboard for FakeClipboard {
    fn sequence(&self) -> u32 {
        self.0.lock().unwrap().seq
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        Ok(match &self.0.lock().unwrap().content {
            Content::Nothing | Content::Image => ClipText::Empty,
            Content::Protected(_) => ClipText::Protected,
            Content::Text(t) if t.len() > MAX_INPUT_BYTES => ClipText::TooLarge,
            Content::Text(t) => ClipText::Text(Zeroizing::new(t.clone())),
        })
    }

    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
        let mut d = self.0.lock().unwrap();
        if d.seq != seq {
            return Ok(false);
        }
        d.put(Content::Text(text.to_string()));
        Ok(true)
    }

    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for FakeClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipError> {
        self.0.lock().unwrap().put(Content::Text(text.to_string()));
        Ok(())
    }
}

impl OtherApp for FakeOther {
    fn copy_text(&self, text: &str) {
        self.0.lock().unwrap().put(Content::Text(text.to_string()));
    }

    fn copy_protected(&self, text: &str) {
        self.0
            .lock()
            .unwrap()
            .put(Content::Protected(text.to_string()));
    }

    fn copy_image(&self) {
        self.0.lock().unwrap().put(Content::Image);
    }

    fn clear(&self) {
        self.0.lock().unwrap().put(Content::Nothing);
    }

    fn paste(&self) -> Option<String> {
        match &self.0.lock().unwrap().content {
            Content::Text(t) | Content::Protected(t) => Some(t.clone()),
            Content::Nothing | Content::Image => None,
        }
    }
}

macro_rules! contract_tests {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let (cb, other) = desk();
                contract::$name(&cb, &other);
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
