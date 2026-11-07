//! Rows (one per finding) and the audit log.
//!
//! The audit records what happened, never what was found: a timestamp, the source name, counts
//! by label and tier, and per finding its line, label, kind, tier and detector id. No values,
//! no lengths, no offsets, no hashes (not even salted ones): the whole project keeps
//! secret-derived data off the disk, so a log cannot become a brute-force oracle.

use std::time::{SystemTime, UNIX_EPOCH};

use redactor_core::{Finding, Tier};
use redactor_wire::{by_label, ByTier, LabelCount};
use serde::Serialize;

/// What the CLI keeps of a finding: where it is (bytes into the input, and the line) and how
/// it was classified.
#[derive(Debug, Clone)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub kind: String,
    pub label: String,
    pub tier: Tier,
    pub detector: String,
}

/// Rows for `findings` (sorted by start, byte offsets into `text`).
pub fn rows_of(text: &str, findings: &[Finding]) -> Vec<Row> {
    let bytes = text.as_bytes();
    let mut line = 1usize;
    let mut cursor = 0usize;
    findings
        .iter()
        .filter(|f| !f.kept)
        .map(|f| {
            let start = f.start.max(cursor);
            line += bytes[cursor..start].iter().filter(|&&b| b == b'\n').count();
            cursor = start;
            Row {
                start: f.start,
                end: f.end,
                line,
                kind: f.kind.clone(),
                label: f.label.clone(),
                tier: f.tier,
                detector: f.detector.clone(),
            }
        })
        .collect()
}

pub fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Certain => "certain",
        Tier::Likely => "likely",
        Tier::Possible => "possible",
    }
}

/// The `--summary` text: counts per label, sorted by label.
pub fn summary(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "No sensitive data detected.".to_string();
    }
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for r in rows {
        *counts.entry(r.label.as_str()).or_default() += 1;
    }
    let mut out = format!("Redacted {} item(s):", rows.len());
    for (label, n) in counts {
        out.push_str(&format!("\n  - {label}: {n}"));
    }
    out
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record<'a> {
    line: usize,
    label: &'a str,
    kind: &'a str,
    tier: &'static str,
    detector: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Audit<'a> {
    version: u32,
    timestamp: String,
    source: &'a str,
    redaction_count: usize,
    by_label: Vec<LabelCount>,
    by_tier: ByTier,
    records: Vec<Record<'a>>,
}

/// The audit document for a run over `source` (a file name or `stdin`).
pub fn audit_json(source: &str, rows: &[Row]) -> String {
    let mut by_tier = ByTier::default();
    for r in rows {
        match r.tier {
            Tier::Certain => by_tier.certain += 1,
            Tier::Likely => by_tier.likely += 1,
            Tier::Possible => by_tier.possible += 1,
        }
    }
    let doc = Audit {
        version: 2,
        timestamp: rfc3339_now(),
        source,
        redaction_count: rows.len(),
        by_label: by_label(rows.iter().map(|r| r.label.as_str())),
        by_tier,
        records: rows
            .iter()
            .map(|r| Record {
                line: r.line,
                label: &r.label,
                kind: &r.kind,
                tier: tier_name(r.tier),
                detector: &r.detector,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&doc).expect("audit serializes")
}

fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339(secs)
}

/// UTC timestamp for Unix seconds (civil-from-days, Howard Hinnant's algorithm).
pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_match_known_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn empty_summary() {
        assert_eq!(summary(&[]), "No sensitive data detected.");
    }
}
