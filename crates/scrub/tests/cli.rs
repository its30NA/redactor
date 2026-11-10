//! `scrub` end to end. Sections 1-3 port every behaviour covered by the v1 test files
//! (`test_pipeline.py`, `test_m3.py`, `test_m5.py`) with the same shapes, texts and exit codes;
//! section 4 covers what v2 adds. All secret-shaped values are generated here at run time.

mod support;

use std::fs;
use std::path::Path;

use support::gen::Gen;
use support::{run, scrub_in};
use tempfile::TempDir;

fn tmp() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn write(dir: &Path, name: &str, content: &str) -> String {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&p, content).unwrap();
    p.to_string_lossy().into_owned()
}

/// A fake legacy-style OpenAI key body: `sk-` + 48 random base62.
fn openai(g: &mut Gen) -> String {
    format!("sk-{}", g.alnum(48))
}

fn pw() -> String {
    ["Plum", "tree", "47", "-", "Kite"].concat()
}

// ---------------------------------------------------------------------------------------------
// 1. Pipeline (v1 tests/test_pipeline.py)
// ---------------------------------------------------------------------------------------------

#[test]
fn end_to_end_env_file() {
    let d = tmp();
    let key = openai(&mut Gen::named("env"));
    let (out, _, code) = run(d.path(), &[], &format!("OPENAI_API_KEY={key}\nDEBUG=true"));
    assert_eq!(code, 0);
    // v2: the key name next to the value wins the label (v1: "OpenAI API Key"); a bare key
    // still gets the provider label (see `single_occurrence_has_no_number`).
    assert_eq!(out, "OPENAI_API_KEY=[REDACTED: OpenAI API Key]\nDEBUG=true");
}

#[test]
fn anthropic_wins_over_openai_on_shared_prefix() {
    let d = tmp();
    let body = Gen::named("ant").alnum(80);
    let (out, _, _) = run(d.path(), &[], &format!("key=sk-ant-api03-{body}"));
    assert_eq!(out, "key=[REDACTED: Anthropic API Key]");
}

#[test]
fn stable_numbering_distinguishes_distinct_values() {
    let d = tmp();
    let mut g = Gen::named("numbering");
    let (a, b) = (openai(&mut g), openai(&mut g));
    let (out, _, _) = run(d.path(), &[], &format!("one={a} two={b} again={a}"));
    assert_eq!(
        out,
        "one=[REDACTED: OpenAI API Key #1] \
         two=[REDACTED: OpenAI API Key #2] \
         again=[REDACTED: OpenAI API Key #1]"
    );
}

#[test]
fn single_occurrence_has_no_number() {
    let d = tmp();
    let key = openai(&mut Gen::named("single"));
    let (out, _, _) = run(d.path(), &[], &format!("k={key}"));
    assert_eq!(out, "k=[REDACTED: OpenAI API Key]");
}

#[test]
fn overlapping_rules_merge_to_cover_the_union() {
    // v1 resolver test: overlapping spans merge to their union, never drop one and leak the
    // rest; the label comes from the higher-confidence contributor.
    let d = tmp();
    let cfg = write(
        d.path(),
        "redactor.toml",
        r#"
[[rules]]
name = "low"
label = "Low"
pattern = "ZK3QW8RT"
confidence = 0.8

[[rules]]
name = "high"
label = "High"
pattern = "W8RTYUL9"
confidence = 0.95
"#,
    );
    let (out, _, _) = run(d.path(), &["-c", &cfg], "id=ZK3QW8RTYUL9!");
    assert_eq!(out, "id=[REDACTED: High]!");
}

#[test]
fn disjoint_matches_stay_untouched_between() {
    let d = tmp();
    let mut g = Gen::named("disjoint");
    let (a, b) = (openai(&mut g), g.github("ghp_"));
    let (out, _, _) = run(d.path(), &[], &format!("x {a} middle text {b} end"));
    assert_eq!(
        out,
        "x [REDACTED: OpenAI API Key] middle text [REDACTED: GitHub Token] end"
    );
}

#[test]
fn allowlist_exempts_matches() {
    let d = tmp();
    let cfg = write(
        d.path(),
        "redactor.toml",
        "[allowlist]\npatterns = [\"sk-xxxx\"]\n",
    );
    let text = format!("example key sk-xxxx{}", Gen::named("allow").alnum(40));
    let (out, err, code) = run(d.path(), &["-c", &cfg, "--summary"], &text);
    assert_eq!(code, 0);
    assert_eq!(out, text);
    assert!(err.contains("No sensitive data detected."), "{err}");
}

#[test]
fn config_disables_detector() {
    let d = tmp();
    let token = Gen::named("bearer").alnum(40);
    let text = format!("Bearer {token}");
    // On by default.
    // v2 keeps the scheme word and redacts the token (v1 redacted "Bearer <token>" whole).
    let (out, _, _) = run(d.path(), &[], &text);
    assert_eq!(out, "Bearer [REDACTED: Bearer Token]");
    // v1 key `disabled_detectors` with the v1 detector name still works.
    let cfg = write(
        d.path(),
        "redactor.toml",
        "disabled_detectors = [\"bearer_token\"]\n",
    );
    // v2: other detectors (context, classifier) still catch a random-looking token, under a
    // generic label; v1 had nothing else to fall back on.
    let (out, _, _) = run(d.path(), &["-c", &cfg], &text);
    assert!(
        !out.contains("Bearer Token") && !out.contains(&token),
        "{out}"
    );
    // With those off as well, nothing is left: the v1 outcome.
    let cfg = write(
        d.path(),
        "both.toml",
        "disabled_detectors = [\"bearer_token\", \"classifier\", \"context\"]\n",
    );
    let (out, _, _) = run(d.path(), &["-c", &cfg], &text);
    assert_eq!(out, text);
}

#[test]
fn email_password_overlap_never_leaks_password() {
    // Regression from v1: an email-shaped password in a connection string must not leak its
    // non-email prefix when PII detection is on and the email match overlaps.
    let d = tmp();
    let p = ["S3cr3t", "Pass"].join("!");
    let text = format!("DATABASE_URL=postgres://user:{p}@db.corp.io:5432/app");
    let (out, _, _) = run(d.path(), &["--pii"], &text);
    assert!(!out.contains("S3cr3t"), "{out}");
    assert!(!out.contains("Pass@db.corp.io"), "{out}");
}

#[test]
fn config_custom_template() {
    let d = tmp();
    let cfg = write(
        d.path(),
        "redactor.toml",
        "placeholder_template = \"<<{label}{suffix}>>\"\n",
    );
    let key = openai(&mut Gen::named("tpl"));
    let (out, _, _) = run(d.path(), &["-c", &cfg], &key);
    assert_eq!(out, "<<OpenAI API Key>>");
}

// ---------------------------------------------------------------------------------------------
// 2. PII, custom rules, preview (v1 tests/test_m3.py)
// ---------------------------------------------------------------------------------------------

#[test]
fn pii_is_off_by_default() {
    let d = tmp();
    let text = "contact jane.doe@example.com or 192.168.1.1";
    let (out, _, _) = run(d.path(), &[], text);
    assert_eq!(out, text);
}

#[test]
fn pii_flag_enables_the_group() {
    let d = tmp();
    let (out, _, _) = run(d.path(), &["--pii"], "email jane.doe@corp.io from 10.0.0.5");
    assert!(out.contains("[REDACTED: Email Address]"), "{out}");
    assert!(out.contains("[REDACTED: IP Address]"), "{out}");
}

#[test]
fn pii_config_key_enables_the_group() {
    let d = tmp();
    let cfg = write(d.path(), "redactor.toml", "redact_pii = true\n");
    let (out, _, _) = run(
        d.path(),
        &["-c", &cfg],
        "email jane.doe@corp.io from 10.0.0.5",
    );
    assert!(out.contains("[REDACTED: Email Address]"), "{out}");
    assert!(out.contains("[REDACTED: IP Address]"), "{out}");
    // The new spelling is accepted too.
    let cfg = write(d.path(), "new.toml", "pii = true\n");
    let (out, _, _) = run(d.path(), &["-c", &cfg], "email jane.doe@corp.io");
    assert!(out.contains("[REDACTED: Email Address]"), "{out}");
}

#[test]
fn individual_pii_detectors() {
    let d = tmp();
    let mut g = Gen::named("pii");
    let card = g.card("4111", 16);
    let spaced = format!(
        "{} {} {} {}",
        &card[0..4],
        &card[4..8],
        &card[8..12],
        &card[12..16]
    );
    let cases: Vec<(String, &str)> = vec![
        ("jane.doe@example.com".into(), "Email Address"),
        ("192.168.100.42".into(), "IP Address"),
        ("123-45-6789".into(), "US Social Security Number"),
        ("+49 30 1234 5678".into(), "Phone Number"),
        (spaced, "Credit Card Number"), // valid Luhn
    ];
    for (text, label) in cases {
        let (out, _, _) = run(d.path(), &["--pii"], &text);
        assert_eq!(out, format!("[REDACTED: {label}]"), "input {text:?}");
    }
}

#[test]
fn credit_card_rejects_non_luhn() {
    let d = tmp();
    let (out, _, _) = run(d.path(), &["--pii"], "1234 5678 9012 3456");
    assert!(!out.contains("Credit Card"), "{out}");
}

#[test]
fn pii_detectors_are_listed_as_off_by_default() {
    let d = tmp();
    let (out, _, code) = run(d.path(), &["detectors"], "");
    assert_eq!(code, 0);
    for id in ["email", "ipv4", "us_ssn", "credit_card", "phone_e164"] {
        let line = out
            .lines()
            .find(|l| l.split_whitespace().next() == Some(id))
            .unwrap_or_else(|| panic!("{id} missing from:\n{out}"));
        assert!(line.contains("pii"), "{line}");
        assert!(line.contains("--pii"), "{line}");
    }
}

#[test]
fn custom_rule_detects_and_labels() {
    let d = tmp();
    let cfg = write(
        d.path(),
        "redactor.toml",
        r#"
[[rules]]
name = "acme_token"
label = "ACME Internal Token"
pattern = '\bACME-[A-Z0-9]{10}\b'
"#,
    );
    let (out, _, _) = run(d.path(), &["-c", &cfg], "see ACME-ABCD123456 end");
    assert_eq!(out, "see [REDACTED: ACME Internal Token] end");
    // v1 input `token=ACME-ABCD123456 end`: v2's context engine takes the unquoted value up to
    // the end of the line, so the union also swallows " end" (never leaks, may over-redact).
    let (out, _, _) = run(d.path(), &["-c", &cfg], "token=ACME-ABCD123456 end");
    assert_eq!(out, "token=[REDACTED: ACME Internal Token]");
}

#[test]
fn custom_rule_group_capture() {
    let d = tmp();
    let cfg = write(
        d.path(),
        "redactor.toml",
        r#"
[[rules]]
name = "secret_header"
label = "Custom Secret"
pattern = 'X-Secret:\s*(\S+)'
group = 1
"#,
    );
    let (out, _, _) = run(d.path(), &["-c", &cfg], "X-Secret: abc123xyz");
    assert_eq!(out, "X-Secret: [REDACTED: Custom Secret]");
}

#[test]
fn preview_reports_line_and_label() {
    let d = tmp();
    let key = openai(&mut Gen::named("preview"));
    let text = format!("line one\nOPENAI_API_KEY={key}\nline three");
    let (out, err, code) = run(d.path(), &["--preview"], &text);
    assert_eq!(code, 0);
    assert!(err.contains("Preview — 1 redaction(s):"), "{err}");
    assert!(err.contains("Ln 2"), "{err}");
    assert!(err.contains("API Key"), "{err}");
    // The masked context shows the label, never the raw secret.
    assert!(!err.contains(&key), "{err}");
    // stdout still carries the sanitized text only.
    assert_eq!(
        out,
        "line one\nOPENAI_API_KEY=[REDACTED: OpenAI API Key]\nline three"
    );
}

#[test]
fn preview_empty() {
    let d = tmp();
    let (_, err, _) = run(d.path(), &["--preview"], "nothing to see here");
    assert_eq!(err.trim_end(), "No sensitive data detected.");
}

#[test]
fn preview_masks_every_secret_on_the_line() {
    // v1 masked only the finding it was listing, so a second secret on the same line showed
    // up raw in the preview. v2 masks them all.
    let d = tmp();
    let mut g = Gen::named("two on a line");
    let (a, b) = (openai(&mut g), g.github("ghp_"));
    let (_, err, _) = run(d.path(), &["--preview"], &format!("a={a} b={b}"));
    assert!(!err.contains(&a) && !err.contains(&b), "{err}");
    assert_eq!(err.matches("Ln 1").count(), 2, "{err}");
}

// ---------------------------------------------------------------------------------------------
// 3. Scanning, git hook, dispatch (v1 tests/test_m5.py)
// ---------------------------------------------------------------------------------------------

#[test]
fn scan_finds_secret() {
    let d = tmp();
    let key = openai(&mut Gen::named("scanfind"));
    let f = write(d.path(), "config.env", &format!("OPENAI_API_KEY={key}\n"));
    let (out, err, code) = run(d.path(), &["scan", &f], "");
    assert_eq!(code, 1);
    assert_eq!(out, format!("{f}: 1 finding(s) — OpenAI API Key\n"));
    assert!(err.contains("1 finding(s) found in 1 file(s)."), "{err}");
}

#[test]
fn scan_skips_binary() {
    let d = tmp();
    let key = openai(&mut Gen::named("binary"));
    let mut bytes = vec![0u8, 1, 2];
    bytes.extend_from_slice(key.as_bytes());
    fs::write(d.path().join("blob.bin"), bytes).unwrap();
    let (out, _, code) = run(d.path(), &["scan", "."], "");
    assert_eq!(code, 0);
    assert_eq!(out, "");
}

#[test]
fn scan_skips_noise_directories() {
    let d = tmp();
    let key = openai(&mut Gen::named("noise"));
    write(d.path(), "src/a.txt", "x = 1\n");
    for noise in [
        ".git/config",
        "node_modules/pkg/index.js",
        "target-less/ok.txt",
    ] {
        write(d.path(), noise, &format!("k={key}\n"));
    }
    let (out, _, code) = run(d.path(), &["scan", "."], "");
    // Only the non-noise directory is scanned.
    assert!(
        !out.contains(".git") && !out.contains("node_modules"),
        "{out}"
    );
    assert!(out.contains("target-less"), "{out}");
    assert_eq!(code, 1);
}

#[test]
fn scan_write_sanitizes_files_in_place() {
    let d = tmp();
    let token = Gen::named("write").alnum(40);
    let f = write(
        d.path(),
        "app.log",
        &format!("token: Bearer {token}\nplain line\n"),
    );
    let (out, err, code) = run(d.path(), &["scan", "--write", &f], "");
    assert_eq!(code, 0, "fix mode succeeds");
    assert!(out.starts_with(&format!("fixed  {f}  (1: ")), "{out}");
    assert!(err.contains("1 finding(s) sanitized 1 file(s)."), "{err}");
    let rewritten = fs::read_to_string(&f).unwrap();
    assert!(!rewritten.contains(&token));
    assert!(
        rewritten.contains("[REDACTED: Bearer Token]"),
        "{rewritten}"
    );
    assert!(rewritten.contains("plain line"));
}

#[test]
fn scan_no_recursive_stays_in_the_directory() {
    let d = tmp();
    let key = openai(&mut Gen::named("norec"));
    write(d.path(), "top.txt", "nothing here\n");
    write(d.path(), "sub/deep.env", &format!("K={key}\n"));
    let dir = d.path().to_string_lossy().into_owned();
    let (_, _, code) = run(d.path(), &["scan", "--no-recursive", &dir], "");
    assert_eq!(code, 0);
    let (_, _, code) = run(d.path(), &["scan", &dir], "");
    assert_eq!(code, 1);
}

#[test]
fn cli_defaults_to_sanitize_on_stdin() {
    let d = tmp();
    let key = openai(&mut Gen::named("default"));
    let (out, _, code) = run(d.path(), &[], &format!("KEY={key}"));
    assert_eq!(code, 0);
    assert!(out.contains("[REDACTED: OpenAI API Key]"), "{out}");
}

#[test]
fn explicit_sanitize_subcommand_and_file_argument() {
    let d = tmp();
    let key = openai(&mut Gen::named("file"));
    let f = write(d.path(), "in.env", &format!("KEY={key}\n"));
    let (a, _, _) = run(d.path(), &[&f], "");
    let (b, _, _) = run(d.path(), &["sanitize", &f], "");
    assert_eq!(a, "KEY=[REDACTED: OpenAI API Key]\n");
    assert_eq!(a, b);
}

#[test]
fn scan_exit_code_is_one_with_findings() {
    let d = tmp();
    let tok = Gen::named("ghscan").github("ghp_");
    write(d.path(), "s.env", &format!("GITHUB_TOKEN={tok}"));
    let (out, _, code) = run(d.path(), &["scan", "."], "");
    assert_eq!(code, 1);
    assert!(out.contains("finding(s)"), "{out}");
}

#[test]
fn scan_clean_tree_exits_zero() {
    let d = tmp();
    write(d.path(), "ok.txt", "nothing secret here\n");
    let (_, _, code) = run(d.path(), &["scan", "."], "");
    assert_eq!(code, 0);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {:?}", out);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).unwrap().permissions().mode() & 0o111 != 0
}
#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
}

#[test]
fn install_hook_writes_refuses_and_forces() {
    let d = tmp();
    git(d.path(), &["init", "-q"]);
    let (_, err, code) = run(d.path(), &["install-hook"], "");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("Installed pre-commit hook at"), "{err}");
    let hook = d.path().join(".git/hooks/pre-commit");
    assert_eq!(fs::read_to_string(&hook).unwrap(), scrub::hook::HOOK_SCRIPT);
    assert!(is_executable(&hook));

    let (_, err, code) = run(d.path(), &["install-hook"], "");
    assert_eq!(code, 1, "an existing hook is not overwritten");
    assert!(err.contains("already exists"), "{err}");

    fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    let (_, _, code) = run(d.path(), &["install-hook", "--force"], "");
    assert_eq!(code, 0);
    assert_eq!(fs::read_to_string(&hook).unwrap(), scrub::hook::HOOK_SCRIPT);
}

#[test]
fn install_hook_outside_a_repo_exits_two() {
    let d = tmp();
    let (_, err, code) = run(d.path(), &["install-hook"], "");
    assert_eq!(code, 2);
    assert!(err.contains("could not install hook"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// 4. New in v2
// ---------------------------------------------------------------------------------------------

#[test]
fn check_reads_staged_blob_not_worktree() {
    let d = tmp();
    git(d.path(), &["init", "-q"]);
    let key = openai(&mut Gen::named("staged"));
    // Staged: a secret. Working tree: cleaned afterwards. The commit would still carry it.
    write(d.path(), "dirty.env", &format!("KEY={key}\n"));
    git(d.path(), &["add", "dirty.env"]);
    write(d.path(), "dirty.env", "KEY=changed-after-staging\n");
    // Staged: clean. Working tree: a secret added afterwards. Nothing secret is committed.
    write(d.path(), "clean.env", "KEY=nothing-here\n");
    git(d.path(), &["add", "clean.env"]);
    write(d.path(), "clean.env", &format!("KEY={key}\n"));

    let (out, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("dirty.env: 1 finding(s) — OpenAI API Key"),
        "{err}"
    );
    assert!(!err.contains("clean.env"), "{err}");
    assert_eq!(out, "");
    assert!(!err.contains(&key));

    // Unstage the dirty one: only the clean blob is left, so the check passes.
    git(d.path(), &["rm", "-q", "-f", "--cached", "dirty.env"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0, "{err}");
}

#[test]
fn check_outside_a_repo_exits_two() {
    let d = tmp();
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 2);
    assert!(err.contains("scrub:"), "{err}");
}

#[test]
fn check_skips_binary_staged_blobs() {
    let d = tmp();
    git(d.path(), &["init", "-q"]);
    let key = openai(&mut Gen::named("bin staged"));
    let mut bytes = vec![0u8, 1, 2];
    bytes.extend_from_slice(key.as_bytes());
    fs::write(d.path().join("b.bin"), bytes).unwrap();
    git(d.path(), &["add", "b.bin"]);
    let (_, _, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0);
}

fn json_of(dir: &Path, args: &[&str], stdin: &str) -> (String, serde_json::Value) {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let (out, err, code) = run(dir, &a, stdin);
    assert_eq!(code, 0, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("stdout is one JSON document");
    (out, v)
}

#[test]
fn json_has_no_values() {
    let d = tmp();
    let mut g = Gen::named("json");
    let (key, password) = (openai(&mut g), pw());
    let text = format!("OPENAI_API_KEY={key}\nDB_PASSWORD={password}\nagain {password}\n");
    let (out, v) = json_of(d.path(), &[], &text);
    assert!(!out.contains(&key), "key leaked");
    assert!(!out.contains(&password), "password leaked");
    let findings = v["findings"].as_array().unwrap();
    assert!(findings.len() >= 3, "{findings:?}");
    for f in findings {
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
fn json_is_the_ui_scan_response() {
    let d = tmp();
    let password = pw();
    // Emoji and CRLF before the secret: offsets are UTF-16 code units.
    let text = format!("😀 hi\r\nDB_PASSWORD={password}\r\n");
    let (_, v) = json_of(d.path(), &[], &text);
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
    assert_eq!(v["verified"], true);
    let f = &v["findings"][0];
    let units: Vec<u16> = text.encode_utf16().collect();
    let (s, e) = (
        f["start"].as_u64().unwrap() as usize,
        f["end"].as_u64().unwrap() as usize,
    );
    assert_eq!(String::from_utf16(&units[s..e]).unwrap(), password);
    assert_eq!(f["line"], 2);
    assert_eq!(v["stats"]["chars"].as_u64().unwrap() as usize, units.len());
    assert_eq!(
        v["output"].as_str().unwrap(),
        "😀 hi\r\nDB_PASSWORD=[REDACTED: Password]\r\n"
    );
    let span = &v["outputSpans"][0];
    assert_eq!(span["findingId"], 0);
}

#[test]
fn json_on_clean_input_has_no_findings() {
    let d = tmp();
    let (_, v) = json_of(d.path(), &[], "nothing to see");
    assert_eq!(v["findings"].as_array().unwrap().len(), 0);
    assert_eq!(v["output"], "nothing to see");
}

#[test]
fn detectors_markdown_lists_every_rule() {
    let d = tmp();
    let (md, _, code) = run(d.path(), &["detectors", "--markdown"], "");
    assert_eq!(code, 0);
    let infos = redactor_core::rule_infos();
    assert!(infos.len() > 100);
    for info in &infos {
        assert!(
            md.contains(&format!("`{}`", info.id)),
            "rule {} missing from the markdown",
            info.id
        );
        assert!(md.contains(info.label), "label {} missing", info.label);
    }
    for family in ["AI providers", "Cloud", "Password hashes"] {
        assert!(md.contains(&format!("## {family}")), "{family}");
    }
    // Plain output lists them too.
    let (plain, _, _) = run(d.path(), &["detectors"], "");
    for info in &infos {
        assert!(plain
            .lines()
            .any(|l| l.split_whitespace().next() == Some(info.id)));
    }
}

#[test]
fn audit_has_no_values_or_hashes() {
    let d = tmp();
    let mut g = Gen::named("audit");
    let (key, password) = (openai(&mut g), pw());
    let text = format!("OPENAI_API_KEY={key}\nDB_PASSWORD={password}\nagain {password}\n");
    let path = d.path().join("audit.json");
    let audit = path.to_string_lossy().into_owned();
    let (_, _, code) = run(d.path(), &["--audit", &audit], &text);
    assert_eq!(code, 0);
    let raw = fs::read_to_string(&path).unwrap();
    assert!(!raw.contains(&key) && !raw.contains(&password));

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // What it does record: when, from where, counts, lines, detectors.
    assert!(v["timestamp"].as_str().unwrap().ends_with('Z'));
    assert_eq!(v["source"], "stdin");
    let records = v["records"].as_array().unwrap();
    assert_eq!(
        v["redactionCount"].as_u64().unwrap() as usize,
        records.len()
    );
    assert!(records.len() >= 3);
    for r in records {
        let mut keys: Vec<_> = r.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["detector", "kind", "label", "line", "tier"]);
    }
    assert!(records.iter().any(|r| r["line"] == 3));
    assert!(v["byLabel"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["label"] == "Password" && l["n"] == 2));
    let tiers: u64 = ["certain", "likely", "possible"]
        .iter()
        .map(|t| v["byTier"][t].as_u64().unwrap())
        .sum();
    assert_eq!(tiers as usize, records.len());

    // No fingerprints: no hash, salt or value-length fields, no long hex runs, and nothing
    // random between two runs over the same input (apart from the timestamp).
    for banned in ["salt", "fingerprint", "hash", "sha", "length", "value"] {
        assert!(
            !raw.to_lowercase().contains(banned),
            "{banned} in audit:\n{raw}"
        );
    }
    let mut run_len = 0;
    for c in raw.chars() {
        run_len = if c.is_ascii_hexdigit() {
            run_len + 1
        } else {
            0
        };
        assert!(run_len < 12, "hex-looking run in audit:\n{raw}");
    }
    let again = d.path().join("audit2.json");
    run(d.path(), &["--audit", &again.to_string_lossy()], &text);
    let strip = |s: &str| {
        s.lines()
            .filter(|l| !l.contains("timestamp"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(&raw), strip(&fs::read_to_string(&again).unwrap()));
}

#[test]
fn audit_dash_goes_to_stderr_and_names_the_file() {
    let d = tmp();
    let f = write(d.path(), "notes.txt", &format!("DB_PASSWORD={}\n", pw()));
    let (out, err, code) = run(d.path(), &["--audit", "-", &f], "");
    assert_eq!(code, 0);
    assert!(!out.contains(&pw()));
    assert!(err.contains("\"source\""), "{err}");
    assert!(err.contains("notes.txt"), "{err}");
    assert!(!err.contains(&pw()));
}

#[test]
fn v1_config_loads() {
    // The v1 example file (verbatim copy in tests/fixtures) and the updated example in
    // examples/ both parse, with the same meaning: three allowlist patterns, everything else
    // default.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        root.join("tests/fixtures/redactor.v1.toml"),
        root.join("../../examples/redactor.example.toml"),
    ] {
        let cfg = scrub::config::Config::load(Some(&path)).unwrap();
        let aws_docs_key = ["AK", "IA", "IOSFODNN7EXAMPLE"].concat();
        assert_eq!(
            cfg.engine.allowlist,
            ["example\\.com", aws_docs_key.as_str(), "sk-xxxx"]
        );
        assert!(cfg.engine.custom_rules.is_empty() && cfg.engine.disabled.is_empty());
        assert!(!cfg.engine.pii && cfg.engine.strip_invisible);
        assert_eq!(cfg.engine.template, "[REDACTED: {label}{suffix}]");
        assert!(cfg.notices.is_empty(), "{:?}", cfg.notices);

        // ...and it behaves the same: allowlisted placeholders survive, real keys do not.
        let d = tmp();
        let mut g = Gen::named("v1cfg");
        let (fake, real) = (
            format!("sk-xxxx{}", g.alnum(40)),
            format!("ghp_{}", g.alnum(36)),
        );
        let p = path.to_string_lossy().into_owned();
        let (out, _, _) = run(d.path(), &["-c", &p], &format!("{fake} {real}"));
        assert_eq!(out, format!("{fake} [REDACTED: GitHub Token]"));
    }
}

#[test]
fn every_v1_config_key_maps() {
    let toml = r#"
disabled_detectors = ["bearer_token"]
enabled_detectors = ["classifier"]
redact_pii = true
placeholder_template = "<{label}{suffix}>"
stable_numbering = true
sensitivity = "paranoid"
strip_invisible = false
terms = ["Acme"]
placeholder = "generic"

[[rules]]
name = "acme"
label = "ACME"
pattern = "ACME-[A-Z]+"
confidence = 0.95
group = 0

[allowlist]
patterns = ["alpha-fixture", "beta-fixture"]

[llm]
enabled = true
"#;
    let cfg = scrub::config::Config::parse(toml).unwrap();
    let e = &cfg.engine;
    assert_eq!(e.disabled, ["bearer_token"]);
    assert_eq!(e.enabled, ["classifier"]);
    assert!(e.pii && !e.strip_invisible);
    assert_eq!(e.template, "<{label}{suffix}>");
    assert_eq!(e.sensitivity, redactor_core::Sensitivity::Paranoid);
    assert_eq!(e.terms, ["Acme"]);
    assert_eq!(e.placeholder, redactor_core::PlaceholderStyle::Generic);
    assert_eq!(e.custom_rules.len(), 1);
    assert_eq!(e.custom_rules[0].id, "acme");
    assert_eq!(e.custom_rules[0].group, 0);
    assert_eq!(e.allowlist, ["alpha-fixture", "beta-fixture"]);
    // The `[llm]` section is ignored with one notice.
    assert_eq!(cfg.notices.len(), 1);
    assert!(cfg.notices[0].contains("[llm]"));
}

#[test]
fn llm_section_is_ignored_with_a_one_line_notice() {
    let d = tmp();
    let cfg = write(
        d.path(),
        "redactor.toml",
        "[llm]\nenabled = true\nmodel = \"x\"\n",
    );
    let (out, err, code) = run(d.path(), &["-c", &cfg], "plain text");
    assert_eq!(code, 0);
    assert_eq!(out, "plain text");
    // One line naming the config file, one line for the ignored section.
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 2, "{err}");
    assert_eq!(lines[0], format!("scrub: using config: {cfg}"));
    assert!(lines[1].contains("[llm]"), "{err}");
}

#[test]
fn config_is_discovered_upwards_from_the_working_directory() {
    let d = tmp();
    write(
        d.path(),
        "redactor.toml",
        "placeholder_template = \"<{label}>\"\n",
    );
    let sub = d.path().join("a/b");
    fs::create_dir_all(&sub).unwrap();
    let key = openai(&mut Gen::named("discover"));
    let (out, _, _) = run(&sub, &[], &key);
    assert_eq!(out, "<OpenAI API Key>");
}

#[test]
fn bad_config_exits_two() {
    let d = tmp();
    let (_, err, code) = run(d.path(), &["-c", "missing.toml"], "x");
    assert_eq!(code, 2);
    assert!(err.starts_with("scrub: "), "{err}");
    let bad = write(d.path(), "bad.toml", "sensitivity = \"loud\"\n");
    let (_, err, code) = run(d.path(), &["-c", &bad], "x");
    assert_eq!(code, 2, "{err}");
    let bad = write(
        d.path(),
        "bad2.toml",
        "[[rules]]\nname=\"r\"\nlabel=\"R\"\npattern=\"(\"\n",
    );
    let (_, err, code) = run(d.path(), &["-c", &bad], "x");
    assert_eq!(code, 2, "{err}");
}

#[test]
fn unknown_config_keys_are_reported() {
    let d = tmp();
    let cfg = write(d.path(), "redactor.toml", "disabled_detector = [\"x\"]\n");
    let (_, err, code) = run(d.path(), &["-c", &cfg], "x");
    assert_eq!(code, 0);
    assert!(err.contains("disabled_detector"), "{err}");
}

#[test]
fn sensitivity_flag_overrides_the_config() {
    let d = tmp();
    // A bare random token with no key next to it: Paranoid redacts it, Relaxed does not.
    let token = Gen::named("sens").alnum(26);
    let text = format!("see {token} for details");
    let (relaxed, _, _) = run(d.path(), &["--sensitivity", "relaxed"], &text);
    assert_eq!(relaxed, text);
    let (paranoid, _, _) = run(d.path(), &["--sensitivity", "paranoid"], &text);
    assert!(!paranoid.contains(&token), "{paranoid}");
    // The flag wins over the config file.
    let cfg = write(d.path(), "redactor.toml", "sensitivity = \"relaxed\"\n");
    let (out, _, _) = run(d.path(), &["-c", &cfg, "--sensitivity", "paranoid"], &text);
    assert!(!out.contains(&token), "{out}");
}

#[test]
fn no_strip_keeps_invisible_characters() {
    let d = tmp();
    let text = "a\u{200b}b\u{202e}c";
    let (out, _, _) = run(d.path(), &[], text);
    assert_eq!(out, "abc");
    let (out, _, _) = run(d.path(), &["--no-strip"], text);
    assert_eq!(out, text);
}

#[test]
fn summary_goes_to_stderr_sorted_by_label() {
    let d = tmp();
    let mut g = Gen::named("summary");
    let (a, b) = (openai(&mut g), g.github("ghp_"));
    let (out, err, _) = run(d.path(), &["--summary"], &format!("{a}\n{b}\n{a}\n"));
    assert!(!out.contains("Redacted"));
    assert_eq!(
        err,
        "Redacted 3 item(s):\n  - GitHub Token: 1\n  - OpenAI API Key: 2\n"
    );
}

#[test]
fn diff_is_a_unified_diff() {
    let d = tmp();
    let key = openai(&mut Gen::named("diff"));
    let f = write(d.path(), "x.env", &format!("a=1\nKEY={key}\nb=2\n"));
    let (out, _, code) = run(d.path(), &["--diff", &f], "");
    assert_eq!(code, 0);
    let mut lines = out.lines();
    assert_eq!(lines.next().unwrap(), format!("--- {f} (original)"));
    assert_eq!(lines.next().unwrap(), format!("+++ {f} (sanitized)"));
    assert!(out.contains("+KEY=[REDACTED: OpenAI API Key]"), "{out}");
    assert!(out.contains(" a=1"), "{out}");
    // Clean input: empty diff.
    let (out, _, _) = run(d.path(), &["--diff"], "nothing");
    assert_eq!(out, "");
}

#[test]
fn unreadable_input_exits_two() {
    let d = tmp();
    let (out, err, code) = run(d.path(), &["no-such-file.txt"], "");
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert!(err.starts_with("scrub: cannot read input:"), "{err}");
}

#[test]
fn missing_scan_path_exits_two() {
    let d = tmp();
    let (_, err, code) = run(d.path(), &["scan", "nope"], "");
    assert_eq!(code, 2);
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn scan_write_leaves_non_utf8_files_alone() {
    let d = tmp();
    let key = openai(&mut Gen::named("latin1"));
    let mut bytes = format!("caf\u{e9} KEY={key}\n").into_bytes();
    bytes[3] = 0xe9; // a lone Latin-1 byte: not valid UTF-8
    bytes.remove(4);
    let p = d.path().join("latin1.txt");
    fs::write(&p, &bytes).unwrap();
    let (_, err, code) = run(d.path(), &["scan", "--write", &p.to_string_lossy()], "");
    assert_eq!(
        code, 1,
        "a secret is still there, so the run does not succeed: {err}"
    );
    assert_eq!(
        fs::read(&p).unwrap(),
        bytes,
        "file must stay byte-identical"
    );
    assert!(err.contains("not valid UTF-8"), "{err}");
}

#[test]
fn version_prints_both_versions() {
    let d = tmp();
    let (out, _, code) = run(d.path(), &["version"], "");
    assert_eq!(code, 0);
    assert!(out.starts_with("scrub 2."), "{out}");
    assert!(out.contains(redactor_core::ENGINE_VERSION));
    // The rule count is the engine's own (what the app shows as `ruleCount`).
    let rules = redactor_core::Engine::new(redactor_core::EngineConfig::default())
        .unwrap()
        .rule_count();
    assert!(out.contains(&format!("{rules} rules")), "{out}");
}

#[cfg(not(windows))]
#[test]
fn clipboard_is_not_supported_off_windows() {
    let d = tmp();
    let (out, err, code) = run(d.path(), &["clipboard"], "");
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert!(err.contains("not supported"), "{err}");
}

#[test]
fn help_lists_every_command() {
    let d = tmp();
    let out = scrub_in(d.path()).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    for c in [
        "scan",
        "check",
        "install-hook",
        "clipboard",
        "detectors",
        "version",
    ] {
        assert!(help.contains(c), "{c} missing from help");
    }
}

#[test]
fn terms_come_from_the_config() {
    let d = tmp();
    let cfg = write(d.path(), "redactor.toml", "terms = [\"Contoso\"]\n");
    let (out, _, _) = run(d.path(), &["-c", &cfg], "ask Contoso about it");
    assert!(!out.contains("Contoso"), "{out}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_full_disk_on_stdout_is_an_error() {
    let d = tmp();
    let f = write(d.path(), "in.txt", &format!("DB_PASSWORD={}\n", pw()));
    let full = fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let out = std::process::Command::new(assert_cmd::cargo::cargo_bin("scrub"))
        .current_dir(d.path())
        .arg(&f)
        .stdout(full)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot write the output"));
}

#[test]
fn scan_names_pruned_directories_and_scans_editor_settings() {
    let d = tmp();
    let key = openai(&mut Gen::named("pruned"));
    write(d.path(), "node_modules/pkg/index.js", &format!("k={key}\n"));
    write(d.path(), "build/out.txt", &format!("k={key}\n"));
    write(d.path(), ".vscode/settings.json", &format!("k={key}\n"));
    write(d.path(), ".idea/workspace.xml", &format!("k={key}\n"));
    let (out, err, code) = run(d.path(), &["scan", "."], "");
    assert_eq!(code, 1, "{err}");
    assert!(
        out.contains("settings.json") && out.contains("workspace.xml"),
        "{out}"
    );
    assert!(
        !out.contains("node_modules") && !out.contains("build"),
        "{out}"
    );
    let skipped = |dir: &str| {
        err.lines()
            .any(|l| l.starts_with("skipped  ") && l.contains(&format!("{dir}  (")))
    };
    assert!(skipped("node_modules") && skipped("build"), "{err}");
    // Named explicitly, a pruned directory is scanned.
    let (out, _, code) = run(d.path(), &["scan", "node_modules"], "");
    assert_eq!(code, 1);
    assert!(out.contains("index.js"), "{out}");
}

#[cfg(unix)]
#[test]
fn scan_reports_an_unreadable_directory() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp();
    write(d.path(), "locked/a.env", "x=1\n");
    let locked = d.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Running as root: permissions do not apply.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let (_, err, code) = run(d.path(), &["scan", "."], "");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("cannot read ./locked"), "{err}");
}
