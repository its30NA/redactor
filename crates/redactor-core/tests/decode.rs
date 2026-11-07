//! Decode layer, propagation and the matcher hook.

mod support;

use std::sync::Arc;

use redactor_core::{
    Engine, EngineConfig, MatcherHit, ScanOutcome, ScanRequest, SecretMatcher, Sensitivity, Source,
    WarningKind,
};
use support::gen::*;

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scan(text: &str) -> ScanOutcome {
    engine().scan(text, &ScanRequest::default())
}

fn warning(out: &ScanOutcome, kind: WarningKind) -> usize {
    out.warnings
        .iter()
        .find(|w| w.kind == kind)
        .map_or(0, |w| w.count)
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Exactly `[start, start + len)` is one finding with the given source.
fn assert_span(out: &ScanOutcome, text: &str, needle: &str, source: Source) {
    let start = text.find(needle).expect("text holds the needle");
    let spans: Vec<_> = out
        .findings
        .iter()
        .map(|f| (f.start, f.end, f.label.as_str(), f.source))
        .collect();
    assert!(
        out.findings
            .iter()
            .any(|f| f.start == start && f.end == start + needle.len() && f.source == source),
        "{text:?}: expected {source:?} at [{start}, {}), got {spans:?}",
        start + needle.len()
    );
    assert!(!out.output.contains(needle), "{}", out.output);
}

// ---------------------------------------------------------------------------------------
// Decode layer
// ---------------------------------------------------------------------------------------

#[test]
fn k8s_base64_password_caught() {
    // The data key names nothing secret; the secret is only visible after decoding.
    let inner = r#"{"db":{"user":"demo","password":"Hunter2-Fake-9x"}}"#;
    let blob = b64_encode(inner.as_bytes());
    let text = format!(
        "apiVersion: v1\nkind: Secret\nmetadata:\n  name: demo\ndata:\n  settings.json: {blob}\n"
    );
    let out = scan(&text);
    assert_span(&out, &text, &blob, Source::Decoded);
    let f = out
        .findings
        .iter()
        .find(|f| f.source == Source::Decoded)
        .unwrap();
    assert_eq!(f.label, "Encoded Password");
    assert!(f.confidence < 0.9 && f.confidence > 0.8, "{}", f.confidence);
    assert_eq!(
        out.output,
        "apiVersion: v1\nkind: Secret\nmetadata:\n  name: demo\ndata:\n  settings.json: [REDACTED: Encoded Password]\n"
    );
}

#[test]
fn basic_auth_base64_caught() {
    let blob = b64_encode(b"demo:Hunter2-Fake-9x");
    // Not in an Authorization header: only the decoded `user:password` shape tells.
    let text = format!("forward the value {blob} to the proxy\n");
    let out = scan(&text);
    assert_span(&out, &text, &blob, Source::Decoded);
    let f = out
        .findings
        .iter()
        .find(|f| f.source == Source::Decoded)
        .unwrap();
    assert_eq!(f.label, "Encoded Credentials");
    // A plain `name:value` pair of words is not a credential.
    let words = b64_encode(b"status:running-fine");
    assert!(scan(&format!("value {words}\n"))
        .findings
        .iter()
        .all(|f| f.source != Source::Decoded));
}

#[test]
fn url_encoded_and_json_escaped_layers() {
    let inner = "password=Hunter2-Fake-9x&user=demo";
    let enc = pct_encode(inner);
    let text = format!("GET /login?state={enc} HTTP/1.1\n");
    let out = scan(&text);
    assert_span(&out, &text, &enc, Source::Decoded);

    let json: String = "password: Hunter2-Fake-9x"
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect();
    let text = format!("{{\"note\": \"{json}\"}}\n");
    let out = scan(&text);
    assert_span(&out, &text, &json, Source::Decoded);

    let hex: String = "password=Hunter2-Fake-9x"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    let text = format!("payload {hex}\n");
    let out = scan(&text);
    assert_span(&out, &text, &hex, Source::Decoded);
}

#[test]
fn decode_depth_limit_warns() {
    let inner = "password=Hunter2-Fake-9x";
    let two = b64_encode(b64_encode(inner.as_bytes()).as_bytes());
    let out = scan(&format!("blob {two}\n"));
    // Two layers deep is still decoded and found, without a warning.
    assert!(out.findings.iter().any(|f| f.source == Source::Decoded));
    assert_eq!(warning(&out, WarningKind::DecodeLimit), 0);

    let three = b64_encode(two.as_bytes());
    let text = format!("blob {three}\n");
    let out = scan(&text);
    assert!(
        warning(&out, WarningKind::DecodeLimit) >= 1,
        "{:?}",
        out.warnings
    );
    // The outer run still looks random, so it is not left in the clear.
    assert!(!out.output.contains(&three));
}

#[test]
fn many_printable_runs_stay_clean() {
    // Many base64 runs of plain text: decoded, scanned in one pass, nothing found, and the
    // runs are not mistaken for random tokens. (The byte budget is unit-tested in decode.rs.)
    let unit = b64_encode(b"hello world, this is plain text") + "\n";
    let text = unit.repeat(5_000);
    let out = scan(&text);
    assert_eq!(out.output.len(), text.len());
    assert!(out.findings.is_empty());
}

#[test]
fn random_base64_is_not_a_layer() {
    let mut g = Gen::named("decode random");
    let text = format!("x = {}\n", b64_encode(&g.bytes(48)));
    let out = scan(&text);
    assert!(out.findings.iter().all(|f| f.source != Source::Decoded));
    assert_eq!(warning(&out, WarningKind::DecodeLimit), 0);
}

// ---------------------------------------------------------------------------------------
// Propagation
// ---------------------------------------------------------------------------------------

#[test]
fn propagation_second_occurrence() {
    let text = "DB_PASSWORD=Hunter2-Fake-9x\n... login failed for Hunter2-Fake-9x";
    let out = scan(text);
    assert!(!out.output.contains("Hunter2-Fake-9x"), "{}", out.output);
    assert_eq!(out.findings.len(), 2);
    let second = text.rfind("Hunter2-Fake-9x").unwrap();
    let prop = out.findings.iter().find(|f| f.start == second).unwrap();
    assert_eq!(prop.source, Source::Propagated);
    assert_eq!(prop.label, "Password");
    assert_eq!(out.findings[0].value_key, prop.value_key);
    assert_eq!(out.findings[0].placeholder, prop.placeholder);
    assert_eq!(
        out.output,
        "DB_PASSWORD=[REDACTED: Password]\n... login failed for [REDACTED: Password]"
    );
}

#[test]
fn propagation_boundary_short_values() {
    // Below 12 characters only whole tokens are propagated.
    let text = "DB_PASSWORD=x9k2prod\nuser x9k2prod logged in; ax9k2prod and x9k2prods stay\n";
    let out = scan(text);
    assert_eq!(
        out.output,
        "DB_PASSWORD=[REDACTED: Password]\nuser [REDACTED: Password] logged in; ax9k2prod and x9k2prods stay\n"
    );
    // From 12 characters on, an occurrence inside a longer token is redacted too.
    let text = "DB_PASSWORD=Hunter2-Fake-9x\nid=XHunter2-Fake-9xY\n";
    let out = scan(text);
    assert!(!out.output.contains("Hunter2-Fake-9x"), "{}", out.output);
    // Values under 6 characters are never propagated.
    let text = "PIN=4711\nroom 4711\n";
    let out = scan(text);
    assert!(out.output.contains("room 4711"), "{}", out.output);
}

#[test]
fn propagation_respects_keep() {
    let text = "DB_PASSWORD=Hunter2-Fake-9x\nagain Hunter2-Fake-9x\n";
    let e = engine();
    let first = e.scan(text, &ScanRequest::default());
    let key = first.findings[0].value_key;
    let kept = e.scan(
        text,
        &ScanRequest {
            keep: &[key],
            ..ScanRequest::default()
        },
    );
    assert_eq!(kept.output, text);
}

#[test]
fn propagation_of_manual_selection() {
    let text = "note Zebra-Alpha-42 and later Zebra-Alpha-42 again";
    let out = engine().scan(
        text,
        &ScanRequest {
            manual: &[(5, 19)],
            ..ScanRequest::default()
        },
    );
    assert_eq!(
        out.output,
        "note [REDACTED: Selected Text] and later [REDACTED: Selected Text] again"
    );
}

// ---------------------------------------------------------------------------------------
// Matcher hook
// ---------------------------------------------------------------------------------------

/// Reports every occurrence of one fixed string (stand-in for the session secrets).
struct Fixed(String);

impl SecretMatcher for Fixed {
    fn find(&self, text: &str) -> Vec<MatcherHit> {
        text.match_indices(self.0.as_str())
            .map(|(i, m)| MatcherHit {
                start: i,
                end: i + m.len(),
                label: "Known Secret".into(),
            })
            .collect()
    }
}

fn with_secret(secret: &str) -> Engine {
    engine().with_matcher(Arc::new(Fixed(secret.to_string())))
}

#[test]
fn matcher_hits_become_findings() {
    let e = with_secret("plain words");
    let text = "these plain words are secret";
    let out = e.scan(text, &ScanRequest::default());
    assert_eq!(out.findings.len(), 1);
    let f = &out.findings[0];
    assert_eq!(f.source, Source::Watchlist);
    assert_eq!(f.label, "Known Secret");
    assert_eq!(&text[f.start..f.end], "plain words");
    assert_eq!(out.output, "these [REDACTED: Known Secret] are secret");
}

#[test]
fn session_secret_url_encoded_in_url() {
    // A remembered secret inside a URL, percent-encoded.
    let secret = "s3cret pass/word!";
    let enc = pct_encode(secret);
    let text = format!("https://demo.example/cb?next=1&k={enc}&x=2\n");
    let out = with_secret(secret).scan(&text, &ScanRequest::default());
    assert_span(&out, &text, &enc, Source::Decoded);
    let f = out
        .findings
        .iter()
        .find(|f| f.source == Source::Decoded)
        .unwrap();
    assert_eq!(f.label, "Encoded Known Secret");
    // One percent escape is enough for a remembered secret.
    let secret = "Zebra!Alpha";
    let text = format!("?k={}\n", pct_encode(secret));
    let out = with_secret(secret).scan(&text, &ScanRequest::default());
    assert!(!out.output.contains("Zebra"), "{}", out.output);
}

#[test]
fn session_secret_base64_in_k8s_secret() {
    let secret = "plain words";
    let blob = b64_encode(secret.as_bytes());
    let text = format!("kind: Secret\ndata:\n  note: {blob}\n");
    let out = with_secret(secret).scan(&text, &ScanRequest::default());
    assert_span(&out, &text, &blob, Source::Decoded);
}

#[test]
fn session_secret_in_paranoid_and_relaxed() {
    let secret = "plain words";
    let blob = b64_encode(secret.as_bytes());
    let text = format!("note: {blob}\n");
    for s in [Sensitivity::Relaxed, Sensitivity::Paranoid] {
        let out = with_secret(secret).scan(
            &text,
            &ScanRequest {
                sensitivity: Some(s),
                ..ScanRequest::default()
            },
        );
        assert!(!out.output.contains(&blob), "{s:?}: {}", out.output);
    }
}
