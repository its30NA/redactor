//! `redactor.toml` loading. The engine crate only knows [`EngineConfig`]; the file
//! format lives here.
//!
//! v1 keys keep working (`disabled_detectors`, `enabled_detectors`, `redact_pii`,
//! `placeholder_template`, `[allowlist] patterns`, `[[rules]]` with `name`), next to the v2
//! spellings (`disabled`, `enabled`, `pii`, `template`, `allowlist = [...]`, `custom_rules`) and
//! the new keys `sensitivity`, `strip_invisible`, `terms` and `placeholder`. An `[llm]` section
//! is ignored with a one-line notice: the LLM pass was removed in v2.
//!
//! A config file can switch detection off, so loading one is never silent and never blind:
//!
//! - The search goes upwards from the working directory and stops at the repository root
//!   (the first directory holding `.git`) or the home directory, whichever comes first.
//! - Every loaded file is named on stderr (`scrub: using config: PATH`).
//! - On Unix a file writable by every user, or owned by another user (other than root), is
//!   refused: on a shared machine someone else could have planted it.
//! - An allowlist entry that matches the empty string, a large share of random secret-shaped
//!   values generated at run time, or every random token of one provider's format (`^ghp_`),
//!   is refused as a catch-all.
//! - [`Config::weakening`] names every allowlist entry and disabled rule (capped), for
//!   `scrub check` to print on every run; `check` refuses a config that leaves it unable to
//!   detect common secrets at all.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use redactor_core::{CustomRule, EngineConfig, PlaceholderStyle, Sensitivity};
use serde::Deserialize;

/// Names searched, in order, in each directory from the working directory upwards.
pub const CONFIG_FILENAMES: [&str; 2] = ["redactor.toml", ".redactor.toml"];

/// A loaded configuration: the engine settings plus one-line notices to print on stderr.
#[derive(Debug)]
pub struct Config {
    pub engine: EngineConfig,
    pub notices: Vec<String>,
    /// The file it came from; None for the defaults.
    pub source: Option<PathBuf>,
    /// The settings in it that lower detection, one human-readable item each.
    pub weakening: Vec<String>,
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Deserialize)]
struct RawRule {
    #[serde(alias = "name")]
    id: String,
    label: String,
    pattern: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default)]
    group: usize,
}

fn default_confidence() -> f32 {
    0.9
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawAllowlist {
    /// v1: `[allowlist] patterns = [...]`.
    Table { patterns: Vec<String> },
    /// v2: `allowlist = [...]`.
    List(Vec<String>),
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    sensitivity: Option<Sensitivity>,
    #[serde(alias = "redact_pii")]
    pii: Option<bool>,
    strip_invisible: Option<bool>,
    #[serde(alias = "disabled_detectors", default)]
    disabled: Vec<String>,
    #[serde(alias = "enabled_detectors", default)]
    enabled: Vec<String>,
    #[serde(alias = "rules", default)]
    custom_rules: Vec<RawRule>,
    allowlist: Option<RawAllowlist>,
    #[serde(default)]
    terms: Vec<String>,
    placeholder: Option<PlaceholderStyle>,
    #[serde(alias = "placeholder_template")]
    template: Option<String>,
    stable_numbering: Option<bool>,
    llm: Option<toml::Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

impl Config {
    /// Loads `path`, or the nearest config file above the working directory, or the defaults.
    pub fn load(path: Option<&Path>) -> Result<Config, ConfigError> {
        let resolved = match path {
            Some(p) => Some(p.to_path_buf()),
            None => std::env::current_dir()
                .ok()
                .and_then(|d| discover(&d, home().as_deref())),
        };
        let Some(file) = resolved else {
            return Ok(Config {
                engine: EngineConfig::default(),
                notices: Vec::new(),
                source: None,
                weakening: Vec::new(),
            });
        };
        let text = std::fs::read_to_string(&file)
            .map_err(|e| ConfigError(format!("cannot read config {}: {e}", file.display())))?;
        if let Err(why) = trusted(&file) {
            return Err(ConfigError(format!(
                "refusing config {}: {why}",
                file.display()
            )));
        }
        let mut config = Config::parse(&text)
            .map_err(|e| ConfigError(format!("invalid config {}: {}", file.display(), e.0)))?;
        config.source = Some(file);
        Ok(config)
    }

    /// Parses config text.
    pub fn parse(src: &str) -> Result<Config, ConfigError> {
        let raw: RawConfig = toml::from_str(src).map_err(|e| ConfigError(e.message().into()))?;
        let mut notices = Vec::new();
        if raw.llm.is_some() {
            notices.push(
                "the [llm] section is ignored: the LLM pass was removed in v2 (nothing is sent anywhere)"
                    .to_string(),
            );
        }
        if raw.stable_numbering == Some(false) {
            notices.push(
                "stable_numbering = false is ignored: the same value always gets the same number"
                    .to_string(),
            );
        }
        for key in raw.unknown.keys() {
            notices.push(format!("unknown config key `{key}` ignored"));
        }

        let allowlist = match raw.allowlist {
            Some(RawAllowlist::Table { patterns }) | Some(RawAllowlist::List(patterns)) => patterns,
            None => Vec::new(),
        };
        for (i, pattern) in allowlist.iter().enumerate() {
            if let Some(why) = catch_all(pattern) {
                return Err(ConfigError(format!(
                    "allowlist entry {} (`{pattern}`) {why}; use a narrower pattern",
                    i + 1
                )));
            }
        }

        let mut weakening = Vec::new();
        if !allowlist.is_empty() {
            let quoted: Vec<String> = allowlist.iter().map(|p| format!("`{p}`")).collect();
            weakening.push(format!("allowlist: {}", capped(&quoted)));
        }
        if !raw.disabled.is_empty() {
            weakening.push(format!("disabled rules: {}", capped(&raw.disabled)));
        }
        if raw.sensitivity == Some(Sensitivity::Relaxed) {
            weakening.push("sensitivity = \"relaxed\"".to_string());
        }

        let mut engine = EngineConfig::default();
        if let Some(s) = raw.sensitivity {
            engine.sensitivity = s;
        }
        if let Some(p) = raw.pii {
            engine.pii = p;
        }
        if let Some(s) = raw.strip_invisible {
            engine.strip_invisible = s;
        }
        engine.disabled = raw.disabled;
        engine.enabled = raw.enabled;
        engine.custom_rules = raw
            .custom_rules
            .into_iter()
            .map(|r| CustomRule {
                id: r.id,
                label: r.label,
                pattern: r.pattern,
                confidence: r.confidence,
                group: r.group,
            })
            .collect();
        engine.allowlist = allowlist;
        engine.terms = raw.terms;
        if let Some(p) = raw.placeholder {
            engine.placeholder = p;
        }
        if let Some(t) = raw.template {
            engine.template = t;
        }
        Ok(Config {
            engine,
            notices,
            source: None,
            weakening,
        })
    }
}

/// How many items of one weakening list are named before "+N more".
const SHOWN: usize = 10;

fn capped(items: &[String]) -> String {
    let mut s = items[..items.len().min(SHOWN)].join(", ");
    if items.len() > SHOWN {
        s.push_str(&format!(" (+{} more)", items.len() - SHOWN));
    }
    s
}

fn home() -> Option<PathBuf> {
    std::env::home_dir().filter(|h| !h.as_os_str().is_empty())
}

/// Walks up from `start` looking for a config file. The search ends at the repository root
/// (a directory holding `.git`) or at `home`, after looking there.
pub fn discover(start: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let home = home.map(|h| h.canonicalize().unwrap_or_else(|_| h.to_path_buf()));
    for dir in start.ancestors() {
        let found = CONFIG_FILENAMES
            .iter()
            .map(|n| dir.join(n))
            .find(|p| p.is_file());
        if found.is_some() {
            return found;
        }
        if dir.join(".git").exists() || home.as_deref() == Some(dir) {
            return None;
        }
    }
    None
}

/// Whether a config file may be trusted: on Unix, not writable by every user and owned by the
/// current user or root.
#[cfg(unix)]
pub fn trusted(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(file).map_err(|e| e.to_string())?;
    if meta.mode() & 0o002 != 0 {
        return Err("every user can write to it (`chmod o-w` fixes that)".into());
    }
    let me = rustix::process::geteuid().as_raw();
    if meta.uid() != me && meta.uid() != 0 {
        return Err(format!(
            "it belongs to another user (uid {}), who could have planted it",
            meta.uid()
        ));
    }
    Ok(())
}

/// Windows has no world-writable bit; its ACLs are left to the system.
#[cfg(not(unix))]
pub fn trusted(_file: &Path) -> Result<(), String> {
    Ok(())
}

/// Why an allowlist pattern is a catch-all, if it is one: it matches the empty string, at
/// least a quarter of a set of random secret-shaped values made at run time, or every random
/// value of some provider's token format (`^ghp_`, `AKIA`, `xox[bp]-`: a whole kind of secret
/// let through). Patterns that are not valid regexes are left to the engine, which reports them.
pub fn catch_all(pattern: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    if re.is_match("") {
        return Some("matches the empty string, so it would let every secret through".into());
    }
    let samples = secret_shaped_samples();
    let hits = samples.iter().filter(|s| re.is_match(s)).count();
    if hits * 4 >= samples.len() {
        return Some(
            "matches many random secret-shaped values, so it would let nearly every secret \
             through"
                .into(),
        );
    }
    let mut draw = sample_rng(0x9F0B_1DE5);
    for prefix in provider_prefixes() {
        let all = (0..PER_PREFIX).all(|_| re.is_match(&format!("{prefix}{}", draw(ALNUM, 40))));
        if all {
            return Some(format!(
                "matches every random token that starts with `{prefix}`, so it would let that \
                 whole kind of secret through"
            ));
        }
    }
    None
}

/// Random values tried per provider prefix; all of them must match for a pattern to count as
/// prefix-wide, so a narrow pattern never trips it by chance.
const PER_PREFIX: usize = 4;

const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// The literal prefixes of the built-in token formats, read from the rule catalog
/// (`ghp_ + 36 base62`, `AKIA/ASIA/... + 16 [A-Z2-7]`), so a new rule is covered without a list
/// to keep in step. GitHub's family is spelt `gh?_` there and is added by hand.
pub fn provider_prefixes() -> Vec<String> {
    let mut out: Vec<String> = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .map(String::from)
        .to_vec();
    for info in redactor_core::rule_infos() {
        let Some((head, _)) = info.example_shape.split_once(" + ") else {
            continue;
        };
        for p in head.split('/') {
            let literal = p.len() >= 3
                && p.bytes().any(|b| b.is_ascii_alphabetic())
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
            if literal && !out.iter().any(|o| o == p) {
                out.push(p.to_string());
            }
        }
    }
    out
}

/// Common secrets, made fresh on every run (a config cannot be tuned to the exact values): a
/// password assignment, an AWS key id, an OpenAI key, a bearer token and a private key block.
/// `scrub check` refuses a config under which the engine finds none of them.
pub fn canary_text() -> String {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ u64::from(std::process::id()).rotate_left(32);
    let mut draw = sample_rng(seed);
    const B32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let pem = ["-----BEGIN ", "RSA PRIVATE KEY-----"].concat();
    let pem_end = ["-----END ", "RSA PRIVATE KEY-----"].concat();
    format!(
        "DB_PASSWORD={}-{}7\nAWS_ACCESS_KEY_ID={}{}\nOPENAI_API_KEY={}{}\n\
         Authorization: Bearer {}\n{pem}\n{}\n{}\n{pem_end}\n",
        draw(ALNUM, 10),
        draw(ALNUM, 6),
        ["AK", "IA"].concat(),
        draw(B32, 16),
        ["sk-", "proj-"].concat(),
        draw(ALNUM, 48),
        draw(ALNUM, 40),
        draw(B64, 64),
        draw(B64, 64),
    )
}

/// A splitmix64 stream drawing strings from an alphabet. A fixed seed keeps verdicts stable.
fn sample_rng(seed: u64) -> impl FnMut(&[u8], usize) -> String {
    let mut state = seed;
    move |alphabet: &[u8], n: usize| {
        (0..n)
            .map(|_| {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D1_049B_1331_11EB);
                z ^= z >> 31;
                alphabet[(z % alphabet.len() as u64) as usize] as char
            })
            .collect()
    }
}

/// Random values shaped like the secrets the engine finds: tokens in several alphabets,
/// passwords, JWT-like and UUID-like strings. A fixed seed keeps the verdict the same from run
/// to run; nothing here is a real secret.
fn secret_shaped_samples() -> Vec<String> {
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const HEX: &[u8] = b"0123456789abcdef";
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const PASS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&*+-.:=?@^_~";
    let mut state: u64 = 0x5EED_5EED_5EED_5EED;
    let mut next = move || {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D1_049B_1331_11EB);
        z ^ (z >> 31)
    };
    let mut draw = |alphabet: &[u8], n: usize| -> String {
        (0..n)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize] as char)
            .collect()
    };
    let mut out = Vec::new();
    for _ in 0..16 {
        out.push(draw(ALNUM, 40));
        out.push(draw(HEX, 40));
        out.push(format!("{}=", draw(B64, 43)));
        out.push(draw(PASS, 16));
        out.push(format!(
            "{}.{}.{}",
            draw(ALNUM, 20),
            draw(ALNUM, 40),
            draw(ALNUM, 32)
        ));
        out.push(format!(
            "{}-{}-{}-{}-{}",
            draw(HEX, 8),
            draw(HEX, 4),
            draw(HEX, 4),
            draw(HEX, 4),
            draw(HEX, 12)
        ));
        out.push(draw(ALNUM, 24));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_the_default() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.engine.sensitivity, Sensitivity::Balanced);
        assert!(c.engine.strip_invisible && !c.engine.pii);
        assert!(c.notices.is_empty());
    }

    #[test]
    fn allowlist_accepts_both_shapes() {
        let a = Config::parse("allowlist = [\"x-fixture\"]").unwrap();
        let b = Config::parse("[allowlist]\npatterns = [\"x-fixture\"]").unwrap();
        assert_eq!(a.engine.allowlist, b.engine.allowlist);
    }

    #[test]
    fn catch_all_allowlists_are_refused() {
        for p in [
            ".*",
            ".",
            "",
            "(?:)",
            "a*",
            "|x",
            "\\w",
            "\\S+",
            "[A-Za-z0-9]",
            "\\d",
            "[a-f]",
            "^.{8,}$",
            "(?i)[a-z]",
            "a",
        ] {
            assert!(catch_all(p).is_some(), "{p} should be refused");
            let toml = format!("allowlist = [{p:?}]");
            let err = Config::parse(&toml).unwrap_err().0;
            assert!(err.contains("narrower pattern"), "{err}");
        }
    }

    #[test]
    fn narrow_allowlists_keep_working() {
        for p in [
            "example\\.com",
            "sk-xxxx",
            "^test_",
            "fixture",
            "dummy",
            "changeme",
            "localhost",
            "(?i)^example",
            "[0-9a-f]{40}",
            "(?i)example-key$",
            "tests/fixtures/.*",
            "not a valid regex (",
        ] {
            assert_eq!(catch_all(p), None, "{p} should be accepted");
        }
    }

    #[test]
    fn weakening_keys_are_listed() {
        let c = Config::parse(
            "sensitivity = \"relaxed\"\ndisabled = [\"jwt\", \"bearer_token\"]\n\
             allowlist = [\"fixture\"]",
        )
        .unwrap();
        assert_eq!(
            c.weakening,
            [
                "allowlist: `fixture`",
                "disabled rules: jwt, bearer_token",
                "sensitivity = \"relaxed\""
            ]
        );
        let c = Config::parse("sensitivity = \"paranoid\"\npii = true").unwrap();
        assert!(c.weakening.is_empty());
        // Long lists are named up to a cap.
        let ids: Vec<String> = (0..13).map(|i| format!("\"r{i}\"")).collect();
        let c = Config::parse(&format!("disabled = [{}]", ids.join(", "))).unwrap();
        assert_eq!(
            c.weakening,
            ["disabled rules: r0, r1, r2, r3, r4, r5, r6, r7, r8, r9 (+3 more)"]
        );
    }

    #[test]
    fn the_default_engine_finds_every_canary() {
        let engine = redactor_core::Engine::new(EngineConfig::default()).unwrap();
        let text = canary_text();
        let found = engine
            .scan(&text, &redactor_core::ScanRequest::default())
            .findings;
        assert!(
            found.len() >= 5,
            "{:?}",
            found.iter().map(|f| &f.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn prefix_wide_allowlists_are_refused() {
        let prefixes = provider_prefixes();
        for p in ["ghp_", "AKIA", "sk-", "xoxb-", "glpat-", "github_pat_"] {
            assert!(prefixes.iter().any(|x| x == p), "{p} missing");
        }
        for p in [
            "^(ghp_|gho_|github_pat_|AKIA|sk-|xox[bpa]-|glpat-)",
            "^ghp_",
            "AKIA",
            "(?i)^xoxb-",
            "glpat-.*",
        ] {
            let why = catch_all(p).unwrap_or_else(|| panic!("{p} should be refused"));
            assert!(why.contains("starts with"), "{p}: {why}");
        }
    }

    #[test]
    fn rule_defaults_match_v1() {
        let c = Config::parse("[[rules]]\nname = \"r\"\nlabel = \"R\"\npattern = \"x\"").unwrap();
        let r = &c.engine.custom_rules[0];
        assert_eq!((r.confidence, r.group), (0.9, 0));
    }

    #[test]
    fn rule_without_a_label_is_an_error() {
        assert!(Config::parse("[[rules]]\nname = \"r\"\npattern = \"x\"").is_err());
    }

    #[test]
    fn stable_numbering_false_gets_a_notice() {
        let c = Config::parse("stable_numbering = false").unwrap();
        assert_eq!(c.notices.len(), 1);
    }

    #[test]
    fn discovery_walks_up() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(d.path().join(".redactor.toml"), "").unwrap();
        let found = discover(&sub, None).unwrap();
        assert!(found.ends_with(".redactor.toml"));
    }

    #[test]
    fn discovery_stops_at_the_repository_root_and_home() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".redactor.toml"), "").unwrap();
        let repo = d.path().join("repo");
        let sub = repo.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        assert_eq!(
            discover(&sub, None),
            None,
            "found above the repository root"
        );
        // The root itself is still searched.
        std::fs::write(repo.join("redactor.toml"), "").unwrap();
        assert!(discover(&sub, None)
            .unwrap()
            .ends_with(Path::new("repo").join("redactor.toml")));

        let home = d.path().join("home");
        let deep = home.join("work");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(discover(&deep, Some(&home)), None, "found above home");
        std::fs::write(home.join(".redactor.toml"), "").unwrap();
        assert!(discover(&deep, Some(&home)).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_configs_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("redactor.toml");
        std::fs::write(&f, "").unwrap();
        assert!(Config::load(Some(&f)).is_ok());
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o666)).unwrap();
        let err = Config::load(Some(&f)).unwrap_err().0;
        assert!(
            err.contains("refusing config") && err.contains("every user"),
            "{err}"
        );
    }
}
