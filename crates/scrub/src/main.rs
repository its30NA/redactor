#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    scrub::install_quiet_panic_hook();
    let code = scrub::run(std::env::args_os());
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(2))
}
