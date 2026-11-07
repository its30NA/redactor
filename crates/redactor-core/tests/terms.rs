//! Always-redact words (`EngineConfig::terms`).

use redactor_core::{Engine, EngineConfig, ScanRequest, Source};

fn engine(terms: &[&str]) -> Engine {
    Engine::new(EngineConfig {
        terms: terms.iter().map(|t| t.to_string()).collect(),
        ..EngineConfig::default()
    })
    .unwrap()
}

fn found<'t>(text: &'t str, terms: &[&str]) -> Vec<&'t str> {
    engine(terms)
        .scan(text, &ScanRequest::default())
        .findings
        .iter()
        .map(|f| &text[f.start..f.end])
        .collect()
}

#[test]
fn terms_case_insensitive_whole_word() {
    let text = "ACME ships to Acme, not to acmeville or xacme.";
    assert_eq!(found(text, &["acme"]), vec!["ACME", "Acme"]);
    let out = engine(&["acme"]).scan(text, &ScanRequest::default());
    assert_eq!(
        out.output,
        "[REDACTED: Custom Term] ships to [REDACTED: Custom Term], not to acmeville or xacme."
    );
    // Multi-word terms and non-ASCII letters.
    assert_eq!(
        found("Hello from demo corp and DEMO CORP.", &["Demo Corp"]),
        vec!["demo corp", "DEMO CORP"]
    );
    assert_eq!(found("Grüße an MÜLLER-net", &["müller"]), vec!["MÜLLER"]);
    assert!(found("Müllerei", &["müller"]).is_empty());
}

#[test]
fn terms_label_custom_term() {
    let text = "host: build-07.internal";
    let out = engine(&["build-07.internal"]).scan(text, &ScanRequest::default());
    assert_eq!(out.findings.len(), 1);
    let f = &out.findings[0];
    assert_eq!(f.label, "Custom Term");
    assert_eq!(f.source, Source::Custom);
    assert!((f.confidence - 0.95).abs() < 1e-6);
    assert_eq!(&text[f.start..f.end], "build-07.internal");
    // A term that happens to be a placeholder word is still redacted: the user asked for it.
    assert_eq!(found("run the test suite", &["test"]), vec!["test"]);
}

#[test]
fn terms_min_len_3() {
    assert!(found("ab cd ab", &["ab", " ", ""]).is_empty());
    assert_eq!(found("ab cd abc", &["ab", "abc"]), vec!["abc"]);
    // Length counts characters after trimming, not bytes.
    assert!(found("ää x", &[" ää "]).is_empty());
}
