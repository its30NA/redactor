//! The engine: built once from an [`EngineConfig`], then `scan` many times.

use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use regex::{Regex, RegexSet};

use crate::classifier::{self, ClassifierDetector};
use crate::config::{EngineConfig, Sensitivity, DEFAULT_TEMPLATE};
use crate::context::{self, looks_like_password, value_score, ContextDetector};
use crate::decode::{decode_layers, Budget};
use crate::detector::Detector;
use crate::finding::{
    Candidate, Finding, ScanOutcome, SecretMatcher, Source, Tier, ValueKey, Warning, WarningKind,
};
use crate::guard::is_placeholder;
use crate::normalize::{normalize_value, HiddenCounts, Shadow};
use crate::occurrences::Occurrences;
use crate::placeholder::{PlaceholderPolicy, GENERIC_LABEL};
use crate::propagate::{propagate, Copies, ValueCopy, BOUNDARY_BELOW, MIN_CHARS};
use crate::redact::{apply, Redaction, Resolved};
use crate::resolve::{outranks, resolve};
use crate::rules::catalog;
use crate::rules::custom::CustomDetector;
use crate::terms::{self, TermsDetector};

/// Per-scan options. Everything is optional; `ScanRequest::default()` uses the engine config.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScanRequest<'a> {
    pub sensitivity: Option<Sensitivity>,
    pub pii: Option<bool>,
    /// The user said "keep" for these values.
    pub keep: &'a [ValueKey],
    /// User-selected byte ranges (into the input) to redact.
    pub manual: &'a [(usize, usize)],
}

/// Why an engine could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// A custom rule has an invalid regex, group or confidence.
    InvalidCustomRule { id: String, reason: String },
    /// An allowlist entry is not a valid regex.
    InvalidAllowlist { index: usize, reason: String },
    /// The placeholder template is empty.
    InvalidTemplate,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::InvalidCustomRule { id, reason } => {
                write!(f, "custom rule {id:?}: {reason}")
            }
            EngineError::InvalidAllowlist { index, reason } => {
                write!(f, "allowlist entry {index}: {reason}")
            }
            EngineError::InvalidTemplate => {
                write!(f, "placeholder template is empty")
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// Label of user-selected ranges.
const MANUAL_LABEL: &str = "Selected Text";
/// Label of a `user:password` pair found inside a decoded layer.
const CREDENTIALS_LABEL: &str = "Credentials";

/// Immutable after construction; `Send + Sync`, so one engine can serve many threads.
pub struct Engine {
    config: EngineConfig,
    policy: PlaceholderPolicy,
    /// Derived from the session key; keys [`ValueKey`]s.
    value_hash_key: [u8; 32],
    detectors: Vec<Box<dyn Detector>>,
    pii_detectors: Vec<Box<dyn Detector>>,
    allowlist: Option<RegexSet>,
    matcher: Option<Arc<dyn SecretMatcher>>,
    /// Placeholders of the configured and the default template, with a label from
    /// `labels`: protected text.
    placeholder_res: Vec<Regex>,
    /// Every label the engine itself can put in a placeholder (`known_labels`).
    labels: Vec<String>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("config", &self.config)
            .field("rules", &self.rule_count())
            .field("matcher", &self.matcher.is_some())
            .finish()
    }
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Engine, EngineError> {
        if config.template.is_empty() {
            return Err(EngineError::InvalidTemplate);
        }
        for (index, pattern) in config.allowlist.iter().enumerate() {
            if let Err(e) = regex::Regex::new(pattern) {
                return Err(EngineError::InvalidAllowlist {
                    index,
                    reason: e.to_string(),
                });
            }
        }
        let allowlist = if config.allowlist.is_empty() {
            None
        } else {
            Some(
                RegexSet::new(&config.allowlist).map_err(|e| EngineError::InvalidAllowlist {
                    index: 0,
                    reason: e.to_string(),
                })?,
            )
        };
        let mut detectors: Vec<Box<dyn Detector>> = catalog(&config);
        // The context engine and the classifier, then the user's custom rules.
        if !config.disabled.iter().any(|d| d == context::ID) {
            detectors.push(Box::new(ContextDetector));
        }
        if !config.disabled.iter().any(|d| d == classifier::ID) {
            detectors.push(Box::new(ClassifierDetector));
        }
        for rule in &config.custom_rules {
            detectors.push(Box::new(CustomDetector::new(rule)?));
        }
        if !config.disabled.iter().any(|d| d == terms::ID) {
            if let Some(t) = TermsDetector::new(&config.terms) {
                detectors.push(Box::new(t));
            }
        }
        let (pii_detectors, detectors): (Vec<_>, Vec<_>) =
            detectors.into_iter().partition(|d| d.is_pii());
        let labels = known_labels(&config);
        let placeholder_res = placeholder_regexes(&config.template, &labels);
        Ok(Engine {
            policy: PlaceholderPolicy {
                style: config.placeholder,
                template: config.template.clone(),
            },
            value_hash_key: blake3::derive_key("redactor-core v2 value key", &config.session_key),
            config,
            detectors,
            pii_detectors,
            allowlist,
            matcher: None,
            placeholder_res,
            labels,
        })
    }

    /// Adds a matcher for known secrets (the watchlist).
    pub fn with_matcher(mut self, m: Arc<dyn SecretMatcher>) -> Engine {
        self.matcher = Some(m);
        self
    }

    /// True when `text` is exactly one placeholder this engine recognises as earlier output:
    /// its template (or the default one) around a label it emits.
    pub fn is_placeholder(&self, text: &str) -> bool {
        self.protected(text, &[]) == [(0, text.len())]
    }

    /// Number of active detection rules (PII rules included).
    pub fn rule_count(&self) -> usize {
        self.detectors
            .iter()
            .chain(&self.pii_detectors)
            .map(|d| d.rule_count())
            .sum()
    }

    /// The opaque key of a value (normalized first, so hidden characters do not matter).
    pub fn value_key(&self, value: &str) -> ValueKey {
        let norm = normalize_value(value);
        let hash = blake3::keyed_hash(&self.value_hash_key, norm.as_bytes());
        let mut k = [0u8; 16];
        k.copy_from_slice(&hash.as_bytes()[..16]);
        ValueKey(k)
    }

    /// The key of a candidate's value. Always-redact terms are keyed case-insensitively, so
    /// `ACME` and `Acme` share one placeholder.
    fn candidate_key(&self, text: &str, c: &Candidate) -> ValueKey {
        let value = &text[c.start..c.end];
        if c.detector == terms::ID {
            self.value_key(&value.to_lowercase())
        } else {
            self.value_key(value)
        }
    }

    pub fn scan(&self, text: &str, req: &ScanRequest) -> ScanOutcome {
        let sensitivity = req.sensitivity.unwrap_or(self.config.sensitivity);
        let pii = req.pii.unwrap_or(self.config.pii);
        let threshold = sensitivity.threshold();

        // 1.-6. Candidates in input offsets.
        let first = self.collect(text, pii, threshold, req.manual, req.keep, &[]);
        let mut raw = first.cands;
        let mut aliases: Aliases = first.aliases.into_iter().collect();

        // 7.-8. Resolve and redact.
        let mut rendered = self.render(text, raw.clone(), req.keep, &aliases);

        // 9. Verify: rescan the output until it comes back clean. Whatever the rescan finds
        // is mapped back to the input and redacted with the rest.
        let mut verified = true;
        let mut rounds = 0usize;
        while rendered.redaction.output != text {
            let (mut new, new_aliases) =
                self.verify_round(text, &rendered, pii, threshold, req.keep);
            aliases.extend(new_aliases);
            if !new.is_empty() {
                let have: HashSet<(usize, usize, &str)> =
                    raw.iter().map(|r| (r.start, r.end, &*r.label)).collect();
                new.retain(|c| !have.contains(&(c.start, c.end, &*c.label)));
            }
            if new.is_empty() {
                break;
            }
            if rounds == VERIFY_CAP {
                verified = false;
                break;
            }
            rounds += 1;
            raw.extend(new);
            rendered = self.render(text, raw.clone(), req.keep, &aliases);
        }

        let mut warnings = Vec::new();
        for (kind, count) in [
            (WarningKind::InvisibleChars, first.counts.invisible),
            (WarningKind::BidiControls, first.counts.bidi),
            (WarningKind::TagChars, first.counts.tag),
            (WarningKind::DecodeLimit, first.decode_limit),
            (WarningKind::VerifyCapHit, usize::from(!verified)),
        ] {
            if count > 0 {
                warnings.push(Warning { kind, count });
            }
        }

        ScanOutcome {
            output: rendered.redaction.output,
            findings: rendered.findings,
            output_spans: rendered.redaction.spans,
            warnings,
            verified,
        }
    }

    /// The first stage over `text`: shadow text, detectors, decode layer, guards and threshold,
    /// manual ranges, propagation. Candidates come back in `text` offsets. Placeholders in
    /// `text` (and the `known` ranges, sorted) are protected: no candidate cuts into one.
    fn collect(
        &self,
        text: &str,
        pii: bool,
        threshold: f32,
        manual: &[(usize, usize)],
        keep: &[ValueKey],
        known: &[(usize, usize)],
    ) -> Collected {
        let shadow = Shadow::new(text);

        // 2. Detectors (and the matcher) on the shadow text.
        let mut raw = self.detect(&shadow.text, pii);

        // 3. Decode layer: encoded runs are decoded and scanned again.
        let mut budget = Budget::for_input(text.len());
        let mut too_deep = 0usize;
        let layers = self.decoded(
            &shadow.text,
            1,
            pii,
            threshold,
            &mut budget,
            &mut too_deep,
            &mut raw,
        );
        let decode_limit = too_deep + budget.skipped;
        if decode_limit == 0 && !layers.is_empty() {
            // A run that decodes to readable text is not random; what it holds was judged
            // inside the layer.
            let inside = within_any(&layers);
            raw.retain(|c| c.source != Source::Classifier || !inside(c.start, c.end));
        }

        // Back to original offsets.
        for c in &mut raw {
            let r = shadow.to_original(c.start..c.end);
            c.start = r.start;
            c.end = r.end;
        }

        // Placeholders already in the text are not touched.
        let protected = self.protected(text, known);
        if !protected.is_empty() {
            protect(text, &mut raw, &protected);
        }

        // 4.-5. Guards and threshold.
        raw.retain(|c| self.passes_gates(text, c, threshold));

        // Manual ranges bypass every gate.
        for &(s, e) in manual {
            let (s, e) = snap(text, s, e);
            if e > s {
                raw.push(Candidate {
                    start: s,
                    end: e,
                    kind: Cow::Borrowed("manual"),
                    label: Cow::Borrowed(MANUAL_LABEL),
                    confidence: 1.0,
                    detector: Cow::Borrowed("manual"),
                    reason: Cow::Borrowed("Selected by you"),
                    source: Source::Manual,
                });
            }
        }

        // 6. Propagation: further occurrences of every value found.
        // An encoded copy takes the key of the value it encodes.
        let aliases: Vec<Alias> = propagate(text, &shadow, &mut raw, |c| {
            !keep.is_empty() && keep.contains(&self.candidate_key(text, c))
        })
        .into_iter()
        .map(|(span, rep)| (span, self.candidate_key(text, &raw[rep])))
        .collect();
        if !protected.is_empty() {
            protect(text, &mut raw, &protected);
        }

        Collected {
            cands: raw,
            aliases,
            counts: shadow.counts,
            decode_limit,
        }
    }

    /// The second stage: resolve the candidates (honouring "keep") and splice placeholders.
    fn render(
        &self,
        text: &str,
        raw: Vec<Candidate>,
        keep: &[ValueKey],
        aliases: &Aliases,
    ) -> Rendered {
        let key_of = |c: &Candidate| match aliases.get(&(c.start, c.end)) {
            Some(k) => *k,
            None => self.candidate_key(text, c),
        };
        // 7. Resolve, honouring "keep". A candidate whose own value is kept stays out of the
        // merge; a merged span whose union value is kept is kept as a whole.
        let is_kept = |c: &Candidate, key: &ValueKey| -> bool {
            c.source != Source::Manual && keep.contains(key)
        };
        let (kept, active): (Vec<Candidate>, Vec<Candidate>) = if keep.is_empty() {
            (Vec::new(), raw)
        } else {
            raw.into_iter().partition(|c| is_kept(c, &key_of(c)))
        };
        let mut spans: Vec<(Candidate, ValueKey, bool)> = Vec::new();
        for c in resolve(active) {
            let key = key_of(&c);
            let k = is_kept(&c, &key);
            spans.push((c, key, k));
        }
        let mut active_ranges: Vec<(usize, usize)> = spans
            .iter()
            .filter(|s| !s.2)
            .map(|s| (s.0.start, s.0.end))
            .collect();
        active_ranges.sort_unstable();
        for c in resolve(kept) {
            // Sorted, non-overlapping ranges: only the last range starting before `c.end`
            // can overlap `c`.
            let i = active_ranges.partition_point(|&(s, _)| s < c.end);
            let overlaps = i > 0 && active_ranges[i - 1].1 > c.start;
            if !overlaps {
                let key = key_of(&c);
                spans.push((c, key, true));
            }
        }
        spans.sort_by_key(|s| (s.0.start, s.0.end));
        one_label_per_value(&mut spans);

        // 8. Redact.
        let to_redact: Vec<Resolved> = spans
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.2)
            .map(|(i, (c, key, _))| Resolved {
                start: c.start,
                end: c.end,
                label: &c.label,
                value_key: *key,
                finding: i,
            })
            .collect();
        let mut redaction = apply(text, &to_redact, &self.policy, self.config.strip_invisible);
        let mut placeholders: Vec<String> = vec![String::new(); spans.len()];
        for (r, p) in to_redact.iter().zip(redaction.placeholders.drain(..)) {
            placeholders[r.finding] = p;
        }
        let findings: Vec<Finding> = spans
            .into_iter()
            .zip(placeholders)
            .map(|((c, key, kept), placeholder)| Finding {
                start: c.start,
                end: c.end,
                kind: c.kind.into_owned(),
                label: c.label.into_owned(),
                tier: Tier::from_confidence(c.confidence),
                confidence: c.confidence,
                detector: c.detector.into_owned(),
                reason: c.reason.into_owned(),
                source: c.source,
                value_key: key,
                placeholder,
                kept,
            })
            .collect();
        Rendered {
            findings,
            redaction,
        }
    }

    /// One verify round: scans the output again and checks that no
    /// redacted value survived in it. Returns what is new, and the encoded copies among it,
    /// in INPUT offsets.
    fn verify_round(
        &self,
        text: &str,
        rendered: &Rendered,
        pii: bool,
        threshold: f32,
        keep: &[ValueKey],
    ) -> (Vec<Candidate>, Vec<Alias>) {
        let red = &rendered.redaction;
        let out = red.output.as_str();
        let known: Vec<(usize, usize)> = red.spans.iter().map(|s| (s.start, s.end)).collect();
        let collected = self.collect(out, pii, threshold, &[], keep, &known);
        let mut found = collected.cands;
        if !keep.is_empty() {
            found.retain(|c| !keep.contains(&self.candidate_key(out, c)));
        }
        found.extend(leaked(text, out, &rendered.findings, &known));
        for c in &mut found {
            let (s, e) = red.to_input(c.start, c.end);
            c.start = s;
            c.end = e;
        }
        found.retain(|c| c.end > c.start);
        let aliases = collected
            .aliases
            .into_iter()
            .map(|((s, e), k)| (red.to_input(s, e), k))
            .collect();
        (found, aliases)
    }

    /// Placeholders in `text` (rendered from the configured or the default template) plus
    /// `known` ranges: sorted, merged.
    fn protected(&self, text: &str, known: &[(usize, usize)]) -> Vec<(usize, usize)> {
        let mut v: Vec<(usize, usize)> = known.to_vec();
        for re in &self.placeholder_res {
            v.extend(re.find_iter(text).map(|m| (m.start(), m.end())));
        }
        // The matcher's labels change at run time (a watchlist), so they are compiled per scan.
        let extra: Vec<String> = self
            .matcher
            .as_ref()
            .map(|m| m.labels())
            .unwrap_or_default()
            .into_iter()
            .filter(|l| !self.labels.contains(l))
            .collect();
        for re in placeholder_regexes(&self.config.template, &extra) {
            v.extend(re.find_iter(text).map(|m| (m.start(), m.end())));
        }
        if v.is_empty() {
            return v;
        }
        v.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(v.len());
        for (s, e) in v {
            match merged.last_mut() {
                Some(last) if s < last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        merged
    }

    /// Every detector, the PII detectors when on, and the matcher, over `text` (a shadow
    /// text). Offsets are into `text`.
    fn detect(&self, text: &str, pii: bool) -> Vec<Candidate> {
        let mut raw: Vec<Candidate> = Vec::new();
        for d in &self.detectors {
            d.detect(text, &mut raw);
        }
        if pii {
            for d in &self.pii_detectors {
                d.detect(text, &mut raw);
            }
        }
        if let Some(m) = &self.matcher {
            for hit in m.find(text) {
                let (s, e) = snap(text, hit.start, hit.end);
                if e > s {
                    raw.push(Candidate {
                        start: s,
                        end: e,
                        kind: Cow::Borrowed("watchlist"),
                        label: Cow::Owned(hit.label),
                        confidence: 1.0,
                        detector: Cow::Borrowed("watchlist"),
                        reason: Cow::Borrowed("Matches a remembered secret"),
                        source: Source::Watchlist,
                    });
                }
            }
        }
        raw
    }

    /// The decode layer over `text` at `depth` (1 = the input). Every
    /// layer is scanned again (detectors, matcher, deeper layers); a hit inside a layer
    /// becomes one `Decoded` candidate covering the whole encoded run in `text`, labelled
    /// "Encoded {inner label}" at the inner confidence minus 0.05. Returns the ranges of the
    /// layers found in `text`.
    #[allow(clippy::too_many_arguments)]
    fn decoded(
        &self,
        text: &str,
        depth: usize,
        pii: bool,
        threshold: f32,
        budget: &mut Budget,
        too_deep: &mut usize,
        out: &mut Vec<Candidate>,
    ) -> Vec<Range<usize>> {
        let layers = decode_layers(text, budget);
        let ranges: Vec<Range<usize>> = layers.iter().map(|l| l.range_in_parent.clone()).collect();
        if layers.is_empty() {
            return ranges;
        }
        if depth > MAX_DECODE_DEPTH {
            *too_deep += layers.len();
            return ranges;
        }
        // All layers of this level in one buffer, so the detectors run once per level. A
        // blank line between layers keeps pairs and phrases from running across.
        let mut combined = String::with_capacity(layers.iter().map(|l| l.text.len() + 2).sum());
        let mut starts = Vec::with_capacity(layers.len());
        for l in &layers {
            starts.push(combined.len());
            combined.push_str(&l.text);
            combined.push_str("\n\n");
        }
        let shadow = Shadow::new(&combined);
        let mut inner = self.detect(&shadow.text, pii);
        self.decoded(
            &shadow.text,
            depth + 1,
            pii,
            threshold,
            budget,
            too_deep,
            &mut inner,
        );
        inner.retain(|c| self.passes_gates(&shadow.text, c, threshold));
        for c in &mut inner {
            let r = shadow.to_original(c.start..c.end);
            c.start = r.start;
            c.end = r.end;
        }
        for (i, l) in layers.iter().enumerate() {
            if let Some(conf) = credential_pair(&l.text) {
                inner.push(Candidate {
                    start: starts[i],
                    end: starts[i] + l.text.len(),
                    kind: Cow::Borrowed("encoded_credentials"),
                    label: Cow::Borrowed(CREDENTIALS_LABEL),
                    confidence: conf,
                    detector: Cow::Borrowed("decode"),
                    reason: Cow::Borrowed("user:password pair"),
                    source: Source::Decoded,
                });
            }
        }
        // The strongest hit per layer speaks for the whole run.
        let mut best: Vec<Option<Candidate>> = vec![None; layers.len()];
        for c in inner {
            let i = starts.partition_point(|&s| s <= c.start);
            if i == 0 {
                continue;
            }
            let slot = &mut best[i - 1];
            if slot.as_ref().is_none_or(|b| c.confidence > b.confidence) {
                *slot = Some(c);
            }
        }
        for (l, c) in layers.iter().zip(best) {
            let Some(c) = c else {
                continue;
            };
            let label = if c.source == Source::Decoded && c.label.starts_with(ENCODED) {
                c.label
            } else {
                Cow::Owned(format!("{ENCODED}{}", c.label))
            };
            out.push(Candidate {
                start: l.range_in_parent.start,
                end: l.range_in_parent.end,
                kind: c.kind,
                label,
                confidence: (c.confidence - 0.05).max(0.0),
                detector: c.detector,
                reason: Cow::Borrowed("Found inside an encoded value"),
                source: Source::Decoded,
            });
        }
        ranges
    }

    /// Guards, then the sensitivity threshold. Watchlist hits and
    /// always-redact terms are exempt: the user told us to redact these.
    fn passes_gates(&self, text: &str, c: &Candidate, threshold: f32) -> bool {
        if c.source == Source::Watchlist || c.source == Source::Custom && c.detector == terms::ID {
            return true;
        }
        if c.confidence < threshold || c.end <= c.start {
            return false;
        }
        let value = normalize_value(&text[c.start..c.end]);
        if is_placeholder(&value) {
            return false;
        }
        if let Some(allow) = &self.allowlist {
            if allow.is_match(&value) {
                return false;
            }
        }
        true
    }
}

/// Layers of layers are decoded; one level more only warns.
const MAX_DECODE_DEPTH: usize = 2;
/// Label prefix of findings inside an encoded run.
const ENCODED: &str = "Encoded ";

/// A decoded layer that is exactly `user:password` (HTTP Basic credentials and their kin).
fn credential_pair(layer: &str) -> Option<f32> {
    let t = layer.trim_end_matches(['\n', '\r']);
    let (user, pass) = t.split_once(':')?;
    let user_ok = (1..=64).contains(&user.len())
        && user
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'@' | b'+' | b'-'));
    let pass_ok = pass.chars().count() >= 6
        && !pass.contains([':', ' ', '\t'])
        && !pass.starts_with("//")
        && !pass.chars().any(char::is_control)
        && !is_placeholder(pass)
        && (looks_like_password(pass) || value_score(pass) >= 0.5);
    (user_ok && pass_ok).then_some(0.85)
}

/// Gives every redacted occurrence of a value the same label, so that one value gets one
/// placeholder: `Password in URL #1` in the URL and in the log line that repeats it, not a
/// second `Password #1` that reads like another secret. The label (and kind) come from the
/// occurrence that [`outranks`] the others, the first one on a tie; `spans` is sorted by
/// start. How each occurrence was found (detector, reason, confidence) stays its own.
fn one_label_per_value(spans: &mut [(Candidate, ValueKey, bool)]) {
    let mut best: HashMap<ValueKey, usize> = HashMap::new();
    for (i, (c, key, kept)) in spans.iter().enumerate() {
        if *kept {
            continue;
        }
        match best.entry(*key) {
            Entry::Vacant(e) => {
                e.insert(i);
            }
            Entry::Occupied(mut e) => {
                if outranks(c, &spans[*e.get()].0) {
                    e.insert(i);
                }
            }
        }
    }
    for i in 0..spans.len() {
        if spans[i].2 {
            continue;
        }
        let w = best[&spans[i].1];
        if w != i && spans[w].0.label != spans[i].0.label {
            let (label, kind) = (spans[w].0.label.clone(), spans[w].0.kind.clone());
            spans[i].0.label = label;
            spans[i].0.kind = kind;
        }
    }
}

/// Verify rounds after the first redaction; then `VerifyCapHit`.
const VERIFY_CAP: usize = 3;

/// An encoded copy: its span, and the key of the value it encodes.
type Alias = ((usize, usize), ValueKey);
/// Encoded copies: span -> the key of the value they encode.
type Aliases = HashMap<(usize, usize), ValueKey>;

/// Output of the first stage (`collect`).
struct Collected {
    cands: Vec<Candidate>,
    /// Spans of encoded copies, with the key of the value each encodes.
    aliases: Vec<Alias>,
    counts: HiddenCounts,
    decode_limit: usize,
}

/// Output of the second stage (`render`).
struct Rendered {
    findings: Vec<Finding>,
    redaction: Redaction,
}

/// Every label the engine itself can put in a placeholder, without the `Encoded ` prefix:
/// the rules' labels, the context engine's, the custom rules', the generic one and the
/// engine's own.
fn known_labels(config: &EngineConfig) -> Vec<String> {
    let mut v: Vec<String> = crate::rules::rule_infos()
        .into_iter()
        .map(|i| i.label.to_string())
        .collect();
    v.extend(context::labels().map(String::from));
    v.extend([MANUAL_LABEL, CREDENTIALS_LABEL, GENERIC_LABEL, terms::LABEL].map(String::from));
    v.extend(config.custom_rules.iter().map(|r| r.label.clone()));
    v.sort_unstable();
    v.dedup();
    v
}

/// Regexes for the placeholders of `template` and of the default template, with a label
/// from `labels`.
fn placeholder_regexes(template: &str, labels: &[String]) -> Vec<Regex> {
    let mut out: Vec<Regex> = placeholder_regex(template, labels).into_iter().collect();
    if template != DEFAULT_TEMPLATE {
        out.extend(placeholder_regex(DEFAULT_TEMPLATE, labels));
    }
    out
}

/// A regex for the placeholders `template` renders with one of `labels` (optionally
/// `Encoded `-prefixed), or `None` when the template has too little literal text to tell a
/// placeholder from ordinary words, or there are no labels. Text merely framed like the
/// template (`"Winter-Maple-2024!"` for `"{label}"`) is not a placeholder.
fn placeholder_regex(template: &str, labels: &[String]) -> Option<Regex> {
    const LABEL: &str = "{label}";
    const SUFFIX: &str = "{suffix}";
    let mut sorted: Vec<&str> = labels
        .iter()
        .map(String::as_str)
        .filter(|l| !l.is_empty())
        .collect();
    if sorted.is_empty() {
        return None;
    }
    // Longest first: the regex alternation takes the first branch that matches.
    sorted.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    sorted.dedup();
    let alternation = sorted
        .iter()
        .map(|l| regex::escape(l))
        .collect::<Vec<_>>()
        .join("|");
    let label_re = format!("(?:{}){{0,1}}(?:{alternation})", regex::escape(ENCODED));
    let mut re = String::new();
    let mut literal = 0usize;
    let mut rest = template;
    loop {
        let label = rest.find(LABEL);
        let suffix = rest.find(SUFFIX);
        let next = match (label, suffix) {
            (Some(l), Some(s)) if s < l => Some((s, false)),
            (Some(l), _) => Some((l, true)),
            (None, Some(s)) => Some((s, false)),
            (None, None) => None,
        };
        let Some((i, is_label)) = next else {
            re.push_str(&regex::escape(rest));
            literal += rest.chars().filter(|c| !c.is_whitespace()).count();
            break;
        };
        re.push_str(&regex::escape(&rest[..i]));
        literal += rest[..i].chars().filter(|c| !c.is_whitespace()).count();
        if is_label {
            rest = &rest[i + LABEL.len()..];
            re.push_str(&label_re);
        } else {
            rest = &rest[i + SUFFIX.len()..];
            re.push_str("(?: #[0-9]{1,9})?");
        }
    }
    if literal < 2 {
        return None;
    }
    Regex::new(&re).ok()
}

/// Keeps candidates out of protected ranges (sorted, disjoint). A candidate that overlaps
/// one is cut to the parts outside it, and a part survives only if it is a substantial value
/// of its own (8+ characters that are not separators): `&key=?token=[REDACTED: …]` is no
/// secret, a private key block around a pasted placeholder still is. Manual ranges stay.
fn protect(text: &str, cands: &mut Vec<Candidate>, protected: &[(usize, usize)]) {
    let overlaps = |c: &Candidate| {
        let i = protected.partition_point(|&(_, e)| e <= c.start);
        i < protected.len() && protected[i].0 < c.end
    };
    if !cands
        .iter()
        .any(|c| c.source != Source::Manual && overlaps(c))
    {
        return;
    }
    let mut out = Vec::with_capacity(cands.len());
    for c in cands.drain(..) {
        if c.source == Source::Manual || !overlaps(&c) {
            out.push(c);
            continue;
        }
        let mut pieces: Vec<(usize, usize)> = Vec::new();
        let mut pos = c.start;
        let mut j = protected.partition_point(|&(_, e)| e <= c.start);
        while j < protected.len() && protected[j].0 < c.end {
            if protected[j].0 > pos {
                pieces.push((pos, protected[j].0));
            }
            pos = pos.max(protected[j].1);
            j += 1;
        }
        if c.end > pos {
            pieces.push((pos, c.end));
        }
        for (s, e) in pieces {
            let piece = &text[s..e];
            let lead = piece.len() - piece.trim_start().len();
            let trail = piece.len() - piece.trim_end().len();
            let (s, e) = (s + lead, e - trail);
            if e > s && substantial(&text[s..e]) {
                let mut p = c.clone();
                p.start = s;
                p.end = e;
                out.push(p);
            }
        }
    }
    *cands = out;
}

/// 8+ characters once separators at the ends are trimmed, and not a placeholder.
fn substantial(piece: &str) -> bool {
    let core = piece.trim_matches(|c: char| c.is_whitespace() || "\"'`,;:=&?()[]{}<>".contains(c));
    core.chars().count() >= 8 && !is_placeholder(core)
}

/// Redacted values (as long as propagation handles) that still occur in the output outside
/// the placeholders (`spans`, sorted). Nothing should, since propagation covered the input;
/// this is the safety net of the verify step.
fn leaked(text: &str, out: &str, findings: &[Finding], spans: &[(usize, usize)]) -> Vec<Candidate> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut values: Vec<(&str, &Finding)> = Vec::new();
    for f in findings.iter().filter(|f| !f.kept) {
        let v = &text[f.start..f.end];
        if v.chars().count() >= MIN_CHARS && seen.insert(v) {
            values.push((v, f));
        }
    }
    if values.is_empty() {
        return Vec::new();
    }
    let bounded: Vec<bool> = values
        .iter()
        .map(|(v, _)| v.chars().count() < BOUNDARY_BELOW)
        .collect();
    let occ = Occurrences::new(
        values.iter().map(|(v, _)| v.as_bytes()).collect(),
        bounded.clone(),
    );
    let glued = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut copies = Copies::default();
    // Between the placeholders, one stretch at a time: a copy never runs into one.
    let mut from = 0;
    for &(s, e) in spans.iter().chain(std::iter::once(&(out.len(), out.len()))) {
        if s > from {
            occ.find(&out.as_bytes()[from..s], |a, b, p| {
                let (a, b) = (from + a, from + b);
                if bounded[p]
                    && (glued(out[..a].chars().next_back()) || glued(out[b..].chars().next()))
                {
                    return;
                }
                copies.add(ValueCopy::new(a, b, p, values[p].1.confidence, None));
            });
        }
        from = from.max(e);
    }
    copies
        .finish()
        .into_iter()
        .map(|c| {
            let f = values[c.value].1;
            Candidate {
                start: c.start,
                end: c.end,
                kind: Cow::Owned(f.kind.clone()),
                label: Cow::Owned(f.label.clone()),
                confidence: f.confidence,
                detector: Cow::Owned(f.detector.clone()),
                reason: Cow::Borrowed("Same value as another finding"),
                source: Source::Propagated,
            }
        })
        .collect()
}

/// A test of whether `[s, e)` lies inside one of `ranges` (sorted by start, possibly
/// overlapping), in logarithmic time: the ranges that start at or before `s` contain it
/// exactly when the furthest end among them reaches `e`.
fn within_any(ranges: &[Range<usize>]) -> impl Fn(usize, usize) -> bool + '_ {
    let mut reach = Vec::with_capacity(ranges.len());
    let mut max_end = 0usize;
    for r in ranges {
        max_end = max_end.max(r.end);
        reach.push(max_end);
    }
    move |s, e| {
        let i = ranges.partition_point(|r| r.start <= s);
        i > 0 && reach[i - 1] >= e
    }
}

/// Clamps `[s, e)` to the text and widens it to char boundaries.
fn snap(text: &str, s: usize, e: usize) -> (usize, usize) {
    let mut s = s.min(text.len());
    let mut e = e.min(text.len());
    if e < s {
        std::mem::swap(&mut s, &mut e);
    }
    while !text.is_char_boundary(s) {
        s -= 1;
    }
    while !text.is_char_boundary(e) {
        e += 1;
    }
    (s, e)
}
