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
    let v = value
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`');
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
    // $VAR / $env:VAR / $db_password / $Password / $API_KEY_V2.
    if let Some(rest) = v.strip_prefix('$') {
        let rest = rest.strip_prefix("env:").unwrap_or(rest);
        let b = rest.as_bytes();
        if !rest.is_empty()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            && !b[0].is_ascii_digit()
        {
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
        if lower.starts_with(p) {
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
