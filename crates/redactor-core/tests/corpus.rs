//! Corpus and accuracy gates.
//!
//! `corpus/positive/*.txt` hold annotated text, `corpus/negative/*.txt` plain text that must
//! come out untouched. The repository's own `README.md` is read live as one more negative file,
//! so the gate always covers the current README. Annotations in positive files:
//!
//! - `⟦Label|value⟧`: must be redacted in Balanced, and a finding on it carries `Label`.
//! - `⟦*|value⟧`: must be redacted in Balanced, any label.
//! - `⟦?Label|value⟧`, `⟦?*|value⟧`: must be redacted in Paranoid only.
//! - `⟦!|value⟧`: bait inside a positive file; must not be touched in Balanced.
//! - `⟦gen:NAME⟧`, `⟦gen:NAME#ID⟧`, `⟦gen:NAME|Label⟧`, `⟦?gen:NAME⟧`: a format-valid fake
//!   token from `support/gen.rs` (the same `NAME#ID` in one file is the same token).
//! - `⟦opt:pii⟧` (PII detection on) and `⟦opt:crlf⟧` (CRLF line ends), on a line of their own.
//!
//! Gates: Balanced covers every required span (with its label), touches no bait and finds
//! nothing in the negatives; Paranoid covers every span; rescanning any output finds nothing.
//!
//! The run prints recall and precision per file. With `REDACTOR_WRITE_ACCURACY=1` it also
//! writes `docs/ACCURACY.md` from the same numbers.

mod support;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use redactor_core::{Engine, EngineConfig, Finding, ScanOutcome, ScanRequest, Sensitivity};
use support::gen::{corpus_token, Gen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    /// Covered in Balanced (and so in Paranoid).
    Balanced,
    /// Covered in Paranoid.
    Paranoid,
    /// Not touched in Balanced.
    Untouched,
}

#[derive(Clone, Debug)]
struct Span {
    start: usize,
    end: usize,
    label: Option<String>,
    want: Want,
    line: usize,
}

#[derive(Debug)]
struct Doc {
    name: String,
    text: String,
    spans: Vec<Span>,
    pii: bool,
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// The repository's README, read live: a negative file shown as `README.md` in the tables.
fn readme() -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    ("README.md".to_string(), text)
}

fn files(kind: &str) -> Vec<(String, String)> {
    let dir = corpus_dir().join(kind);
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .map(|p| {
            let name = format!("{kind}/{}", p.file_name().unwrap().to_string_lossy());
            let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, text)
        })
        .collect();
    out.sort();
    out
}

const OPEN: char = '⟦';
const CLOSE: char = '⟧';

/// Expands markers into the scan text and the spans to check.
fn parse(name: &str, raw: &str) -> Doc {
    let mut pii = false;
    let mut crlf = false;
    // Option lines are removed whole.
    let mut body = String::with_capacity(raw.len());
    let mut skipped = 0usize;
    for line in raw.split_inclusive('\n') {
        let t = line.trim();
        if !t.is_empty() && t.starts_with("⟦opt:") && t.ends_with(CLOSE) {
            for opt in t.split(CLOSE).filter(|o| !o.is_empty()) {
                match opt.trim().trim_start_matches(OPEN) {
                    "opt:pii" => pii = true,
                    "opt:crlf" => crlf = true,
                    other => panic!("{name}: unknown option {other:?}"),
                }
            }
            skipped += 1;
            continue;
        }
        body.push_str(line);
    }

    let mut text = String::with_capacity(body.len() + 256);
    let mut spans = Vec::new();
    let mut tokens: HashMap<String, (String, Option<&'static str>)> = HashMap::new();
    let mut gen_index = 0usize;
    // Option lines sit at the top: line numbers count them.
    let mut line = 1 + skipped;
    let push = |text: &mut String, s: &str, line: &mut usize| {
        for c in s.chars() {
            if c == '\n' {
                *line += 1;
                if crlf && !text.ends_with('\r') {
                    text.push('\r');
                }
            }
            text.push(c);
        }
    };
    let mut rest = body.as_str();
    while let Some(i) = rest.find(OPEN) {
        push(&mut text, &rest[..i], &mut line);
        let after = &rest[i + OPEN.len_utf8()..];
        let j = after
            .find(CLOSE)
            .unwrap_or_else(|| panic!("{name}:{line}: unclosed marker"));
        let inner = &after[..j];
        rest = &after[j + CLOSE.len_utf8()..];
        assert!(
            !inner.contains(OPEN),
            "{name}:{line}: nested markers are not supported"
        );

        let (paranoid, inner) = match inner.strip_prefix('?') {
            Some(r) => (true, r),
            None => (false, inner),
        };
        let start_line = line;
        if let Some(gen_name) = inner.strip_prefix("gen:") {
            let (gen_name, label_override) = match gen_name.split_once('|') {
                Some((s, l)) => (s, Some(l)),
                None => (gen_name, None),
            };
            let key = if gen_name.contains('#') {
                gen_name.to_string()
            } else {
                gen_index += 1;
                format!("{gen_name}#auto{gen_index}")
            };
            let name_only = gen_name.split('#').next().unwrap();
            let (token, label) = tokens
                .entry(key.clone())
                .or_insert_with(|| {
                    let mut g = Gen::named(&format!("{name}:{key}"));
                    corpus_token(&mut g, name_only)
                        .unwrap_or_else(|| panic!("{name}:{line}: unknown generator {name_only:?}"))
                })
                .clone();
            let label = match label_override {
                Some("*") => None,
                Some(l) => Some(l.to_string()),
                None => label.map(str::to_string),
            };
            let start = text.len();
            push(&mut text, &token, &mut line);
            spans.push(Span {
                start,
                end: text.len(),
                label,
                want: if paranoid {
                    Want::Paranoid
                } else {
                    Want::Balanced
                },
                line: start_line,
            });
            continue;
        }
        let (label, value) = inner
            .split_once('|')
            .unwrap_or_else(|| panic!("{name}:{line}: marker without '|': {inner:?}"));
        assert!(!value.is_empty(), "{name}:{line}: empty value");
        let (label, want) = match label {
            "!" => {
                assert!(!paranoid, "{name}:{line}: '?!' makes no sense");
                (None, Want::Untouched)
            }
            "*" => (
                None,
                if paranoid {
                    Want::Paranoid
                } else {
                    Want::Balanced
                },
            ),
            l => (
                Some(l.to_string()),
                if paranoid {
                    Want::Paranoid
                } else {
                    Want::Balanced
                },
            ),
        };
        let start = text.len();
        push(&mut text, value, &mut line);
        spans.push(Span {
            start,
            end: text.len(),
            label,
            want,
            line: start_line,
        });
    }
    push(&mut text, rest, &mut line);
    assert!(
        !text.contains(CLOSE),
        "{name}: stray closing marker in the text"
    );
    Doc {
        name: name.to_string(),
        text,
        spans,
        pii,
    }
}

fn scan(engine: &Engine, text: &str, s: Sensitivity, pii: bool) -> ScanOutcome {
    engine.scan(
        text,
        &ScanRequest {
            sensitivity: Some(s),
            pii: Some(pii),
            ..ScanRequest::default()
        },
    )
}

fn redacted(findings: &[Finding]) -> impl Iterator<Item = &Finding> {
    findings.iter().filter(|f| !f.kept)
}

/// Every byte of `[s, e)` lies inside some finding.
fn covered(findings: &[Finding], s: usize, e: usize) -> bool {
    let mut ranges: Vec<(usize, usize)> = redacted(findings)
        .filter(|f| f.start < e && f.end > s)
        .map(|f| (f.start, f.end))
        .collect();
    ranges.sort_unstable();
    let mut pos = s;
    for (a, b) in ranges {
        if a > pos {
            return false;
        }
        pos = pos.max(b);
        if pos >= e {
            return true;
        }
    }
    pos >= e
}

fn touches(findings: &[Finding], s: usize, e: usize) -> bool {
    redacted(findings).any(|f| f.start < e && f.end > s)
}

fn label_ok(findings: &[Finding], span: &Span) -> bool {
    match &span.label {
        None => true,
        Some(l) => {
            redacted(findings).any(|f| f.start < span.end && f.end > span.start && &f.label == l)
        }
    }
}

/// Rescanning the output changes nothing and finds nothing.
fn rescan_clean(engine: &Engine, out: &ScanOutcome, s: Sensitivity, pii: bool) -> Option<String> {
    let again = scan(engine, &out.output, s, pii);
    if again.output == out.output && again.findings.is_empty() && out.verified {
        return None;
    }
    let what: Vec<String> = again
        .findings
        .iter()
        .map(|f| format!("{}..{} {} ({})", f.start, f.end, f.label, f.detector))
        .collect();
    Some(format!(
        "rescan at {s:?} is not clean (verified={}): {}",
        out.verified,
        what.join(", ")
    ))
}

#[derive(Default)]
struct Row {
    name: String,
    /// Spans required in Balanced / covered (and labelled right) in Balanced.
    b_total: usize,
    b_hit: usize,
    /// All spans / covered in Paranoid.
    p_total: usize,
    p_hit: usize,
    /// Balanced findings, and those that overlap an annotated span.
    findings: usize,
    on_target: usize,
    /// Paranoid findings (negatives only).
    p_findings: usize,
}

impl Row {
    fn precision(&self) -> Option<f64> {
        (self.findings > 0).then(|| self.on_target as f64 / self.findings as f64)
    }
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.1}%", 100.0 * n as f64 / d as f64)
    }
}

fn line_of(text: &str, pos: usize) -> usize {
    text[..pos].matches('\n').count() + 1
}

#[test]
fn corpus_gates() {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let mut failures: Vec<String> = Vec::new();
    let mut pos_rows: Vec<Row> = Vec::new();
    let mut neg_rows: Vec<Row> = Vec::new();

    let positives = files("positive");
    let mut negatives = files("negative");
    negatives.push(readme());
    assert!(positives.len() >= 40, "need at least 40 positive files");
    assert!(negatives.len() >= 20, "need at least 20 negative files");

    for (name, raw) in &positives {
        let doc = parse(name, raw);
        assert!(
            doc.spans.iter().any(|s| s.want != Want::Untouched),
            "{name}: a positive file needs annotations"
        );
        let b = scan(&engine, &doc.text, Sensitivity::Balanced, doc.pii);
        let p = scan(&engine, &doc.text, Sensitivity::Paranoid, doc.pii);
        let mut row = Row {
            name: doc.name.clone(),
            ..Row::default()
        };
        for span in &doc.spans {
            let at = format!("{}:{}", doc.name, span.line);
            match span.want {
                Want::Balanced => {
                    row.b_total += 1;
                    let cov = covered(&b.findings, span.start, span.end);
                    let lab = label_ok(&b.findings, span);
                    if cov && lab {
                        row.b_hit += 1;
                    } else if !cov {
                        failures.push(format!(
                            "{at}: Balanced misses {:?} ({})",
                            &doc.text[span.start..span.end],
                            span.label.as_deref().unwrap_or("*")
                        ));
                    } else {
                        let got: Vec<&str> = redacted(&b.findings)
                            .filter(|f| f.start < span.end && f.end > span.start)
                            .map(|f| f.label.as_str())
                            .collect();
                        failures.push(format!(
                            "{at}: Balanced label {got:?}, expected {:?}",
                            span.label.as_deref().unwrap_or("*")
                        ));
                    }
                }
                Want::Paranoid => {}
                Want::Untouched => {
                    if touches(&b.findings, span.start, span.end) {
                        failures.push(format!(
                            "{at}: Balanced touches bait {:?}",
                            &doc.text[span.start..span.end]
                        ));
                    }
                    continue;
                }
            }
            row.p_total += 1;
            if covered(&p.findings, span.start, span.end) {
                row.p_hit += 1;
            } else {
                failures.push(format!(
                    "{at}: Paranoid misses {:?}",
                    &doc.text[span.start..span.end]
                ));
            }
        }
        for f in redacted(&b.findings) {
            row.findings += 1;
            let hit = doc
                .spans
                .iter()
                .any(|s| s.want != Want::Untouched && f.start < s.end && f.end > s.start);
            if hit {
                row.on_target += 1;
            } else {
                println!(
                    "  extra: {}:{} {:?} as {} ({})",
                    doc.name,
                    line_of(&doc.text, f.start),
                    &doc.text[f.start..f.end],
                    f.label,
                    f.detector
                );
            }
        }
        for (out, s) in [(&b, Sensitivity::Balanced), (&p, Sensitivity::Paranoid)] {
            if let Some(msg) = rescan_clean(&engine, out, s, doc.pii) {
                failures.push(format!("{}: {msg}", doc.name));
            }
            // Every label the engine emits is one it recognises in a placeholder later.
            for f in &out.findings {
                if !f.placeholder.is_empty() && !engine.is_placeholder(&f.placeholder) {
                    failures.push(format!(
                        "{}: placeholder {:?} is not recognised as one",
                        doc.name, f.placeholder
                    ));
                }
            }
        }
        pos_rows.push(row);
    }

    for (name, raw) in &negatives {
        assert!(
            !raw.contains(OPEN) && !raw.contains(CLOSE),
            "{name}: negative files have no markers"
        );
        let b = scan(&engine, raw, Sensitivity::Balanced, false);
        let p = scan(&engine, raw, Sensitivity::Paranoid, false);
        for f in redacted(&b.findings) {
            failures.push(format!(
                "{name}:{}: Balanced finding {:?} as {} ({}, {:.2})",
                line_of(raw, f.start),
                &raw[f.start..f.end],
                f.label,
                f.detector,
                f.confidence
            ));
        }
        for f in redacted(&p.findings) {
            println!(
                "  paranoid: {name}:{} {:?} as {} ({})",
                line_of(raw, f.start),
                &raw[f.start..f.end],
                f.label,
                f.detector
            );
        }
        for (out, s) in [(&b, Sensitivity::Balanced), (&p, Sensitivity::Paranoid)] {
            if let Some(msg) = rescan_clean(&engine, out, s, false) {
                failures.push(format!("{name}: {msg}"));
            }
        }
        neg_rows.push(Row {
            name: name.clone(),
            findings: redacted(&b.findings).count(),
            p_findings: redacted(&p.findings).count(),
            ..Row::default()
        });
    }

    let table = render_table(&pos_rows, &neg_rows);
    println!("{table}");
    if std::env::var_os("REDACTOR_WRITE_ACCURACY").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/ACCURACY.md");
        std::fs::write(&path, accuracy_md(&pos_rows, &neg_rows)).unwrap();
        println!("wrote {}", path.display());
    }
    assert!(
        failures.is_empty(),
        "{} gate failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

struct Totals {
    b_total: usize,
    b_hit: usize,
    p_total: usize,
    p_hit: usize,
    findings: usize,
    on_target: usize,
    neg_b: usize,
    neg_p: usize,
}

fn totals(pos: &[Row], neg: &[Row]) -> Totals {
    Totals {
        b_total: pos.iter().map(|r| r.b_total).sum(),
        b_hit: pos.iter().map(|r| r.b_hit).sum(),
        p_total: pos.iter().map(|r| r.p_total).sum(),
        p_hit: pos.iter().map(|r| r.p_hit).sum(),
        findings: pos.iter().map(|r| r.findings).sum(),
        on_target: pos.iter().map(|r| r.on_target).sum(),
        neg_b: neg.iter().map(|r| r.findings).sum(),
        neg_p: neg.iter().map(|r| r.p_findings).sum(),
    }
}

fn render_table(pos: &[Row], neg: &[Row]) -> String {
    let mut s = String::new();
    let w = pos
        .iter()
        .chain(neg)
        .map(|r| r.name.len())
        .max()
        .unwrap_or(10)
        .max(10);
    let _ = writeln!(
        s,
        "{:w$}  {:>11}  {:>8}  {:>11}  {:>8}  {:>8}  {:>9}",
        "file", "balanced", "recall", "paranoid", "recall", "findings", "precision"
    );
    for r in pos {
        let _ = writeln!(
            s,
            "{:w$}  {:>11}  {:>8}  {:>11}  {:>8}  {:>8}  {:>9}",
            r.name,
            format!("{}/{}", r.b_hit, r.b_total),
            pct(r.b_hit, r.b_total),
            format!("{}/{}", r.p_hit, r.p_total),
            pct(r.p_hit, r.p_total),
            r.findings,
            r.precision()
                .map_or("-".into(), |p| format!("{:.1}%", 100.0 * p))
        );
    }
    for r in neg {
        let _ = writeln!(
            s,
            "{:w$}  {:>11}  {:>8}  {:>11}  {:>8}  {:>8}  {:>9}",
            r.name,
            "-",
            "-",
            format!("({} P)", r.p_findings),
            "-",
            r.findings,
            "-"
        );
    }
    let t = totals(pos, neg);
    let _ = writeln!(
        s,
        "TOTAL {} positive files, {} negative files: Balanced recall {} ({}/{}), Paranoid recall {} ({}/{}), \
         precision on positives {} ({}/{}), negative findings Balanced {} / Paranoid {}",
        pos.len(),
        neg.len(),
        pct(t.b_hit, t.b_total),
        t.b_hit,
        t.b_total,
        pct(t.p_hit, t.p_total),
        t.p_hit,
        t.p_total,
        pct(t.on_target, t.findings),
        t.on_target,
        t.findings,
        t.neg_b,
        t.neg_p
    );
    s
}

fn accuracy_md(pos: &[Row], neg: &[Row]) -> String {
    let t = totals(pos, neg);
    let mut s = String::new();
    let _ = writeln!(s, "# Accuracy");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Generated by `REDACTOR_WRITE_ACCURACY=1 cargo test -p redactor-core --test corpus` \
         (engine {}). Do not edit by hand.",
        redactor_core::ENGINE_VERSION
    );
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "The corpus lives in `corpus/`. Positive files mark every secret; vendor-shaped tokens \
         are generated at test time from seeded templates, so no real or realistic token is \
         stored in the repository. Negative files contain no secrets at all: code, lockfiles, \
         logs, prose and this project's README."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Gates");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "- Balanced (the default) redacts every marked secret, with the expected label where \
         the corpus names one, and finds nothing in the negative files."
    );
    let _ = writeln!(
        s,
        "- Paranoid also redacts the secrets marked as Paranoid-only (plain words and other \
         values that need a lower bar)."
    );
    let _ = writeln!(
        s,
        "- Scanning any output again changes nothing and finds nothing."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Summary");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Measure | Value |");
    let _ = writeln!(s, "|---|---|");
    let _ = writeln!(s, "| Positive files | {} |", pos.len());
    let _ = writeln!(s, "| Negative files | {} |", neg.len());
    let _ = writeln!(s, "| Marked secrets (Balanced) | {} |", t.b_total);
    let _ = writeln!(s, "| Marked secrets (Paranoid) | {} |", t.p_total);
    let _ = writeln!(
        s,
        "| Balanced recall | {} ({}/{}) |",
        pct(t.b_hit, t.b_total),
        t.b_hit,
        t.b_total
    );
    let _ = writeln!(
        s,
        "| Paranoid recall | {} ({}/{}) |",
        pct(t.p_hit, t.p_total),
        t.p_hit,
        t.p_total
    );
    let _ = writeln!(
        s,
        "| Balanced precision on positive files | {} ({}/{}) |",
        pct(t.on_target, t.findings),
        t.on_target,
        t.findings
    );
    let _ = writeln!(s, "| Balanced findings in negative files | {} |", t.neg_b);
    let _ = writeln!(s, "| Paranoid findings in negative files | {} |", t.neg_p);
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Precision counts Balanced findings that overlap a marked secret. A finding outside \
         the marks in a positive file lowers it; in a negative file every finding is a false \
         positive."
    );
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "The detectors are tuned against this corpus, so the numbers show that every known \
         case is handled. They are not a measure of accuracy on text the corpus does not \
         cover. The Paranoid findings in negative files are long hex digests and the mixed-case \
         repository owner name in the README's links, which Paranoid reports on purpose."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Positive files");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "| File | Balanced | Recall | Paranoid | Recall | Findings | Precision |"
    );
    let _ = writeln!(s, "|---|---:|---:|---:|---:|---:|---:|");
    for r in pos {
        let _ = writeln!(
            s,
            "| `{}` | {}/{} | {} | {}/{} | {} | {} | {} |",
            r.name.trim_start_matches("positive/"),
            r.b_hit,
            r.b_total,
            pct(r.b_hit, r.b_total),
            r.p_hit,
            r.p_total,
            pct(r.p_hit, r.p_total),
            r.findings,
            r.precision()
                .map_or("-".into(), |p| format!("{:.1}%", 100.0 * p))
        );
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "## Negative files");
    let _ = writeln!(s);
    let _ = writeln!(s, "| File | Balanced findings | Paranoid findings |");
    let _ = writeln!(s, "|---|---:|---:|");
    for r in neg {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} |",
            r.name.trim_start_matches("negative/"),
            r.findings,
            r.p_findings
        );
    }
    s
}

#[test]
fn parser_expands_markers() {
    let doc = parse(
        "t",
        "⟦opt:crlf⟧\nA=⟦Password|pw-Fake-1⟧\nB=⟦gen:github_pat#a⟧ C=⟦gen:github_pat#a⟧\n⟦?*|word⟧ ⟦!|bait⟧\n",
    );
    assert_eq!(doc.spans.len(), 5);
    assert!(doc.text.starts_with("A=pw-Fake-1\r\nB=ghp_"));
    let s = &doc.spans[0];
    assert_eq!(&doc.text[s.start..s.end], "pw-Fake-1");
    assert_eq!(s.label.as_deref(), Some("Password"));
    let (a, b) = (&doc.spans[1], &doc.spans[2]);
    assert_eq!(&doc.text[a.start..a.end], &doc.text[b.start..b.end]);
    assert_eq!(a.label.as_deref(), Some("GitHub Token"));
    assert_eq!(doc.spans[3].want, Want::Paranoid);
    assert_eq!(doc.spans[4].want, Want::Untouched);
    assert_eq!(doc.spans[4].line, 4);
}
