//! The shell's static hardening: Tauri config and capabilities, checked as files.

use std::collections::BTreeSet;

use redactor_app::ipc_names::COMMANDS;
use serde_json::Value;

fn read_json(rel: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The window controls the custom title bar needs, and event listening. Nothing else.
const CORE: &[&str] = &[
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-internal-toggle-maximize",
    "core:window:allow-close",
    "core:window:allow-is-maximized",
    "core:window:allow-start-dragging",
    "core:event:allow-listen",
    "core:event:allow-unlisten",
];

#[test]
fn capabilities_only_whitelisted_commands() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files, ["main.json"], "one capability file only");

    let cap = read_json("capabilities/main.json");
    assert_eq!(cap["windows"], serde_json::json!(["main"]));
    assert!(cap.get("remote").is_none(), "no remote URLs may use IPC");
    assert!(cap.get("webviews").is_none());
    assert!(cap.get("platforms").is_none());

    let granted: BTreeSet<String> = cap["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            p.as_str()
                .expect("permissions are plain identifiers, no scoped objects")
                .to_string()
        })
        .collect();
    let expected: BTreeSet<String> = COMMANDS
        .iter()
        .map(|c| format!("allow-{}", c.replace('_', "-")))
        .chain(CORE.iter().map(|s| s.to_string()))
        .collect();
    assert_eq!(granted, expected);
    for p in &granted {
        for banned in [
            "fs:",
            "shell:",
            "http:",
            "opener:",
            "updater:",
            "clipboard-manager:",
            ":default",
            "webview:",
            "app:",
        ] {
            assert!(!p.contains(banned), "{p} must not be granted");
        }
    }
    assert_eq!(COMMANDS.len(), 14, "the IPC contract has 14 commands");
}

#[test]
fn csp_has_no_unsafe_eval_or_remote() {
    let conf = read_json("tauri.conf.json");
    let csp = conf["app"]["security"]["csp"]
        .as_str()
        .expect("a CSP string");
    let directives: Vec<(&str, Vec<&str>)> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let mut it = d.split_whitespace();
            (it.next().unwrap(), it.collect())
        })
        .collect();
    let get = |name: &str| -> Vec<&str> {
        directives
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .1
            .clone()
    };
    assert_eq!(get("default-src"), ["'none'"]);
    assert_eq!(get("script-src"), ["'self'"]);
    assert_eq!(get("style-src"), ["'self'"]);
    assert_eq!(get("img-src"), ["'self'", "data:"]);
    assert_eq!(get("font-src"), ["'self'"]);
    assert_eq!(get("connect-src"), ["ipc:", "http://ipc.localhost"]);
    for bad in [
        "unsafe-eval",
        "unsafe-inline",
        "wasm-unsafe-eval",
        "https:",
        "*",
        "blob:",
    ] {
        assert!(
            !csp.split_whitespace()
                .any(|t| t.trim_matches('\'') == bad.trim_matches('\'')),
            "CSP must not allow {bad}"
        );
    }
    assert!(conf["app"]["security"].get("devCsp").is_none());
    assert!(conf["app"]["security"]
        .get("dangerousDisableAssetCspModification")
        .is_none());
    // The CSP the UI's tests run under (app/ui/vite.config.ts) is the same one.
    let vite = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/vite.config.ts"),
    )
    .unwrap();
    for (name, values) in &directives {
        let line = format!("{name} {}", values.join(" "));
        assert!(vite.contains(&line), "vite.config.ts lacks `{line}`");
    }
}

#[test]
fn hardening_flags_are_set() {
    let conf = read_json("tauri.conf.json");
    let sec = &conf["app"]["security"];
    assert_eq!(sec["freezePrototype"], true);
    assert_eq!(sec["pattern"]["use"], "isolation");
    assert_eq!(sec["capabilities"], serde_json::json!(["main"]));
    assert_eq!(conf["app"]["withGlobalTauri"], false);
    // The window is built in code (navigation lock, incognito, WebView2 settings).
    assert_eq!(conf["app"]["windows"], serde_json::json!([]));
    assert!(conf["plugins"].is_null() || conf["plugins"].as_object().unwrap().is_empty());
    assert_eq!(conf["identifier"], "io.github.its30na.redactor");
    assert_eq!(conf["productName"], "Redactor");
    assert_eq!(conf["version"], "2.0.0");
    assert_eq!(conf["bundle"]["targets"], serde_json::json!(["nsis"]));
    assert_eq!(
        conf["bundle"]["windows"]["nsis"]["installMode"],
        "currentUser"
    );
}

#[test]
fn webview2_browser_args_are_pinned() {
    let args: Vec<&str> = redactor_app::browser_args::BROWSER_ARGS
        .split_whitespace()
        .collect();
    assert_eq!(
        args,
        [
            // wry's own defaults, which setting the arguments would otherwise drop.
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
            "--dns-prefetch-disable",
            "--disable-background-networking",
        ]
    );
    let src = |rel: &str| {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
    };
    // The window is built with them.
    assert!(
        src("src/app.rs").contains(".additional_browser_args(crate::browser_args::BROWSER_ARGS)")
    );
    // The wry version whose defaults the list repeats; a bump means checking them again.
    assert!(src("../../Cargo.lock").contains("name = \"wry\"\nversion = \"0.57.1\""));
}
