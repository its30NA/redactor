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

/// True when `value` is a template, a variable reference, filler or an earlier placeholder.
pub fn is_placeholder(value: &str) -> bool {
    let v = value
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if v.is_empty() {
        return true;
    }
    if v.starts_with("[REDACTED") {
        return true;
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
    // $VAR / $env:VAR / $db_password / $Password. A capitalized name with digits
    // (`$CedarQuill37`) is how a password starts with `$`, not how a variable is named.
    if let Some(rest) = v.strip_prefix('$') {
        let env = rest.strip_prefix("env:");
        let rest = env.unwrap_or(rest);
        let b = rest.as_bytes();
        let variable = env.is_some()
            || !b.iter().any(|c| c.is_ascii_lowercase())
            || b.first()
                .is_some_and(|c| c.is_ascii_lowercase() || *c == b'_')
            || !b.iter().any(|c| c.is_ascii_digit());
        if !rest.is_empty()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            && !b[0].is_ascii_digit()
            && variable
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
            "[REDACTED: Password]",
            "%PASSWORD%",
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
        ] {
            assert!(!is_placeholder(v), "{v}");
        }
    }
}
