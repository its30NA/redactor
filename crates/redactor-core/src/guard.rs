//! Placeholder guard: values that are templates, variables or filler,
//! not secrets.

/// Common stand-in values, compared case-insensitively after trimming quotes.
const PLACEHOLDER_WORDS: &[&str] = &[
    "changeme",
    "change_me",
    "change-me",
    "changeit",
    "replace_me",
    "replace-me",
    "replaceme",
    "null",
    "nil",
    "none",
    "true",
    "false",
    "undefined",
    "empty",
    "example",
    "placeholder",
    "redacted",
    "secret",
    "password",
    "passwd",
    "pass",
    "token",
    "apikey",
    "api_key",
    "todo",
    "tbd",
    "xxx",
    "your_api_key_here",
    "your_password",
    "yourpassword",
    "mypassword",
    "dummy",
    "test",
];

/// True when `value` is a template, a variable reference or filler. Earlier placeholders are
/// the engine's to tell (`Engine::is_placeholder`, labels it emits only): it keeps candidates
/// out of them before the guards run, so text merely framed like one is no exception here.
pub fn is_placeholder(value: &str) -> bool {
    let t = value.trim();
    // `'$ecretPlum'`: shells, YAML and `.env` files read `$` inside single quotes literally.
    let single_quoted = t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'');
    let v = t.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if v.is_empty() {
        return true;
    }
    // The shortcuts below are for plain words. Digits mixed into the letters
    // (`$ummer2024x`, `Tr0ub4dor-here`, `config.Zq81mXv0pLr3`) make a password
    // that merely looks like a variable, a template or a config path.
    if mixed_in_digits(v) {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if PLACEHOLDER_WORDS.contains(&lower.as_str()) {
        return true;
    }
    // your_api_key_here, <your-token>, insert_key_here, my-secret-here
    if lower.starts_with("your_")
        || lower.starts_with("your-")
        || lower.ends_with("_here")
        || lower.ends_with("-here")
    {
        return true;
    }
    // <token>, ${VAR}, {{ x }}, %(x)s, $(cmd)
    let wrapped = |open: &str, close: &str| v.starts_with(open) && v.ends_with(close);
    if wrapped("<", ">")
        || wrapped("${", "}")
        || wrapped("{{", "}}")
        || wrapped("%(", ")s")
        || wrapped("$(", ")")
        || wrapped("%", "%") && v.len() > 2 && !v[1..v.len() - 1].contains('%')
    {
        return true;
    }
    // $VAR / $env:VAR / $db_password / $Password / $API_KEY_V2. In single quotes only an
    // environment-style name (`'$DB_PASS'`, which Compose still expands) or a stand-in word
    // (`'$password'`) is one; `'$ecretPlum'` is a literal value.
    if let Some(rest) = v.strip_prefix('$') {
        let rest = rest.strip_prefix("env:").unwrap_or(rest);
        let b = rest.as_bytes();
        let name = !rest.is_empty()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            && !b[0].is_ascii_digit();
        let env_style = !b.iter().any(u8::is_ascii_lowercase);
        let word = PLACEHOLDER_WORDS.contains(&rest.to_ascii_lowercase().as_str());
        if name && (!single_quoted || env_style || word) {
            return true;
        }
    }
    // process.env.X, os.environ[...], os.getenv(...), secrets.X, vars.X, env.X
    for p in [
        "process.env.",
        "process.env[",
        "os.environ",
        "os.getenv(",
        "secrets.",
        "vars.",
        "env.",
        "getenv(",
        "config.",
        "settings.",
    ] {
        // `config.db.password` is a reference; `Config.2024!x` is a value that starts
        // like one. A prefix with a `.` holds a member path (names, then an optional call or
        // index at the end).
        if lower.starts_with(p) && (!p.ends_with('.') || member_path(&v[p.len()..])) {
            return true;
        }
    }
    // xxxx, ****, ...., ••••, ----
    if v.chars().count() >= 3
        && v.chars()
            .all(|c| matches!(c, 'x' | 'X' | '*' | '.' | '•' | '#' | '-' | '_' | '…'))
    {
        return true;
    }
    false
}

/// [`is_placeholder`] for `text[start..end]`, read with the single quotes around it when it
/// has them (`'$ecretPlum'` is a literal, `$ecretPlum` a variable).
pub fn is_placeholder_at(text: &str, start: usize, end: usize, value: &str) -> bool {
    let quoted = text[..start].ends_with('\'') && text[end..].starts_with('\'');
    if quoted && value.trim_start().starts_with('$') {
        is_placeholder(&format!("'{}'", value.trim()))
    } else {
        is_placeholder(value)
    }
}

/// `db.password`, `GITHUB_TOKEN`, `get('db.password')`, `items[0]`: names joined by `.`,
/// with an optional call or index at the end.
fn member_path(rest: &str) -> bool {
    let (path, tail) = match rest.find(['(', '[']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let closed = tail.is_empty() || tail.ends_with(')') || tail.ends_with(']');
    closed
        && !path.is_empty()
        && path.split('.').all(|seg| {
            seg.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

/// True when an alphanumeric run of `v` mixes digits into its letters: a letter after a
/// digit (`2024x`, `b6y`) or digits in a mixed-case word (`CedarQuill37`). Identifier
/// shapes stay plain: `API_KEY_V2`, `S3_BUCKET`, `oauth2`, `sha256`.
pub(crate) fn mixed_in_digits(v: &str) -> bool {
    v.split(|c: char| !c.is_ascii_alphanumeric()).any(|run| {
        let b = run.as_bytes();
        b.iter().any(u8::is_ascii_digit)
            && (b
                .windows(2)
                .any(|w| w[0].is_ascii_digit() && w[1].is_ascii_alphabetic())
                || b.iter().any(u8::is_ascii_uppercase) && b.iter().any(u8::is_ascii_lowercase))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        for v in [
            "changeme",
            "REPLACE_ME",
            "\"CHANGEME\"",
            "your_api_key_here",
            "<token>",
            "${DB_PASSWORD}",
            "{{ secret }}",
            "%(password)s",
            "$DB_PASS",
            "$env:API_KEY",
            "process.env.API_KEY",
            "os.environ['X']",
            "secrets.GITHUB_TOKEN",
            "xxxx",
            "****",
            "...",
            "null",
            "%PASSWORD%",
            "${S3_SECRET_V2}",
            "$API_KEY_V2",
            "settings.oauth2.client_secret",
            "config.db.password",
            "config.get('db.password')",
            "'$DB_PASS'",
            "'$password'",
        ] {
            assert!(is_placeholder(v), "{v}");
        }
    }

    #[test]
    fn real_values_pass() {
        for v in [
            "Hunter2-Fake-9x",
            "s3cr3t!",
            "$2b$12$abc",
            "x9$Lk",
            "$CedarQuill37",
            "%xx",
            "envoy",
            "$ummer2024x",
            "$tr0ngPassw0rd",
            "Tr0ub4dor-here",
            "your-Majesty-1982x",
            "config.Zq81mXv0pLr3",
            "Config.2024!x",
            "settings.2024-Summer",
            "'$ummer2024'",
            "'$ecretPlum'",
            "'$ecret-Plum47'",
            "%Tgb6yhn7%",
            "${Hunter2Fake9x}",
            // Earlier placeholders are the engine's to tell (known labels only).
            "[REDACTED-but-real-Hunter2x",
            "[REDACTED: Hunter2-Fake-9x]",
        ] {
            assert!(!is_placeholder(v), "{v}");
        }
    }
}
