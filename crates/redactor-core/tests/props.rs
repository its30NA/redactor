//! Property tests: random mixes of key names, separators, values, quotes, line
//! ends, emoji, hidden characters and placeholders.
//!
//! 5 000 cases by default; `PROPTEST_CASES` overrides. Checked for every input: no panic,
//! findings and placeholders on char boundaries, no planted secret left in the output, and
//! rescanning the output changes nothing.

mod support;

use proptest::prelude::*;
use redactor_core::{Engine, EngineConfig, ScanOutcome, ScanRequest, Sensitivity};
use support::gen::*;

/// A piece of the input. `Secret` pieces carry a value the engine must redact.
#[derive(Clone, Debug)]
enum Piece {
    Noise(String),
    Secret { text: String, value: String },
}

const KEYS: &[&str] = &[
    "password",
    "DB_PASSWORD",
    "api_key",
    "token",
    "secret",
    "passwd",
    "X-Api-Key",
    "Passwort",
    "client_secret",
    "access_token",
    "key",
    "user",
    "name",
    "max_tokens",
];
const SEPS: &[&str] = &[
    "=", ": ", " = ", ":", " ", "=\"", "\": \"", "?", "&", ";", ",", "--",
];
const WORDS: &[&str] = &[
    "hello",
    "user",
    "the",
    "is",
    "lautet",
    "est",
    "login",
    "failed",
    "for",
    "ok",
    "42",
    "2026-11-06",
    "v1.2.3",
    "true",
    "changeme",
    "${DB_PASSWORD}",
];
const NOISE: &[&str] = &[
    "\n",
    "\r\n",
    "\t",
    " ",
    "\"",
    "'",
    "😀",
    "👨\u{200D}👩\u{200D}👧",
    "🇩🇪",
    "\u{200B}",
    "\u{202E}",
    "\u{2066}",
    "\u{FEFF}",
    "\u{E0041}",
    "\u{00AD}",
    "مرحبا",
    "é",
    "ｐａｓｓ",
    "[REDACTED: Password]",
    "[REDACTED: Access Token #2]",
    "[REDACTED: API Key]",
    "#",
    "//",
];
/// Human passwords that the context engine must catch on a `KEY=value` line.
const PASSWORDS: &[&str] = &[
    "Hunter2-Fake-9x",
    "Tr0ub4dor&3",
    "Winter2024!",
    "Zebra!Lamp(42)",
    "p@ss^w0rd*7",
    "S3cr3t!Pass",
    "Kx9!mPq2vL",
];
/// Strong keys and the line shapes that always name a secret.
const SECRET_LINES: &[(&str, &str, &str)] = &[
    ("DB_PASSWORD=", "", ""),
    ("password: ", "", ""),
    ("password = ", "", ""),
    ("\"password\": \"", "\"", ""),
    ("API_TOKEN=", "", ""),
    ("--password ", "", ""),
    ("export SECRET_KEY='", "'", ""),
];

fn noise() -> impl Strategy<Value = Piece> {
    prop_oneof![
        prop::sample::select(KEYS).prop_map(|s| Piece::Noise(s.to_string())),
        prop::sample::select(SEPS).prop_map(|s| Piece::Noise(s.to_string())),
        prop::sample::select(WORDS).prop_map(|s| Piece::Noise(s.to_string())),
        prop::sample::select(NOISE).prop_map(|s| Piece::Noise(s.to_string())),
        any::<char>().prop_map(|c| Piece::Noise(c.to_string())),
    ]
}

/// A hidden character that may sit inside a value without changing it.
const INSIDE: &[&str] = &["", "", "", "\u{200B}", "\u{2066}", "\u{FEFF}"];

fn secret() -> impl Strategy<Value = Piece> {
    let line = (
        prop::sample::select(SECRET_LINES),
        prop::sample::select(PASSWORDS),
        prop::sample::select(INSIDE),
        any::<prop::sample::Index>(),
        prop::bool::ANY,
    )
        .prop_map(|((pre, post, _), pw, hidden, at, crlf)| {
            let cut = at.index(pw.len() - 1) + 1;
            let shown = format!("{}{hidden}{}", &pw[..cut], &pw[cut..]);
            let nl = if crlf { "\r\n" } else { "\n" };
            Piece::Secret {
                text: format!("{nl}{pre}{shown}{post}{nl}"),
                value: pw.to_string(),
            }
        });
    let token = (any::<u64>(), 0usize..6).prop_map(|(seed, kind)| {
        let mut g = Gen::new(seed);
        let t = match kind {
            0 => g.github("ghp_"),
            1 => format!("sk-proj-{}", g.alnum(60)),
            2 => g.jwt(),
            3 => format!("AKIA{}", g.from(B32, 16)),
            4 => format!("xoxb-{}-{}-{}", g.digits(12), g.digits(13), g.alnum(24)),
            _ => format!("glpat-{}", g.alnum(20)),
        };
        Piece::Secret {
            text: format!(" {t} "),
            value: t,
        }
    });
    prop_oneof![line, token]
}

fn input() -> impl Strategy<Value = Vec<Piece>> {
    prop::collection::vec(prop_oneof![4 => noise(), 1 => secret()], 0..40)
}

fn strip_hidden(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !matches!(c,
                '\u{200B}'..='\u{200F}'
                | '\u{2060}'..='\u{2064}'
                | '\u{FEFF}'
                | '\u{00AD}'
                | '\u{180E}'
                | '\u{034F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{E0000}'..='\u{E007F}')
        })
        .collect()
}

fn check_spans(text: &str, out: &ScanOutcome) -> Result<(), TestCaseError> {
    for f in &out.findings {
        prop_assert!(f.start < f.end && f.end <= text.len());
        prop_assert!(text.is_char_boundary(f.start) && text.is_char_boundary(f.end));
    }
    for w in out.findings.windows(2) {
        prop_assert!(w[0].end <= w[1].start, "overlapping findings");
    }
    for s in &out.output_spans {
        prop_assert!(s.end <= out.output.len());
        prop_assert!(out.output.is_char_boundary(s.start) && out.output.is_char_boundary(s.end));
        prop_assert_eq!(
            &out.output[s.start..s.end],
            &out.findings[s.finding].placeholder
        );
    }
    Ok(())
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases(),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn scan_properties(pieces in input(), paranoid in prop::bool::ANY) {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let text: String = pieces
            .iter()
            .map(|p| match p {
                Piece::Noise(s) => s.as_str(),
                Piece::Secret { text, .. } => text.as_str(),
            })
            .collect();
        let req = ScanRequest {
            sensitivity: Some(if paranoid { Sensitivity::Paranoid } else { Sensitivity::Balanced }),
            ..ScanRequest::default()
        };
        let out = engine.scan(&text, &req);
        check_spans(&text, &out)?;
        prop_assert!(out.verified, "verify cap hit");

        let visible = strip_hidden(&out.output);
        for p in &pieces {
            if let Piece::Secret { value, .. } = p {
                prop_assert!(!visible.contains(value.as_str()), "{value:?} left in {:?}", out.output);
            }
        }

        let again = engine.scan(&out.output, &req);
        prop_assert!(
            again.findings.is_empty() && again.output == out.output,
            "rescan of {:?} found {:?}",
            out.output,
            again.findings.iter().map(|f| &out.output[f.start..f.end]).collect::<Vec<_>>()
        );
    }
}

/// Keys and commands of the context engine, each followed by text that runs past its
/// windows.
const CONTEXT_PREFIXES: &[&str] = &[
    "curl -u admin:",
    "docker run ",
    "docker login -u me -p ",
    "zip -P ",
    "mysql -p",
    "sqlplus scott/",
    "net use Z: ",
    "password=",
    "DB_PASSWORD=\"",
    "password: ",
    "password = \"",
    "\"password\": \"",
    "--password ",
    "--password=",
    "level=warn password=",
    "<password>",
    "the password is ",
    "密码是",
    "variable \"db_password\" {\n  default = \"",
];

/// Arbitrary Unicode with the characters the parsers stop at mixed in, up to 1 500 chars.
fn unicode_tail() -> impl Strategy<Value = String> {
    let c = prop_oneof![
        4 => any::<char>(),
        1 => prop::sample::select(vec![' ', '\n', '"', '\'', '=', ':', '-', '&', ';', '\\', '`']),
    ];
    prop::collection::vec(c, 0..1500).prop_map(|v| v.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases() / 5,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// No panic and findings on char boundaries, whatever Unicode follows a key.
    #[test]
    fn context_prefixes_with_unicode(
        prefix in prop::sample::select(CONTEXT_PREFIXES),
        pad in 0usize..4,
        tail in unicode_tail(),
    ) {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let text = format!("{prefix}{}{tail}", "a".repeat(pad));
        let out = engine.scan(&text, &ScanRequest::default());
        check_spans(&text, &out)?;
    }
}
