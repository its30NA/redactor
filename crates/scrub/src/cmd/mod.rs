//! Command implementations.

pub mod check;
pub mod clipboard;
pub mod detectors;
pub mod hook;
pub mod sanitize;
pub mod scan;

use redactor_core::{Engine, EngineConfig, Warning, WarningKind, ENGINE_VERSION};

use crate::cli::{Cli, Command, EngineArgs};
use crate::config::Config;
use crate::CliError;

pub fn dispatch(cli: Cli) -> Result<i32, CliError> {
    match cli.command {
        None => sanitize::run(&cli.sanitize),
        Some(Command::Sanitize(args)) => sanitize::run(&args),
        Some(Command::Scan(args)) => scan::run(&args),
        Some(Command::Check(args)) => check::run(&args),
        Some(Command::InstallHook(args)) => hook::run(&args),
        Some(Command::Clipboard(args)) => clipboard::run(&args),
        Some(Command::Detectors(args)) => {
            detectors::run(&args)?;
            Ok(0)
        }
        Some(Command::Version) => {
            let rules = Engine::new(EngineConfig::default()).map_or(0, |e| e.rule_count());
            crate::out(&format!(
                "scrub {} (engine {ENGINE_VERSION}, {rules} rules)\n",
                env!("CARGO_PKG_VERSION")
            ))?;
            Ok(0)
        }
    }
}

/// The engine and the config it was built from.
pub struct Loaded {
    pub engine: Engine,
    pub config: Config,
}

/// Loads the config (file, then command-line overrides), prints its notices, builds the engine.
pub fn load_engine(args: &EngineArgs) -> Result<Engine, CliError> {
    load(args).map(|l| l.engine)
}

/// [`load_engine`], keeping the config. A loaded file is always named on stderr.
pub fn load(args: &EngineArgs) -> Result<Loaded, CliError> {
    let config = Config::load(args.config.as_deref()).map_err(|e| CliError::new(2, e.0))?;
    if let Some(path) = &config.source {
        eprintln!("scrub: using config: {}", path.display());
    }
    for n in &config.notices {
        eprintln!("scrub: {n}");
    }
    let mut engine = config.engine.clone();
    if let Some(s) = args.sensitivity {
        engine.sensitivity = s.into();
    }
    if args.pii {
        engine.pii = true;
    }
    if args.no_strip {
        engine.strip_invisible = false;
    }
    let engine =
        Engine::new(engine).map_err(|e| CliError::new(2, format!("invalid config: {e}")))?;
    Ok(Loaded {
        engine: test_hooks(engine),
        config,
    })
}

/// Debug builds only: `SCRUB_TEST_VERIFY_CAP` lowers the engine's verify cap, so the tests
/// can reach it. Release builds have no such hook.
fn test_hooks(engine: Engine) -> Engine {
    #[cfg(debug_assertions)]
    if let Some(cap) = std::env::var("SCRUB_TEST_VERIFY_CAP")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return engine.with_verify_cap(cap);
    }
    engine
}

/// Prints every engine warning about the input `name` on stderr, one line each. True when one
/// of them means the output may still hold a secret the engine did not look at; the run then
/// ends with exit code 2, like any output that cannot be trusted complete.
///
/// - `DecodeLimit`: an encoded part was nested too deep or past the decode budget, so what it
///   holds was never scanned. Exit 2.
/// - `VerifyCapHit`: the rescan stopped at its limit, so the output was not verified clean.
///   Exit 2.
/// - `InvisibleChars`, `BidiControls`, `TagChars`: hidden characters were seen through while
///   scanning (and stripped from the output unless `strip_invisible = false`). A note; the
///   exit code stays.
pub fn warn_engine(name: &str, warnings: &[Warning]) -> bool {
    let mut untrusted = false;
    for w in warnings {
        let n = w.count;
        match w.kind {
            WarningKind::DecodeLimit => {
                untrusted = true;
                eprintln!(
                    "scrub: warning: {name}: {n} encoded part(s) not decoded (nested too deep or \
                     past the decode limit); a secret inside may remain, review it before sharing"
                );
            }
            WarningKind::VerifyCapHit => {
                untrusted = true;
                eprintln!(
                    "scrub: warning: {name}: the output could not be verified clean (the rescan \
                     limit was reached); a secret may remain, review it before sharing"
                );
            }
            WarningKind::InvisibleChars => eprintln!(
                "scrub: note: {name}: {n} invisible character(s) (zero-width, joiners, variation \
                 selectors, fillers) were seen through while scanning"
            ),
            WarningKind::BidiControls => eprintln!(
                "scrub: note: {name}: {n} direction control(s) (bidi overrides and isolates) were \
                 seen through while scanning"
            ),
            WarningKind::TagChars => eprintln!(
                "scrub: note: {name}: {n} tag character(s) (invisible ASCII copies) were seen \
                 through while scanning"
            ),
        }
    }
    untrusted
}

/// Warns, on stderr, about every setting in the loaded config file that lowers detection, one
/// line each, naming every allowlist entry and disabled rule (capped with "+N more").
pub fn warn_weakening(config: &Config) {
    let (Some(path), false) = (&config.source, config.weakening.is_empty()) else {
        return;
    };
    eprintln!(
        "scrub: warning: {} lowers detection for this check:",
        path.display()
    );
    for w in &config.weakening {
        eprintln!("scrub:   {w}");
    }
}

/// Refuses an engine that finds none of a set of common secrets: a config that leaves
/// `scrub check` unable to block anything is an error, not a warning.
pub fn require_detection(engine: &Engine, config: &Config) -> Result<(), CliError> {
    let canary = crate::config::canary_text();
    if !engine
        .scan(&canary, &redactor_core::ScanRequest::default())
        .findings
        .is_empty()
    {
        return Ok(());
    }
    let from = config
        .source
        .as_ref()
        .map_or_else(|| "these settings".to_string(), |p| p.display().to_string());
    Err(CliError::new(
        2,
        format!(
            "{from} leaves `scrub check` unable to block anything: with it, not even a \
             password, an AWS key, an OpenAI key, a bearer token or a private key is detected; \
             re-enable rules or narrow the allowlist"
        ),
    ))
}

/// Sorted, de-duplicated labels joined for the one-line reports.
pub fn kinds(labels: impl Iterator<Item = String>) -> String {
    let set: std::collections::BTreeSet<String> = labels.collect();
    set.into_iter().collect::<Vec<_>>().join(", ")
}
