//! Encoded copies of a found value: base64 (both alphabets, padded or not, alone or inside a
//! longer blob), hex, percent and JSON escapes are redacted with the value's placeholder.

mod support;

use redactor_core::{Engine, EngineConfig, ScanOutcome, ScanRequest, Source};
use support::gen::*;

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scan(text: &str) -> ScanOutcome {
    engine().scan(text, &ScanRequest::default())
}

/// A password-like value with a symbol in it (so percent encoding has to escape it).
fn value(g: &mut Gen) -> String {
    format!("{}!{}", g.alnum(8), g.alnum(6))
}

fn hex(s: &str, upper: bool) -> String {
    s.bytes()
        .map(|b| {
            if upper {
                format!("{b:02X}")
            } else {
                format!("{b:02x}")
            }
        })
        .collect()
}

/// `DB_PASSWORD=<v>`, then one line per `(prefix, encoded copy, suffix)`. In each line the
/// part of the copy that carries the value is redacted with the value's placeholder (for a
/// copy inside a longer base64 blob, the rest of the blob stays), with the value's key, and
/// a rescan finds nothing.
fn assert_copies_redacted(v: &str, lines: &[(&str, String, &str)]) {
    let mut text = format!("DB_PASSWORD={v}\n");
    for (pre, enc, post) in lines {
        text.push_str(&format!("{pre}{enc}{post}\n"));
    }
    let out = scan(&text);
    assert!(out.verified);
    let got: Vec<&str> = out.output.lines().collect();
    assert_eq!(got[0], "DB_PASSWORD=[REDACTED: Password]");
    for (line, (pre, enc, post)) in got[1..].iter().zip(lines) {
        let (left, right) = line
            .split_once("[REDACTED: Password]")
            .unwrap_or_else(|| panic!("not redacted: {line}"));
        assert!(left.starts_with(pre) && right.ends_with(post), "{line}");
        let (kept_head, kept_tail) = (&left[pre.len()..], &right[..right.len() - post.len()]);
        assert!(
            enc.starts_with(kept_head) && enc.ends_with(kept_tail),
            "{line}"
        );
    }
    let key = out.findings[0].value_key;
    assert!(out.findings.iter().all(|f| f.value_key == key));
    assert!(out.findings[1..]
        .iter()
        .all(|f| f.source == Source::Propagated));
    assert_eq!(out.findings.len(), lines.len() + 1);
    let again = scan(&out.output);
    assert!(again.findings.is_empty(), "rescan: {:?}", again.findings);
    assert_eq!(again.output, out.output);
}

#[test]
fn base64_copies() {
    let mut g = Gen::named("encoded base64");
    let v = value(&mut g);
    let std = b64_encode(v.as_bytes());
    let url = b64url_encode(v.as_bytes());
    assert_copies_redacted(
        &v,
        &[
            ("backup blob: ", std.clone(), ""),
            ("unpadded: ", std.trim_end_matches('=').to_string(), " ok"),
            ("url-safe: ", url, ""),
            // Inside longer blobs, at every alignment.
            (
                "note: ",
                b64_encode(format!("backup {v} end").as_bytes()),
                "",
            ),
            (
                "doc: ",
                b64_encode(format!("{{\"k\": [\"{v}\"]}}").as_bytes()),
                "",
            ),
            ("two: ", b64_encode(format!("ab {v}").as_bytes()), ""),
            ("path: https://x.example/cb/", std, ""),
        ],
    );
}

#[test]
fn hex_percent_and_json_copies() {
    let mut g = Gen::named("encoded other");
    let v = value(&mut g);
    let (head, tail) = v.split_once('!').unwrap();
    assert_copies_redacted(
        &v,
        &[
            ("hex: ", hex(&v, false), ""),
            ("HEX: ", hex(&v, true), ""),
            ("link: https://x.example/cb?q=", percent_encode(&v), "&x=1"),
            ("json: {\"p\": \"", format!("{head}\\u0021{tail}"), "\"}"),
        ],
    );
}

/// Two encodings deep: base64 of a JSON document that escapes the value.
#[test]
fn nested_copy() {
    let mut g = Gen::named("encoded nested");
    let v = value(&mut g);
    let (head, tail) = v.split_once('!').unwrap();
    let doc = format!("{{\"p\": \"{head}\\u0021{tail}\"}}");
    assert_copies_redacted(&v, &[("blob: ", b64_encode(doc.as_bytes()), "")]);
}

/// A value written out plainly inside an escaped JSON string is redacted alone, not with the
/// whole string; short values glued to letters inside a decoded blob do not count; unrelated
/// blobs are left alone.
#[test]
fn precision() {
    let mut g = Gen::named("encoded precision");
    let v = value(&mut g);
    let text = format!("DB_PASSWORD={v}\nlog: \"first line\\nthen {v} again\"\n");
    let out = scan(&text);
    assert_eq!(
        out.output,
        "DB_PASSWORD=[REDACTED: Password]\nlog: \"first line\\nthen [REDACTED: Password] again\"\n"
    );

    let short = format!("{}#{}", g.alnum(4), g.alnum(3));
    let glued = b64_encode(format!("x{short}y").as_bytes());
    let unrelated = b64_encode(format!("backup of {} done", g.alnum(12)).as_bytes());
    let text = format!("DB_PASSWORD={short}\nblob: {glued}\nother: {unrelated}\n");
    let out = scan(&text);
    assert_eq!(out.findings.len(), 1, "{:?}", out.findings);
    assert!(out.output.contains(&glued) && out.output.contains(&unrelated));
}

/// Base64 wrapped across lines (`base64 -w 76`, audit 2026-11-10, finding 1): a copy of the
/// value split by a line break is redacted with the value's placeholder, every character that
/// carries a bit of it on both lines included. Nothing outside the blob is touched (the rest of
/// a line, misaligned base64, may read as a random string on the rescan).
#[test]
fn wrapped_base64_copy() {
    let mut g = Gen::named("encoded wrapped");
    let v = value(&mut g);
    // The value starts at byte 47 to 56 of the document, so its characters run from about 63
    // to 95 and the break after 76 always falls inside them.
    for pad in 0..10 {
        let doc = format!(
            "notes from the migration, kept{}\nthe old one was {v} until today\nend\n",
            "x".repeat(pad)
        );
        let blob = b64_encode(doc.as_bytes());
        let lines: Vec<&str> = blob
            .as_bytes()
            .chunks(76)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        let head = format!("DB_PASSWORD={v}\nbackup:\n");
        let text = format!("{head}{}\n", lines.join("\n"));
        let out = scan(&text);
        assert!(out.verified);
        let key = out.findings[0].value_key;
        // The blob characters that carry a bit of the value, as offsets into `text`.
        let at = doc.find(&v).unwrap();
        let (a, b) = (at, at + v.len());
        let first = 4 * (a / 3) + a % 3;
        let last = 4 * ((b - 1) / 3) + (b - 1) % 3 + 1;
        assert!(
            first < 76 && last >= 76,
            "pad {pad}: the value does not straddle"
        );
        let offset = |j: usize| head.len() + j + j / 76;
        let copy = out
            .findings
            .iter()
            .find(|f| f.start >= head.len() && f.source == Source::Propagated)
            .unwrap_or_else(|| panic!("pad {pad}: not redacted: {}", out.output));
        assert_eq!(copy.value_key, key, "pad {pad}");
        assert!(
            copy.start <= offset(first) && offset(last) < copy.end,
            "pad {pad}: {:?} vs {}..={}",
            (copy.start, copy.end),
            offset(first),
            offset(last)
        );
        assert!(text[copy.start..copy.end].contains('\n'), "pad {pad}");
        assert!(
            out.findings[1..].iter().all(|f| f.start >= head.len()),
            "pad {pad}: {:?}",
            out.findings
        );
        let again = scan(&out.output);
        assert!(again.findings.is_empty(), "rescan: {:?}", again.findings);
    }
}

/// Always-redact terms seed the encoded-copy search even when the text holds no plain copy:
/// a term glued to a letter or two inside a short base64 payload (no word boundary, so the
/// decode layer's term search misses it) is still found (review of the 2026-11-10 audit
/// fixes, finding 6 incomplete).
#[test]
fn terms_seed_the_encoded_copy_search() {
    let engine = Engine::new(EngineConfig {
        terms: vec!["correct-horse-battery".into(), "Kx9mPq2vLz-Hunter".into()],
        ..EngineConfig::default()
    })
    .unwrap();
    for payload in [
        "xcorrect-horse-battery",
        "correct-horse-batteryx",
        "xycorrect-horse-battery",
        "xCORRECT-HORSE-BATTERY",
        "xKx9mPq2vLz-Hunter",
        "xyKx9mPq2vLz-Hunter",
    ] {
        for blob in [b64_encode(payload.as_bytes()), hex(payload, false)] {
            let text = format!("k {blob}\n");
            let out = engine.scan(&text, &ScanRequest::default());
            assert!(
                !out.output.contains(&blob[4..blob.len() - 4]),
                "{payload} as {blob}: {}",
                out.output
            );
            assert!(
                out.findings.iter().any(|f| f.label.contains("Custom Term")),
                "{payload}: {:?}",
                out.findings
            );
        }
    }
    // A payload with only part of a term costs no finding.
    let blob = b64_encode(b"xcorrect-horse-batter!");
    let out = engine.scan(&format!("k {blob}\n"), &ScanRequest::default());
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

/// Reports every occurrence of one fixed string (stand-in for the app's remembered secrets).
struct Fixed(&'static str);

impl redactor_core::SecretMatcher for Fixed {
    fn find(&self, text: &str) -> Vec<redactor_core::MatcherHit> {
        text.match_indices(self.0)
            .map(|(i, m)| redactor_core::MatcherHit {
                start: i,
                end: i + m.len(),
                label: "Known Secret".into(),
            })
            .collect()
    }
}

/// A remembered secret glued to a letter inside a short base64 payload, with no plain copy:
/// the decode layer's matcher search finds it (the matcher has no word boundary). Not covered
/// yet: hex shorter than the decode layer's 32 digits, or hex of bytes that are not text,
/// since the engine cannot seed the encoded-copy search with values it does not hold.
#[test]
fn matcher_values_glued_in_short_base64() {
    for (value, payload) in [
        ("Kx9mPq2vLz-Hunter", "xKx9mPq2vLz-Hunter"),
        ("Kx9mPq2vLz-Hunter", "xyKx9mPq2vLz-Hunter"),
        ("Kx9mPq2vLz-Hunter", "Kx9mPq2vLz-Hunterx"),
        ("Plum-tree47x", "qPlum-tree47x"),
        ("Plum-tree47x", "\u{1}\u{2}\u{3}Plum-tree47x\u{4}"),
    ] {
        let engine = engine().with_matcher(std::sync::Arc::new(Fixed(value)));
        let blob = b64_encode(payload.as_bytes());
        let out = engine.scan(&format!("k {blob}\n"), &ScanRequest::default());
        assert!(
            !out.output.contains(&blob[4..blob.len() - 4]),
            "{payload:?} as {blob}: {}",
            out.output
        );
    }
}
