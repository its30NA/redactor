//! The detector interface. Every source of candidates (the pattern catalog, structured
//! detectors, the context engine, the classifier, custom rules) implements it.

use crate::finding::Candidate;

/// A pure function of its input: no I/O, no shared mutable state.
pub trait Detector: Send + Sync {
    /// Stable id, used by `EngineConfig::disabled` / `enabled`.
    fn id(&self) -> &'static str;

    /// Appends candidates found in `text` (the shadow text). Offsets are bytes into `text`
    /// and must lie on char boundaries.
    fn detect(&self, text: &str, out: &mut Vec<Candidate>);

    /// PII detectors only run when PII detection is on.
    fn is_pii(&self) -> bool {
        false
    }

    /// Number of rules this detector implements (for `Engine::rule_count`).
    fn rule_count(&self) -> usize {
        1
    }
}
