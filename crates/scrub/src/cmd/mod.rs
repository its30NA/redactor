//! Command implementations.

pub mod check;
pub mod clipboard;
pub mod detectors;
pub mod hook;
pub mod sanitize;
pub mod scan;

use redactor_core::{Engine, EngineConfig, ENGINE_VERSION};

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
            detectors::run(&args);
            Ok(0)
        }
        Some(Command::Version) => {
            let rules = Engine::new(EngineConfig::default()).map_or(0, |e| e.rule_count());
            crate::out(&format!(
                "scrub {} (engine {ENGINE_VERSION}, {rules} rules)\n",
                env!("CARGO_PKG_VERSION")
            ));
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
    Ok(Loaded { engine, config })
}

/// Warns, on stderr, about every setting in the loaded config file that lowers detection.
pub fn warn_weakening(config: &Config) {
    let (Some(path), false) = (&config.source, config.weakening.is_empty()) else {
        return;
    };
    eprintln!(
        "scrub: warning: {} lowers detection for this check: {}",
        path.display(),
        config.weakening.join("; ")
    );
}

/// Sorted, de-duplicated labels joined for the one-line reports.
pub fn kinds(labels: impl Iterator<Item = String>) -> String {
    let set: std::collections::BTreeSet<String> = labels.collect();
    set.into_iter().collect::<Vec<_>>().join(", ")
}
