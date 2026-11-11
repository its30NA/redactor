//! HTTP headers, URLs, query parameters, connection strings and credential files.

use super::{check, validate, Bound, Family, Hit, Rule, Verdict};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Http, id, label, shape, anchors, re, conf)
}

/// A cookie header value is `name=value[; name=value]`, not prose.
/// A documented field, not a value: `name=<value>`, `token=\|code=…` (a Markdown table that
/// lists the fields a rule reads).
fn described(h: &Hit) -> Verdict {
    let s = h.secret();
    let elided = s.ends_with('…') || s.ends_with("...");
    if s.starts_with(['<', '\\', '|']) || elided {
        Verdict::Reject
    } else {
        Verdict::Keep
    }
}

/// `.npmrc` shape: the setting starts its line or follows a registry scope's `:`
/// (`//registry.npmjs.org/:_authToken=…`); `self._password = v` is code.
fn npmrc_line(h: &Hit) -> Verdict {
    let before = h.before(256);
    let line = before.rsplit('\n').next().unwrap_or("");
    if line.trim().is_empty() || line.ends_with(':') {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

fn cookie_pairs(h: &Hit) -> Verdict {
    let s = h.secret();
    let name = s.split('=').next().unwrap_or("");
    let name_ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-$%".contains(&b));
    if s.contains('=') && name_ok {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

/// Basic credentials are base64 of `user:password`.
fn basic_auth(h: &Hit) -> Verdict {
    match validate::b64_decode(h.secret()) {
        Some(b) if b.contains(&b':') && std::str::from_utf8(&b).is_ok() => {
            Verdict::Conf(0.95, "Authorization: Basic with base64 user:password")
        }
        _ => Verdict::Keep,
    }
}

/// `Password=` is a connection-string password only on a line with other connection keys.
fn connection_string(h: &Hit) -> Verdict {
    // `Password=${DB_PASSWORD};`: a template, not a value.
    if ["${", "$(", "{{"].iter().any(|t| h.secret().starts_with(t)) {
        return Verdict::Reject;
    }
    let line = h.line();
    if !line.contains(';') {
        return Verdict::Reject;
    }
    let line = line.to_ascii_lowercase();
    let keys = [
        "server=",
        "data source=",
        "host=",
        "uid=",
        "user id=",
        "userid=",
        "database=",
        "driver=",
        "initial catalog=",
        "dsn=",
        "user=",
        "username=",
        "provider=",
    ];
    if line.contains(';') && keys.iter().any(|k| line.contains(k)) {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

/// A `.pgpass` host field: a hostname, an address or `*`, never a pure number (times).
fn pgpass(h: &Hit) -> Verdict {
    let host = h.text[h.start..h.end].split(':').next().unwrap_or("");
    // `/etc/shadow`: `name:hash:19800:0:99999:7:::` ends in day counts and empty fields.
    let pw = &h.text[h.s_start..h.s_end];
    if pw.contains(':') && pw.bytes().all(|b| b.is_ascii_digit() || b == b':') {
        return Verdict::Reject;
    }
    if host
        .bytes()
        .any(|b| b.is_ascii_alphabetic() || b == b'.' || b == b'*')
    {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

/// Longest password with `/`, `?` or `#` in it that the userinfo scan reads.
const MAX_SPECIAL_PASSWORD: usize = 256;

/// The `user:password@` after `scheme://`, read in code so that neither part has a length
/// limit. The password is everything up to the last `@` that a host character follows.
/// Neither scan crosses a `/`, and every `://` holds one, so the work stays linear. A
/// password with `/`, `?` or `#` in it (`app:Xy7/kL9@db`) is read past them, at most
/// [`MAX_SPECIAL_PASSWORD`] bytes, unless what stands before the first of them is a port
/// (`host:8443/users/@me`). A long random userinfo without a colon is a token
/// (`https://<token>@git.example.com/…`).
fn userinfo(text: &str, _start: usize, end: usize) -> Option<(usize, usize, usize)> {
    let rest = &text[end..];
    let user = rest
        .find(|c: char| c.is_whitespace() || ":/?#@\"'<>[]".contains(c))
        .unwrap_or(rest.len());
    let host = |c: u8| c.is_ascii_alphanumeric() || b".[]_-".contains(&c);
    if rest[user..].starts_with('@') {
        let tok = &rest[..user];
        let token = user >= 20
            && rest.as_bytes().get(user + 1).is_some_and(|&c| host(c))
            && tok.bytes().any(|c| c.is_ascii_digit())
            && tok.bytes().any(|c| c.is_ascii_alphabetic())
            && crate::context::value_score(tok) >= 0.5;
        return token.then_some((end, end + user, end + user + 1));
    }
    if !rest[user..].starts_with(':') {
        return None;
    }
    let ss = end + user + 1;
    let rest = &text[ss..];
    let b = rest.as_bytes();
    let run = rest
        .find(|c: char| c.is_whitespace() || "/?#\"'<>".contains(c))
        .unwrap_or(rest.len());
    let last_at = |run: usize| {
        (1..run.saturating_sub(1))
            .rev()
            .find(|&i| b[i] == b'@' && host(b[i + 1]))
    };
    if let Some(at) = last_at(run) {
        return Some((ss, ss + at, ss + at + 1));
    }
    // `app:Xy7/kL9mQ2@db`: the password goes on past `/`, `?`, `#` to an `@` and a host.
    if run == 0 || b[..run].iter().all(u8::is_ascii_digit) || run == rest.len() {
        return None;
    }
    let mut hi = (run + MAX_SPECIAL_PASSWORD).min(rest.len());
    while !rest.is_char_boundary(hi) {
        hi -= 1;
    }
    let wide = run
        + rest[run..hi]
            .find(|c: char| c.is_whitespace() || "\"'<>".contains(c))
            .unwrap_or(hi - run);
    // The first `@` with a host after it; a later one belongs to the path or query.
    let at = (run..wide.saturating_sub(1)).find(|&i| b[i] == b'@' && host(b[i + 1]))?;
    if rest[..at].contains("://") {
        return None;
    }
    // The host runs to a port, a path, a query or the end.
    let he = (at + 1..wide).find(|&i| !host(b[i])).unwrap_or(wide);
    let after_host = b.get(he).copied();
    if !(he == wide || matches!(after_host, Some(b':' | b'/' | b'?' | b'#'))) {
        return None;
    }
    Some((ss, ss + at, ss + at + 1))
}

/// Pagination cursors are opaque but not secret: `page_token`, `next_token`.
fn not_a_cursor(h: &Hit) -> Verdict {
    let name = h.text[h.start + 1..h.s_start - 1].to_ascii_lowercase();
    let cursor = [
        "page",
        "next",
        "prev",
        "continuation",
        "sync",
        "cursor",
        "pagination",
        "start",
    ]
    .iter()
    .any(|p| name.starts_with(p));
    if cursor {
        Verdict::Reject
    } else {
        described(h)
    }
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "auth_bearer",
            "Bearer Token",
            "Authorization: Bearer <token>",
            &["authorization"],
            r#"(?i:(?:proxy-)?authorization)["']?\s*[:=]\s*["']?(?i:bearer)\s+(?P<s>[A-Za-z0-9._~+/-]{8,}=*)"#,
            0.95,
        )
        .ci()
        .back(6)
        .fwd(16384)
        .bounds(Bound::WordDash, Bound::Word),
        r(
            "auth_basic",
            "HTTP Basic Credentials",
            "Authorization: Basic <base64 user:password>",
            &["authorization"],
            r#"(?i:(?:proxy-)?authorization)["']?\s*[:=]\s*["']?(?i:basic)\s+(?P<s>[A-Za-z0-9+/]{8,}={0,2})"#,
            0.8,
        )
        .ci()
        .back(6)
        .fwd(2048)
        .bounds(Bound::WordDash, Bound::Base64)
        .check(basic_auth),
        r(
            "auth_token_scheme",
            "Access Token",
            "Authorization: Token <token>",
            &["authorization"],
            r#"(?i:(?:proxy-)?authorization)["']?\s*[:=]\s*["']?(?i:token)\s+(?P<s>[A-Za-z0-9._~+/-]{8,}=*)"#,
            0.9,
        )
        .ci()
        .back(6)
        .fwd(4096)
        .bounds(Bound::WordDash, Bound::Word),
        r(
            "auth_digest_response",
            "HTTP Digest Response",
            "Authorization: Digest … response=\"<hex>\"",
            &["authorization"],
            r#"(?i:(?:proxy-)?authorization)\s*:\s*(?i:digest)\s[^\r\n]{0,600}?(?i:response)\s*=\s*"?(?P<s>[0-9a-fA-F]{32,})"#,
            0.85,
        )
        .ci()
        .back(6)
        .fwd(900)
        .bounds(Bound::WordDash, Bound::None),
        r(
            "auth_aws4_signature",
            "AWS Signature",
            "AWS4-HMAC-SHA256 … Signature=<64 hex>",
            &["aws4-hmac-sha256"],
            r"(?i:aws4-hmac-sha256)[^\r\n]{0,600}?Signature=(?P<s>[0-9a-f]{64,})",
            0.95,
        )
        .ci()
        .fwd(900)
        .bounds(Bound::Word, Bound::Word),
        r(
            "auth_ntlm",
            "NTLM/Negotiate Token",
            "Authorization: NTLM|Negotiate <base64>",
            &["authorization"],
            r#"(?i:(?:proxy-)?authorization)["']?\s*:\s*["']?(?i:ntlm|negotiate)\s+(?P<s>[A-Za-z0-9+/]{20,}={0,2})"#,
            0.9,
        )
        .ci()
        .back(6)
        .fwd(8192)
        .bounds(Bound::WordDash, Bound::Base64),
        r(
            "bearer_token",
            "Bearer Token",
            "Bearer <20+ token chars>",
            &["bearer"],
            r"(?i:bearer)\s+(?P<s>[A-Za-z0-9._~+/-]{20,}=*)",
            0.8,
        )
        .ci()
        .fwd(16384)
        .check(check::random),
        r(
            "api_key_header",
            "API Key",
            "X-Api-Key / api-key / x-goog-api-key / Ocp-Apim-Subscription-Key: <key>",
            &["api-key", "ocp-apim-subscription-key"],
            r#"(?i:(?:x-goog-|x-)?api-key|ocp-apim-subscription-key)["']?\s*:\s*["']?(?P<s>[A-Za-z0-9._~+/=-]{8,})"#,
            0.9,
        )
        .ci()
        .back(7)
        .fwd(1024)
        .bounds(Bound::WordDash, Bound::Word),
        r(
            "auth_token_header",
            "Access Token",
            "X-Auth-Token / X-Access-Token / PRIVATE-TOKEN / X-Vault-Token / X-Shopify-Access-Token: <token>",
            &[
                "x-auth-token",
                "x-access-token",
                "private-token",
                "x-vault-token",
                "x-shopify-access-token",
            ],
            r#"(?i:x-auth-token|x-access-token|private-token|x-vault-token|x-shopify-access-token)["']?\s*:\s*["']?(?P<s>[A-Za-z0-9._~+/=-]{8,})"#,
            0.9,
        )
        .ci()
        .fwd(4096)
        .bounds(Bound::WordDash, Bound::Word),
        r(
            "cookie_header",
            "HTTP Cookie",
            "Cookie: <rest of line>",
            &["cookie"],
            r#"(?i:cookie)["']?\s*:\s*["']?(?P<s>[^\r\n"']{4,})"#,
            0.85,
        )
        .ci()
        .fwd(16384)
        .bounds(Bound::WordDash, Bound::None)
        .check(cookie_pairs),
        r(
            "set_cookie_header",
            "HTTP Cookie",
            "Set-Cookie: name=<value>",
            &["set-cookie"],
            r#"(?i:set-cookie)["']?\s*:\s*["']?[^=\s;"']{1,128}=(?P<s>[^;\r\n"']{8,})"#,
            0.8,
        )
        .ci()
        .fwd(8192)
        .bounds(Bound::WordDash, Bound::None)
        .check(described),
        r(
            "session_id",
            "Session ID",
            "PHPSESSID/JSESSIONID/session_id/sid = 16+ chars",
            &["phpsessid", "jsessionid", "sessionid", "session_id", "sessid", "connect.sid", "sid"],
            r#"(?i:phpsessid|jsessionid|session_?id|sessid|connect\.sid|sid)["']?\s*[=:]\s*["']?(?P<s>[A-Za-z0-9%._+/-]{16,})"#,
            0.75,
        )
        .ci()
        .back(8)
        .fwd(1024)
        .check(check::random),
        r(
            "url_userinfo_password",
            "Password in URL",
            "scheme://user:<password>@host",
            &["://"],
            r"[A-Za-z][A-Za-z0-9+.-]{0,30}://",
            0.95,
        )
        .back(32)
        .fwd(3)
        .bounds(Bound::Word, Bound::None)
        .tail(userinfo),
        r(
            "url_query_token",
            "Access Token",
            "?access_token=|reset_token=|…_token=|token=|code=… in a URL",
            &["token=", "code="],
            r#"[?&#;](?i:(?:[a-z0-9]{1,24}[_-]){0,3}token|code)=(?P<s>[^&;\s"'<>#]{8,})"#,
            0.85,
        )
        .ci()
        .back(80)
        .fwd(8192)
        .bounds(Bound::None, Bound::None)
        .check(not_a_cursor),
        r(
            "jdbc_oracle_password",
            "Password",
            "jdbc:oracle:thin:user/<password>@host",
            &["jdbc:oracle:"],
            r#"(?i:jdbc:oracle:(?:thin|oci8?)):[A-Za-z0-9_$#.]{1,128}/(?P<s>[^@\s"'<>]{1,256})@"#,
            0.9,
        )
        .ci()
        .fwd(512)
        .bounds(Bound::None, Bound::None)
        .check(described),
        r(
            "url_query_client_secret",
            "Client Secret",
            "?client_secret=… in a URL",
            &["client_secret="],
            r#"[?&#;](?i:client_secret)=(?P<s>[^&;\s"'<>#]{6,})"#,
            0.95,
        )
        .ci()
        .back(1)
        .fwd(1024)
        .bounds(Bound::None, Bound::None)
        .check(described),
        r(
            "url_query_key",
            "API Key",
            "?key=|api_key=|apikey=… in a URL (16+ chars)",
            &["key="],
            r#"[?&#;](?i:key|api_key|apikey|api-key|access_key|subscription-key)=(?P<s>[^&;\s"'<>#]{16,})"#,
            0.8,
        )
        .ci()
        .back(14)
        .fwd(1024)
        .bounds(Bound::None, Bound::None)
        .check(check::random),
        r(
            "url_query_password",
            "Password",
            "?password=|passwd=|pwd=|pass=… in a URL",
            &["password=", "passwd=", "pwd=", "pass=", "pw="],
            r#"[?&#;](?i:password|passwd|pwd|pass|pw)=(?P<s>[^&;\s"'<>#]{1,})"#,
            0.9,
        )
        .ci()
        .back(1)
        .fwd(1024)
        .bounds(Bound::None, Bound::None)
        .check(described),
        r(
            "url_query_signature",
            "Signature",
            "?sig=|signature=… in a URL (16+ chars)",
            &["sig=", "signature="],
            r#"[?&#;](?i:sig|signature)=(?P<s>[^&;\s"'<>#]{16,})"#,
            0.75,
        )
        .ci()
        .back(1)
        .fwd(2048)
        .bounds(Bound::None, Bound::None),
        r(
            "connection_string_password",
            "Password",
            "Server=…;User Id=…;Password=<value>;",
            &["password", "pwd"],
            // A quote inside the value (`P@ss'word1`) is a character of it; one right
            // before the `;` or the end closes a quoted value.
            r#"(?i:password|pwd)\s*=\s*["'{]?(?P<s>[^;"'}\r\n]+(?:'[^;"'}\s]+)*)"#,
            0.95,
        )
        .ci()
        .fwd(1024)
        .bounds(Bound::Word, Bound::None)
        .check(connection_string),
        r(
            "pgpass_line",
            "Password",
            "host:port:database:user:<password> (.pgpass)",
            &[],
            r"(?m)^[^:\s#][^:\s]{0,252}:(?:[0-9]{1,5}|\*):[^:\s]{1,63}:[^:\s]{1,63}:(?P<s>[^\r\n]{4,})",
            0.8,
        )
        .bounds(Bound::None, Bound::LineEnd)
        .check(pgpass),
        r(
            "docker_config_auth",
            "Registry Credentials",
            "\"auth\": \"<base64 user:password>\" (Docker config)",
            &["\"auth\"", "\"identitytoken\""],
            r#""(?:auth|identitytoken)"\s*:\s*"(?P<s>[A-Za-z0-9+/=._-]{8,})"#,
            0.95,
        )
        .fwd(8192)
        .bounds(Bound::None, Bound::Quote),
        r(
            "npmrc_auth",
            "npm Registry Token",
            "_authToken=… / _auth=… / _password=… (.npmrc)",
            &["_authToken", "_auth", "_password"],
            r#"(?:_authToken|_auth|_password)\s*=\s*["']?(?P<s>[^\s"']{8,})"#,
            0.95,
        )
        .fwd(2048)
        .bounds(Bound::Word, Bound::None)
        .check(npmrc_line),
    ]
}
