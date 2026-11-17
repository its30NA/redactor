//! Win+V clipboard history: after an in-place scrub, the raw item is deleted from the
//! history, best effort. Only items whose text equals the scrubbed raw text are touched.

use windows::ApplicationModel::DataTransfer::{
    Clipboard, ClipboardHistoryItem, ClipboardHistoryItemsResultStatus, StandardDataFormats,
};

/// Deletes history items whose text equals `raw`. True when at least one was deleted.
pub fn delete_matching(raw: &str) -> bool {
    matching_items(raw)
        .into_iter()
        .filter(|item| Clipboard::DeleteItemFromHistory(item).unwrap_or(false))
        .count()
        > 0
}

/// True when the history holds an item whose text equals `text`.
pub fn contains(text: &str) -> bool {
    !matching_items(text).is_empty()
}

/// Whether the clipboard history is switched on (Settings > System > Clipboard).
pub fn enabled() -> bool {
    Clipboard::IsHistoryEnabled().unwrap_or(false)
}

fn matching_items(text: &str) -> Vec<ClipboardHistoryItem> {
    let mut out = Vec::new();
    if !enabled() {
        return out;
    }
    let Ok(result) = Clipboard::GetHistoryItemsAsync().and_then(|op| op.join()) else {
        return out;
    };
    if result.Status().ok() != Some(ClipboardHistoryItemsResultStatus::Success) {
        return out;
    }
    let Ok(items) = result.Items() else {
        return out;
    };
    let Ok(text_format) = StandardDataFormats::Text() else {
        return out;
    };
    let n = items.Size().unwrap_or(0);
    for i in 0..n {
        let Ok(item) = items.GetAt(i) else { continue };
        let Ok(view) = item.Content() else { continue };
        if !view.Contains(&text_format).unwrap_or(false) {
            continue;
        }
        let Ok(t) = view.GetTextAsync().and_then(|op| op.join()) else {
            continue;
        };
        // Compare as UTF-16 units, without making another copy of either text.
        if t.iter().copied().eq(text.encode_utf16()) {
            out.push(item);
        }
    }
    out
}
