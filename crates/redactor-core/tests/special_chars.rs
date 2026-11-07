//! After a clear secret key, every value that is legal in its syntax is found at Balanced,
//! whatever special characters it holds and at any length. References, templates and
//! placeholders after the same keys are not.
//!
//! The values are built at test time: a password of two words and two digits with one special
//! character at its start, in its middle or at its end, for nine syntaxes. Each syntax leaves
//! out the characters it forbids or reinterprets at that spot itself (a blank ends an unquoted
//! value, `#` starts a comment, `"` closes a JSON string, a shell reads `$` and `(`).

mod support;

use redactor_core::{Engine, EngineConfig, ScanOutcome, ScanRequest, Sensitivity};
use support::gen::Gen;

/// The 32 ASCII special characters.
const SPECIALS: &str = "!@#$%^&*()_+-=[]{}|;:',.<>/?`~\\\"";

/// Name, text with `{v}` for the value, and the characters the syntax forbids or reinterprets
/// in a value.
const SYNTAXES: &[(&str, &str, &str)] = &[
    ("env", "DB_PASSWORD={v}\n", " #\"'`$\\"),
    ("env_dq", "DB_PASSWORD=\"{v}\"\n", "\"\\$`"),
    // YAML plain scalars: indicators at the start, `: ` and ` #` inside, flow characters.
    ("yaml", "password: {v}\n", " #'\"&*!|>%@`{}[],?:-"),
    ("yaml_dq", "password: \"{v}\"\n", "\"\\"),
    ("json", "{\"password\": \"{v}\"}\n", "\"\\"),
    ("ini", "password = {v}\n", " ;#\"'"),
    // A shell word: quotes, expansions, operators and redirections are the shell's.
    (
        "flag",
        "mysql -u app --password={v} db\n",
        " \"'`$\\&|;<>()#",
    ),
    // A sentence: quotes, brackets and sentence punctuation belong to the prose.
    ("prose", "the password is {v} for now\n", " \"'`.,;:!?()"),
    ("logfmt", "level=warn password={v} user=app\n", " \"="),
];

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scan(e: &Engine, text: &str) -> ScanOutcome {
    e.scan(
        text,
        &ScanRequest {
            sensitivity: Some(Sensitivity::Balanced),
            ..ScanRequest::default()
        },
    )
}

/// Bytes of `[s, e)` that no redacted finding covers.
fn uncovered(out: &ScanOutcome, s: usize, e: usize) -> usize {
    let mut spans: Vec<(usize, usize)> = out
        .findings
        .iter()
        .filter(|f| !f.kept)
        .map(|f| (f.start, f.end))
        .collect();
    spans.sort_unstable();
    let mut pos = s;
    for (fs, fe) in spans {
        if fs <= pos && fe > pos {
            pos = fe;
        }
    }
    e.saturating_sub(pos)
}

/// Scans `template` with `value` in place of `{v}`; `Some(description)` when part of the value
/// is left in the output or the result is not verified.
fn leak(e: &Engine, name: &str, template: &str, value: &str) -> Option<String> {
    let at = template.find("{v}").unwrap();
    let text = template.replace("{v}", value);
    let out = scan(e, &text);
    let left = uncovered(&out, at, at + value.len());
    (left > 0 || !out.verified).then(|| {
        let shown: String = value.chars().take(40).collect();
        format!(
            "{name} {shown:?} (len {}): {left} bytes left, verified={}, {:?}",
            value.len(),
            out.verified,
            out.findings
                .iter()
                .map(|f| (
                    f.start - at.min(f.start),
                    f.end - at.min(f.end),
                    f.detector.as_str()
                ))
                .collect::<Vec<_>>()
        )
    })
}

fn allowed(excluded: &str) -> impl Iterator<Item = char> + '_ {
    SPECIALS.chars().filter(move |c| !excluded.contains(*c))
}

/// `Cedar`, `Quill` and `37`, with `c` at the start, in the middle or at the end.
fn with_special(c: char) -> [String; 3] {
    let (a, b, n) = ("Cedar", "Quill", "37");
    [
        format!("{c}{a}{b}{n}"),
        format!("{a}{c}{b}{n}"),
        format!("{a}{b}{n}{c}"),
    ]
}

#[test]
fn special_characters_after_secret_keys_are_found() {
    let e = engine();
    let mut total = 0;
    let mut failed = Vec::new();
    for &(name, template, excluded) in SYNTAXES {
        for c in allowed(excluded) {
            for v in with_special(c) {
                total += 1;
                failed.extend(leak(&e, name, template, &v));
            }
        }
    }
    assert_eq!(total, 675, "the matrix changed size");
    assert!(
        failed.is_empty(),
        "{} of {total} leak:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// Bracket pairs inside a password: balanced, so neither a truncated expression nor a
/// structure.
#[test]
fn balanced_pairs_are_found() {
    let e = engine();
    let mut failed = Vec::new();
    for (template, v) in [
        ("PGPASSWORD={v}\n", "(Orchid)Lantern42"),
        ("DB_PASSWORD={v}\n", "p{ss}w0rd!x9"),
        ("password={v}\n", "Tulip[Harbor]-91"),
    ] {
        failed.extend(leak(&e, template.trim(), template, v));
    }
    for &(name, template, excluded) in SYNTAXES {
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')] {
            if excluded.contains(open) || excluded.contains(close) {
                continue;
            }
            for v in [
                format!("{open}Orchid{close}Lantern42"),
                format!("Tulip{open}Harbor{close}91"),
                format!("Cedar{open}Quill{close}{open}37{close}x"),
            ] {
                failed.extend(leak(&e, name, template, &v));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// A passphrase-like value: words of letters joined by the syntax's own special characters,
/// ending in two digits. It reads as words, not as a random string, so only the key finds it.
fn passphrase(g: &mut Gen, excluded: &str, n: usize) -> String {
    let seps: Vec<char> = allowed(excluded).collect();
    let mut s = String::with_capacity(n + 16);
    while s.len() + 2 < n {
        let len = 3 + g.below(6);
        let word = g.from(b"abcdefghijklmnopqrstuvwxyz", len);
        let mut cs = word.chars();
        if g.below(3) == 0 {
            s.extend(cs.next().map(|c| c.to_ascii_uppercase()));
        }
        s.extend(cs);
        s.push(seps[g.below(seps.len())]);
    }
    s.truncate(n - 2);
    // Ends on a word character: a lone special at the end is the matrix's job.
    while s.ends_with(|c: char| !c.is_ascii_alphanumeric()) {
        s.pop();
        s.insert(0, 'k');
    }
    s.push_str("37");
    s
}

#[test]
fn long_values_after_secret_keys_are_found() {
    let e = engine();
    let mut g = Gen::named("special chars long values");
    let mut failed = Vec::new();
    for &(name, template, excluded) in SYNTAXES {
        for n in [600, 2048, 65536] {
            let v = passphrase(&mut g, excluded, n);
            assert_eq!(v.len(), n);
            failed.extend(leak(&e, name, template, &v));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Code, references, templates and placeholders after the same keys: not secrets.
#[test]
fn references_and_placeholders_are_not_found() {
    let e = engine();
    let mut failed = Vec::new();
    for text in [
        // Code
        "password=getPassword()\n",
        "password = config.get(\"db\", \"password\")\n",
        "password=os.environ[\"DB_PW\"]\n",
        "password=process.env.DB_PASSWORD\n",
        "password=ENV['DB_PASSWORD']\n",
        // Templates and references
        "password: {{ .Values.db.password }}\n",
        "password=${DB_PASSWORD}\n",
        "{\"password\": \"$DB_PASSWORD\"}\n",
        "password=%(db_password)s\n",
        "password={db_password}\n",
        "DB_PASSWORD=${DB_PASSWORD}\n",
        "mysql -u app --password=${DB_PASSWORD} db\n",
        "level=warn password=${DB_PASSWORD} user=app\n",
        // Placeholders and empty values
        "password: <your-password>\n",
        "password=[REDACTED]\n",
        "password=********\n",
        "password: !vault |\n",
        "password=(none)\n",
        "password=null\n",
        "password: \"\"\n",
        "password=[]\n",
        "password={}\n",
        "DB_PASSWORD=[]\n",
        "DB_PASSWORD=(none)\n",
        "level=warn password=[REDACTED] user=app\n",
        "mysql -u app --password=<your-password> db\n",
        // Prose
        "set the password (see below)\n",
    ] {
        let out = scan(&e, text);
        if !out.findings.is_empty() {
            failed.push(format!(
                "{text:?}: {:?}",
                out.findings
                    .iter()
                    .map(|f| &text[f.start..f.end])
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
