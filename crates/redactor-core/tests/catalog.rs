//! Structured detectors and catalog behaviour through the full engine.

mod support;

use std::time::Instant;

use redactor_core::{
    rule_infos, CustomRule, Engine, EngineConfig, Family, ScanRequest, Sensitivity, Source, Tier,
};
use support::gen::*;

#[test]
fn prose_mentions_are_not_secrets() {
    for text in [
        "Crypto material: `-----BEGIN PGP PRIVATE KEY BLOCK-----`, PuTTY files and more.\n",
        r#"pattern = re.compile(r"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----.*?-----END")"#,
        "so the context (``Authorization: Bearer ``, ``Cookie: ``) stays legible\n",
        "-----BEGIN PRIVATE KEY-----\n(truncated)\n",
    ] {
        let out = scan_at(text, Sensitivity::Paranoid);
        assert!(out.findings.is_empty(), "{text:?}: {:?}", out.findings);
    }
}

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scan_at(text: &str, s: Sensitivity) -> redactor_core::ScanOutcome {
    engine().scan(
        text,
        &ScanRequest {
            sensitivity: Some(s),
            ..ScanRequest::default()
        },
    )
}

#[test]
fn pgp_private_block() {
    let mut g = Gen::named("pgp");
    let block = format!(
        "-----BEGIN PGP PRIVATE KEY BLOCK-----\nVersion: test\n\n{}\n{}\n={}\n-----END PGP PRIVATE KEY BLOCK-----",
        g.from(B64, 64),
        g.from(B64, 64),
        g.from(B64, 4)
    );
    let text = format!("here is my key\n{block}\nthanks\n");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(
        out.output,
        "here is my key\n[REDACTED: PGP Private Key]\nthanks\n"
    );
    assert_eq!(out.findings[0].tier, Tier::Certain);
}

#[test]
fn private_key_block_without_end_marker_still_covered() {
    let mut g = Gen::named("pem-trunc");
    let body = g.from(B64, 64);
    let text = format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{body}\n{body}");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(out.output, "[REDACTED: Private Key]");
}

#[test]
fn json_escaped_private_key() {
    let mut g = Gen::named("pem-json");
    let text = format!(
        r#"{{"type": "service_account", "private_key": "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n"}}"#,
        g.from(B64, 64)
    );
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(
        out.output,
        r#"{"type": "service_account", "private_key": "[REDACTED: Private Key]\n"}"#
    );
}

#[test]
fn putty_key() {
    let mut g = Gen::named("putty");
    let key = format!(
        "PuTTY-User-Key-File-2: ssh-rsa\r\nEncryption: none\r\nComment: k\r\nPublic-Lines: 1\r\n{}\r\nPrivate-Lines: 2\r\n{}\r\n{}\r\nPrivate-MAC: {}",
        g.from(B64, 64),
        g.from(B64, 64),
        g.from(B64, 30),
        g.hex(40)
    );
    let text = format!("{key}\r\nnext line\r\n");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(out.output, "[REDACTED: PuTTY Private Key]\r\nnext line\r\n");
}

#[test]
fn bcrypt_hash() {
    let mut g = Gen::named("bcrypt");
    let hash = format!("$2y$10${}", g.from(CRYPT, 53));
    let out = scan_at(&format!("admin:{hash}\n"), Sensitivity::Balanced);
    assert_eq!(out.output, "admin:[REDACTED: Password Hash]\n");
}

#[test]
fn seed_phrase_valid_checksum() {
    let mut g = Gen::named("seed-valid");
    for words in [12, 15, 18, 21, 24] {
        let phrase = g.bip39(words, true);
        let text = format!("Recovery:\n{phrase}\nkeep it safe");
        let out = scan_at(&text, Sensitivity::Balanced);
        assert_eq!(
            out.output, "Recovery:\n[REDACTED: Recovery Phrase]\nkeep it safe",
            "{words} words"
        );
    }
}

#[test]
fn seed_phrase_numbered_list() {
    let mut g = Gen::named("seed-numbered");
    let phrase = g.bip39(12, true);
    let numbered: Vec<String> = phrase
        .split(' ')
        .enumerate()
        .map(|(i, w)| format!("{}. {w}", i + 1))
        .collect();
    let text = numbered.join("\n");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].label, "Recovery Phrase");
}

#[test]
fn seed_phrase_bad_checksum_ignored() {
    let mut g = Gen::named("seed-bad");
    let phrase = g.bip39(12, false);
    let text = format!("words: {phrase}\n");
    assert!(scan_at(&text, Sensitivity::Balanced).findings.is_empty());
    // Paranoid still flags the run of seed words.
    let paranoid = scan_at(&text, Sensitivity::Paranoid);
    assert_eq!(paranoid.findings.len(), 1);
    assert_eq!(paranoid.findings[0].tier, Tier::Possible);
}

#[test]
fn jwt_header_must_decode() {
    let mut g = Gen::named("jwt");
    let good = g.jwt();
    let out = scan_at(&format!("token {good}\n"), Sensitivity::Balanced);
    assert_eq!(out.output, "token [REDACTED: JSON Web Token]\n");

    // Starts with eyJ but the header is not a JWT header.
    let bad = format!(
        "{}.{}.{}",
        b64url_encode(br#"{"hello":"world"}"#),
        b64url_encode(br#"{"sub":"x"}"#),
        b64url_encode(&g.bytes(16))
    );
    let out = scan_at(&format!("token {bad}\n"), Sensitivity::Paranoid);
    assert!(out.findings.iter().all(|f| f.detector != "jwt"));
}

#[test]
fn github_checksum_raises_confidence() {
    let mut g = Gen::named("gh-crc");
    let valid = g.github("ghp_");
    let out = scan_at(&format!("t={valid}\n"), Sensitivity::Balanced);
    assert_eq!(out.findings[0].confidence, 0.99);
    // Same shape, broken checksum: still found, lower confidence.
    let mut broken = valid.clone();
    let last = broken.pop().unwrap();
    broken.push(if last == 'A' { 'B' } else { 'A' });
    let out = scan_at(&format!("t={broken}\n"), Sensitivity::Balanced);
    assert_eq!(out.findings[0].detector, "github_pat");
    assert!(out.findings[0].confidence < 0.95);
}

#[test]
fn specific_rule_beats_generic_label() {
    let mut g = Gen::named("ant");
    let key = format!("sk-ant-api03-{}AA", g.alnum(93));
    let out = scan_at(&format!("ANTHROPIC_API_KEY={key}\n"), Sensitivity::Balanced);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].detector, "anthropic_api_key");
    assert_eq!(
        out.output,
        "ANTHROPIC_API_KEY=[REDACTED: Anthropic API Key]\n"
    );
}

#[test]
fn same_token_twice_same_placeholder() {
    let mut g = Gen::named("twice");
    let a = g.github("ghp_");
    let b = g.github("ghp_");
    let text = format!("first {a}\nsecond {b}\nagain {a}\n");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(
        out.output,
        "first [REDACTED: GitHub Token #1]\nsecond [REDACTED: GitHub Token #2]\nagain [REDACTED: GitHub Token #1]\n"
    );
}

#[test]
fn zero_width_split_token_is_caught() {
    let mut g = Gen::named("zw");
    let token = g.github("ghp_");
    let split = format!("{}\u{200B}{}", &token[..10], &token[10..]);
    let out = scan_at(&format!("key: {split}\n"), Sensitivity::Balanced);
    assert_eq!(out.output, "key: [REDACTED: GitHub Token]\n");
}

/// Characters that render as nothing (Arabic letter mark, Mongolian selectors, variation
/// selectors, Hangul fillers) do not hide a token, and the placeholder covers them too.
#[test]
fn invisible_split_token_is_caught_per_range() {
    let mut g = Gen::named("invisible ranges");
    for ch in [
        '\u{061C}',
        '\u{180B}',
        '\u{180D}',
        '\u{180F}',
        '\u{FE00}',
        '\u{FE0E}',
        '\u{E0100}',
        '\u{E01EF}',
        '\u{115F}',
        '\u{1160}',
        '\u{3164}',
        '\u{FFA0}',
    ] {
        let token = g.github("ghp_");
        let split = format!("{}{ch}{}", &token[..10], &token[10..]);
        let text = format!("key: {split}\n");
        let out = scan_at(&text, Sensitivity::Balanced);
        assert_eq!(
            out.output, "key: [REDACTED: GitHub Token]\n",
            "U+{:04X}",
            ch as u32
        );
        assert_eq!(out.findings.len(), 1, "U+{:04X}", ch as u32);
        let f = &out.findings[0];
        assert_eq!(
            (f.start, f.end),
            (5, 5 + split.len()),
            "U+{:04X}",
            ch as u32
        );
        let warned = out
            .warnings
            .iter()
            .find(|w| w.kind == redactor_core::WarningKind::InvisibleChars)
            .map(|w| w.count);
        assert_eq!(warned, Some(1), "U+{:04X}", ch as u32);
    }
}

#[test]
fn fullwidth_token_is_caught() {
    let mut g = Gen::named("fw");
    let token = g.github("ghp_");
    let wide: String = token
        .chars()
        .map(|c| char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap())
        .collect();
    let out = scan_at(&format!("key: {wide}\n"), Sensitivity::Balanced);
    assert_eq!(out.output, "key: [REDACTED: GitHub Token]\n");
}

#[test]
fn documentation_filler_is_ignored() {
    let text = format!("GITHUB_TOKEN=ghp_{}\n", "x".repeat(36));
    assert!(scan_at(&text, Sensitivity::Paranoid).findings.is_empty());
    let text = "DATABASE_URL=postgres://app:${DB_PASSWORD}@db.example.com/app\n";
    assert!(scan_at(text, Sensitivity::Paranoid).findings.is_empty());
}

#[test]
fn pii_off_by_default_and_on_request() {
    let text = "mail jane.doe@example.com\n";
    assert!(scan_at(text, Sensitivity::Paranoid).findings.is_empty());
    let out = engine().scan(
        text,
        &ScanRequest {
            pii: Some(true),
            ..ScanRequest::default()
        },
    );
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].source, Source::Pii);
    assert_eq!(out.output, "mail [REDACTED: Email Address]\n");
}

#[test]
fn disabled_rule_does_not_run() {
    let mut g = Gen::named("disabled");
    let token = g.github("ghp_");
    let e = Engine::new(EngineConfig {
        disabled: vec!["github_pat".into()],
        ..EngineConfig::default()
    })
    .unwrap();
    // The token may still look random to the classifier; the rule itself is off.
    assert!(e
        .scan(&format!("t={token}\n"), &ScanRequest::default())
        .findings
        .iter()
        .all(|f| f.detector != "github_pat"));
    assert_eq!(e.rule_count(), engine().rule_count() - 1);
}

#[test]
fn custom_rule_with_group() {
    let e = Engine::new(EngineConfig {
        custom_rules: vec![CustomRule {
            id: "acme_ticket".into(),
            label: "ACME Ticket Key".into(),
            pattern: r"ACME-KEY:(\w{8})".into(),
            confidence: 0.9,
            group: 1,
        }],
        ..EngineConfig::default()
    })
    .unwrap();
    let out = e.scan("x ACME-KEY:abcd1234 y", &ScanRequest::default());
    assert_eq!(out.output, "x ACME-KEY:[REDACTED: ACME Ticket Key] y");
    assert_eq!(out.findings[0].source, Source::Custom);
    assert_eq!(out.findings[0].detector, "acme_ticket");

    let bad = EngineConfig {
        custom_rules: vec![CustomRule {
            id: "bad".into(),
            label: "Bad".into(),
            pattern: "(".into(),
            confidence: 0.9,
            group: 0,
        }],
        ..EngineConfig::default()
    };
    assert!(Engine::new(bad).is_err());
}

#[test]
fn allowlist_drops_value() {
    let mut g = Gen::named("allow");
    let token = g.github("ghp_");
    let e = Engine::new(EngineConfig {
        allowlist: vec![format!("^{token}$")],
        ..EngineConfig::default()
    })
    .unwrap();
    assert!(e
        .scan(&format!("t={token}\n"), &ScanRequest::default())
        .findings
        .is_empty());
}

#[test]
fn findings_json_carries_no_values() {
    let mut g = Gen::named("json");
    let token = g.github("ghp_");
    let pw = g.alnum(16);
    let text = format!("t={token}\npostgres://u:{pw}@db.example.com/x\n");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(out.findings.len(), 2);
    let json = serde_json::to_string(&out.findings).unwrap();
    assert!(!json.contains(&token));
    assert!(!json.contains(&pw));
    assert!(!out.output.contains(&token) && !out.output.contains(&pw));
}

#[test]
fn rule_catalog_metadata() {
    let infos = rule_infos();
    assert!(infos.len() >= 150, "only {} rules", infos.len());
    let mut ids: Vec<&str> = infos.iter().map(|r| r.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate rule ids");
    for r in &infos {
        assert!(
            !r.label.is_empty() && !r.example_shape.is_empty(),
            "{}",
            r.id
        );
        assert!((0.0..=1.0).contains(&r.confidence), "{}", r.id);
        assert!(r
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
    }
    for fam in [
        Family::Ai,
        Family::Dev,
        Family::Saas,
        Family::Cloud,
        Family::Crypto,
        Family::Hash,
        Family::Http,
        Family::Pii,
    ] {
        assert!(infos.iter().any(|r| r.family == fam), "{fam:?} empty");
    }
    // Every rule (PII included) counts.
    assert_eq!(engine().rule_count(), infos.len());
}

/// ~1 MiB of realistic mixed text with a few secrets sprinkled in.
fn mixed_mib(g: &mut Gen) -> String {
    let filler = [
        "2026-11-06T10:15:02.123Z INFO  request handled path=/api/v1/users/42 status=200 duration_ms=12\n",
        "    at com.example.service.UserService.lookup(UserService.java:118)\n",
        "fn main() { let config = load_config(\"settings.toml\").expect(\"config\"); println!(\"{config:?}\"); }\n",
        "The quick brown fox jumps over the lazy dog while the deployment pipeline finishes its checks.\n",
        "Die Konfiguration wurde erfolgreich geladen; bitte prüfen Sie die Protokolle für Details.\n",
        "{\"id\": \"5f1c2a9e-8d3b-4c6a-9e7f-1a2b3c4d5e6f\", \"name\": \"widget\", \"tags\": [\"a\", \"b\"], \"price\": 9.99}\n",
        "commit 3f2a9c1e7b4d6a8f0e1c2b3a4d5e6f7a8b9c0d1e Merge branch 'feature/login' into main\n",
        "GET https://example.com/search?q=rust+regex&page=2 HTTP/1.1\nHost: example.com\nAccept: */*\n",
    ];
    let mut s = String::with_capacity(1 << 20);
    let mut i = 0;
    while s.len() < (1 << 20) {
        s.push_str(filler[i % filler.len()]);
        if i % 997 == 0 {
            s.push_str(&format!("GITHUB_TOKEN={}\n", g.github("ghp_")));
        }
        i += 1;
    }
    s
}

#[test]
fn mixed_mib_redacts_sprinkled_tokens() {
    let mut g = Gen::named("mib");
    let text = mixed_mib(&mut g);
    let out = engine().scan(&text, &ScanRequest::default());
    let tokens = text.matches("GITHUB_TOKEN=").count();
    assert_eq!(out.findings.len(), tokens, "false positives in filler text");
    assert!(out.findings.iter().all(|f| f.detector == "github_pat"));
}

#[test]
fn dense_anchors_still_find_the_real_token() {
    // 60k "eyJ" anchors push the JWT rule past its work budget; the linear fallback must
    // still find the real token after them.
    let mut g = Gen::named("dense");
    let jwt = g.jwt();
    let text = format!("{} {jwt}\n", "eyJ".repeat(60_000));
    let out = scan_at(&text, Sensitivity::Balanced);
    let pos = text.find(&jwt).unwrap();
    assert!(out
        .findings
        .iter()
        .any(|f| f.detector == "jwt" && f.start == pos && f.end == pos + jwt.len()));
}

fn adversarial_inputs() -> Vec<(&'static str, String)> {
    let mib = 1 << 20;
    vec![
        ("one line of a", "a".repeat(mib)),
        ("eyJ repeated", "eyJ".repeat(mib / 3)),
        ("sk- repeated", "sk-".repeat(mib / 3)),
        ("AC repeated", "AC".repeat(mib / 2)),
        ("password= repeated", "password=".repeat(mib / 9)),
        ("100k password= lines", "password=\n".repeat(100_000)),
        (
            "100k password=<value> lines",
            "password=Hunter2-Fake-9x\n".repeat(100_000),
        ),
        ("://@ repeated", "a://b:c@".repeat(mib / 8)),
        ("zero-width", "\u{200B}".repeat(mib / 3)),
        ("authorization", "Authorization: Bearer ".repeat(mib / 22)),
        ("hex run", "0123456789abcdef".repeat(mib / 16)),
        ("digits", "4".repeat(mib)),
        ("seed words", "abandon ".repeat(mib / 8)),
    ]
}

/// Release-mode timing of 1 MiB adversarial inputs, PII on.
#[test]
#[ignore]
fn perf_adversarial_mib() {
    let e = engine();
    for (name, text) in adversarial_inputs() {
        let t = Instant::now();
        e.scan(
            &text,
            &ScanRequest {
                pii: Some(true),
                sensitivity: Some(Sensitivity::Paranoid),
                ..ScanRequest::default()
            },
        );
        println!("{name}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Release-mode timing: `cargo test -p redactor-core --release --test catalog -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf_mixed_mib() {
    let mut g = Gen::named("mib");
    let text = mixed_mib(&mut g);
    let e = engine();
    e.scan("warm up", &ScanRequest::default());
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let out = e.scan(&text, &ScanRequest::default());
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        assert!(!out.findings.is_empty());
    }
    let pii = Instant::now();
    e.scan(
        &text,
        &ScanRequest {
            pii: Some(true),
            ..ScanRequest::default()
        },
    );
    println!(
        "1 MiB mixed text: best {best:.1} ms over 5 runs ({} rules); with PII: {:.1} ms",
        e.rule_count(),
        pii.elapsed().as_secs_f64() * 1000.0
    );
}

/// A vendor pattern names the span even where the context engine also found it by its key.
#[test]
fn vendor_label_beats_context_label() {
    let mut g = Gen::named("vendor label");
    let openai = format!("sk-proj-{}", g.alnum(60));
    let gh = g.github("ghp_");
    let generic = format!("sk-{}", g.alnum(32));
    for (text, label) in [
        (format!("OPENAI_API_KEY={generic}\n"), "OpenAI API Key"),
        (format!("openai:\n  api_key: {generic}\n"), "OpenAI API Key"),
        (format!("{{\"apiKey\": \"{generic}\"}}"), "OpenAI API Key"),
        (format!("OPENAI_API_KEY={openai}\n"), "OpenAI API Key"),
        (format!("openai:\n  api_key: {openai}\n"), "OpenAI API Key"),
        (format!("{{\"api_key\": \"{openai}\"}}"), "OpenAI API Key"),
        (format!("GITHUB_TOKEN={gh}\n"), "GitHub Token"),
        (format!("password: {gh}\n"), "GitHub Token"),
    ] {
        let out = scan_at(&text, Sensitivity::Balanced);
        assert_eq!(out.findings.len(), 1, "{text:?}: {:?}", out.findings);
        assert_eq!(out.findings[0].label, label, "{text:?}");
    }
}

/// The word list itself (sorted, one per line) and lists with repeated words are not a
/// recovery phrase, even where a window happens to pass the checksum.
#[test]
fn seed_phrase_ignores_word_lists() {
    let list = include_str!("../src/rules/bip39_english.txt");
    let out = scan_at(list, Sensitivity::Paranoid);
    assert!(out.findings.is_empty(), "{} findings", out.findings.len());
    let caps = "  \"permissions\": [\n    \"allow-open-text-file\",\n    \"allow-session-list\",\n    \
                \"allow-session-remember\",\n    \"allow-session-add\",\n    \"allow-session-forget\",\n    \
                \"allow-session-clear\",\n    \"allow-get-settings\"\n  ]\n";
    let out = scan_at(caps, Sensitivity::Paranoid);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

/// `/etc/shadow` lines are not `.pgpass` lines: their tail is day counts, not a password.
#[test]
fn shadow_lines_are_not_pgpass() {
    let text = "daemon:*:19800:0:99999:7:::\nbob:!:19802:0:99999:7:::\n";
    let out = scan_at(text, Sensitivity::Paranoid);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

/// Rule descriptions in a Markdown table name the fields, they hold no values.
#[test]
fn rule_descriptions_are_not_secrets() {
    let text = "| `set_cookie_header` | HTTP Cookie | Set-Cookie: name=<value> | 0.80 | on |\n\
| `url_query_token` | Access Token | ?access_token=\\|refresh_token=\\|token=\\|code=… in a URL | 0.85 | on |\n\
| `url_query_password` | Password | ?password=\\|passwd=\\|pwd=\\|pass=… in a URL | 0.90 | on |\n";
    let out = scan_at(text, Sensitivity::Balanced);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
    // Real values still count.
    let out = scan_at(
        "GET /cb?code=Zq81mXv0pLr3TfK9 HTTP/1.1\nSet-Cookie: sid=Zq81mXv0pLr3TfK9wQe2; Path=/\n",
        Sensitivity::Balanced,
    );
    assert_eq!(out.findings.len(), 2, "{:?}", out.findings);
}
