//! The engines the shell scans with.
//!
//! An [`Engine`] is immutable, and three settings are part of its config: the always-redact
//! words (`EngineConfig.terms`), the placeholder style and `stripInvisible`. Sensitivity and PII
//! come with each request. So the set keeps one engine per `stripInvisible` value for the current
//! words and style, drops them when those change and builds new ones off the UI thread.
//! Session secrets do not need a rebuild: their matcher reads the live session list.
//!
//! All engines share one per-process session key, so a finding's `valueKey` (what the UI sends
//! back in `keep`) stays the same across rebuilds.

use std::sync::{Arc, Mutex, MutexGuard};

use redactor_core::{Engine, EngineConfig, PlaceholderStyle};

use crate::session::{SessionMatcher, SessionStore};
use crate::settings::Settings;

/// The settings an engine is built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub placeholder: PlaceholderStyle,
    pub terms: Vec<String>,
}

impl Profile {
    pub fn of(settings: &Settings) -> Profile {
        Profile {
            placeholder: settings.placeholder_style,
            terms: settings.always_redact.clone(),
        }
    }
}

struct State {
    generation: u64,
    profile: Profile,
    /// Index 0: `strip_invisible == false`, 1: `true`.
    engines: [Option<Arc<Engine>>; 2],
}

pub struct EngineSet {
    session_key: [u8; 32],
    session: Arc<SessionStore>,
    state: Mutex<State>,
}

impl EngineSet {
    pub fn new(session: Arc<SessionStore>, profile: Profile) -> EngineSet {
        EngineSet {
            // The core's default config draws a random key per process; keep that one.
            session_key: EngineConfig::default().session_key,
            session,
            state: Mutex::new(State {
                generation: 0,
                profile,
                engines: [None, None],
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn build(&self, profile: &Profile, strip_invisible: bool) -> Arc<Engine> {
        let config = EngineConfig {
            strip_invisible,
            placeholder: profile.placeholder,
            terms: profile.terms.clone(),
            session_key: self.session_key,
            ..EngineConfig::default()
        };
        // The default template and no custom rules or allowlist: building cannot fail.
        let engine = Engine::new(config).unwrap_or_else(|_| std::process::abort());
        Arc::new(engine.with_matcher(Arc::new(SessionMatcher(self.session.clone()))))
    }

    /// The engine for the current profile, built on first use (on the calling thread, which
    /// for IPC commands is a worker thread, never the UI thread).
    pub fn get(&self, strip_invisible: bool) -> Arc<Engine> {
        let (generation, profile) = {
            let st = self.lock();
            if let Some(e) = &st.engines[usize::from(strip_invisible)] {
                return e.clone();
            }
            (st.generation, st.profile.clone())
        };
        let built = self.build(&profile, strip_invisible);
        let mut st = self.lock();
        if st.generation == generation {
            st.engines[usize::from(strip_invisible)] = Some(built.clone());
        }
        built
    }

    /// Switches to a new profile. Returns true when it differs from the current one; the caller
    /// then warms the new engine with [`EngineSet::get`] on a background thread. Both old
    /// engines are dropped at once, so a scan that comes before the warm-up has finished builds
    /// the new engine itself, on its own worker thread.
    pub fn set_profile(&self, profile: Profile) -> bool {
        let mut st = self.lock();
        if st.profile == profile {
            return false;
        }
        st.generation += 1;
        st.profile = profile;
        st.engines = [None, None];
        true
    }

    /// Active rules of the current engine (for `appInfo`).
    pub fn rule_count(&self) -> usize {
        self.get(true).rule_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redactor_core::{ScanRequest, Source};

    #[test]
    fn value_keys_survive_rebuilds() {
        let set = EngineSet::new(
            Arc::new(SessionStore::new()),
            Profile {
                placeholder: PlaceholderStyle::Labeled,
                terms: vec![],
            },
        );
        let text = format!("DB_PASSWORD={}", ["Plum", "tree42", "-Kite"].concat());
        let text = text.as_str();
        let a = set.get(true).scan(text, &ScanRequest::default());
        assert!(set.set_profile(Profile {
            placeholder: PlaceholderStyle::Generic,
            terms: vec!["Nightjar".into()],
        }));
        let b = set.get(true).scan(text, &ScanRequest::default());
        assert!(!a.findings.is_empty());
        assert_eq!(a.findings[0].value_key, b.findings[0].value_key);
        assert_ne!(a.output, b.output, "the style changed");
        let c = set.get(false).scan(text, &ScanRequest::default());
        assert_eq!(a.findings[0].value_key, c.findings[0].value_key);
    }

    #[test]
    fn terms_reach_the_engine() {
        let set = EngineSet::new(
            Arc::new(SessionStore::new()),
            Profile {
                placeholder: PlaceholderStyle::Labeled,
                terms: vec![],
            },
        );
        let text = "Ask Nightjar about it.";
        assert!(set
            .get(true)
            .scan(text, &ScanRequest::default())
            .findings
            .is_empty());
        set.set_profile(Profile {
            placeholder: PlaceholderStyle::Labeled,
            terms: vec!["nightjar".into()],
        });
        let out = set.get(true).scan(text, &ScanRequest::default());
        assert_eq!(out.findings.len(), 1);
        assert_eq!(out.findings[0].source, Source::Custom);
        assert!(!set.set_profile(Profile {
            placeholder: PlaceholderStyle::Labeled,
            terms: vec!["nightjar".into()],
        }));
    }
}
