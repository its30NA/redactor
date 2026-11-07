//! The JSON contract as the CLI and the UI see it.

use redactor_core::{Engine, EngineConfig, Sensitivity};
use redactor_wire::{plain_request, scan, ScanRequestIn, WarningKindOut, MAX_INPUT_BYTES};

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

/// Built at run time so no secret-shaped literal sits in the repository.
fn fake_password() -> String {
    ["Plum", "tree", "47", "-", "Kite"].concat()
}

fn scan_text(text: &str) -> serde_json::Value {
    let req = plain_request(text.to_string(), Sensitivity::Balanced, false, true);
    serde_json::to_value(scan(&engine(), &req)).unwrap()
}

#[test]
fn response_has_exactly_the_contract_keys() {
    let text = format!("DB_PASSWORD={}\n", fake_password());
    let v = scan_text(&text);
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "findings",
            "output",
            "outputSpans",
            "stats",
            "verified",
            "warnings"
        ]
    );
    let mut stats: Vec<_> = v["stats"].as_object().unwrap().keys().cloned().collect();
    stats.sort();
    assert_eq!(stats, ["byLabel", "byTier", "chars", "ms"]);
    assert_eq!(v["findings"][0]["tier"], "certain");
    assert_eq!(v["findings"][0]["source"], "context");
    assert_eq!(v["findings"][0]["valueKey"].as_str().unwrap().len(), 32);
}

#[test]
fn offsets_are_utf16_code_units() {
    let pw = fake_password();
    let text = format!("😀 {pw}\n𝄞 password: {pw}\n");
    let v = scan_text(&text);
    let units: Vec<u16> = text.encode_utf16().collect();
    for f in v["findings"].as_array().unwrap() {
        let (s, e) = (
            f["start"].as_u64().unwrap() as usize,
            f["end"].as_u64().unwrap() as usize,
        );
        assert_eq!(String::from_utf16(&units[s..e]).unwrap(), pw);
    }
    assert_eq!(v["stats"]["chars"].as_u64().unwrap() as usize, units.len());
}

#[test]
fn json_carries_no_secret_value() {
    let pw = fake_password();
    let text = format!("DB_PASSWORD={pw}\nagain {pw}\n");
    let v = scan_text(&text);
    assert!(!v["findings"].as_array().unwrap().is_empty());
    assert!(!v.to_string().contains(&pw));
}

#[test]
fn request_defaults_for_keep_and_manual() {
    let req: ScanRequestIn = serde_json::from_str(
        r#"{"text":"x","sensitivity":"relaxed","pii":false,"stripInvisible":true}"#,
    )
    .unwrap();
    assert_eq!(req.sensitivity, Sensitivity::Relaxed);
    assert!(req.keep.is_empty() && req.manual.is_empty());
}

#[test]
fn oversized_input_is_refused_with_a_warning() {
    let big = "a".repeat(MAX_INPUT_BYTES + 1);
    let req = plain_request(big, Sensitivity::Balanced, false, true);
    let resp = scan(&engine(), &req);
    assert_eq!(resp.warnings[0].kind, WarningKindOut::InputTooLarge);
    assert!(resp.findings.is_empty() && resp.output.is_empty());
}
