//! `scrub check`, file decoding and config loading against the ways around them: renamed and
//! oddly named staged files, UTF-16 text, planted config files. Secret-shaped values are
//! generated at run time.

mod support;

use std::fs;
use std::path::Path;

use redactor_wire::text::{encode, TextEncoding};
use support::gen::Gen;
use support::run;
use tempfile::TempDir;

fn pw() -> String {
    ["Plum", "tree", "47", "-", "Kite"].concat()
}

fn openai(name: &str) -> String {
    format!("sk-{}", Gen::named(name).alnum(48))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {:?}", out);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A fresh repository with rename detection explicitly on, as most users have it. Git for Windows
/// refuses index paths with a newline unless `core.protectNTFS` is off; such names reach a
/// repository from other systems, so the odd-name tests stage them on Windows too.
fn repo() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["config", "diff.renames", "copies"]);
    git(d.path(), &["config", "core.protectNTFS", "false"]);
    d
}

fn put(dir: &Path, name: &str, content: impl AsRef<[u8]>) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, content).unwrap();
}

/// A 31-line example env file; `password` fills the one value.
fn env_file(password: &str) -> String {
    let mut s = String::new();
    for i in 0..30 {
        s.push_str(&format!("SETTING_{i}=value-{i}\n"));
    }
    s.push_str(&format!("DB_PASSWORD={password}\n"));
    s
}

// ---------------------------------------------------------------------------------------------
// check: what is staged, however it is named or moved
// ---------------------------------------------------------------------------------------------

#[test]
fn check_catches_a_renamed_and_edited_file() {
    let d = repo();
    put(d.path(), "app.env.example", env_file(""));
    git(d.path(), &["add", "app.env.example"]);
    git(d.path(), &["commit", "-q", "-m", "example"]);
    git(d.path(), &["mv", "app.env.example", "app.env"]);
    put(d.path(), "app.env", env_file(&pw()));
    git(d.path(), &["add", "app.env"]);
    // Git itself sees a rename (R9x) here.
    let status = git(d.path(), &["diff", "--cached", "--name-status", "-M"]);
    assert!(status.starts_with('R'), "{status}");

    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("app.env: 1 finding(s)"), "{err}");
    assert!(!err.contains(&pw()));
}

#[test]
fn check_catches_a_copied_file() {
    let d = repo();
    put(d.path(), "a.env", env_file(""));
    git(d.path(), &["add", "a.env"]);
    git(d.path(), &["commit", "-q", "-m", "a"]);
    put(d.path(), "b.env", env_file(&pw()));
    git(d.path(), &["add", "b.env"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("b.env: 1 finding(s)"), "{err}");
}

#[test]
fn check_ignores_deletions() {
    let d = repo();
    put(d.path(), "old.env", env_file(&pw()));
    git(d.path(), &["add", "old.env"]);
    git(d.path(), &["commit", "-q", "--no-verify", "-m", "old"]);
    git(d.path(), &["rm", "-q", "old.env"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("old.env"), "{err}");
}

/// Not on Windows: Git for Windows reads `0:` as a drive letter and never stores such a name, so
/// the trick cannot reach a repository there. The code under test is the same on every platform.
#[cfg(not(windows))]
#[test]
fn check_reads_by_blob_id_not_by_name() {
    let d = repo();
    // `git show :0:other.env` would read the clean `other.env`.
    put(d.path(), "other.env", "NOTHING=here\n");
    git(d.path(), &["add", "other.env"]);
    let secret = format!("DB_PASSWORD={}\n", pw());
    let id = {
        let p = d.path().join("payload.tmp");
        fs::write(&p, &secret).unwrap();
        let id = git(d.path(), &["hash-object", "-w", "payload.tmp"]);
        fs::remove_file(p).unwrap();
        id.trim().to_string()
    };
    // Staged directly into the index, so the name works on every platform.
    git(
        d.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{id},0:other.env"),
        ],
    );
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("0:other.env: 1 finding(s)"), "{err}");
}

#[test]
fn check_handles_names_with_spaces_newlines_and_non_ascii() {
    let d = repo();
    let secret = env_file(&pw());
    let p = d.path().join("payload.tmp");
    fs::write(&p, &secret).unwrap();
    let id = git(d.path(), &["hash-object", "-w", "payload.tmp"]);
    fs::remove_file(p).unwrap();
    for name in ["with space.env", "new\nline.env", "grüße/ü.env"] {
        git(
            d.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{},{name}", id.trim()),
            ],
        );
    }
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("with space.env: 1 finding(s)"), "{err}");
    assert!(err.contains("new\\nline.env: 1 finding(s)"), "{err}");
    assert!(err.contains("grüße/ü.env: 1 finding(s)"), "{err}");
}

#[test]
fn check_skips_submodules_with_a_line() {
    let d = repo();
    put(d.path(), "x.txt", "x\n");
    git(d.path(), &["add", "x.txt"]);
    git(d.path(), &["commit", "-q", "-m", "x"]);
    let head = git(d.path(), &["rev-parse", "HEAD"]);
    git(
        d.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},vendor/lib", head.trim()),
        ],
    );
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("skipped  vendor/lib  (submodule)"), "{err}");
}

/// A staged symlink is checked as git stores it: the target path. The file it points to is
/// not read (it is not part of the commit).
#[test]
fn check_reads_a_staged_symlink_as_its_target_path() {
    let d = repo();
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "secret.env", env_file(&pw()));
    let target = outside.path().join("secret.env");
    let target = target.to_string_lossy();
    let p = d.path().join("payload.tmp");
    fs::write(&p, target.as_bytes()).unwrap();
    let id = git(d.path(), &["hash-object", "-w", "payload.tmp"]);
    fs::remove_file(p).unwrap();
    git(
        d.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{},link.env", id.trim()),
        ],
    );
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0, "{err}");

    // A secret in the link text itself is found.
    let p = d.path().join("payload.tmp");
    fs::write(&p, format!("DB_PASSWORD={}", pw())).unwrap();
    let id = git(d.path(), &["hash-object", "-w", "payload.tmp"]);
    fs::remove_file(p).unwrap();
    git(
        d.path(),
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{},link.env", id.trim()),
        ],
    );
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("link.env (symlink target): 1 finding(s)"),
        "{err}"
    );
}

#[test]
fn check_skips_binary_with_a_line_and_scans_utf16() {
    let d = repo();
    let mut bin = vec![0u8, 1, 2];
    bin.extend_from_slice(openai("bin").as_bytes());
    put(d.path(), "b.bin", bin);
    put(
        d.path(),
        "ps.env",
        encode(&env_file(&pw()), TextEncoding::Utf16Le { bom: true }),
    );
    git(d.path(), &["add", "b.bin", "ps.env"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("skipped  b.bin  (binary)"), "{err}");
    assert!(err.contains("ps.env: 1 finding(s)"), "{err}");
}

#[test]
fn check_blocks_text_it_cannot_decode() {
    let d = repo();
    // A UTF-16LE BOM followed by an odd number of bytes.
    put(d.path(), "broken.txt", [0xFF, 0xFE, b'a', 0, b'b']);
    git(d.path(), &["add", "broken.txt"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("cannot check broken.txt"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// UTF-16 files in scan and sanitize
// ---------------------------------------------------------------------------------------------

#[test]
fn scan_finds_secrets_in_utf16_and_writes_utf16_back() {
    let d = tempfile::tempdir().unwrap();
    let key = openai("utf16");
    for (name, enc) in [
        ("le.txt", TextEncoding::Utf16Le { bom: true }),
        ("be.txt", TextEncoding::Utf16Be { bom: true }),
        ("bom8.txt", TextEncoding::Utf8 { bom: true }),
    ] {
        put(d.path(), name, encode(&format!("KEY={key}\r\nok\r\n"), enc));
    }
    let (out, err, code) = run(d.path(), &["scan", "."], "");
    assert_eq!(code, 1, "{err}");
    assert_eq!(out.lines().count(), 3, "{out}");
    assert!(err.contains("3 finding(s) found in 3 file(s)."), "{err}");

    let (_, err, code) = run(d.path(), &["scan", "--write", "."], "");
    assert_eq!(code, 0, "{err}");
    let le = fs::read(d.path().join("le.txt")).unwrap();
    assert_eq!(
        le,
        encode(
            "KEY=[REDACTED: OpenAI API Key]\r\nok\r\n",
            TextEncoding::Utf16Le { bom: true }
        )
    );
    let be = fs::read(d.path().join("be.txt")).unwrap();
    assert_eq!(&be[..2], &[0xFE, 0xFF]);
}

#[test]
fn scan_reports_every_skipped_file() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "img.bin", [0x89, b'P', b'N', b'G', 0, 0, 0, 1]);
    put(d.path(), "big.log", vec![b'a'; 5 * 1024 * 1024 + 1]);
    put(d.path(), "bad.txt", [0xEF, 0xBB, 0xBF, 0xC3]);
    let (_, err, code) = run(d.path(), &["scan", "."], "");
    assert!(err.contains("img.bin  (binary)"), "{err}");
    assert!(err.contains("big.log  (larger than 5 MiB)"), "{err}");
    assert!(
        err.contains("bad.txt  (starts with a UTF-8 (BOM) BOM"),
        "{err}"
    );
    assert_eq!(code, 2, "an undecodable text file is an error: {err}");
}

#[test]
fn sanitize_decodes_utf16_and_answers_in_kind() {
    let d = tempfile::tempdir().unwrap();
    let enc = TextEncoding::Utf16Le { bom: true };
    put(
        d.path(),
        "in.txt",
        encode(&format!("DB_PASSWORD={}\n", pw()), enc),
    );
    let f = d.path().join("in.txt").to_string_lossy().into_owned();
    let out = support::scrub_in(d.path()).arg(&f).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        encode("DB_PASSWORD=[REDACTED: Password]\n", enc)
    );
    // Binary input is refused, not passed through.
    put(d.path(), "b.bin", [0u8, 1, 2, 3, 0, 0]);
    let b = d.path().join("b.bin").to_string_lossy().into_owned();
    let (out, err, code) = run(d.path(), &[&b], "");
    assert_eq!((out.as_str(), code), ("", 2), "{err}");
    assert!(err.contains("binary"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Config files
// ---------------------------------------------------------------------------------------------

#[test]
fn a_planted_catch_all_allowlist_is_refused() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), ".redactor.toml", "allowlist = [\".*\"]\n");
    let sub = d.path().join("work");
    fs::create_dir_all(&sub).unwrap();
    let (out, err, code) = run(&sub, &[], &format!("DB_PASSWORD={}", pw()));
    assert_eq!(code, 2, "{err}");
    assert_eq!(out, "");
    assert!(err.contains("narrower pattern"), "{err}");
}

#[test]
fn config_above_the_repository_root_is_not_used() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), ".redactor.toml", "disabled = [\"context\"]\n");
    let r = d.path().join("repo");
    fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q"]);
    let (out, err, _) = run(&r, &[], &format!("DB_PASSWORD={}", pw()));
    assert!(!out.contains(&pw()), "{out}");
    assert!(!err.contains("using config"), "{err}");
}

#[test]
fn a_loaded_config_is_named_and_check_warns_about_weakening() {
    let d = repo();
    put(
        d.path(),
        "redactor.toml",
        "sensitivity = \"relaxed\"\nallowlist = [\"fixture\"]\ndisabled = [\"jwt\"]\n",
    );
    put(d.path(), "ok.txt", "nothing\n");
    git(d.path(), &["add", "ok.txt"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("scrub: using config: "), "{err}");
    assert!(err.contains("lowers detection"), "{err}");
    for key in ["allowlist: `fixture`", "disabled rules: jwt", "relaxed"] {
        assert!(err.contains(key), "{key}: {err}");
    }
    // A narrow allowlist still works.
    let fixture = format!("DB_PASSWORD=fixture{}", Gen::named("fx").alnum(12));
    let (out, _, _) = run(d.path(), &[], &fixture);
    assert_eq!(out, fixture);
}

#[test]
fn check_warns_about_a_template_of_its_own() {
    let d = repo();
    put(
        d.path(),
        "redactor.toml",
        "template = \"<{label}{suffix}>\"\n",
    );
    put(d.path(), "ok.txt", "nothing\n");
    git(d.path(), &["add", "ok.txt"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("lowers detection"), "{err}");
    assert!(err.contains("template = \"<{label}{suffix}>\""), "{err}");
}

#[cfg(unix)]
#[test]
fn a_world_writable_config_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "redactor.toml", "pii = true\n");
    let f = d.path().join("redactor.toml");
    fs::set_permissions(&f, fs::Permissions::from_mode(0o666)).unwrap();
    let (out, err, code) = run(d.path(), &[], "text");
    assert_eq!((out.as_str(), code), ("", 2), "{err}");
    assert!(err.contains("refusing config"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Binary or text: one stray NUL does not hide a file
// ---------------------------------------------------------------------------------------------

/// Text with one NUL byte and two secrets, as in a hand-edited env file.
fn nul_text() -> Vec<u8> {
    let mut raw = b"x\0\n".to_vec();
    raw.extend_from_slice(Gen::named("nul").github("ghp_").as_bytes());
    raw.extend_from_slice(format!("\nDB_PASSWORD={}\n", pw()).as_bytes());
    raw
}

#[test]
fn check_scans_text_with_a_stray_nul() {
    let d = repo();
    put(d.path(), "nul.env", nul_text());
    git(d.path(), &["add", "nul.env"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("nul.env: 2 finding(s)"), "{err}");
    assert!(!err.contains("skipped"), "{err}");
}

#[test]
fn check_blocks_large_text_with_a_nul_in_its_head() {
    let d = repo();
    let mut raw = nul_text();
    raw.resize(5 * 1024 * 1024 + 10, b'a');
    put(d.path(), "big.env", raw);
    git(d.path(), &["add", "big.env"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("cannot check big.env"), "{err}");
}

#[test]
fn check_scans_bomless_utf16_that_is_mostly_not_ascii() {
    let d = repo();
    let mut text = String::new();
    for _ in 0..20 {
        text.push_str("日本語のテキストと説明の行です\r\n");
    }
    text.push_str(&format!("パスワード\r\nDB_PASSWORD={}\r\n", pw()));
    put(
        d.path(),
        "ja.txt",
        encode(&text, TextEncoding::Utf16Le { bom: false }),
    );
    git(d.path(), &["add", "ja.txt"]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("ja.txt: 1 finding(s)"), "{err}");
}

#[test]
fn scan_finds_secrets_next_to_a_nul_and_write_leaves_the_file_alone() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "nul.env", nul_text());
    let (out, err, code) = run(d.path(), &["scan", "."], "");
    assert_eq!(code, 1, "{err}");
    assert!(out.contains("nul.env: 2 finding(s)"), "{out}");
    let (_, err, code) = run(d.path(), &["scan", "--write", "."], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("contains NUL bytes, left unchanged"), "{err}");
    assert_eq!(fs::read(d.path().join("nul.env")).unwrap(), nul_text());
}

// ---------------------------------------------------------------------------------------------
// A committed config cannot switch the check off
// ---------------------------------------------------------------------------------------------

fn staged_github_and_aws(d: &Path) {
    let mut g = Gen::named("weakened");
    let aws = format!(
        "{}{}",
        ["AK", "IA"].concat(),
        g.from(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567", 16)
    );
    put(
        d,
        "keys.env",
        format!("GH={}\nAWS_ACCESS_KEY_ID={aws}\n", g.github("ghp_")),
    );
    git(d, &["add", "keys.env"]);
}

#[test]
fn a_committed_prefix_wide_allowlist_is_refused() {
    let d = repo();
    put(
        d.path(),
        "redactor.toml",
        "allowlist = [\"^(ghp_|gho_|github_pat_|AKIA|sk-|xox[bpa]-|glpat-)\"]\n",
    );
    staged_github_and_aws(d.path());
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("narrower pattern"), "{err}");
}

#[test]
fn a_config_that_disables_every_rule_is_an_error_for_check() {
    let d = repo();
    let mut ids: Vec<String> = redactor_core::rule_infos()
        .iter()
        .map(|i| format!("{:?}", i.id))
        .collect();
    ids.extend(["\"context\"", "\"classifier\"", "\"terms\""].map(String::from));
    put(
        d.path(),
        "redactor.toml",
        format!("disabled = [{}]\n", ids.join(", ")),
    );
    staged_github_and_aws(d.path());
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("unable to block anything"), "{err}");
    assert!(
        err.contains("more)"),
        "the disabled list is named, capped: {err}"
    );
}

#[test]
fn check_names_every_allowlist_entry_and_a_narrow_one_keeps_working() {
    let d = repo();
    put(
        d.path(),
        "redactor.toml",
        "allowlist = [\"fixture\", \"^example\\\\.invalid$\"]\n",
    );
    staged_github_and_aws(d.path());
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("allowlist: `fixture`, `^example\\.invalid$`"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------------------------
// check: git config that changes git's output does not change what is checked
// ---------------------------------------------------------------------------------------------

#[test]
fn check_from_a_subfolder_sees_the_whole_index_despite_diff_relative() {
    let d = repo();
    for (k, v) in [
        ("diff.relative", "true"),
        ("diff.noprefix", "true"),
        ("diff.mnemonicPrefix", "true"),
        ("core.quotePath", "true"),
        ("color.ui", "always"),
        ("color.diff", "always"),
        ("diff.external", "false"),
    ] {
        git(d.path(), &["config", k, v]);
    }
    put(d.path(), "sub/ok.txt", "nothing\n");
    put(d.path(), "top.env", env_file(&pw()));
    put(d.path(), "grüße.env", env_file(&pw()));
    git(d.path(), &["add", "sub/ok.txt", "top.env", "grüße.env"]);
    let (_, err, code) = run(&d.path().join("sub"), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("top.env: 1 finding(s)"), "{err}");
    assert!(err.contains("grüße.env: 1 finding(s)"), "{err}");
}

#[test]
fn check_reads_the_staged_blob_not_a_replacement() {
    let d = repo();
    put(d.path(), "app.env", env_file(&pw()));
    git(d.path(), &["add", "app.env"]);
    let staged = git(d.path(), &["rev-parse", ":app.env"]);
    put(d.path(), "clean.tmp", "NOTHING=here\n");
    let clean = git(d.path(), &["hash-object", "-w", "clean.tmp"]);
    fs::remove_file(d.path().join("clean.tmp")).unwrap();
    git(d.path(), &["replace", staged.trim(), clean.trim()]);
    let (_, err, code) = run(d.path(), &["check"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("app.env: 1 finding(s)"), "{err}");
}

#[test]
fn check_outside_a_repository_prints_one_line() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x.toml", "pii = true\n");
    let (out, err, code) = run(d.path(), &["check", "-c", "x.toml"], "");
    assert_eq!((out.as_str(), code), ("", 2), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.contains("not inside a git work tree"), "{err}");
}
