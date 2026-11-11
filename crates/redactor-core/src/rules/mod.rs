//! The pattern catalog and the structured detectors.
//!
//! # How the catalog stays fast
//!
//! Almost every rule has a literal *anchor* (a vendor prefix like `ghp_`, a header name, a
//! marker inside the token like `.atlasv1.`). All anchors of all rules go into two
//! Aho-Corasick automata (case-sensitive and ASCII case-insensitive), so the whole text is
//! read once. Only around an anchor hit does the rule's own regex run, on a small window
//! (`back` bytes before the anchor, `fwd` bytes after). Rules without any literal anchor
//! (Discord bot tokens, Bitcoin WIF keys, `.pgpass` lines, most PII) run their regex over the
//! whole text; there are few of them and the `regex` crate is linear time.
//!
//! The regex crate has no lookaround, so boundaries are checked in code ([`Bound`]): a match
//! glued to a word character on either side is rejected.

use std::borrow::Cow;
use std::sync::OnceLock;

use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use regex::Regex;

use crate::config::EngineConfig;
use crate::detector::Detector;
use crate::finding::{Candidate, Source};

mod ai;
mod cloud;
mod crypto;
pub(crate) mod custom;
mod dev;
mod hashes;
mod http;
mod pii;
mod saas;
pub(crate) mod validate;

/// Groups rules for docs and toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Ai,
    Dev,
    Saas,
    Cloud,
    Crypto,
    Hash,
    Http,
    Pii,
    /// The context engine (one entry: values named by their key, flag or sentence).
    Context,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Ai => "ai",
            Family::Dev => "dev",
            Family::Saas => "saas",
            Family::Cloud => "cloud",
            Family::Crypto => "crypto",
            Family::Hash => "hash",
            Family::Http => "http",
            Family::Pii => "pii",
            Family::Context => "context",
        }
    }

    /// Heading used in `docs/DETECTORS.md`.
    pub fn title(self) -> &'static str {
        match self {
            Family::Ai => "AI providers",
            Family::Dev => "Code hosting and developer tools",
            Family::Saas => "SaaS and messaging",
            Family::Cloud => "Cloud",
            Family::Crypto => "Keys and crypto material",
            Family::Hash => "Password hashes",
            Family::Http => "HTTP, URLs and connection strings",
            Family::Pii => "Personal data (off unless PII is on)",
            Family::Context => "Context (secrets known by the name next to them)",
        }
    }
}

/// Public description of one rule (for `scrub detectors` and the docs).
#[derive(Clone, Debug, PartialEq)]
pub struct RuleInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub family: Family,
    /// Value-free description of what is matched, e.g. `ghp_ + 36 base62`.
    pub example_shape: &'static str,
    /// Base confidence (validators may raise or lower it per match).
    pub confidence: f32,
    pub default_enabled: bool,
}

/// Every built-in rule, catalog order.
pub fn rule_infos() -> Vec<RuleInfo> {
    let mut out: Vec<RuleInfo> = compiled().rules.iter().map(|r| r.rule.info()).collect();
    out.extend(crypto::structured_infos());
    out.push(RuleInfo {
        id: "context",
        label: "Password / Token / Secret",
        family: Family::Context,
        example_shape: "values of secret-named keys, options and sentences: DB_PASSWORD=…, \
                        --token …, curl -u user:…, \"the wifi password is …\"",
        confidence: 0.75,
        default_enabled: true,
    });
    out.push(RuleInfo {
        id: "classifier",
        label: "Random String",
        family: Family::Context,
        example_shape: "unlabeled tokens that look randomly generated (20+ characters, \
                        3+ character classes; Paranoid: 16+, and long hex runs)",
        confidence: 0.55,
        default_enabled: true,
    });
    out
}

/// The built-in detectors for a config (custom rules are built separately).
pub fn catalog(cfg: &EngineConfig) -> Vec<Box<dyn Detector>> {
    let active = |id: &str, default_enabled: bool| -> bool {
        !cfg.disabled.iter().any(|d| d == id)
            && (default_enabled || cfg.enabled.iter().any(|e| e == id))
    };
    let cat = compiled();
    let mut secrets = vec![false; cat.rules.len()];
    let mut pii = vec![false; cat.rules.len()];
    for (i, r) in cat.rules.iter().enumerate() {
        if active(r.rule.id, r.rule.default_enabled) {
            if r.rule.family == Family::Pii {
                pii[i] = true;
            } else {
                secrets[i] = true;
            }
        }
    }
    let mut out: Vec<Box<dyn Detector>> = Vec::new();
    out.push(Box::new(CatalogDetector {
        catalog: cat,
        mask: secrets,
        pii: false,
    }));
    out.push(Box::new(CatalogDetector {
        catalog: cat,
        mask: pii,
        pii: true,
    }));
    out.extend(crypto::structured(&active));
    out
}

// ---------------------------------------------------------------------------------------
// Rule definitions
// ---------------------------------------------------------------------------------------

/// Which neighbour characters reject a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    /// No check.
    None,
    /// Not `[A-Za-z0-9_]`.
    Word,
    /// Not `[A-Za-z0-9_-]` (header names: `Cookie` must not be the tail of `Set-Cookie`).
    WordDash,
    /// Not `[A-Za-z0-9_+/-]` (long alphanumeric tokens that could be a slice of base64).
    Base64,
    /// Right side only: end of line or end of text.
    LineEnd,
    /// Right side only: the closing `"` of a JSON string (or the end of the text). A boundary
    /// rather than part of the pattern, so that a value longer than its window still reaches
    /// the window end and widens it.
    Quote,
}

impl Bound {
    fn rejects(self, c: Option<char>) -> bool {
        let Some(c) = c else { return false };
        match self {
            Bound::None => false,
            Bound::Word => c.is_ascii_alphanumeric() || c == '_',
            Bound::WordDash => c.is_ascii_alphanumeric() || c == '_' || c == '-',
            Bound::Base64 => c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '/' | '-'),
            Bound::LineEnd => !matches!(c, '\r' | '\n'),
            Bound::Quote => c != '"',
        }
    }
}

/// What a validator says about one match.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Verdict {
    Reject,
    Keep,
    /// Keep with this confidence and reason.
    Conf(f32, &'static str),
}

/// One match handed to a validator.
pub(crate) struct Hit<'t> {
    pub text: &'t str,
    /// Whole regex match.
    pub start: usize,
    pub end: usize,
    /// The secret part (group `s`, or the whole match).
    pub s_start: usize,
    pub s_end: usize,
}

impl<'t> Hit<'t> {
    pub fn secret(&self) -> &'t str {
        &self.text[self.s_start..self.s_end]
    }

    /// Up to `n` bytes of text before the match (snapped to a char boundary).
    pub fn before(&self, n: usize) -> &'t str {
        let s = floor_boundary(self.text, self.start.saturating_sub(n));
        &self.text[s..self.start]
    }

    /// The line the match starts on, clipped to 1 KiB on either side of the match so that
    /// validators stay cheap on one-line megabyte inputs.
    pub fn line(&self) -> &'t str {
        const CLIP: usize = 1024;
        let lo = floor_boundary(self.text, self.start.saturating_sub(CLIP));
        let hi = ceil_boundary(self.text, (self.end + CLIP).min(self.text.len()));
        let ls = self.text[lo..self.start]
            .rfind('\n')
            .map_or(lo, |i| lo + i + 1);
        let le = self.text[self.start..hi]
            .find('\n')
            .map_or(hi, |i| self.start + i);
        &self.text[ls..le]
    }
}

pub(crate) type CheckFn = fn(&Hit) -> Verdict;

/// Reads the rest of a match in code, past what a regex can bound: given the text and the
/// regex match `start..end`, returns `(secret start, secret end, match end)`, or `None` when
/// the text does not continue as the rule needs. The scan must stop at a delimiter that
/// every anchor of the rule contains, so that the work over all matches stays linear.
pub(crate) type TailFn = fn(&str, usize, usize) -> Option<(usize, usize, usize)>;

/// A regex rule. Built with [`Rule::new`] and the chained setters.
#[derive(Clone)]
pub(crate) struct Rule {
    pub id: &'static str,
    pub label: &'static str,
    pub family: Family,
    pub shape: &'static str,
    /// Literal anchors; empty means "scan the whole text with the regex".
    pub anchors: &'static [&'static str],
    /// Anchors match ASCII case-insensitively.
    pub ci: bool,
    /// How far before the anchor start the match may begin.
    pub back: usize,
    /// How far after the anchor start the match may end.
    pub fwd: usize,
    /// The regex; a named group `s` marks the secret inside the match.
    pub re: &'static str,
    pub conf: f32,
    pub left: Bound,
    pub right: Bound,
    pub check: Option<CheckFn>,
    pub tail: Option<TailFn>,
    pub default_enabled: bool,
}

impl Rule {
    pub fn new(
        family: Family,
        id: &'static str,
        label: &'static str,
        shape: &'static str,
        anchors: &'static [&'static str],
        re: &'static str,
        conf: f32,
    ) -> Rule {
        Rule {
            id,
            label,
            family,
            shape,
            anchors,
            ci: false,
            back: 0,
            fwd: 512,
            re,
            conf,
            left: Bound::Word,
            right: Bound::Word,
            check: None,
            tail: None,
            default_enabled: true,
        }
    }

    pub fn ci(mut self) -> Rule {
        self.ci = true;
        self
    }

    pub fn back(mut self, n: usize) -> Rule {
        self.back = n;
        self
    }

    pub fn fwd(mut self, n: usize) -> Rule {
        self.fwd = n;
        self
    }

    pub fn bounds(mut self, left: Bound, right: Bound) -> Rule {
        self.left = left;
        self.right = right;
        self
    }

    pub fn check(mut self, f: CheckFn) -> Rule {
        self.check = Some(f);
        self
    }

    pub fn tail(mut self, f: TailFn) -> Rule {
        self.tail = Some(f);
        self
    }

    fn info(&self) -> RuleInfo {
        RuleInfo {
            id: self.id,
            label: self.label,
            family: self.family,
            example_shape: self.shape,
            confidence: self.conf,
            default_enabled: self.default_enabled,
        }
    }
}

fn all_rules() -> Vec<Rule> {
    let mut v = Vec::new();
    v.extend(ai::rules());
    v.extend(dev::rules());
    v.extend(saas::rules());
    v.extend(cloud::rules());
    v.extend(crypto::rules());
    v.extend(hashes::rules());
    v.extend(http::rules());
    v.extend(pii::rules());
    v
}

// ---------------------------------------------------------------------------------------
// Compiled catalog
// ---------------------------------------------------------------------------------------

struct CompiledRule {
    rule: Rule,
    re: Regex,
    group: Option<usize>,
}

struct Compiled {
    rules: Vec<CompiledRule>,
    cs: Option<AhoCorasick>,
    /// Pattern id -> (rule index, anchor length).
    cs_map: Vec<(usize, usize)>,
    ci: Option<AhoCorasick>,
    ci_map: Vec<(usize, usize)>,
    /// Rules without anchors.
    full: Vec<usize>,
}

/// Compiled once per process: the catalog is static data.
fn compiled() -> &'static Compiled {
    static CATALOG: OnceLock<Compiled> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let rules: Vec<CompiledRule> = all_rules()
            .into_iter()
            .map(|rule| {
                // Built-in patterns are tested; a failure here is a programming error that
                // every test run would catch.
                let re = Regex::new(rule.re)
                    .unwrap_or_else(|e| panic!("rule {}: invalid regex: {e}", rule.id));
                let group = re.capture_names().position(|n| n == Some("s"));
                CompiledRule { rule, re, group }
            })
            .collect();
        let mut cs_pats = Vec::new();
        let mut cs_map = Vec::new();
        let mut ci_pats = Vec::new();
        let mut ci_map = Vec::new();
        let mut full = Vec::new();
        for (i, r) in rules.iter().enumerate() {
            if r.rule.anchors.is_empty() {
                full.push(i);
            }
            for a in r.rule.anchors {
                if r.rule.ci {
                    ci_pats.push(*a);
                    ci_map.push((i, a.len()));
                } else {
                    cs_pats.push(*a);
                    cs_map.push((i, a.len()));
                }
            }
        }
        let build = |pats: &[&str], ci: bool| {
            if pats.is_empty() {
                return None;
            }
            Some(
                AhoCorasick::builder()
                    .match_kind(MatchKind::Standard)
                    .kind(Some(AhoCorasickKind::DFA))
                    .ascii_case_insensitive(ci)
                    .build(pats)
                    .expect("anchor automaton builds"),
            )
        };
        Compiled {
            cs: build(&cs_pats, false),
            ci: build(&ci_pats, true),
            rules,
            cs_map,
            ci_map,
            full,
        }
    })
}

/// Runs the enabled subset of the catalog.
struct CatalogDetector {
    catalog: &'static Compiled,
    mask: Vec<bool>,
    pii: bool,
}

impl Detector for CatalogDetector {
    fn id(&self) -> &'static str {
        if self.pii {
            "catalog_pii"
        } else {
            "catalog"
        }
    }

    fn is_pii(&self) -> bool {
        self.pii
    }

    fn rule_count(&self) -> usize {
        self.mask.iter().filter(|m| **m).count()
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        let cat = self.catalog;
        let first = out.len();
        // Anchor hits per rule, as (anchor start, anchor length).
        let mut hits: Vec<Vec<(usize, usize)>> = vec![Vec::new(); cat.rules.len()];
        for (ac, map) in [(&cat.cs, &cat.cs_map), (&cat.ci, &cat.ci_map)] {
            let Some(ac) = ac else { continue };
            for m in ac.find_overlapping_iter(text) {
                let (ri, alen) = map[m.pattern().as_usize()];
                if !self.mask[ri] {
                    continue;
                }
                let rule = &cat.rules[ri].rule;
                // Cheap pre-check: a match that must start at the anchor cannot be glued to
                // the character before it.
                if rule.back == 0
                    && rule.left.rejects(text[..m.start()].chars().next_back())
                    && !escape_ends_at(text, m.start())
                {
                    continue;
                }
                hits[ri].push((m.start(), alen));
            }
        }
        for (ri, h) in hits.iter_mut().enumerate() {
            if !h.is_empty() {
                h.sort_unstable();
                h.dedup_by_key(|x| x.0);
                run_anchored(&cat.rules[ri], text, h, out);
            }
        }
        for &ri in &cat.full {
            if self.mask[ri] {
                run_full(&cat.rules[ri], text, out);
            }
        }
        // Several anchors of one rule can find the same match.
        let mut mine: Vec<Candidate> = out.drain(first..).collect();
        mine.sort_by(|a, b| (a.start, a.end, &a.detector).cmp(&(b.start, b.end, &b.detector)));
        mine.dedup_by(|a, b| a.start == b.start && a.end == b.end && a.detector == b.detector);
        out.extend(mine);
    }
}

/// Runs one rule around its anchor hits (sorted by position).
///
/// Each hit gets its own window `[a - back, a + fwd)`; a match counts only if it contains
/// that anchor. Anchors inside a match already accepted are skipped. When an anchor repeats
/// densely (`eyJeyJeyJ…` with a 16 KiB window) the windows would overlap and the work would
/// grow quadratically, so each rule has a byte budget; past it, the rule finishes with one
/// linear regex pass over the rest of the text.
///
/// The window bounds the regex, not the value: a match that runs into the end of its window
/// may continue past it, so the window doubles until the match ends inside it (or the text
/// ends). The doubled windows count against the same budget.
fn run_anchored(r: &CompiledRule, text: &str, hits: &[(usize, usize)], out: &mut Vec<Candidate>) {
    let rule = &r.rule;
    let budget = 4 * text.len() + (1 << 20);
    let mut spent = 0usize;
    let mut covered_until = 0usize;
    for &(a, alen) in hits {
        if a < covered_until {
            continue;
        }
        let ws = floor_boundary(text, a.saturating_sub(rule.back));
        let mut reach = rule.fwd;
        loop {
            let we = ceil_boundary(text, a.saturating_add(reach).min(text.len()));
            spent += we - ws;
            if spent > budget {
                run_linear(r, text, ws.max(covered_until), out);
                return;
            }
            match in_window(r, text, ws, we, a, alen) {
                Window::Cut => reach = reach.saturating_mul(2),
                Window::Found(c, me) => {
                    out.push(c);
                    covered_until = me;
                    break;
                }
                Window::Nothing => break,
            }
        }
    }
}

enum Window {
    /// An accepted candidate and the end of its match.
    Found(Candidate, usize),
    /// The match holding the anchor reaches the end of the window and the text goes on.
    Cut,
    Nothing,
}

/// The first accepted match in `text[ws..we]` that holds the anchor at `a`.
///
/// A match cut by the window end widens the window only if it would be accepted as it is
/// (the right boundary aside): a value the validator rejects in its first `fwd` bytes is not
/// read any further, which keeps runs like `password=password=…` cheap.
fn in_window(r: &CompiledRule, text: &str, ws: usize, we: usize, a: usize, alen: usize) -> Window {
    let window = &text[ws..we];
    for m in r.re.find_iter(window) {
        let (ms, me) = (ws + m.start(), ws + m.end());
        if ms > a {
            break;
        }
        if me < a + alen {
            continue;
        }
        // Group positions only for the one match that holds the anchor.
        let (ss, se) = match r.group {
            Some(g) => match r.re.captures_at(window, m.start()).and_then(|c| c.get(g)) {
                Some(g) => (ws + g.start(), ws + g.end()),
                None => (ms, me),
            },
            None => (ms, me),
        };
        let cut = me == we && we < text.len();
        return match finish(&r.rule, text, (ms, me), (ss, se), !cut) {
            Some(_) if cut => Window::Cut,
            Some((c, me)) => Window::Found(c, me),
            None => Window::Nothing,
        };
    }
    Window::Nothing
}

/// The rule's tail (if any), then [`accept`]. Returns the candidate and the end of the match.
fn finish(
    rule: &Rule,
    text: &str,
    (ms, me): (usize, usize),
    (ss, se): (usize, usize),
    right: bool,
) -> Option<(Candidate, usize)> {
    let (ss, se, me) = match rule.tail {
        Some(tail) => tail(text, ms, me)?,
        None => (ss, se, me),
    };
    accept(rule, text, (ms, me), (ss, se), right).map(|c| (c, me))
}

/// One left-to-right pass from `from` (the fallback of [`run_anchored`]).
fn run_linear(r: &CompiledRule, text: &str, from: usize, out: &mut Vec<Candidate>) {
    let mut pos = from;
    while pos <= text.len() {
        let Some(caps) = r.re.captures_at(text, pos) else {
            break;
        };
        let m = caps.get(0).expect("group 0");
        let (ss, se) = match r.group.and_then(|g| caps.get(g)) {
            Some(g) => (g.start(), g.end()),
            None => (m.start(), m.end()),
        };
        let mut next = m.end();
        if let Some((c, me)) = finish(&r.rule, text, (m.start(), m.end()), (ss, se), true) {
            out.push(c);
            next = next.max(me);
        }
        pos = if next > m.start() {
            next
        } else {
            ceil_boundary(text, m.start() + 1)
        };
        if m.start() >= text.len() {
            break;
        }
    }
}

fn run_full(r: &CompiledRule, text: &str, out: &mut Vec<Candidate>) {
    match r.rule.id {
        // Each rule without an anchor reads the whole text; a cheap test of what any match
        // needs first keeps the common case (no candidate at all) fast.
        "bitcoin_wif" if !has_run(text.as_bytes(), 51, |b| b.is_ascii_alphanumeric()) => {}
        "pgpass_line" => {
            for (s, e) in lines_with(text, b':', 4) {
                run_linear(r, &text[..e], s, out);
            }
        }
        _ => run_linear(r, text, 0, out),
    }
}

/// A run of at least `n` bytes for which `class` holds. Probes every `n / 2`-th byte, so text
/// of short words costs a fraction of a pass.
fn has_run(bytes: &[u8], n: usize, class: impl Fn(u8) -> bool) -> bool {
    let step = (n / 2).max(1);
    let mut i = step.min(bytes.len());
    let mut done = 0usize;
    while i < bytes.len() {
        if class(bytes[i]) && i >= done {
            let mut s = i;
            while s > done && class(bytes[s - 1]) {
                s -= 1;
            }
            let mut e = i + 1;
            while e < bytes.len() && class(bytes[e]) {
                e += 1;
            }
            if e - s >= n {
                return true;
            }
            done = e;
        }
        i += step;
    }
    false
}

/// `(start, end)` of every line (without its `\n`) holding at least `min` copies of `byte`.
fn lines_with(text: &str, byte: u8, min: usize) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = bytes[from..].iter().position(|&b| b == byte) {
        let at = from + i;
        let ls = bytes[from..at]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(from, |p| from + p + 1);
        let le = bytes[at..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |p| at + p);
        if bytes[ls..le].iter().filter(|&&b| b == byte).count() >= min {
            out.push((ls, le));
        }
        from = le;
        if from >= bytes.len() {
            break;
        }
    }
    out
}

/// Boundary checks, filler rejection and the validator. `right: false` skips the right
/// boundary (for a match cut short by its window).
fn accept(
    rule: &Rule,
    text: &str,
    (ms, me): (usize, usize),
    (ss, se): (usize, usize),
    right: bool,
) -> Option<Candidate> {
    if rule.left.rejects(text[..ms].chars().next_back()) && !escape_ends_at(text, ms)
        || right && rule.right.rejects(text[me..].chars().next())
    {
        return None;
    }
    // Trim whitespace a permissive value class may have picked up.
    let secret = &text[ss..se];
    let trimmed = secret.trim_end();
    let se = ss + trimmed.len();
    let ss = se - trimmed.trim_start().len();
    if se <= ss {
        return None;
    }
    if rule.family != Family::Pii && is_filler(&text[ss..se]) {
        return None;
    }
    let mut conf = rule.conf;
    let mut reason = rule.shape;
    if let Some(check) = rule.check {
        let hit = Hit {
            text,
            start: ms,
            end: me,
            s_start: ss,
            s_end: se,
        };
        match check(&hit) {
            Verdict::Reject => return None,
            Verdict::Keep => {}
            Verdict::Conf(c, why) => {
                conf = c;
                reason = why;
            }
        }
    }
    Some(Candidate {
        start: ss,
        end: se,
        kind: Cow::Borrowed(rule.id),
        label: Cow::Borrowed(rule.label),
        confidence: conf,
        detector: Cow::Borrowed(rule.id),
        reason: Cow::Borrowed(reason),
        source: if rule.family == Family::Pii {
            Source::Pii
        } else {
            Source::Pattern
        },
    })
}

/// An escape ends right at `pos`: `\n`, `\r` or `\t` in a JSON or C string, or an ANSI
/// colour code (`ESC[1;33m`, also written out as `\x1b[`, `\033[`, `\e[`, `\u001b[`). Its
/// last letter is no word character glued to what follows.
pub(crate) fn escape_ends_at(text: &str, pos: usize) -> bool {
    let pre = &text.as_bytes()[..pos];
    if let [head @ .., b'\\', b'n' | b'r' | b't'] = pre {
        let more = head.iter().rev().take_while(|&&c| c == b'\\').count();
        return more % 2 == 0;
    }
    let Some(rest) = pre.strip_suffix(b"m") else {
        return false;
    };
    let params = rest
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_digit() || **c == b';')
        .count();
    let Some(intro) = rest[..rest.len() - params].strip_suffix(b"[") else {
        return false;
    };
    [
        &b"\x1b"[..],
        b"\\x1b",
        b"\\x1B",
        b"\\033",
        b"\\e",
        b"\\E",
        b"\\u001b",
        b"\\u001B",
    ]
    .iter()
    .any(|e| intro.ends_with(e))
}

/// Documentation filler such as `ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx` or `AKIA****…`:
/// one character makes up more than 60 % of a value of 16+ characters.
pub(crate) fn is_filler(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 16 {
        return false;
    }
    let mut counts = [0usize; 256];
    for &c in b {
        counts[c as usize] += 1;
    }
    let max = counts.iter().copied().max().unwrap_or(0);
    max * 10 > b.len() * 6
}

pub(crate) fn floor_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

pub(crate) fn ceil_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------------------
// Validators shared by several families
// ---------------------------------------------------------------------------------------

pub(crate) mod check {
    use super::{Hit, Verdict};

    /// At least two of lowercase / uppercase / digit: rejects dictionary-like identifiers
    /// such as `sk-learn-model-selection-tools`.
    pub fn random(h: &Hit) -> Verdict {
        if super::validate::char_classes(h.secret()) >= 2 {
            Verdict::Keep
        } else {
            Verdict::Reject
        }
    }

    /// The match must follow a query separator (`?`, `&`, `#`, `;`).
    pub fn after_query_sep(h: &Hit) -> Verdict {
        match h.text[..h.start].chars().next_back() {
            Some('?' | '&' | '#' | ';') => Verdict::Keep,
            _ => Verdict::Reject,
        }
    }
}
