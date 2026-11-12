//! The hotkey rules per OS, from the case table the UI's tests read too
//! (`tests/fixtures/hotkey_cases.json`), so the Rust and UI checks cannot drift apart.

use redactor_app::hotkey::Hotkey;
use redactor_app::os::Os;

#[derive(serde::Deserialize)]
struct Case {
    os: String,
    combo: String,
    expect: String,
}

#[test]
fn every_case_in_the_shared_table() {
    let raw = include_str!("fixtures/hotkey_cases.json");
    let cases: Vec<Case> = serde_json::from_str(raw).expect("valid case table");
    assert!(cases.len() > 50, "the table lost rows");
    for c in &cases {
        let os = Os::from_name(&c.os).unwrap_or_else(|| panic!("unknown os {:?}", c.os));
        // `expect` is a name when it is one this OS accepts as written, else a message.
        let want = match Hotkey::parse(&c.expect, os) {
            Ok(h) if h.name(os) == c.expect => Ok(c.expect.clone()),
            _ => Err(c.expect.clone()),
        };
        let got = Hotkey::parse(&c.combo, os)
            .map(|h| h.name(os))
            .map_err(|e| e.to_string());
        assert_eq!(got, want, "{} {:?}", c.os, c.combo);
    }
}

#[test]
fn os_names_are_the_ui_platform_names() {
    assert_eq!(Os::from_name("windows"), Some(Os::Windows));
    assert_eq!(Os::from_name("macos"), Some(Os::Mac));
    assert_eq!(Os::from_name("linux"), Some(Os::Linux));
    assert_eq!(Os::from_name("unsupported"), None);
}
