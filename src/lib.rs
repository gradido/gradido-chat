//! Gradido Chat: a lean native client for the chat of a Gradido community.
//!
//! - [`api`]: the server's GraphQL endpoint and the session token.
//! - [`core`]: chat logic without a window (threads, text, search, strings).
//! - [`platform`]: what differs between operating systems.
//! - [`app`]: the state between the two, and the windows (`ui/*.slint`).

slint::include_modules!();

pub mod api;
pub mod app;
pub mod core;
pub mod platform;

// Phones start here instead of in `main.rs`:
//
// #[cfg(target_os = "android")]
// #[unsafe(no_mangle)]
// fn android_main(android: slint::android::AndroidApp) {
//     slint::android::init(android).unwrap();
//     app::run().unwrap();
// }
