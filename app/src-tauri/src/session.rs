//! Session secrets: values the user asked to remember for this session only.
//!
//! The raw values live in this process's memory and nowhere else: never on disk, never hashed,
//! never sent over IPC (the UI sees `{ id, label, addedAt }` only, not even the length). They
//! are zeroized when forgotten, wiped (idle, lock, sleep, tray) and at quit.
//!
//! Matching borrows the values under a read lock and searches the text with `memchr::memmem`.
//! There is deliberately no prebuilt automaton: a search structure would hold its own copies of
//! the values (Aho-Corasick keeps the pattern bytes in its transition tables and prefilters),
//! and those copies could not be zeroized. A handful of values against a few MiB is fast enough.

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use redactor_core::{MatcherHit, SecretMatcher, Utf16Map};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use zeroize::{Zeroize, Zeroizing};

/// Minimum length of a typed session secret, in characters (`sessionAdd`).
pub const MIN_TYPED: usize = 6;
/// Minimum length of a remembered selection or finding, in characters. Shorter values would
/// match all over ordinary text.
pub const MIN_REMEMBERED: usize = 4;
/// Longest value, in bytes (room for a private key block).
pub const MAX_VALUE_BYTES: usize = 16 * 1024;
/// How many secrets one session holds.
pub const MAX_SECRETS: usize = 200;
/// Longest label, in characters.
pub const MAX_LABEL: usize = 60;
/// Label used when the UI sends none.
pub const DEFAULT_LABEL: &str = "Remembered Secret";

/// What the UI sees of a session secret: never the value, not even its length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSecret {
    pub id: String,
    pub label: String,
    pub added_at: String,
}

/// Why a secret was not remembered. Shown to the user as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    TooShort(usize),
    TooLong,
    BadRange,
    LabelTooLong,
    LabelHasValue,
    Full,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::TooShort(n) => write!(f, "At least {n} characters."),
            SessionError::TooLong => f.write_str("That is too long to remember."),
            SessionError::BadRange => f.write_str("Nothing selected."),
            SessionError::LabelTooLong => write!(f, "Labels are at most {MAX_LABEL} characters."),
            SessionError::LabelHasValue => {
                f.write_str("The label contains the secret itself; choose another label.")
            }
            SessionError::Full => write!(f, "This session holds up to {MAX_SECRETS} secrets."),
        }
    }
}

impl std::error::Error for SessionError {}

/// A secret's bytes. Zeroized in place when dropped.
///
/// A boxed slice has no spare capacity, so every byte the value ever occupied is the slice.
pub struct SecretValue(Box<[u8]>);

#[cfg(test)]
static WIPES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl SecretValue {
    fn new(value: &str) -> SecretValue {
        SecretValue(value.as_bytes().into())
    }

    fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// Overwrites the value with zeros (the length stays).
    fn wipe(&mut self) {
        self.0.zeroize();
        #[cfg(test)]
        WIPES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretValue(<hidden>)")
    }
}

struct Entry {
    meta: SessionSecret,
    value: SecretValue,
}

#[derive(Default)]
struct Inner {
    entries: Vec<Entry>,
    next_id: u64,
}

/// The session's secrets. Shared by the IPC commands and the engine's matcher.
#[derive(Default)]
pub struct SessionStore {
    inner: RwLock<Inner>,
}

impl SessionStore {
    pub fn new() -> SessionStore {
        SessionStore::default()
    }

    // A poisoned lock only means another thread panicked mid-update; the release build aborts
    // on panic anyway, so recover the data rather than lose the ability to wipe it.
    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn list(&self) -> Vec<SessionSecret> {
        self.read().entries.iter().map(|e| e.meta.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.read().entries.is_empty()
    }

    /// Remembers `text[start..end]` (UTF-16 offsets, as the UI has them), trimmed of whitespace.
    pub fn remember(
        &self,
        text: &str,
        start: usize,
        end: usize,
        label: &str,
    ) -> Result<Vec<SessionSecret>, SessionError> {
        if end <= start {
            return Err(SessionError::BadRange);
        }
        let map = Utf16Map::new(text);
        if end > map.utf16_len() {
            return Err(SessionError::BadRange);
        }
        let (s, e) = (map.to_byte(start), map.to_byte(end));
        self.insert(text[s..e].trim(), label, MIN_REMEMBERED)
    }

    /// Remembers a value typed into the masked field.
    pub fn add(&self, secret: &str, label: &str) -> Result<Vec<SessionSecret>, SessionError> {
        self.insert(secret.trim(), label, MIN_TYPED)
    }

    pub fn forget(&self, id: &str) -> Vec<SessionSecret> {
        let mut g = self.write();
        g.entries.retain(|e| e.meta.id != id);
        g.entries.iter().map(|e| e.meta.clone()).collect()
    }

    /// Forgets everything (Forget all, and every wipe).
    pub fn clear(&self) -> Vec<SessionSecret> {
        self.write().entries.clear();
        Vec::new()
    }

    fn insert(
        &self,
        raw: &str,
        label: &str,
        min_chars: usize,
    ) -> Result<Vec<SessionSecret>, SessionError> {
        let value = normalize(raw);
        if value.chars().count() < min_chars {
            return Err(SessionError::TooShort(min_chars));
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(SessionError::TooLong);
        }
        let label = label.trim();
        let label = if label.is_empty() {
            DEFAULT_LABEL
        } else {
            label
        };
        if label.chars().count() > MAX_LABEL {
            return Err(SessionError::LabelTooLong);
        }
        // The label ends up in placeholders in the output; it must not carry the value.
        if lowercase_zeroizing(&normalize(label)).contains(lowercase_zeroizing(&value).as_str()) {
            return Err(SessionError::LabelHasValue);
        }
        let label: String = label.chars().filter(|c| !c.is_control()).collect();
        let mut g = self.write();
        if !g
            .entries
            .iter()
            .any(|e| e.value.bytes() == value.as_bytes())
        {
            if g.entries.len() >= MAX_SECRETS {
                return Err(SessionError::Full);
            }
            g.next_id += 1;
            let id = format!("s{}", g.next_id);
            g.entries.push(Entry {
                meta: SessionSecret {
                    id,
                    label,
                    added_at: now_rfc3339(),
                },
                value: SecretValue::new(&value),
            });
        }
        Ok(g.entries.iter().map(|e| e.meta.clone()).collect())
    }
}

/// The engine runs matchers on its normalized shadow text (invisible, bidi and tag characters
/// removed, NFKC per character), so values are stored in the same form.
///
/// No copy of the value is left behind unzeroed: the characters are walked in place (no
/// `Vec<char>` copy), and the output never grows by reallocation, which would free the old
/// buffer without zeroing it; see [`push_zeroizing`]. (The NFKC iterator keeps at most one
/// character's decomposition in its own small buffer.)
fn normalize(raw: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(raw.len()));
    if raw.is_ascii() {
        out.push_str(raw);
        return out;
    }
    let mut prev: Option<char> = None;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        let before = prev.replace(c);
        if is_hidden(c) {
            // Keep a zero-width joiner between two non-ASCII characters (emoji sequences).
            let joins = c == '\u{200D}'
                && before.is_some_and(|b| !b.is_ascii())
                && chars.peek().is_some_and(|n| !n.is_ascii());
            if !joins {
                continue;
            }
        }
        for n in std::iter::once(c).nfkc() {
            push_zeroizing(&mut out, n);
        }
    }
    out
}

/// Appends `c` without letting `String` reallocate: when the capacity is used up, the text
/// moves to a new buffer twice as large and the old one is zeroized as it is dropped.
fn push_zeroizing(out: &mut Zeroizing<String>, c: char) {
    if out.capacity() - out.len() < c.len_utf8() {
        let mut bigger = Zeroizing::new(String::with_capacity((out.capacity() * 2).max(16)));
        bigger.push_str(out);
        *out = bigger;
    }
    out.push(c);
}

/// `s.to_lowercase()`, built with [`push_zeroizing`] so no unzeroed copy is left behind.
fn lowercase_zeroizing(s: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(s.len()));
    for c in s.chars().flat_map(char::to_lowercase) {
        push_zeroizing(&mut out, c);
    }
    out
}

fn is_hidden(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}'
        | '\u{2060}'..='\u{2064}'
        | '\u{FEFF}'
        | '\u{00AD}'
        | '\u{180E}'
        | '\u{034F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2066}'..='\u{2069}'
        | '\u{E0000}'..='\u{E007F}')
}

/// UTC now as `YYYY-MM-DDTHH:MM:SSZ`.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339(secs)
}

fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Feeds the session secrets to the engine (`Source::Watchlist`).
pub struct SessionMatcher(pub Arc<SessionStore>);

impl SecretMatcher for SessionMatcher {
    fn find(&self, text: &str) -> Vec<MatcherHit> {
        let g = self.0.read();
        let hay = text.as_bytes();
        let mut hits = Vec::new();
        for e in &g.entries {
            let needle = e.value.bytes();
            for at in memchr::memmem::find_iter(hay, needle) {
                hits.push(MatcherHit {
                    start: at,
                    end: at + needle.len(),
                    label: e.meta.label.clone(),
                });
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redactor_core::{Engine, EngineConfig, ScanRequest, Source};

    /// A password-shaped test value, built at run time (no secret-shaped literals in the repo).
    fn fake_secret(seed: u8) -> String {
        let parts = ["Plum", "tree", &format!("{}", 40 + seed), "-", "Kite"];
        parts.concat()
    }

    #[test]
    fn remember_from_selection_utf16() {
        let store = SessionStore::new();
        let secret = fake_secret(1);
        // An astral emoji (2 UTF-16 units, 4 bytes) and a CRLF before the secret.
        let text = format!("😀 note\r\npw {secret}\r\nend");
        let start = text[..text.find(&secret).unwrap()].encode_utf16().count();
        let end = start + secret.encode_utf16().count();
        let list = store.remember(&text, start, end, "Staging DB").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "Staging DB");
        assert_eq!(store.read().entries[0].value.bytes(), secret.as_bytes());
        // A range past the end or reversed is refused.
        let store = SessionStore::new();
        assert_eq!(
            store.remember(&text, end, start, "x"),
            Err(SessionError::BadRange)
        );
        assert_eq!(
            store.remember(&text, 0, 10_000, "x"),
            Err(SessionError::BadRange)
        );
    }

    #[test]
    fn remember_trims_the_selection() {
        let store = SessionStore::new();
        let secret = fake_secret(2);
        let text = format!("a  {secret}\r\n b");
        let list = store
            .remember(&text, 1, text.encode_utf16().count() - 2, "")
            .unwrap();
        assert_eq!(list[0].label, DEFAULT_LABEL);
        assert_eq!(store.read().entries[0].value.bytes(), secret.as_bytes());
    }

    #[test]
    fn add_min_len_6() {
        let store = SessionStore::new();
        assert_eq!(store.add("abcde", "x"), Err(SessionError::TooShort(6)));
        assert_eq!(store.add("  abcde  ", "x"), Err(SessionError::TooShort(6)));
        assert!(store.is_empty());
        assert_eq!(store.add("abcdef", "Six").unwrap().len(), 1);
        // Remembering the same value twice keeps one entry.
        assert_eq!(store.add("abcdef", "Again").unwrap().len(), 1);
    }

    #[test]
    fn label_must_not_contain_the_value() {
        let store = SessionStore::new();
        let secret = fake_secret(3);
        let label = format!("db {}", secret.to_uppercase());
        assert_eq!(store.add(&secret, &label), Err(SessionError::LabelHasValue));
        let long = "L".repeat(MAX_LABEL + 1);
        assert_eq!(store.add(&secret, &long), Err(SessionError::LabelTooLong));
    }

    #[test]
    fn matcher_hits_all_occurrences() {
        let store = Arc::new(SessionStore::new());
        let a = fake_secret(4);
        let b = fake_secret(5);
        store.add(&a, "First").unwrap();
        store.add(&b, "Second").unwrap();
        let text = format!("x={a} y={b}\nagain {a}, in a url https://h/?p={a}&q=1");
        let engine = Engine::new(EngineConfig::default())
            .unwrap()
            .with_matcher(Arc::new(SessionMatcher(store.clone())));
        let out = engine.scan(&text, &ScanRequest::default());
        let watch: Vec<_> = out
            .findings
            .iter()
            .filter(|f| f.source == Source::Watchlist)
            .collect();
        assert_eq!(watch.len(), 4, "{:?}", out.findings);
        assert!(!out.output.contains(&a));
        assert!(!out.output.contains(&b));
        assert!(out.output.contains("[REDACTED: First"));
        assert!(out.output.contains("[REDACTED: Second"));
        // The matcher sees changes without a new engine.
        store.clear();
        let out = engine.scan(&text, &ScanRequest::default());
        assert!(out.findings.iter().all(|f| f.source != Source::Watchlist));
    }

    #[test]
    fn normalize_grows_without_reallocating_and_keeps_joiners() {
        // U+FDFA expands to 18 characters under NFKC: far more than the input's capacity.
        let raw = "\u{FDFA}\u{FDFA}x";
        let out = normalize(raw);
        let expected: String = raw.nfkc().collect();
        assert_eq!(out.as_str(), expected);
        assert!(out.capacity() >= out.len());
        // A joiner between two emoji stays; one next to ASCII goes, as do other hidden ones.
        assert_eq!(
            normalize("\u{1F469}\u{200D}\u{1F4BB}").as_str(),
            "\u{1F469}\u{200D}\u{1F4BB}"
        );
        assert_eq!(
            normalize("ab\u{200D}\u{1F4BB}\u{200B}").as_str(),
            "ab\u{1F4BB}"
        );
        assert_eq!(
            lowercase_zeroizing("\u{130}X").as_str(),
            "\u{130}X".to_lowercase()
        );
    }

    #[test]
    fn matcher_finds_values_hidden_by_invisible_characters() {
        let store = Arc::new(SessionStore::new());
        let secret = fake_secret(6);
        // Remembered from a selection that carried a zero-width space.
        let split = format!("{}\u{200B}{}", &secret[..4], &secret[4..]);
        store
            .remember(&split, 0, split.encode_utf16().count(), "Split")
            .unwrap();
        let engine = Engine::new(EngineConfig::default())
            .unwrap()
            .with_matcher(Arc::new(SessionMatcher(store)));
        let text = format!("plain {secret} and split {split}");
        let out = engine.scan(&text, &ScanRequest::default());
        assert!(!out.output.contains(&secret));
        assert!(!out.output.contains(&secret[4..]));
    }

    #[test]
    fn forget_and_clear() {
        let store = SessionStore::new();
        store.add(&fake_secret(7), "One").unwrap();
        let list = store.add(&fake_secret(8), "Two").unwrap();
        assert_eq!(list.len(), 2);
        let left = store.forget(&list[0].id);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].label, "Two");
        assert_eq!(store.forget("no-such-id").len(), 1);
        assert!(store.clear().is_empty());
        assert!(store.is_empty());
        // Ids are not reused.
        let again = store.add(&fake_secret(9), "Three").unwrap();
        assert!(!list.iter().any(|s| s.id == again[0].id));
    }

    #[test]
    fn wipe_clears_session() {
        let store = Arc::new(SessionStore::new());
        store.add(&fake_secret(10), "One").unwrap();
        let wiper = crate::wipe::Wiper::new(store.clone());
        let before = WIPES.load(std::sync::atomic::Ordering::SeqCst);
        let changed = wiper.wipe();
        assert!(changed.is_empty());
        assert!(store.is_empty());
        assert!(WIPES.load(std::sync::atomic::Ordering::SeqCst) > before);
    }

    #[test]
    fn session_meta_has_no_value() {
        let store = SessionStore::new();
        let secret = fake_secret(11);
        let list = store.add(&secret, "Label").unwrap();
        let json = serde_json::to_string(&list).unwrap();
        assert!(!json.contains(&secret));
        let obj = serde_json::to_value(&list[0]).unwrap();
        let mut keys: Vec<_> = obj.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["addedAt", "id", "label"]);
        assert!(!format!("{:?}", store.read().entries[0].value).contains(&secret));
    }

    #[test]
    fn values_zeroized_on_drop() {
        let secret = fake_secret(12);
        let mut v = SecretValue::new(&secret);
        assert_eq!(v.bytes(), secret.as_bytes());
        v.wipe();
        assert_eq!(v.bytes().len(), secret.len());
        assert!(v.bytes().iter().all(|&b| b == 0));
        // Dropping runs the same wipe.
        let v = SecretValue::new(&secret);
        let before = WIPES.load(std::sync::atomic::Ordering::SeqCst);
        drop(v);
        assert!(WIPES.load(std::sync::atomic::Ordering::SeqCst) > before);
    }

    #[test]
    fn added_at_is_utc_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_177_730), "2026-10-28T08:55:30Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }
}
