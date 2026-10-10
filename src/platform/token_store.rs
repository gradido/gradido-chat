//! The token rests in the system's own credential store, never in a file.
//!
//! `keyring-core` is the common face; each platform brings its store crate.
//! Where none is wired up (or the store is not running, e.g. a Linux session
//! without a Secret Service), `available()` is false and the token simply
//! lives as long as the program does.

use keyring_core::Entry;
use std::sync::LazyLock;

const SERVICE: &str = "gradido-chat";

static READY: LazyLock<bool> = LazyLock::new(|| {
    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new().ok();
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "android"))))]
    let store = zbus_secret_service_keyring_store::Store::new().ok();
    // macOS: apple_native_keyring_store::keychain::Store::new()
    // iOS:   apple_native_keyring_store::protected::Store::new()
    // Android: android_native_keyring_store::Store::new() — needs the JNI
    //          context from `android_main` before the first use.
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    let store: Option<std::sync::Arc<keyring_core::CredentialStore>> = None;

    match store {
        Some(store) => {
            keyring_core::set_default_store(store);
            true
        }
        None => false,
    }
});

pub fn available() -> bool {
    *READY
}

/// One entry per server, so two communities do not overwrite each other.
fn entry(server: &str) -> Option<Entry> {
    if !available() {
        return None;
    }
    Entry::new(SERVICE, server).ok()
}

pub fn load(server: &str) -> Option<String> {
    entry(server)?.get_password().ok().filter(|token| !token.is_empty())
}

pub fn save(server: &str, token: &str) -> bool {
    entry(server).is_some_and(|e| e.set_password(token).is_ok())
}

pub fn forget(server: &str) {
    if let Some(entry) = entry(server) {
        let _ = entry.delete_credential();
    }
}
