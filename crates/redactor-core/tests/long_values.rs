//! Secrets are found at any length: a detector whose pattern or window has an upper bound must
//! not let a longer value through silently, and glued URL chains are found in one pass.

mod support;

use redactor_core::{Engine, EngineConfig, ScanOutcome, ScanRequest, Sensitivity, WarningKind};
use support::gen::*;

fn scan(text: &str) -> ScanOutcome {
    scan_at(text, Sensitivity::Balanced)
}

fn scan_at(text: &str, s: Sensitivity) -> ScanOutcome {
    Engine::new(EngineConfig::default()).unwrap().scan(
        text,
        &ScanRequest {
            sensitivity: Some(s),
            ..ScanRequest::default()
        },
    )
}

/// What a value looks like.
#[derive(Clone, Copy)]
enum Kind {
    Alnum,
    Base64,
    Hex,
}

fn value(g: &mut Gen, kind: Kind, n: usize) -> String {
    match kind {
        Kind::Alnum => g.alnum(n),
        // Starts and ends on a letter or digit: the classifier trims `+` and `/` at the ends
        // of a token, which is its own rule and not what this test is about.
        Kind::Base64 => {
            let mut s = b64_encode(&g.bytes(n * 3 / 4 + 3));
            s.truncate(n - 2);
            format!("{}{s}{}", g.alnum(1), g.alnum(1))
        }
        Kind::Hex => g.hex(n),
    }
}

/// One text per detector with an upper bound in its pattern or its window, `{v}` standing for
/// the value. Grouped by rule family.
const CASES: &[(&str, &str, Kind)] = &[
    // HTTP, URLs and connection strings
    (
        "url redis",
        "redis://default:{v}@cache.example:6379\n",
        Kind::Alnum,
    ),
    (
        "url env",
        "DATABASE_URL=postgres://app:{v}@db.example:5432/app\n",
        Kind::Alnum,
    ),
    (
        "url credentials file",
        "https://deploy:{v}@vcs.example\n",
        Kind::Alnum,
    ),
    (
        "url in prose",
        "connect with amqp://svc:{v}@mq.example/vhost today\n",
        Kind::Alnum,
    ),
    ("bearer header", "Authorization: Bearer {v}\n", Kind::Alnum),
    ("bearer", "curl -H 'Bearer {v}'\n", Kind::Alnum),
    ("basic header", "Authorization: Basic {v}\n", Kind::Base64),
    ("token scheme", "Authorization: Token {v}\n", Kind::Alnum),
    (
        "digest response",
        "Authorization: Digest username=\"u\", response=\"{v}\"\n",
        Kind::Hex,
    ),
    ("ntlm", "Authorization: NTLM {v}\n", Kind::Base64),
    ("api key header", "X-Api-Key: {v}\n", Kind::Alnum),
    ("auth token header", "X-Auth-Token: {v}\n", Kind::Alnum),
    ("cookie", "Cookie: session={v}\n", Kind::Alnum),
    ("set-cookie", "Set-Cookie: id={v}; Path=/\n", Kind::Alnum),
    ("session id", "PHPSESSID={v}\n", Kind::Alnum),
    (
        "query token",
        "https://api.example/cb?access_token={v}&x=1\n",
        Kind::Alnum,
    ),
    (
        "query client secret",
        "https://api.example/t?client_secret={v}\n",
        Kind::Alnum,
    ),
    ("query key", "https://api.example/m?key={v}\n", Kind::Alnum),
    (
        "query password",
        "https://api.example/l?password={v}\n",
        Kind::Alnum,
    ),
    (
        "query signature",
        "https://cdn.example/f?sig={v}\n",
        Kind::Alnum,
    ),
    (
        "connection string",
        "Server=db;User Id=sa;Password={v};\n",
        Kind::Alnum,
    ),
    ("pgpass", "db.example:5432:app:admin:{v}\n", Kind::Alnum),
    (
        "docker auth",
        "{\"auths\": {\"r.example\": {\"auth\": \"{v}\"}}}\n",
        Kind::Base64,
    ),
    ("npmrc", "//registry.example/:_authToken={v}\n", Kind::Alnum),
    // Cloud
    (
        "azure shared access key",
        "Endpoint=sb://ns.example/;SharedAccessKeyName=k;SharedAccessKey={v}\n",
        Kind::Base64,
    ),
    (
        "azure sas",
        "BlobEndpoint=https://a.example/;SharedAccessSignature=sv=2022-11-02&sig={v}\n",
        Kind::Base64,
    ),
    (
        "aws presigned",
        "https://b.example/o?X-Amz-Credential={v}&X-Amz-Date=20240101T000000Z\n",
        Kind::Alnum,
    ),
    ("cosmos", "cosmos_key = \"{v}\"\n", Kind::Base64),
    // Context: values named by their key. Past the context engine's own value limits (512
    // bytes unquoted, 1 KiB quoted) a random value is still found by the classifier.
    ("password=", "password={v}\n", Kind::Alnum),
    ("yaml api_key", "api_key: {v}\n", Kind::Alnum),
    (
        "json client_secret",
        "{\"client_secret\": \"{v}\"}\n",
        Kind::Alnum,
    ),
    ("export token", "export X_TOKEN={v}\n", Kind::Alnum),
    // Vendor formats with a bounded length: a longer value is no longer that format, the
    // classifier finds it as a random string.
    (
        "ai openai legacy",
        // Split so the source never holds the whole OpenAI marker: secret scanners would flag it.
        concat!("sk-abcdefghij0123456789T3Blbk", "FJ{v}\n"),
        Kind::Alnum,
    ),
    ("ai fireworks", "fw_{v}\n", Kind::Alnum),
    ("ai pinecone", "pcsk_ab12_{v}\n", Kind::Alnum),
    (
        "dev gitlab pat",
        "glpat-abcdefghij0123456789.{v}\n",
        Kind::Alnum,
    ),
    ("dev gitlab deploy", "gldt-{v}\n", Kind::Alnum),
    ("dev gitlab scim", "glsoat-{v}\n", Kind::Alnum),
    ("dev gitlab ci", "glcbt-64_{v}\n", Kind::Alnum),
    (
        "dev gitlab runner registration",
        "GR1348941{v}\n",
        Kind::Alnum,
    ),
    ("dev bitbucket", "ATBB{v}\n", Kind::Alnum),
    ("dev dockerhub", "dckr_pat_{v}\n", Kind::Alnum),
    ("dev heroku", "HRKU-{v}\n", Kind::Alnum),
    ("dev supabase", "sb_secret_{v}\n", Kind::Alnum),
    ("dev planetscale", "pscale_pw_{v}\n", Kind::Alnum),
    ("dev planetscale oauth", "pscale_oauth_{v}\n", Kind::Alnum),
    ("dev terraform", "abcdefghijklmn.atlasv1.{v}\n", Kind::Alnum),
    ("dev doppler", "dp.pt.{v}\n", Kind::Alnum),
    ("saas notion", "ntn_{v}\n", Kind::Alnum),
    ("saas figma", "figd_{v}\n", Kind::Alnum),
    ("saas stripe webhook", "whsec_{v}\n", Kind::Alnum),
    (
        "saas discord bot",
        "MTA1234567890123456789012.Gabcde.{v}\n",
        Kind::Alnum,
    ),
    (
        "saas discord webhook",
        "https://discord.com/api/webhooks/123456789012345678/{v}\n",
        Kind::Alnum,
    ),
    (
        "saas slack bot",
        "xoxb-1234567890-1234567890-{v}\n",
        Kind::Alnum,
    ),
    (
        "saas slack user",
        "xoxp-1234567890-1234567890-1234567890-{v}\n",
        Kind::Hex,
    ),
    (
        "saas slack app",
        "xapp-1-A0123456789-1234567890123-{v}\n",
        Kind::Hex,
    ),
    (
        "saas teams",
        "https://acme.webhook.office.com/webhookb2/{v}\n",
        Kind::Alnum,
    ),
    ("saas firebase", "AAAAabc1234:APA91b{v}\n", Kind::Alnum),
    ("saas tailscale", "tskey-auth-k1a2b3c4-{v}\n", Kind::Alnum),
    ("saas telegram", "123456789:AA{v}\n", Kind::Alnum),
    (
        "cloud aws secret",
        "aws_secret_access_key = {v}\n",
        Kind::Base64,
    ),
    (
        "cloud aws presigned signature",
        "https://b.example/o?X-Amz-Signature={v}\n",
        Kind::Hex,
    ),
    (
        "cloud azure storage",
        "DefaultEndpointsProtocol=https;AccountName=a;AccountKey={v}\n",
        Kind::Base64,
    ),
    (
        "cloud cloudflare",
        "CLOUDFLARE_API_TOKEN={v}\n",
        Kind::Alnum,
    ),
    ("cloud alibaba", "LTAI{v}\n", Kind::Alnum),
    ("cloud tencent", "AKID{v}\n", Kind::Alnum),
    ("crypto wif", "K{v}\n", Kind::Alnum),
    (
        "crypto wireguard",
        "[Interface]\nPrivateKey = {v}\n",
        Kind::Base64,
    ),
    ("hash sha512 crypt", "$6$saltsalt${v}\n", Kind::Alnum),
    ("hash sha256 crypt", "$5$saltsalt${v}\n", Kind::Alnum),
    ("hash md5 crypt", "$1$saltsalt${v}\n", Kind::Alnum),
    ("hash apr1", "$apr1$saltsalt${v}\n", Kind::Alnum),
    ("hash yescrypt", "$y$j9T$saltsalt${v}\n", Kind::Alnum),
    (
        "hash django",
        "pbkdf2_sha256$600000$salt${v}\n",
        Kind::Base64,
    ),
    ("hash htpasswd", "admin:{SHA}{v}=\n", Kind::Base64),
];

/// Every byte of `[s, e)` lies inside a redacted finding.
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

/// Scans each case at 1 KiB and 8 KiB and lists the ones that leave part of the value.
fn leaks(cases: &[(&str, &str, Kind)], s: Sensitivity) -> Vec<String> {
    let mut g = Gen::named("long values");
    let mut failed = Vec::new();
    for &(name, template, kind) in cases {
        for n in [1024, 8192] {
            let v = value(&mut g, kind, n);
            let text = template.replace("{v}", &v);
            let out = scan_at(&text, s);
            let left: usize = template
                .match_indices("{v}")
                .enumerate()
                .map(|(i, (p, _))| {
                    let s = p + i * (v.len() - 3);
                    uncovered(&out, s, s + v.len())
                })
                .sum();
            if left > 0 || !out.verified {
                failed.push(format!(
                    "{name} @ {n}: {left} bytes not redacted, verified={}, {:?}",
                    out.verified,
                    out.findings
                        .iter()
                        .map(|f| (f.start, f.end, f.detector.as_str()))
                        .collect::<Vec<_>>()
                ));
            }
        }
    }
    failed
}

#[test]
fn long_values_are_found() {
    let failed = leaks(CASES, Sensitivity::Balanced);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Formats whose length is fixed by what they encode (a Mailchimp key is 32 hex digits, LM
/// and NT hashes are 32 hex digits each): a longer hex run there is no such value, and a hex
/// run alone is flagged as random only at Paranoid, where commit hashes are acceptable noise.
#[test]
fn long_hex_in_fixed_formats_is_found_at_paranoid() {
    let fixed: &[(&str, &str, Kind)] = &[
        ("mailchimp", "{v}-us6\n", Kind::Hex),
        ("pwdump", "admin:500:{v}:{v}:::\n", Kind::Hex),
    ];
    let failed = leaks(fixed, Sensitivity::Paranoid);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// URL passwords past the old 256-character limit, found by the URL rule itself with nothing
/// left over and no warning.
#[test]
fn long_url_passwords_are_found() {
    let mut g = Gen::named("long url passwords");
    for n in [257, 300, 4096, 65536] {
        let pw = g.alnum(n);
        for (pre, post) in [
            ("redis://default:", "@cache.example:6379\n"),
            ("DATABASE_URL=postgres://app:", "@db.example:5432/app\n"),
            ("https://deploy:", "@vcs.example\n"),
        ] {
            let text = format!("{pre}{pw}{post}");
            let out = scan(&text);
            assert!(out.verified && out.warnings.is_empty(), "{pre} {n}");
            assert_eq!(out.findings.len(), 1, "{pre} {n}: {:?}", out.findings);
            let f = &out.findings[0];
            assert_eq!((f.start, f.end), (pre.len(), pre.len() + n), "{pre} {n}");
            assert_eq!(f.detector, "url_userinfo_password", "{pre} {n}");
            assert_eq!(f.label, "Password in URL", "{pre} {n}");
        }
    }
}

/// `a://b:c@a://b:c@…`: each URL ends where the next one starts. Every password is found in
/// the first pass, so the verify loop has nothing left to add.
#[test]
fn glued_url_chain_is_found_in_one_pass() {
    for copies in [2, 10, 11, 12, 30, 1000] {
        let text = "a://b:c@".repeat(copies);
        let out = scan(&text);
        // The last copy has no host after its `@`.
        assert_eq!(out.findings.len(), copies - 1, "{copies} copies");
        assert!(out.verified, "{copies} copies");
        assert!(
            !out.warnings
                .iter()
                .any(|w| w.kind == WarningKind::VerifyCapHit),
            "{copies} copies"
        );
        for (i, f) in out.findings.iter().enumerate() {
            assert_eq!((f.start, f.end), (8 * i + 6, 8 * i + 7), "{copies} copies");
        }
    }
}

/// The password runs to the last `@` that a host follows, as the pattern did before.
#[test]
fn url_password_ends_at_the_last_host_at() {
    for (text, pw) in [
        ("smtp://mail:p@ss@mx.example:25\n", "p@ss"),
        ("ftp://u:p@ss@ @h.example\n", "p"),
        ("redis://:only-pw-9x@cache.example\n", "only-pw-9x"),
        ("http://u:pw-1x@[::1]:8080/\n", "pw-1x"),
    ] {
        let out = scan(text);
        let got: Vec<&str> = out
            .findings
            .iter()
            .filter(|f| f.detector == "url_userinfo_password")
            .map(|f| &text[f.start..f.end])
            .collect();
        assert_eq!(got, [pw], "{text:?}");
    }
    for text in [
        "https://user@host.example/\n",
        "postgres://db.example:5432/app\n",
        "a://b:c@\n",
    ] {
        let out = scan(text);
        assert!(
            !out.findings
                .iter()
                .any(|f| f.detector == "url_userinfo_password"),
            "{text:?}: {:?}",
            out.findings
        );
    }
}

/// A value longer than its rule's window is found by that rule, not only by the classifier:
/// the window widens until the match ends inside it.
#[test]
fn long_values_keep_their_rule() {
    let mut g = Gen::named("long values keep their rule");
    let cases: &[(&str, &str, Kind)] = &[
        (
            "docker_config_auth",
            "{\"auths\": {\"r.example\": {\"auth\": \"{v}\"}}}\n",
            Kind::Base64,
        ),
        (
            "auth_digest_response",
            "Authorization: Digest username=\"u\", response=\"{v}\"\n",
            Kind::Hex,
        ),
        ("auth_basic", "Authorization: Basic {v}\n", Kind::Base64),
        (
            "url_query_token",
            "https://api.example/cb?access_token={v}&x=1\n",
            Kind::Alnum,
        ),
        (
            "npmrc_auth",
            "//registry.example/:_authToken={v}\n",
            Kind::Alnum,
        ),
        (
            "terraform_cloud_token",
            "abcdefghijklmn.atlasv1.{v}\n",
            Kind::Alnum,
        ),
        (
            "url_userinfo_password",
            "amqp://svc:{v}@mq.example/vhost\n",
            Kind::Alnum,
        ),
    ];
    for &(rule, template, kind) in cases {
        for n in [20_000, 100_000] {
            let v = value(&mut g, kind, n);
            let text = template.replace("{v}", &v);
            let s = template.find("{v}").unwrap();
            let out = scan(&text);
            assert!(
                out.findings
                    .iter()
                    .any(|f| f.detector == rule && f.start <= s && f.end >= s + n),
                "{rule} @ {n}: {:?}",
                out.findings
                    .iter()
                    .map(|f| (f.start, f.end, f.detector.as_str()))
                    .collect::<Vec<_>>()
            );
        }
    }
}
