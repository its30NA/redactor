//! Windows-only code: clipboard, clipboard history, session lock and sleep, WebView2 settings.
//! The only place in the app where `unsafe` is allowed; every block has a
//! `// SAFETY:` comment.
#![allow(unsafe_code)]

pub mod clipboard;
pub mod history;
pub mod lock;
pub mod webview;
