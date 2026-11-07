//! Performance gates, release mode only:
//! `cargo test -p redactor-core --release --test perf -- --ignored --nocapture`.

mod support;

use std::time::Instant;

use redactor_core::{Engine, EngineConfig, ScanRequest, Sensitivity};
use support::gen::*;

/// ~1 MiB of realistic mixed text with a token every ~1000 lines.
fn mixed_mib(g: &mut Gen) -> String {
    let filler = [
        "2026-11-06T10:15:02.123Z INFO  request handled path=/api/v1/users/42 status=200 duration_ms=12\n",
        "    at com.example.service.UserService.lookup(UserService.java:118)\n",
        "fn main() { let config = load_config(\"settings.toml\").expect(\"config\"); println!(\"{config:?}\"); }\n",
        "The quick brown fox jumps over the lazy dog while the deployment pipeline finishes its checks.\n",
        "Die Konfiguration wurde erfolgreich geladen; bitte prüfen Sie die Protokolle für Details.\n",
        "{\"id\": \"5f1c2a9e-8d3b-4c6a-9e7f-1a2b3c4d5e6f\", \"name\": \"widget\", \"tags\": [\"a\", \"b\"], \"price\": 9.99}\n",
        "commit 3f2a9c1e7b4d6a8f0e1c2b3a4d5e6f7a8b9c0d1e Merge branch 'feature/login' into main\n",
        "GET https://example.com/search?q=rust+regex&page=2 HTTP/1.1\nHost: example.com\nAccept: */*\n",
    ];
    let mut s = String::with_capacity(1 << 20);
    let mut i = 0;
    while s.len() < (1 << 20) {
        s.push_str(filler[i % filler.len()]);
        if i % 997 == 0 {
            s.push_str(&format!("GITHUB_TOKEN={}\n", g.github("ghp_")));
        }
        i += 1;
    }
    s
}

#[test]
#[ignore]
fn perf_1mib_under_250ms() {
    let mut g = Gen::named("perf mib");
    let text = mixed_mib(&mut g);
    let e = Engine::new(EngineConfig::default()).unwrap();
    e.scan("warm up", &ScanRequest::default());
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let out = e.scan(&text, &ScanRequest::default());
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        assert!(out.verified);
        assert_eq!(out.findings.len(), text.matches("GITHUB_TOKEN=").count());
    }
    println!("1 MiB mixed text, Balanced: best {best:.1} ms over 5 runs");
    assert!(best < 250.0, "{best:.1} ms");
}

/// 10 MiB: one line of `a`, a million zero-width spaces, nested base64 and 100k `password=`
/// lines, scanned in Paranoid with PII on.
#[test]
#[ignore]
fn adversarial_10mib_under_5s() {
    let mib = 1 << 20;
    let mut text = String::with_capacity(10 * mib + 1024);
    text.push_str(&"a".repeat(3 * mib));
    text.push('\n');
    text.push_str(&"\u{200B}".repeat(1_000_000));
    text.push('\n');
    let mut nested = String::from("password=Hunter2-Fake-9x token=Zq81mXv0pLr3TfK9wQe2Lm");
    for _ in 0..3 {
        nested = b64_encode(nested.as_bytes());
    }
    while text.len() < 7 * mib {
        text.push_str(&nested);
        text.push('\n');
    }
    text.push_str(&"password=\n".repeat(100_000));
    let filler = "The quick brown fox jumps over the lazy dog; password=Tr0ub4dor&3 key=value\n";
    while text.len() < 10 * mib {
        text.push_str(filler);
    }
    let e = Engine::new(EngineConfig::default()).unwrap();
    let t = Instant::now();
    let out = e.scan(
        &text,
        &ScanRequest {
            sensitivity: Some(Sensitivity::Paranoid),
            pii: Some(true),
            ..ScanRequest::default()
        },
    );
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "10 MiB adversarial, Paranoid + PII: {ms:.0} ms, {} findings, warnings {:?}",
        out.findings.len(),
        out.warnings
    );
    assert!(!out.output.contains("Tr0ub4dor&3"));
    assert!(ms < 5000.0, "{ms:.0} ms");
}

/// Context shapes that once grew faster than the text: Terraform `variable` blocks left
/// open (each one read the next blocks' `default` lines again), and a value the text uses
/// as an identifier again and again inside one run of letters (each use looked back over
/// the whole run). 8 MiB must take about 8 times as long as 1 MiB.
#[test]
#[ignore]
fn context_shapes_scale_linearly() {
    let mib = 1 << 20;
    type Make = fn(usize) -> String;
    let shapes: [(&str, Make); 2] = [
        ("open terraform blocks", |n| {
            let block = "variable \"db_password\" {\n  default = \"Tulip-Harbor-91\"\n";
            block.repeat(n / block.len())
        }),
        ("identifier in a long run", |n| {
            let mut s = String::from("password = hunter2prod\n");
            let unit = "\u{e9}hunter2prod";
            s.push_str(&unit.repeat(n / unit.len()));
            s
        }),
    ];
    let e = Engine::new(EngineConfig::default()).unwrap();
    e.scan("warm up", &ScanRequest::default());
    for (name, make) in shapes {
        let mut ms = [0.0f64; 2];
        for (i, n) in [mib, 8 * mib].into_iter().enumerate() {
            let text = make(n);
            let t = Instant::now();
            let out = e.scan(&text, &ScanRequest::default());
            ms[i] = t.elapsed().as_secs_f64() * 1000.0;
            assert!(out.verified, "{name}");
        }
        println!(
            "{name}: 1 MiB {:.0} ms, 8 MiB {:.0} ms ({:.1}x)",
            ms[0],
            ms[1],
            ms[1] / ms[0]
        );
        assert!(ms[1] < 16.0 * ms[0] + 50.0, "{name}: {ms:?}");
    }
}

/// Grows `unit(g, i)` (i = 0, 1, …) until the text holds `size` bytes.
fn fill(size: usize, g: &mut Gen, mut unit: impl FnMut(&mut Gen, usize) -> String) -> String {
    let mut s = String::with_capacity(size + 4096);
    let mut i = 0;
    while s.len() < size {
        s.push_str(&unit(g, i));
        i += 1;
    }
    s
}

/// Escapes `s` as the content of a JSON string.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// One adversarial shape at about `size` bytes.
fn shape(name: &str, size: usize) -> String {
    let mut g = Gen::named(name);
    match name {
        // One line of 40-character random tokens separated by spaces.
        "long line" => fill(size, &mut g, |g, _| format!("{} ", g.alnum(40))),
        // Private key headers, each with three body lines, never terminated.
        "unterminated BEGIN" => fill(size, &mut g, |g, _| {
            format!(
                "-----BEGIN RSA PRIVATE KEY-----\n{}\n{}\n{}\n",
                g.from(B64, 64),
                g.from(B64, 64),
                g.from(B64, 64)
            )
        }),
        // A different password on every line.
        "many distinct values" => fill(size, &mut g, |g, i| {
            format!("user{i} password={}-{}\n", g.alnum(10), g.alnum(8))
        }),
        // One password, then the same value again and again (100k times per MiB).
        "one value repeated" => {
            let v = format!("{}-{}", g.alnum(4), g.alnum(4));
            let mut s = format!("DB_PASSWORD={v}\n");
            s.push_str(&fill(size, &mut g, |_, i| {
                format!("{v}{}", if i % 8 == 7 { '\n' } else { ' ' })
            }));
            s
        }
        // Long base64 blobs of readable text with passwords inside.
        "long base64" => fill(size, &mut g, |g, _| {
            let mut plain = String::new();
            while plain.len() < 192 << 10 {
                plain.push_str(&format!(
                    "line of a config file password={} and more words here\n",
                    g.alnum(16)
                ));
            }
            format!("blob: {}\n", b64_encode(plain.as_bytes()))
        }),
        // JSON strings escaped eight levels deep, with \u escapes at the bottom.
        "deep JSON" => fill(size, &mut g, |g, _| {
            let mut s = format!(
                "{{\"password\": \"{}\\u0021{}\", \"note\": \"caf\\u00e9\"}}",
                g.alnum(8),
                g.alnum(8)
            );
            for _ in 0..8 {
                s = format!("{{\"inner\": \"{}\"}}", json_escape(&s));
            }
            s.push('\n');
            s
        }),
        // All of the above in 256 KiB pieces.
        "mix" => {
            let parts = [
                "long line",
                "unterminated BEGIN",
                "many distinct values",
                "one value repeated",
                "long base64",
                "deep JSON",
            ];
            let mut s = String::with_capacity(size + (256 << 10));
            let mut k = 0;
            while s.len() < size {
                s.push_str(&shape(parts[k % parts.len()], 256 << 10));
                s.push('\n');
                k += 1;
            }
            s
        }
        _ => unreachable!("{name}"),
    }
}

/// Adversarial shapes at 1 MiB and 8 MiB (best of two runs each), Paranoid with PII on: the
/// 8 MiB scan may take about ten times as long as the 1 MiB scan, not more (linear scaling;
/// the bound is 12x, as caches and the allocator favour the small run by up to a fifth).
/// `REDACTOR_PERF_MIB=20` adds a run at that size; `REDACTOR_PERF_SHAPE=mix` runs one shape.
#[test]
#[ignore]
fn adversarial_shapes_scale_linearly() {
    let e = Engine::new(EngineConfig::default()).unwrap();
    e.scan("warm up", &ScanRequest::default());
    let req = ScanRequest {
        sensitivity: Some(Sensitivity::Paranoid),
        pii: Some(true),
        ..ScanRequest::default()
    };
    let extra: Option<usize> = std::env::var("REDACTOR_PERF_MIB")
        .ok()
        .and_then(|v| v.parse().ok());
    let only = std::env::var("REDACTOR_PERF_SHAPE").ok();
    let mib = 1usize << 20;
    let mut worst = f64::MAX;
    let mut failures = Vec::new();
    for name in [
        "long line",
        "unterminated BEGIN",
        "many distinct values",
        "one value repeated",
        "long base64",
        "deep JSON",
        "mix",
    ] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let mut times = Vec::new();
        for size in [1, 8].into_iter().chain(extra) {
            let text = shape(name, size * mib);
            // Best of two runs.
            let mut secs = f64::MAX;
            let mut out = None;
            for _ in 0..2 {
                let t = Instant::now();
                out = Some(e.scan(&text, &req));
                secs = secs.min(t.elapsed().as_secs_f64());
            }
            let out = out.expect("scanned");
            let rate = text.len() as f64 / mib as f64 / secs;
            worst = worst.min(rate);
            println!(
                "{name:>20} {size:>3} MiB: {:>8.1} ms {rate:>7.1} MiB/s, {} findings, verified {}",
                secs * 1000.0,
                out.findings.len(),
                out.verified
            );
            times.push(secs);
        }
        // A floor of 5 ms keeps timer noise on tiny runs from failing the ratio.
        if times[1] > 12.0 * times[0].max(0.005) {
            failures.push(format!(
                "{name}: 8 MiB {:.0} ms vs 1 MiB {:.0} ms",
                times[1] * 1000.0,
                times[0] * 1000.0
            ));
        }
    }
    println!("worst throughput: {worst:.1} MiB/s");
    assert!(failures.is_empty(), "not linear: {failures:?}");
}
