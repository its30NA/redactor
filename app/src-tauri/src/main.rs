// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    redactor_app::install_quiet_panic_hook();
    redactor_app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The Redactor desktop app runs on Windows. On this platform, use the `scrub` CLI.");
    std::process::exit(2);
}
