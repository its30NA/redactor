//! Crafted input that once cost far more than its size: memory is counted per scan (the
//! same input allocates the same bytes every run, so these bounds are deterministic).

mod support;

use redactor_core::{Engine, EngineConfig, ScanRequest};
use support::heap;

#[global_allocator]
static HEAP: heap::Counting = heap::Counting;

/// `password=abab…` lines of growing length, then a long `abab…` run (the audit's 98 KB
/// input, 2026-11-10 finding 3). Every value is a prefix of the next, so the run holds every
/// value at every other position; that once took 1.33 GB. The run is one placeholder, and the
/// scan stays within a few megabytes.
#[test]
fn nested_values_and_a_long_copy() {
    let mut text: String = (6..181)
        .map(|k| format!("password={}\n", "ab".repeat(k)))
        .collect();
    text.push_str("note ");
    text.push_str(&"ab".repeat(32_000));
    text.push('\n');
    let e = Engine::new(EngineConfig::default()).unwrap();
    let (out, peak) = heap::peak(|| e.scan(&text, &ScanRequest::default()));
    assert!(out.verified);
    assert_eq!(out.output.lines().count(), 176);
    assert!(out
        .output
        .lines()
        .take(175)
        .all(|l| l.starts_with("password=[REDACTED: Password #")));
    assert_eq!(
        out.output.lines().last(),
        Some("note [REDACTED: Password #176]")
    );
    assert!(peak < 64 << 20, "peak heap {} MiB", peak >> 20);
}
