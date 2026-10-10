//! The system's own notification for a message that arrived unseen.

/// Never blocks: a notification daemon that does not answer must not stall the chat.
pub fn show(title: &str, body: &str) {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let (title, body) = (title.to_string(), body.to_string());
        std::thread::spawn(move || {
            let mut note = notify_rust::Notification::new();
            note.appname("Gradido Chat").summary(&title).body(&body);
            #[cfg(all(unix, not(target_os = "macos")))]
            note.icon("mail-message-new");
            let _ = note.show();
        });
    }
    // Android: NotificationManager through `jni`; iOS: UNUserNotificationCenter.
    // Both also need a background wake-up (FCM / APNs) the server does not send yet.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = (title, body);
}
