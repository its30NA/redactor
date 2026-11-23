// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    redactor_app::platform::no_core_dumps();
    redactor_app::install_quiet_panic_hook();
    if let Err(redactor_app::platform::Unavailable(why)) = redactor_app::run() {
        eprintln!("{why}");
        std::process::exit(2);
    }
}
