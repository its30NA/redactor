//! Engine pipeline glue: shadow text, manual ranges, matcher hits, keep, warnings.

use std::sync::Arc;

use redactor_core::{
    Engine, EngineConfig, MatcherHit, ScanRequest, SecretMatcher, Source, WarningKind,
};

/// Reports every occurrence of one fixed string (stand-in for the watchlist).
struct Fixed(&'static str);

impl SecretMatcher for Fixed {
    fn find(&self, text: &str) -> Vec<MatcherHit> {
        text.match_indices(self.0)
            .map(|(i, m)| MatcherHit {
                start: i,
                end: i + m.len(),
                label: "Known Secret".into(),
            })
            .collect()
    }
}

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

#[test]
fn manual_range_redacted() {
    let text = "hello world plain";
    let out = engine().scan(
        text,
        &ScanRequest {
            manual: &[(12, 17)],
            ..ScanRequest::default()
        },
    );
    assert_eq!(out.output, "hello world [REDACTED: Selected Text]");
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].source, Source::Manual);
    assert_eq!((out.findings[0].start, out.findings[0].end), (12, 17));
    let span = out.output_spans[0];
    assert_eq!(
        &out.output[span.start..span.end],
        "[REDACTED: Selected Text]"
    );
}

#[test]
fn manual_range_snaps_to_char_boundaries() {
    let text = "a😀b";
    // Byte 2 is inside the emoji: the range grows to cover it.
    let out = engine().scan(
        text,
        &ScanRequest {
            manual: &[(2, 3), (100, 200)],
            ..ScanRequest::default()
        },
    );
    assert_eq!(out.findings.len(), 1);
    assert_eq!((out.findings[0].start, out.findings[0].end), (1, 5));
}

#[test]
fn matcher_hits_survive_zero_width_split() {
    let engine = engine().with_matcher(Arc::new(Fixed("hunter2hunter2")));
    let text = "pw hunter2\u{200B}hunter2 and hunter2hunter2.";
    let out = engine.scan(text, &ScanRequest::default());
    assert_eq!(out.findings.len(), 2);
    assert!(out.findings.iter().all(|f| f.source == Source::Watchlist));
    // Same value (after normalization) -> same key -> same placeholder, no number.
    assert_eq!(out.findings[0].value_key, out.findings[1].value_key);
    assert_eq!(
        out.output,
        "pw [REDACTED: Known Secret] and [REDACTED: Known Secret]."
    );
    assert!(out
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::InvisibleChars && w.count == 1));
}

#[test]
fn keep_leaves_value_in_place() {
    let engine = engine().with_matcher(Arc::new(Fixed("hunter2hunter2")));
    let text = "a hunter2hunter2 b";
    let first = engine.scan(text, &ScanRequest::default());
    let key = first.findings[0].value_key;
    let second = engine.scan(
        text,
        &ScanRequest {
            keep: &[key],
            ..ScanRequest::default()
        },
    );
    assert_eq!(second.output, text);
    assert_eq!(second.findings.len(), 1);
    assert!(second.findings[0].kept);
    assert!(second.findings[0].placeholder.is_empty());
    assert!(second.output_spans.is_empty());
}

#[test]
fn value_keys_depend_on_session_key() {
    let a = Engine::new(EngineConfig::default())
        .unwrap()
        .with_matcher(Arc::new(Fixed("hunter2hunter2")));
    let b = Engine::new(EngineConfig::default())
        .unwrap()
        .with_matcher(Arc::new(Fixed("hunter2hunter2")));
    let ka = a.scan("hunter2hunter2", &ScanRequest::default()).findings[0].value_key;
    let kb = b.scan("hunter2hunter2", &ScanRequest::default()).findings[0].value_key;
    assert_ne!(ka, kb);
}

#[test]
fn hidden_characters_warned_and_stripped() {
    let text = "a\u{200B}b \u{202E}c\u{E0041} 👨\u{200D}👩";
    let out = engine().scan(text, &ScanRequest::default());
    assert_eq!(out.output, "ab c 👨\u{200D}👩");
    let count = |k| {
        out.warnings
            .iter()
            .find(|w| w.kind == k)
            .map_or(0, |w| w.count)
    };
    assert_eq!(count(WarningKind::InvisibleChars), 1);
    assert_eq!(count(WarningKind::BidiControls), 1);
    assert_eq!(count(WarningKind::TagChars), 1);

    let keep = Engine::new(EngineConfig {
        strip_invisible: false,
        ..EngineConfig::default()
    })
    .unwrap()
    .scan(text, &ScanRequest::default());
    assert_eq!(keep.output, text);
}

#[test]
fn invalid_config_is_rejected() {
    let bad = EngineConfig {
        allowlist: vec!["(unclosed".into()],
        ..EngineConfig::default()
    };
    assert!(Engine::new(bad).is_err());
    let bad = EngineConfig {
        template: String::new(),
        ..EngineConfig::default()
    };
    assert!(Engine::new(bad).is_err());
}

/// One value, one placeholder: the URL rule, the context engine and propagation name the same
/// password differently, and every occurrence renders with the strongest of those labels.
#[test]
fn one_value_one_label() {
    let text = "DATABASE_URL=postgres://app:Orchid-Lantern-42!@db.example:5432/app\n\
                login failed for user app with password Orchid-Lantern-42!\n\
                retry with Orchid-Lantern-42!\n";
    for pii in [false, true] {
        let out = engine().scan(
            text,
            &ScanRequest {
                pii: Some(pii),
                ..ScanRequest::default()
            },
        );
        let pw: Vec<_> = out
            .findings
            .iter()
            .filter(|f| &text[f.start..f.end] == "Orchid-Lantern-42!")
            .collect();
        assert_eq!(pw.len(), 3, "pii={pii}: {:?}", out.findings);
        for f in &pw {
            assert_eq!(f.label, "Password in URL", "pii={pii}");
            assert_eq!(f.placeholder, pw[0].placeholder, "pii={pii}");
        }
        assert_eq!(
            out.output.matches(pw[0].placeholder.as_str()).count(),
            3,
            "pii={pii}: {}",
            out.output
        );
        // Each finding reports the label it renders with.
        for s in &out.output_spans {
            let f = &out.findings[s.finding];
            assert_eq!(&out.output[s.start..s.end], f.placeholder);
            assert!(f.placeholder.contains(&f.label), "{f:?}");
        }
        // A rescan of the output finds nothing and changes nothing.
        let again = engine().scan(
            &out.output,
            &ScanRequest {
                pii: Some(pii),
                ..ScanRequest::default()
            },
        );
        assert!(again.findings.is_empty(), "pii={pii}: {:?}", again.findings);
        assert_eq!(again.output, out.output);
    }
}

/// Two different values under one label still get numbers of their own, in order of first
/// appearance, and each value keeps its number wherever it occurs.
#[test]
fn different_values_keep_their_numbers() {
    let text = "DATABASE_URL=postgres://app:Orchid-Lantern-42!@db.example:5432/app\n\
                CACHE_URL=redis://svc:Velvet-Harbor-77~@cache.example:6379\n\
                login failed with password Orchid-Lantern-42!\n\
                retry with Velvet-Harbor-77~\n";
    let out = engine().scan(text, &ScanRequest::default());
    let placeholder = |v: &str| -> Vec<String> {
        out.findings
            .iter()
            .filter(|f| &text[f.start..f.end] == v)
            .map(|f| f.placeholder.clone())
            .collect()
    };
    let a = placeholder("Orchid-Lantern-42!");
    let b = placeholder("Velvet-Harbor-77~");
    assert_eq!(a.len(), 2, "{:?}", out.findings);
    assert_eq!(b.len(), 2, "{:?}", out.findings);
    assert!(
        a.iter().all(|p| p == "[REDACTED: Password in URL #1]"),
        "{a:?}"
    );
    assert!(
        b.iter().all(|p| p == "[REDACTED: Password in URL #2]"),
        "{b:?}"
    );
}

fn engine_at(sensitivity: redactor_core::Sensitivity) -> Engine {
    Engine::new(EngineConfig {
        sensitivity,
        ..EngineConfig::default()
    })
    .unwrap()
}

/// A value that only looks like a variable, a template or a config path once its digits are
/// ignored is a password: the guard shortcuts do not throw it away, at any sensitivity.
#[test]
fn guard_keeps_values_with_mixed_in_digits() {
    use redactor_core::Sensitivity;
    let cases = [
        ("password=$ummer2024x", "$ummer2024x"),
        ("DB_PASS: $tr0ngPassw0rd", "$tr0ngPassw0rd"),
        ("password=Tr0ub4dor-here", "Tr0ub4dor-here"),
        ("password=your-Majesty-1982x", "your-Majesty-1982x"),
        ("secret=config.Zq81mXv0pLr3", "Zq81mXv0pLr3"),
        ("password=%Tgb6yhn7%", "Tgb6yhn7"),
    ];
    for s in [Sensitivity::Balanced, Sensitivity::Paranoid] {
        for (text, value) in cases {
            let out = engine_at(s).scan(text, &ScanRequest::default());
            assert!(
                !out.output.contains(value),
                "{s:?}: {text} -> {}",
                out.output
            );
        }
    }
}

/// Real variable references, templates and plain config words stay untouched.
#[test]
fn guard_still_skips_references_and_config_words() {
    use redactor_core::Sensitivity;
    for s in [Sensitivity::Balanced, Sensitivity::Paranoid] {
        for text in [
            "password=$DB_PASSWORD",
            "password=$db_password",
            "password=${DB_PASSWORD}",
            "password=${S3_SECRET_V2}",
            "password=%DB_PASS%",
            "password=$env:API_KEY",
            "secret=config.db_password",
            "secret=settings.oauth2.client_secret",
            "api_key=process.env.API_KEY_V2",
            "password=your_password_here",
            "password={{ secrets.db_pass }}",
        ] {
            let out = engine_at(s).scan(text, &ScanRequest::default());
            assert_eq!(out.output, text, "{s:?}: {:?}", out.findings);
        }
    }
}

fn engine_with_template(template: &str) -> Engine {
    Engine::new(EngineConfig {
        template: template.into(),
        ..EngineConfig::default()
    })
    .unwrap()
}

/// Text framed like the template is not a placeholder unless its label is one the engine
/// emits: the values inside are redacted.
#[test]
fn template_framed_text_is_not_a_placeholder() {
    let cases = [
        (
            "\"{label}\"",
            "password = \"Winter-Maple-2024!\"\n",
            &["Winter-Maple-2024!"][..],
        ),
        (
            "<{label}{suffix}>",
            "<add key=\"DbPassword\" value=\"Tulip-Harbor-91x\" />\n",
            &["Tulip-Harbor-91x"][..],
        ),
        (
            "[{label}]",
            "{\"db\": {\"password\": \"Hunter2-Fake-9x\"}, \"keys\": [\"Zq81mXv0pLr3TfK9wQe2Lm\"]}\n",
            &["Hunter2-Fake-9x", "Zq81mXv0pLr3TfK9wQe2Lm"][..],
        ),
        (
            redactor_core::DEFAULT_TEMPLATE,
            "note [REDACTED: see password=Hunter2-Fake-9x] end\n",
            &["Hunter2-Fake-9x"][..],
        ),
    ];
    for (template, text, values) in cases {
        let out = engine_with_template(template).scan(text, &ScanRequest::default());
        for v in values {
            assert!(
                !out.output.contains(v),
                "{template}: {v} survives in {}",
                out.output
            );
        }
    }
}

/// Real placeholders of a previous run stay protected, whatever the template or style:
/// scrubbing scrubbed output changes nothing.
#[test]
fn scrubbing_scrubbed_output_changes_nothing() {
    use redactor_core::PlaceholderStyle;
    let text = "DB_PASSWORD=Orchid-Lantern-42!\n\
                API_PASSWORD=Velvet-Harbor-77~\n\
                url=postgres://app:Juniper-Coast-55%@db.example:5432/app\n\
                blob=cGFzc3dvcmQ9SHVudGVyMi1GYWtlLTl4\n\
                seen Orchid-Lantern-42! again\n";
    for template in [
        redactor_core::DEFAULT_TEMPLATE,
        "\"{label}\"",
        "<{label}{suffix}>",
        "[{label}]",
        "{{{label}{suffix}}}",
        "***{label}{suffix}***",
    ] {
        for style in [PlaceholderStyle::Labeled, PlaceholderStyle::Generic] {
            let engine = Engine::new(EngineConfig {
                template: template.into(),
                placeholder: style,
                ..EngineConfig::default()
            })
            .unwrap();
            let first = engine.scan(text, &ScanRequest::default());
            assert!(
                first.findings.len() >= 4,
                "{template} {style:?}: {}",
                first.output
            );
            let second = engine.scan(&first.output, &ScanRequest::default());
            assert_eq!(
                second.output, first.output,
                "{template} {style:?}: {:?}",
                second.findings
            );
            assert!(second.findings.is_empty(), "{template} {style:?}");
        }
    }
}

/// A watchlist with user-chosen labels, which it lists.
struct Labeled(&'static str, &'static str);

impl SecretMatcher for Labeled {
    fn find(&self, text: &str) -> Vec<MatcherHit> {
        text.match_indices(self.0)
            .map(|(i, m)| MatcherHit {
                start: i,
                end: i + m.len(),
                label: self.1.into(),
            })
            .collect()
    }

    fn labels(&self) -> Vec<String> {
        vec![self.1.into()]
    }
}

/// A matcher's own labels count as known labels, even one shaped like a password.
#[test]
fn matcher_labels_are_known_placeholders() {
    let engine = engine_with_template("<{label}>")
        .with_matcher(Arc::new(Labeled("Plum-tree47", "Vault-Key-2024x")));
    let first = engine.scan(
        "password=Plum-tree47
",
        &ScanRequest::default(),
    );
    assert_eq!(first.output, "password=<Vault-Key-2024x>\n");
    let second = engine.scan(&first.output, &ScanRequest::default());
    assert_eq!(second.output, first.output, "{:?}", second.findings);
    assert!(second.findings.is_empty(), "{:?}", second.findings);
    // Without the matcher the label is ordinary text in a password field.
    let bare = engine_with_template("<{label}>").scan(&first.output, &ScanRequest::default());
    assert_ne!(bare.output, first.output);
}

/// Every Default_Ignorable_Code_Point (and the interlinear annotation controls) is removed
/// before detection: a token split by one is still found, and the placeholder covers the
/// invisible character too (2026-11-10 audit, finding 5).
#[test]
fn default_ignorables_do_not_hide_a_token() {
    let head = concat!("gl", "pat-Qz7Wm3X");
    let tail = "k9Pld2VteR5yN";
    for ch in [
        '\u{FE0F}',
        '\u{206A}',
        '\u{FFF9}',
        '\u{17B4}',
        '\u{1D173}',
        '\u{200B}',
        '\u{2065}',
        '\u{1BCA0}',
        '\u{E0FFF}',
    ] {
        let text = format!("see {head}{ch}{tail} here\n");
        let out =
            engine_at(redactor_core::Sensitivity::Paranoid).scan(&text, &ScanRequest::default());
        assert_eq!(
            out.output, "see [REDACTED: GitLab Token] here\n",
            "U+{:04X}",
            ch as u32
        );
        let keep = Engine::new(EngineConfig {
            strip_invisible: false,
            sensitivity: redactor_core::Sensitivity::Paranoid,
            ..EngineConfig::default()
        })
        .unwrap()
        .scan(&text, &ScanRequest::default());
        assert_eq!(
            keep.output, "see [REDACTED: GitLab Token] here\n",
            "placeholder covers U+{:04X}",
            ch as u32
        );
    }
}

/// A copy of a found password split by an invisible character is redacted too.
#[test]
fn copy_split_by_default_ignorable_is_redacted() {
    let text = "password=Hunter2-Fake-9x\nlogin failed for Hunter2\u{206A}-Fake-9x\n";
    let out = engine().scan(text, &ScanRequest::default());
    assert!(!out.output.contains("Hunter2"), "{}", out.output);
    assert!(!out.output.contains("Fake-9x"), "{}", out.output);
}

/// U+FE0F stays only right after an emoji (or before a keycap mark).
#[test]
fn emoji_presentation_selector_kept_after_emoji_only() {
    for text in ["I \u{2764}\u{FE0F} it", "key 1\u{FE0F}\u{20E3} ok"] {
        assert_eq!(engine().scan(text, &ScanRequest::default()).output, text);
    }
    let out = engine().scan("ab\u{FE0F}cd", &ScanRequest::default());
    assert_eq!(out.output, "abcd");
}

/// A value the matcher finds gets the encoded-copy search too, as a found password does
/// (2026-11-10 audit, finding 6).
#[test]
fn matcher_values_get_encoded_copy_search() {
    let engine = engine().with_matcher(Arc::new(Fixed("Kx9!mPq2")));
    let text = r#"note Kx9!mPq2 here; b64 S3g5IW1QcTI= hex 4b7839216d507132 pct Kx9%21mPq2 json "Kx9!mPq2""#;
    let out = engine.scan(text, &ScanRequest::default());
    for copy in [
        "Kx9!",
        "S3g5IW1QcTI",
        "4b7839216d507132",
        "Kx9%21",
        "Kx9\\u0021",
    ] {
        assert!(!out.output.contains(copy), "{copy} left: {}", out.output);
    }
    assert_eq!(
        out.output.matches("[REDACTED: ").count(),
        5,
        "{}",
        out.output
    );
}

/// Always-redact terms get the encoded-copy search too; terms under the propagation minimum
/// (six characters) do not, as no other value does.
#[test]
fn term_values_get_encoded_copy_search() {
    let engine = Engine::new(EngineConfig {
        terms: vec!["bluebird-labs".into(), "Zorvex".into()],
        ..EngineConfig::default()
    })
    .unwrap();
    let text = "from bluebird-labs: b64 Ymx1ZWJpcmQtbGFicw== hex 626c7565626972642d6c616273\n\
                Zorvex b64 Wm9ydmV4 hex 5a6f72766578\n";
    let out = engine.scan(text, &ScanRequest::default());
    for copy in [
        "bluebird",
        "Ymx1ZWJpcmQtbGFicw",
        "626c7565626972642d6c616273",
        "Zorvex",
        "Wm9ydmV4",
        "5a6f72766578",
    ] {
        assert!(!out.output.contains(copy), "{copy} left: {}", out.output);
    }
}

/// An escape JSON does not know (`\x`) does not stop the `\u` escapes after it from being
/// decoded (2026-11-10 audit, finding 9).
#[test]
fn unknown_escape_does_not_hide_a_later_unicode_escape() {
    let text = concat!(
        r"note: C:\\dir\x gl",
        r"pat\u002dQz7Wm3Xk9Pld2VteR5yN",
        "\n"
    );
    let out = engine().scan(text, &ScanRequest::default());
    assert!(
        !out.output.contains("Qz7Wm3Xk9Pld2VteR5yN"),
        "{}",
        out.output
    );
}

/// The guards take a value for an earlier placeholder only when the engine would (a label it
/// emits): `[REDACTED` in front of a value, or a made-up label, does not hide it (batch 2
/// leftover of the 2026-11-10 audit).
#[test]
fn guard_takes_only_real_placeholders_as_redacted() {
    for (text, value) in [
        ("password=[REDACTED-but-real-Hunter2x\n", "Hunter2x"),
        ("password=[REDACTED]Hunter2-Fake-9x\n", "Hunter2-Fake-9x"),
        (
            "password: \"[REDACTED: Orchid Lantern 42x]\"\n",
            "Orchid Lantern 42x",
        ),
        (
            "DB_PASSWORD=[REDACTED:Velvet-Harbor-77]\n",
            "Velvet-Harbor-77",
        ),
        // A key and value inside a made-up label.
        (
            "note [REDACTED: password=Hunter2-Fake-9x]\n",
            "Hunter2-Fake-9x",
        ),
        (
            "x [REDACTED: Password: Tulip-Harbor-91x]\n",
            "Tulip-Harbor-91x",
        ),
    ] {
        let out = engine().scan(text, &ScanRequest::default());
        assert!(!out.output.contains(value), "{text:?} -> {:?}", out.output);
    }
    // Real placeholders, with a number or the encoded prefix, stay as they are.
    for text in [
        "password=[REDACTED: Password #1]\n",
        "token: [REDACTED: Encoded Password #2]\n",
        "DB_PASSWORD=\"[REDACTED: Password]\"\n",
        "secret = [REDACTED: Secret] (rotated yesterday)\n",
    ] {
        let out = engine().scan(text, &ScanRequest::default());
        assert_eq!(out.output, text, "{:?}", out.findings);
    }
}
