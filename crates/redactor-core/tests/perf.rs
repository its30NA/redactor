//! Performance gates, release mode only, one test at a time:
//! `cargo test -p redactor-core --release --test perf -- --ignored --nocapture --test-threads=1`.

mod support;

use std::time::Instant;

use redactor_core::{Engine, EngineConfig, ScanOutcome, ScanRequest, Sensitivity};
use support::gen::*;
use support::heap;

#[global_allocator]
static HEAP: heap::Counting = heap::Counting;

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

/// The audit's 649 KB of `password=abab…` lines (2026-11-10, finding 3): every value a
/// prefix of the next, so the search for copies listed each value at every position of every
/// longer one. It took 53.8 s on the audit's machine.
#[test]
#[ignore]
fn nested_values_under_1s() {
    let text: String = (6..801)
        .map(|k| format!("password={}\n", "ab".repeat(k)))
        .collect();
    let e = Engine::new(EngineConfig::default()).unwrap();
    let t = Instant::now();
    let out = e.scan(&text, &ScanRequest::default());
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("649 KB of nested values: {ms:.0} ms");
    assert!(out.verified);
    assert_eq!(out.findings.len(), 795);
    assert!(ms < 1000.0, "{ms:.0} ms");
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
        // `password=abab…` lines, each value a prefix of the next (audit 2026-11-10, finding
        // 3: the copy search was cubic here). The longest value grows with the square root of
        // the size.
        "nested values" => {
            let mut s = String::with_capacity(size + (16 << 10));
            let mut k = 6;
            while s.len() < size {
                s.push_str(&format!("password={}\n", "ab".repeat(k)));
                k += 1;
            }
            s
        }
        // The same nested values, then one long `abab…` run that holds each of them at every
        // other position (one candidate per value and position: 98 KB took 1.33 GB).
        "nested copies" => {
            let mut s: String = (6..181)
                .map(|k| format!("password={}\n", "ab".repeat(k)))
                .collect();
            s.push_str("note ");
            s.push_str(&"ab".repeat(size.saturating_sub(s.len()) / 2));
            s.push('\n');
            s
        }
        // Every rotation of one password, then the password again and again: a copy of a
        // different rotation starts at every position, and the copies of one rotation touch
        // (one candidate per byte).
        "rotations" => {
            let v = g.alnum(12);
            let mut s: String = (0..12)
                .map(|i| format!("password={}{}\n", &v[i..], &v[..i]))
                .collect();
            s.push_str("note ");
            s.push_str(&v.repeat(size.saturating_sub(s.len()) / 12));
            s.push('\n');
            s
        }
        // All of the above in 256 KiB pieces.
        "mix" => {
            let parts = [
                "long line",
                "unterminated BEGIN",
                "many distinct values",
                "one value repeated",
                "long base64",
                "deep JSON",
                "nested values",
                "nested copies",
                "rotations",
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

/// The shapes of [`adversarial_shapes_scale_linearly`].
const SHAPES: [&str; 10] = [
    "long line",
    "unterminated BEGIN",
    "many distinct values",
    "one value repeated",
    "long base64",
    "deep JSON",
    "nested values",
    "nested copies",
    "rotations",
    "mix",
];

/// One shape at one size: the best time of `runs` scans, and the heap the first scan held
/// at its peak (counted, so the same every run).
struct Measured {
    secs: f64,
    peak: usize,
    out: ScanOutcome,
}

fn measure(e: &Engine, req: &ScanRequest, text: &str, runs: usize) -> Measured {
    let mut secs = f64::MAX;
    let mut first: Option<(ScanOutcome, usize)> = None;
    for _ in 0..runs {
        let t = Instant::now();
        let (out, peak) = heap::peak(|| e.scan(text, req));
        secs = secs.min(t.elapsed().as_secs_f64());
        first.get_or_insert((out, peak));
    }
    let (out, peak) = first.expect("scanned");
    Measured { secs, peak, out }
}

/// Adversarial shapes at 1 MiB and 8 MiB, Paranoid with PII on, must scale linearly.
///
/// Memory is the stable measure: the heap a scan holds at its peak is counted by the
/// allocator, the same bytes every run. The 8 MiB scan may hold at most 10 times what the
/// 1 MiB scan holds (plus 4 MiB), and no scan more than 32 bytes per input byte plus 1 KiB
/// per finding (plus 4 MiB): a copy that ends up inside another finding must not cost a
/// candidate of its own. Time is the best of five scans at 1 MiB and of three at 8 MiB. The 8 MiB scan may take at most 20 times as long: 8 is linear, caches favour the
/// small scan of a shape with many findings by up to about 2x (the long line grew 12.5 to
/// 17.5x), and quadratic growth is 64x. A shape that misses the time bound is measured once
/// more before it fails, and every shape must run at 1 MiB/s or faster at 8 MiB.
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
    for name in SHAPES {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let texts: Vec<(usize, String)> = [1, 8]
            .into_iter()
            .chain(extra)
            .map(|size| (size, shape(name, size * mib)))
            .collect();
        let mut runs: Vec<Measured> = Vec::new();
        for (size, text) in &texts {
            let m = measure(&e, &req, text, if *size == 1 { 5 } else { 3 });
            let rate = text.len() as f64 / mib as f64 / m.secs;
            if *size == 8 {
                worst = worst.min(rate);
            }
            println!(
                "{name:>20} {size:>3} MiB: {:>8.1} ms {rate:>7.1} MiB/s, peak heap {:>6.1} MiB, {} findings, verified {}",
                m.secs * 1000.0,
                m.peak as f64 / mib as f64,
                m.out.findings.len(),
                m.out.verified
            );
            assert!(m.out.verified, "{name} {size} MiB");
            let bound = 32 * text.len() + 1024 * m.out.findings.len() + 4 * mib;
            if m.peak > bound {
                failures.push(format!(
                    "{name}: peak heap {} MiB at {size} MiB, bound {} MiB",
                    m.peak / mib,
                    bound / mib
                ));
            }
            runs.push(m);
        }
        let (small, large) = (&runs[0], &runs[1]);
        if large.peak > 10 * small.peak + 4 * mib {
            failures.push(format!(
                "{name}: peak heap {} MiB at 8 MiB vs {} MiB at 1 MiB",
                large.peak / mib,
                small.peak / mib
            ));
        }
        // A floor of 5 ms keeps timer noise on tiny runs from failing the ratio.
        let too_slow = |small: f64, large: f64| large > 20.0 * small.max(0.005);
        let (mut t1, mut t8) = (small.secs, large.secs);
        if too_slow(t1, t8) {
            println!("{name:>20}: {:.1}x, measuring again", t8 / t1);
            t1 = t1.min(measure(&e, &req, &texts[0].1, 5).secs);
            t8 = t8.min(measure(&e, &req, &texts[1].1, 3).secs);
        }
        if too_slow(t1, t8) {
            failures.push(format!(
                "{name}: 8 MiB {:.0} ms vs 1 MiB {:.0} ms",
                t8 * 1000.0,
                t1 * 1000.0
            ));
        }
        let rate = texts[1].1.len() as f64 / mib as f64 / t8;
        if rate < 1.0 {
            failures.push(format!("{name}: {rate:.2} MiB/s at 8 MiB"));
        }
    }
    println!("worst throughput at 8 MiB: {worst:.1} MiB/s");
    assert!(failures.is_empty(), "not linear: {failures:?}");
}
