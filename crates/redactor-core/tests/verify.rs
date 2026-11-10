//! Verify loop and robustness: placeholders are protected,
//! rescanning an output changes nothing, offsets stay exact around CRLF and emoji.

mod support;

use redactor_core::{
    Engine, EngineConfig, PlaceholderStyle, ScanOutcome, ScanRequest, Sensitivity, Source,
};
use support::gen::*;

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scan_at(e: &Engine, text: &str, s: Sensitivity) -> ScanOutcome {
    e.scan(
        text,
        &ScanRequest {
            sensitivity: Some(s),
            ..ScanRequest::default()
        },
    )
}

/// Every finding and output span sits on char boundaries, and every output span holds its
/// finding's placeholder.
fn assert_well_formed(text: &str, out: &ScanOutcome) {
    for f in &out.findings {
        assert!(f.start < f.end && f.end <= text.len(), "{f:?}");
        assert!(text.is_char_boundary(f.start) && text.is_char_boundary(f.end));
    }
    for s in &out.output_spans {
        assert!(out.output.is_char_boundary(s.start) && out.output.is_char_boundary(s.end));
        assert_eq!(
            &out.output[s.start..s.end],
            out.findings[s.finding].placeholder
        );
    }
}

/// Scanning the output again gives the same text and no findings, at every sensitivity.
fn assert_fixed_point(e: &Engine, text: &str) {
    for s in [
        Sensitivity::Relaxed,
        Sensitivity::Balanced,
        Sensitivity::Paranoid,
    ] {
        let out = scan_at(e, text, s);
        assert!(out.verified, "{s:?}: not verified for {text:?}");
        assert_well_formed(text, &out);
        let again = scan_at(e, &out.output, s);
        let spans: Vec<(&str, &str)> = again
            .findings
            .iter()
            .map(|f| (&out.output[f.start..f.end], f.detector.as_str()))
            .collect();
        assert!(
            again.findings.is_empty() && again.output == out.output,
            "{s:?}: rescan of {:?} found {spans:?}",
            out.output
        );
    }
}

#[test]
fn placeholders_are_protected() {
    let e = engine();
    for text in [
        "&key=?token=[REDACTED: Access Token]",
        "url?key=?token=[REDACTED: Access Token]&x=1",
        "password=[REDACTED: Password]",
        "the password is [REDACTED: Password #2] now",
        "Authorization: Bearer [REDACTED: Bearer Token]",
        "key1:[REDACTED: OpenAI API Key],tok=[REDACTED: GitHub Token];aws=[REDACTED: AWS Access Key ID]",
        "--token [REDACTED: Token] --password=[REDACTED: Password]",
        "postgres://admin:[REDACTED: Password in URL]@db.example.com:5432/app",
        "secret: [REDACTED: Secret]\napi_key: \"[REDACTED: API Key]\"\n",
        "[REDACTED: Selected Text][REDACTED: Password]",
    ] {
        for s in [Sensitivity::Balanced, Sensitivity::Paranoid] {
            let out = scan_at(&e, text, s);
            assert!(
                out.findings.is_empty() && out.output == text,
                "{text:?} at {s:?}: {:?} -> {:?}",
                out.findings
                    .iter()
                    .map(|f| &text[f.start..f.end])
                    .collect::<Vec<_>>(),
                out.output
            );
        }
    }
}

#[test]
fn custom_template_placeholders_are_protected() {
    let e = Engine::new(EngineConfig {
        template: "<<{label}{suffix}>>".into(),
        placeholder: PlaceholderStyle::Generic,
        ..EngineConfig::default()
    })
    .unwrap();
    let text = "DB_PASSWORD=Hunter2-Fake-9x\nAPI_TOKEN=Zq81mXv0pLr3TfK9wQe2Lm\n";
    let out = scan_at(&e, text, Sensitivity::Balanced);
    assert_eq!(
        out.output,
        "DB_PASSWORD=<<Secret #1>>\nAPI_TOKEN=<<Secret #2>>\n"
    );
    let again = scan_at(&e, &out.output, Sensitivity::Paranoid);
    assert!(again.findings.is_empty(), "{:?}", again.findings);
}

#[test]
fn idempotent_rescan() {
    let mut g = Gen::named("idempotent");
    let e = engine();
    let text = format!(
        "DB_PASSWORD=Hunter2-Fake-9x\n\
         GITHUB_TOKEN={gh}\n\
         curl -u admin:Sup3rS3cret! https://api.example.com/v1?access_token={tok}&x=1\n\
         &key=?token={tok}\n\
         Authorization: Bearer {jwt}\n\
         password: \"correct horse battery staple\"\n\
         Mein Passwort ist Tr0ub4dor&3 bitte nicht teilen\n\
         the wifi pw is Kx9!mPq2vL - keep it safe\n\
         login failed for Hunter2-Fake-9x\n\
         seed: {seed}\n",
        gh = g.github("ghp_"),
        tok = g.alnum(32),
        jwt = g.jwt(),
        seed = g.bip39(12, true),
    );
    assert_fixed_point(&e, &text);
}

/// The first pass keeps `password = x9k2prod` because the text uses `x9k2prod` like an
/// identifier (`,x9k2prod,`), but that use sits inside a cookie that gets redacted. The
/// output no longer explains the value away, so the rescan finds it and the verify loop
/// redacts it in the same scan.
#[test]
fn verify_catches_secret_revealed_by_merge() {
    let e = engine();
    let text = "password = x9k2prod\nCookie: sid=1,x9k2prod,z\n";
    let out = scan_at(&e, text, Sensitivity::Balanced);
    assert!(out.verified);
    assert!(!out.output.contains("x9k2prod"), "{}", out.output);
    let pos = text.find("x9k2prod").unwrap();
    let f = out
        .findings
        .iter()
        .find(|f| f.start == pos)
        .expect("the revealed value is a finding in input offsets");
    assert_eq!(f.end, pos + "x9k2prod".len());
    assert_eq!(f.source, Source::Context);
    assert_well_formed(text, &out);
    assert_fixed_point(&e, text);
}

#[test]
fn crlf_values_exclude_cr() {
    let mut g = Gen::named("crlf");
    let gh = g.github("ghp_");
    let e = engine();
    let text = format!(
        "DB_PASSWORD=Hunter2-Fake-9x\r\n\
         password: correct horse battery staple\r\n\
         [db]\r\npassword = Winter2024!\r\n\
         GITHUB_TOKEN={gh}\r\n\
         export API_KEY='p&ss;w0rd#1'\r\n\
         the wifi pw is Kx9!mPq2vL\r\n"
    );
    let out = scan_at(&e, &text, Sensitivity::Balanced);
    assert_well_formed(&text, &out);
    for v in [
        "Hunter2-Fake-9x",
        "correct horse battery staple",
        "Winter2024!",
        gh.as_str(),
        "p&ss;w0rd#1",
        "Kx9!mPq2vL",
    ] {
        let pos = text.find(v).unwrap();
        assert!(
            out.findings
                .iter()
                .any(|f| f.start == pos && f.end == pos + v.len()),
            "{v:?} not found exactly: {:?}",
            out.findings
                .iter()
                .map(|f| &text[f.start..f.end])
                .collect::<Vec<_>>()
        );
    }
    for f in &out.findings {
        assert!(!text[f.start..f.end].contains('\r'), "{f:?}");
    }
    assert_eq!(
        out.output.matches("\r\n").count(),
        text.matches("\r\n").count()
    );
    assert_fixed_point(&e, &text);
}

#[test]
fn emoji_before_secret_offsets() {
    let mut g = Gen::named("emoji");
    let gh = g.github("ghp_");
    let e = engine();
    for prefix in [
        "😀",
        "🔑 ",
        "👨\u{200D}👩\u{200D}👧 ",
        "🇩🇪 ",
        "\t🙂\t",
        "e\u{301}\u{1F600}",
        "مرحبا 🌙 ",
    ] {
        for (line, value) in [
            ("password=Hunter2-Fake-9x", "Hunter2-Fake-9x"),
            (&format!("token: {gh}") as &str, gh.as_str()),
        ] {
            let text = format!("{prefix}{line}\n{prefix}done\n");
            let out = scan_at(&e, &text, Sensitivity::Balanced);
            assert_well_formed(&text, &out);
            let pos = text.find(value).unwrap();
            assert!(
                out.findings
                    .iter()
                    .any(|f| f.start == pos && f.end == pos + value.len()),
                "{text:?}: {:?}",
                out.findings
            );
            assert!(out.output.starts_with(prefix), "{:?}", out.output);
            assert!(!out.output.contains(value));
            assert_fixed_point(&e, &text);
        }
    }
}

/// At the verify cap the last round's findings are still redacted; the outcome says it is
/// not verified, with a warning.
#[test]
fn verify_cap_applies_the_last_round() {
    use redactor_core::WarningKind;
    let text = "password = x9k2prod\nCookie: sid=1,x9k2prod,z\n";
    let out = scan_at(&engine().with_verify_cap(0), text, Sensitivity::Balanced);
    assert!(!out.output.contains("x9k2prod"), "{}", out.output);
    assert!(!out.verified);
    assert!(
        out.warnings
            .iter()
            .any(|w| w.kind == WarningKind::VerifyCapHit),
        "{:?}",
        out.warnings
    );
    assert_well_formed(text, &out);
    // Below the cap the same text verifies.
    let out = scan_at(&engine().with_verify_cap(1), text, Sensitivity::Balanced);
    assert!(
        out.verified && out.warnings.is_empty(),
        "{:?}",
        out.warnings
    );
}
