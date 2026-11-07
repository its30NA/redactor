//! The engine bridge. The mapping itself lives in `redactor-wire`, shared with the
//! `scrub` CLI; this module re-exports it under the name the rest of the shell uses.

pub use redactor_wire::*;

#[cfg(test)]
mod tests {
    use super::*;
    use redactor_core::{Engine, EngineConfig, Sensitivity};

    fn engine() -> Engine {
        Engine::new(EngineConfig::default()).unwrap()
    }

    fn fake_password() -> String {
        ["Plum", "tree", "47", "-", "Kite"].concat()
    }

    /// A random-looking base62 token, generated at test time (SplitMix64, fixed seed).
    fn random_token(n: usize) -> String {
        const A: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let mut x: u64 = 0x5EED_1234;
        (0..n)
            .map(|_| {
                x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = x;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                A[((z ^ (z >> 31)) % 62) as usize] as char
            })
            .collect()
    }

    fn request(text: &str) -> ScanRequestIn {
        plain_request(text.to_string(), Sensitivity::Balanced, false, true)
    }

    fn utf16_index(text: &str, byte: usize) -> usize {
        text[..byte].encode_utf16().count()
    }

    #[test]
    fn utf16_offsets_with_emoji_and_crlf() {
        let pw = fake_password();
        let text = format!("😀👨‍👩‍👧 Grüße\r\n\tline two\r\nDB_PASSWORD={pw}\r\nmore 𝄞 text\r\n");
        let resp = scan(&engine(), &request(&text));
        assert_eq!(resp.findings.len(), 1, "{:?}", resp.findings);
        let f = &resp.findings[0];
        let byte = text.find(&pw).unwrap();
        assert_eq!(f.start, utf16_index(&text, byte));
        assert_eq!(f.end, utf16_index(&text, byte + pw.len()));
        assert_eq!(f.line, 3);
        // The UTF-16 slice of the input is exactly the value, without the \r.
        let units: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&units[f.start..f.end]).unwrap(), pw);
        // Output span: the placeholder, in UTF-16 units of the output.
        let span = &resp.output_spans[0];
        assert_eq!(span.finding_id, 0);
        let out_units: Vec<u16> = resp.output.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&out_units[span.start..span.end]).unwrap(),
            f.placeholder
        );
        assert_eq!(resp.stats.chars, units.len());
        assert_eq!(resp.stats.by_tier.certain + resp.stats.by_tier.likely, 1);
    }

    #[test]
    fn manual_ranges_map_from_utf16() {
        let text = "😀 keep this WORD out";
        let start = utf16_index(text, text.find("WORD").unwrap());
        let mut req = request(text);
        req.manual = vec![RangeIn {
            start,
            end: start + 4,
        }];
        let resp = scan(&engine(), &req);
        assert_eq!(resp.findings.len(), 1);
        assert_eq!(
            (resp.findings[0].start, resp.findings[0].end),
            (start, start + 4)
        );
        assert!(!resp.output.contains("WORD"));
    }

    #[test]
    fn keep_by_value_key() {
        let pw = fake_password();
        let text = format!("password: {pw}");
        let e = engine();
        let first = scan(&e, &request(&text));
        let key = serde_json::to_value(&first.findings[0]).unwrap()["valueKey"]
            .as_str()
            .unwrap()
            .to_string();
        let mut req = request(&text);
        req.keep = vec![key, "not-a-key".into()];
        let kept = scan(&e, &req);
        assert!(kept.findings[0].kept);
        assert!(kept.output.contains(&pw));
        assert_eq!(kept.removed(), 0);
    }

    #[test]
    fn response_contains_no_secret_values() {
        let pw = fake_password();
        let token = random_token(40);
        let text = format!("DB_PASSWORD={pw}\nexport API_TOKEN={token}\n");
        let resp = scan(&engine(), &request(&text));
        assert!(resp.removed() >= 2, "{:?}", resp.findings);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains(&pw), "password leaked into the response");
        assert!(!json.contains(&token), "token leaked into the response");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        for f in v["findings"].as_array().unwrap() {
            let mut keys: Vec<_> = f.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            assert_eq!(
                keys,
                [
                    "confidence",
                    "detector",
                    "end",
                    "id",
                    "kept",
                    "kind",
                    "label",
                    "line",
                    "placeholder",
                    "reason",
                    "source",
                    "start",
                    "tier",
                    "valueKey"
                ]
            );
        }
    }

    #[test]
    fn input_too_large_warning() {
        let big = "a".repeat(MAX_INPUT_BYTES + 1);
        let resp = scan(&engine(), &request(&big));
        assert!(resp.output.is_empty());
        assert!(resp.findings.is_empty());
        assert!(!resp.verified);
        assert_eq!(resp.warnings.len(), 1);
        assert_eq!(resp.warnings[0].kind, WarningKindOut::InputTooLarge);
        assert_eq!(resp.warnings[0].count, MAX_INPUT_BYTES + 1);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["warnings"][0]["kind"], "inputTooLarge");
        // Exactly at the cap still scans.
        let edge = "a".repeat(MAX_INPUT_BYTES);
        let resp = scan(&engine(), &request(&edge));
        assert!(resp.warnings.is_empty());
        assert_eq!(resp.output.len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn request_json_shape_and_zeroize() {
        let json = r#"{"text":"hello","sensitivity":"paranoid","pii":true,"stripInvisible":false,
            "keep":[],"manual":[{"start":0,"end":2}]}"#;
        let req: ScanRequestIn = serde_json::from_str(json).unwrap();
        assert_eq!(req.sensitivity, Sensitivity::Paranoid);
        assert!(req.pii && !req.strip_invisible);
        assert!(!format!("{req:?}").contains("hello"));
        let resp = scan(&engine(), &req);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["outputSpans"][0]["findingId"], 0);
        assert!(v["stats"]["byTier"].is_object());
        assert!(v["stats"]["byLabel"].is_array());
    }

    #[test]
    fn by_label_sorts_by_count() {
        let l = by_label(["A", "B", "B", "C", "A", "B"].into_iter());
        assert_eq!(
            l,
            [
                LabelCount {
                    label: "B".into(),
                    n: 3
                },
                LabelCount {
                    label: "A".into(),
                    n: 2
                },
                LabelCount {
                    label: "C".into(),
                    n: 1
                },
            ]
        );
    }
}
