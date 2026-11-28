//! The token classifier and its standalone detector.

mod support;

use redactor_core::{secret_probability, Engine, EngineConfig, ScanRequest, Sensitivity, Source};
use support::gen::*;

fn scan_at(text: &str, s: Sensitivity) -> redactor_core::ScanOutcome {
    Engine::new(EngineConfig::default()).unwrap().scan(
        text,
        &ScanRequest {
            sensitivity: Some(s),
            ..ScanRequest::default()
        },
    )
}

#[test]
fn random_tokens_score_high() {
    let mut g = Gen::named("classifier random tokens");
    for i in 0..200 {
        let n = 20 + i % 45;
        let t = match i % 3 {
            0 => g.alnum(n),
            1 => g.from(B64, n),
            _ => g.from(B64URL, n),
        };
        let p = secret_probability(&t);
        assert!(p > 0.9, "{t}: {p}");
    }
}

#[test]
fn identifiers_score_low() {
    for t in [
        "getUserAccountSettings",
        "node_modules",
        "2024-11-30",
        "v1.2.3",
        "AbstractSingletonProxyFactoryBean",
        "HTTP_PROXY_AUTHORIZATION_HEADER",
        "/usr/local/lib/python3.12/site-packages",
        "550e8400-e29b-41d4-a716-446655440000",
        "2026-11-06T21:15:00.000Z",
        "ticketBlob",
        "Hello",
    ] {
        let p = secret_probability(t);
        assert!(p < 0.2, "{t}: {p}");
    }
}

#[test]
fn unlabeled_random_token_is_possible_in_balanced() {
    let mut g = Gen::named("classifier standalone");
    let token = g.alnum(32);
    let text = format!("the build printed {token} and stopped\n");
    let out = scan_at(&text, Sensitivity::Balanced);
    assert_eq!(out.findings.len(), 1, "{:?}", out.findings);
    let f = &out.findings[0];
    assert_eq!(&text[f.start..f.end], token);
    assert_eq!(f.source, Source::Classifier);
    assert_eq!(f.tier, redactor_core::Tier::Possible);
    assert!(!out.output.contains(&token));
}

#[test]
fn shorter_random_token_only_in_paranoid() {
    let mut g = Gen::named("classifier paranoid");
    let token = g.alnum(17);
    let text = format!("ref {token} end\n");
    assert!(scan_at(&text, Sensitivity::Balanced).findings.is_empty());
    let out = scan_at(&text, Sensitivity::Paranoid);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].source, Source::Classifier);
}

#[test]
fn git_sha_not_standalone_in_balanced() {
    let mut g = Gen::named("classifier git sha");
    let sha = g.hex(40);
    for text in [
        format!("commit {sha}\nAuthor: demo\n"),
        format!("merged {sha} into main\n"),
    ] {
        assert!(
            scan_at(&text, Sensitivity::Balanced).findings.is_empty(),
            "{text}"
        );
    }
    // Paranoid: hex after `commit` stays clean, an unexplained hex run does not.
    assert!(scan_at(&format!("commit {sha}\n"), Sensitivity::Paranoid)
        .findings
        .is_empty());
    let out = scan_at(&format!("merged {sha} into main\n"), Sensitivity::Paranoid);
    assert_eq!(out.findings.len(), 1);
}

#[test]
fn repetitive_hex_is_not_random() {
    for text in [
        format!("x {} y\n", "a".repeat(40)),
        format!("x {} y\n", "0123456789abcdef".repeat(4)),
        format!("x {} y\n", "deadbeef".repeat(6)),
        // Long numbers are numbers, not hex keys.
        "MAX = 340282366920938463463374607431768211456\n".to_string(),
        "x = 11417981541647681583767488212054764084468383744\n".to_string(),
    ] {
        let out = scan_at(&text, Sensitivity::Paranoid);
        assert!(out.findings.is_empty(), "{text}: {:?}", out.findings);
    }
    // Words joined by separators are names, whatever their letters look like.
    for text in [
        "script Nyiakeng_Puachue_Hmong\n",
        "use Anatolian_Hieroglyphs-Table\n",
    ] {
        let out = scan_at(text, Sensitivity::Balanced);
        assert!(out.findings.is_empty(), "{text}: {:?}", out.findings);
    }
}

#[test]
fn identifiers_and_hashes_stay_clean() {
    let mut g = Gen::named("classifier clean");
    let integrity = b64_encode(&g.bytes(64));
    for text in [
        "fn getUserAccountSettings(node_modules: &Path) -> AbstractSingletonProxyFactoryBean {}\n"
            .to_string(),
        "id 550e8400-e29b-41d4-a716-446655440000 at 2026-11-06T21:15:00.000Z\n".to_string(),
        format!("\"integrity\": \"sha512-{integrity}\"\n"),
        "see /usr/local/lib/python3.12/site-packages/numpy/core/_multiarray_umath.so\n".to_string(),
    ] {
        let out = scan_at(&text, Sensitivity::Balanced);
        assert!(out.findings.is_empty(), "{text}: {:?}", out.findings);
    }
}

/// A file path is as random as the most random name in it, not as the whole run with its
/// slashes: a path with generated folder names (macOS temp folders, Xcode DerivedData, a
/// timestamped release) is clean, because none of these names is a Balanced finding alone.
#[test]
fn paths_with_generated_folder_names_stay_clean() {
    for text in [
        "/var/folders/2s/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/\n",
        "export TMPDIR=\"/var/folders/2s/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/\"\n",
        "cd /private/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/\n",
        "/var/folders/2s/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/.tmpAbC123/out.txt\n",
        "/var/folders/2s/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/tmpab12cd_x/out\n",
        "/Users/sam/Library/Developer/Xcode/DerivedData/App-gqzvnlmrxkdtfwbmhdpzqksyvjcu/Build\n",
        "/opt/app/releases/20261112T093000Z/bin\n",
        "ls /usr/lib/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/\n",
    ] {
        let out = scan_at(text, Sensitivity::Balanced);
        assert!(out.findings.is_empty(), "{text}: {:?}", out.findings);
    }
}

/// What a generated folder name is alone, its path is too: Paranoid still reports it.
#[test]
fn generated_folder_name_is_still_a_paranoid_finding() {
    let text = "cd /var/folders/2s/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/\n";
    let out = scan_at(text, Sensitivity::Paranoid);
    assert_eq!(out.findings.len(), 1, "{:?}", out.findings);
    let f = &out.findings[0];
    assert!(text[f.start..f.end].contains("h6hvcg6x4mq0tg5m5bkhyzb40000gn"));
}

/// A random name inside a path is still found in Balanced.
#[test]
fn random_name_in_a_path_is_still_found() {
    let token = "Zq81mXv0pLr3TfK9wQe2Lm4x";
    for text in [
        format!("/etc/secrets/{token}\n"),
        format!("cat /Users/sam/Library/Caches/{token}/key\n"),
        format!("/var/folders/2s/h6hvcg6x4mq0tg5m5bkhyzb40000gn/T/{token}\n"),
    ] {
        let out = scan_at(&text, Sensitivity::Balanced);
        assert_eq!(out.findings.len(), 1, "{text}: {:?}", out.findings);
        let f = &out.findings[0];
        assert!(text[f.start..f.end].contains(token), "{text}: {f:?}");
    }
}

/// A secret with slashes in it is not read as folder names, with or without folders in
/// front: its parts are short enough to pass alone, together they are found whole.
#[test]
fn secret_with_slashes_stays_one_finding() {
    for (text, secret) in [
        (
            "Zq81mXv0pLr3/TfK9wQe2Lm4x/Rk7Vt2pQ9Lm4xZw8\n",
            "Zq81mXv0pLr3/TfK9wQe2Lm4x/Rk7Vt2pQ9Lm4xZw8",
        ),
        (
            "keys/prod/Zq81mXv0pLr3/TfK9wQe2Lm4x/Rk7Vt2pQ9Lm4xZw8\n",
            "Zq81mXv0pLr3/TfK9wQe2Lm4x/Rk7Vt2pQ9Lm4xZw8",
        ),
        (
            "/etc/keys/Zq81mXv0pLr3TfK9wQe2Lm/4x\n",
            "Zq81mXv0pLr3TfK9wQe2Lm/4x",
        ),
        // No folder names: not a path, so the two-character part does not split it.
        (
            "Zq81mXv0pLr3TfK9wQ/e2/Lm4xRk7Vt2pQ9Lm4x\n",
            "Zq81mXv0pLr3TfK9wQ/e2/Lm4xRk7Vt2pQ9Lm4x",
        ),
    ] {
        let out = scan_at(text, Sensitivity::Balanced);
        assert_eq!(out.findings.len(), 1, "{text}: {:?}", out.findings);
        let f = &out.findings[0];
        assert!(text[f.start..f.end].contains(secret), "{text}: {f:?}");
    }
}
