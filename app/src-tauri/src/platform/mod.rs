//! The platform layer: everything the shell needs from the operating system.
//!
//! Each OS lives in its own submodule. Only these submodules may use `unsafe`.

#[cfg(windows)]
pub mod windows;
