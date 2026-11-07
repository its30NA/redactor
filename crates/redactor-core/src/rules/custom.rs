//! User-defined regex rules (`EngineConfig::custom_rules`).

use std::borrow::Cow;

use regex::Regex;

use crate::config::CustomRule;
use crate::detector::Detector;
use crate::engine::EngineError;
use crate::finding::{Candidate, Source};

pub(crate) struct CustomDetector {
    id: String,
    label: String,
    re: Regex,
    group: usize,
    confidence: f32,
}

impl CustomDetector {
    pub fn new(rule: &CustomRule) -> Result<CustomDetector, EngineError> {
        let err = |reason: String| EngineError::InvalidCustomRule {
            id: rule.id.clone(),
            reason,
        };
        if rule.id.is_empty() || rule.label.is_empty() {
            return Err(err("id and label must not be empty".into()));
        }
        if !(0.0..=1.0).contains(&rule.confidence) {
            return Err(err("confidence must be between 0 and 1".into()));
        }
        let re = Regex::new(&rule.pattern).map_err(|e| err(e.to_string()))?;
        if rule.group >= re.captures_len() {
            return Err(err(format!("pattern has no group {}", rule.group)));
        }
        Ok(CustomDetector {
            id: rule.id.clone(),
            label: rule.label.clone(),
            re,
            group: rule.group,
            confidence: rule.confidence,
        })
    }
}

impl Detector for CustomDetector {
    fn id(&self) -> &'static str {
        "custom"
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        for caps in self.re.captures_iter(text) {
            let Some(m) = caps.get(self.group) else {
                continue;
            };
            if m.is_empty() {
                continue;
            }
            out.push(Candidate {
                start: m.start(),
                end: m.end(),
                kind: Cow::Owned(self.id.clone()),
                label: Cow::Owned(self.label.clone()),
                confidence: self.confidence,
                detector: Cow::Owned(self.id.clone()),
                reason: Cow::Borrowed("Custom rule"),
                source: Source::Custom,
            });
        }
    }
}
