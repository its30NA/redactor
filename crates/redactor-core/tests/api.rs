//! Shape of the public API.

use redactor_core::{
    Engine, EngineConfig, Finding, ScanRequest, Sensitivity, Source, Tier, ValueKey, ENGINE_VERSION,
};

#[test]
fn api_shape() {
    let engine = Engine::new(EngineConfig::default()).expect("default config builds");
    let out = engine.scan("", &ScanRequest::default());
    assert_eq!(out.output, "");
    assert!(out.findings.is_empty());
    assert!(out.output_spans.is_empty());
    assert!(out.warnings.is_empty());
    assert!(out.verified);
    assert_eq!(ENGINE_VERSION, env!("CARGO_PKG_VERSION"));
}

#[test]
fn engine_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Engine>();
}

#[test]
fn default_config_values() {
    let cfg = EngineConfig::default();
    assert_eq!(cfg.sensitivity, Sensitivity::Balanced);
    assert!(!cfg.pii);
    assert!(cfg.strip_invisible);
    assert_eq!(cfg.template, "[REDACTED: {label}{suffix}]");
    // Two default configs carry different session keys (random per process and call).
    assert_ne!(cfg.session_key, EngineConfig::default().session_key);
    // Debug never prints the session key.
    let dbg = format!("{cfg:?}");
    assert!(!dbg.contains(&format!("{:?}", cfg.session_key)));
}

#[test]
fn finding_has_no_value_field() {
    let f = Finding {
        start: 3,
        end: 9,
        kind: "github_pat".into(),
        label: "GitHub Token".into(),
        tier: Tier::Certain,
        confidence: 0.99,
        detector: "github_pat".into(),
        reason: "format".into(),
        source: Source::Pattern,
        value_key: ValueKey([7; 16]),
        placeholder: "[REDACTED: GitHub Token]".into(),
        kept: false,
    };
    let json = serde_json::to_value(&f).expect("serializes");
    let obj = json.as_object().expect("object");
    for key in obj.keys() {
        if key.to_ascii_lowercase().contains("value") {
            assert_eq!(key, "valueKey", "unexpected value-like key {key}");
        }
    }
    assert!(obj.contains_key("valueKey"));
    assert_eq!(obj["tier"], "certain");
    assert_eq!(obj["source"], "pattern");
    // The value key is an opaque hex string.
    assert_eq!(obj["valueKey"], "07".repeat(16));
}

#[test]
fn value_key_hex_roundtrip() {
    let k = ValueKey([0xAB; 16]);
    assert_eq!(ValueKey::from_hex(&k.to_hex()), Some(k));
    assert_eq!(ValueKey::from_hex("zz"), None);
}

/// `is_hidden` is the engine's own test for what it removes before detection (the app
/// stores remembered values with it).
#[test]
fn is_hidden_is_what_the_engine_removes() {
    use redactor_core::is_hidden;
    for c in [
        '\u{200B}',
        '\u{061C}',
        '\u{3164}',
        '\u{FFF9}',
        '\u{E0100}',
        '\u{202E}',
    ] {
        assert!(is_hidden(Some('a'), c, Some('b')), "{:04X}", c as u32);
    }
    assert!(!is_hidden(Some('a'), 'b', Some('c')));
    // Kept: a joiner between emoji, U+FE0F after one.
    assert!(!is_hidden(Some('\u{1F469}'), '\u{200D}', Some('\u{1F4BB}')));
    assert!(!is_hidden(Some('\u{2764}'), '\u{FE0F}', None));
    assert!(is_hidden(Some('a'), '\u{FE0F}', None));
}
