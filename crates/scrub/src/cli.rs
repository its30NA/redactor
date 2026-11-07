//! Argument parsing (clap).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use redactor_core::Sensitivity;

/// Redact secrets from text before it leaves your machine.
#[derive(Debug, Parser)]
#[command(
    name = "scrub",
    version,
    about = "Redact secrets from text before sharing it with external AI assistants.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(flatten)]
    pub sanitize: SanitizeArgs,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sanitize a file or stdin (the default when no command is given).
    #[command(hide = true)]
    Sanitize(SanitizeArgs),
    /// Scan files and directories; exit 1 if secrets are found.
    Scan(ScanArgs),
    /// Scan the git-staged files (reads the staged blobs, for the pre-commit hook).
    Check(CheckArgs),
    /// Install the pre-commit hook into this repository.
    InstallHook(HookArgs),
    /// Sanitize the clipboard in place (Windows only).
    Clipboard(ClipboardArgs),
    /// List every detection rule.
    Detectors(DetectorsArgs),
    /// Print the scrub and engine versions.
    Version,
}

/// Options shared by every command that runs the engine.
#[derive(Debug, Args, Clone, Default)]
pub struct EngineArgs {
    /// Path to a redactor.toml config file (default: searched upwards from the current directory).
    #[arg(short = 'c', long, value_name = "CONFIG")]
    pub config: Option<PathBuf>,
    /// How eager detection is (overrides the config file).
    #[arg(long, value_enum, value_name = "LEVEL")]
    pub sensitivity: Option<SensitivityArg>,
    /// Also redact personal data: email, phone, IBAN, card numbers, IP addresses, SSN.
    #[arg(long)]
    pub pii: bool,
    /// Keep invisible, bidi and tag characters in the output (default: remove them).
    #[arg(long)]
    pub no_strip: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SensitivityArg {
    Relaxed,
    Balanced,
    Paranoid,
}

impl From<SensitivityArg> for Sensitivity {
    fn from(s: SensitivityArg) -> Sensitivity {
        match s {
            SensitivityArg::Relaxed => Sensitivity::Relaxed,
            SensitivityArg::Balanced => Sensitivity::Balanced,
            SensitivityArg::Paranoid => Sensitivity::Paranoid,
        }
    }
}

#[derive(Debug, Args, Clone, Default)]
pub struct SanitizeArgs {
    /// File to sanitize; omit or `-` for stdin.
    #[arg(value_name = "FILE")]
    pub input: Option<String>,
    #[command(flatten)]
    pub engine: EngineArgs,
    /// Per-label summary to stderr.
    #[arg(long)]
    pub summary: bool,
    /// Per-redaction report to stderr (values are masked).
    #[arg(long)]
    pub preview: bool,
    /// Print a unified diff instead of the sanitized text. The diff shows the original lines,
    /// secrets included: do not share it.
    #[arg(long, conflicts_with = "json")]
    pub diff: bool,
    /// Write a JSON audit log: when, from where, counts, lines and detectors; never values or
    /// fingerprints ('-' = stderr).
    #[arg(long, value_name = "PATH")]
    pub audit: Option<String>,
    /// Print the scan result as JSON (the desktop app's ScanResponse) instead of the text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Files or directories (default: '.').
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,
    #[command(flatten)]
    pub engine: EngineArgs,
    /// Rewrite files in place, sanitized.
    #[arg(long)]
    pub write: bool,
    /// Do not descend into directories.
    #[arg(long)]
    pub no_recursive: bool,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(flatten)]
    pub engine: EngineArgs,
}

#[derive(Debug, Args)]
pub struct HookArgs {
    /// Overwrite an existing hook.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ClipboardArgs {
    #[command(flatten)]
    pub engine: EngineArgs,
}

#[derive(Debug, Args)]
pub struct DetectorsArgs {
    /// Print Markdown (the source of docs/DETECTORS.md) instead of a plain table.
    #[arg(long)]
    pub markdown: bool,
}
