//! Everything that differs between operating systems sits behind this module.
//!
//! | Service        | Windows            | Linux                 | macOS / iOS / Android (to come)        |
//! | -------------- | ------------------ | --------------------- | -------------------------------------- |
//! | token store    | Credential Manager | Secret Service (DBus) | Keychain / Keychain / Android Keystore |
//! | notification   | toast              | org.freedesktop DBus  | UserNotifications / … / JNI            |
//! | sound          | WASAPI             | ALSA                  | CoreAudio / CoreAudio / AAudio         |
//! | open a link    | ShellExecute       | xdg-open              | `open` / UIApplication / Intent        |
//! | settings file  | %APPDATA%          | ~/.config             | app container                          |
//!
//! Adding a platform means adding a `cfg` arm in each file here, and its
//! credential store in `Cargo.toml`. Nothing outside this module asks which
//! system it runs on. The tray icon is the one exception: it is a Slint
//! element (`ui/tray.slint`) and simply not created on a phone.

pub mod notify;
pub mod settings;
pub mod sound;
pub mod token_store;

/// Hands a link to the system: `https://`, `mailto:`, `jitsi-meet://`.
pub fn open_link(url: &str) {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let url = url.to_string();
        // xdg-open may take its time; never on the UI thread.
        std::thread::spawn(move || {
            let _ = open::that(url);
        });
    }
    // Android: an ACTION_VIEW intent through `jni`; iOS: UIApplication.open.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = url;
}

/// Whether this system can show a tray icon at all.
pub const HAS_TRAY: bool = cfg!(not(any(target_os = "android", target_os = "ios")));
