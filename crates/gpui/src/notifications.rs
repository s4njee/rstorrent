//! Completion notifications: a desktop ping when a download finishes.
//!
//! A port of the notification half of `src-tauri/src/notifications.rs`. macOS
//! gets a real `NSUserNotification` delivered by raw message sends (the class
//! is not bound at the `objc2-app-kit` feature set this crate uses); Windows
//! gets a toast through `tauri-winrt-notification`; everywhere else the Log
//! pane's completion line (written by the caller) is the notification, so a
//! headless or Linux build never pretends to have pinged.

/// Tell the user `name` finished downloading.
///
/// Best-effort by design: a notification that cannot be shown must not fail
/// the poll tick that earned it. The caller always writes the Log pane line
/// too, so the completion is visible even when this is a no-op.
pub fn post_completion(name: &str, size_bytes: i64) {
    let _ = (name, size_bytes);
    #[cfg(target_os = "macos")]
    mac::notify(name, size_bytes);
    #[cfg(windows)]
    toast::notify(name, size_bytes);
}

#[cfg(windows)]
mod toast {
    //! A WinRT toast, ported from the Windows arm of
    //! `src-tauri/src/notifications.rs`.
    //!
    //! Click-to-select is not wired yet: the Tauri shell used `on_activated`
    //! to focus the window and select the torrent. When it is added, prefer
    //! GPUI's own `App::show_system_notification` / `on_system_notification_response`
    //! (gpui-pre registers the AUMID and routes activations back onto the
    //! foreground executor), which needs an `&App` threaded through to here.

    use tauri_winrt_notification::{Duration, Sound, Toast};

    /// The AppUserModelID the Tauri installer registered. Toasts addressed to
    /// it only show once something has registered that ID (an installer's
    /// Start-menu shortcut, or an `AppUserModelId` registry key).
    const APP_ID: &str = "com.rstorrent.app";

    pub fn notify(name: &str, size_bytes: i64) {
        let title = name.to_owned();
        let body = format!("Download complete · {}", crate::format::bytes(size_bytes));
        // `show` makes synchronous WinRT calls (activation, XML parsing, the
        // notifier), so keep them off the UI thread. A thread that cannot be
        // spawned costs the toast, nothing else.
        let _ = std::thread::Builder::new()
            .name("completion-notification".to_owned())
            .spawn(move || {
                // Never let a WinRT failure (or a panic inside the crate)
                // escape: the notification is best-effort.
                let _ = std::panic::catch_unwind(|| deliver(&title, &body));
            });
    }

    fn deliver(title: &str, body: &str) {
        let show = |app_id: &str| {
            Toast::new(app_id)
                .title(title)
                .text1(body)
                .sound(Some(Sound::Default))
                .duration(Duration::Short)
                .show()
        };
        // Toasts are addressed by AppUserModelID, which only exists once an
        // installer has written a Start-menu shortcut. A bare exe (the zip
        // from tools/bundle-gpui-windows.ps1, or `cargo run`) has none, so fall
        // back to PowerShell's AUMID: the toast still shows, attributed to
        // PowerShell. Note that Windows may accept an unregistered AUMID
        // without an error and simply not display the toast, in which case the
        // fallback never runs; the installer follow-up should register
        // `APP_ID` so that path is the one that works.
        if show(APP_ID).is_err() {
            let _ = show(Toast::POWERSHELL_APP_ID);
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    //! `NSUserNotificationCenter` delivery by hand, kept behind one function
    //! so the `unsafe` stays in one place and the rest of the crate never
    //! sees it.

    #![allow(unsafe_code)]

    use std::panic::AssertUnwindSafe;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    /// The value of `NSUserNotificationDefaultSoundName`, which is a string
    /// constant — not a class method. Asking the class for it throws
    /// "unrecognized selector", which is how the first finished download used
    /// to abort the app.
    const DEFAULT_SOUND_NAME: &str = "DefaultSoundName";

    pub fn notify(name: &str, size_bytes: i64) {
        let body = format!("Download complete · {}", crate::format::bytes(size_bytes));
        // An Objective-C exception must cost the notification, never the app:
        // Rust cannot unwind through one, so an uncaught throw aborts.
        let _ = objc2::exception::catch(AssertUnwindSafe(|| deliver(name, &body)));
    }

    fn deliver(title: &str, body: &str) {
        let Some(center_class) = AnyClass::get(c"NSUserNotificationCenter") else {
            return;
        };
        let Some(notification) = build(title, body) else {
            return;
        };
        // SAFETY: a class method on a real class, called on the GPUI main
        // thread (which AppKit owns); nil (an unbundled process) is checked.
        unsafe {
            let center: Option<Retained<AnyObject>> =
                msg_send![center_class, defaultUserNotificationCenter];
            if let Some(center) = center {
                let _: () = msg_send![&*center, deliverNotification: &*notification];
            }
        }
    }

    /// The notification itself, split from delivery so a test can build one
    /// without a bundle (an unbundled process has no notification center).
    pub(super) fn build(title: &str, body: &str) -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"NSUserNotification")?;
        // SAFETY: `new` on a real class returns a +1 object or nil, and each
        // setter below is a documented `NSUserNotification` property taking an
        // `NSString`.
        unsafe {
            let notification: Option<Retained<AnyObject>> = msg_send![class, new];
            let notification = notification?;
            let title = NSString::from_str(title);
            let body = NSString::from_str(body);
            let sound = NSString::from_str(DEFAULT_SOUND_NAME);
            let _: () = msg_send![&*notification, setTitle: &*title];
            let _: () = msg_send![&*notification, setInformativeText: &*body];
            let _: () = msg_send![&*notification, setSoundName: &*sound];
            Some(notification)
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn a_completion_notification_builds_without_throwing() {
        // The regression: building one used to send an unknown selector and
        // abort the process, so reaching the assertion at all is the test.
        let built = objc2::exception::catch(|| super::mac::build("ubuntu.iso", "done").is_some());
        assert_eq!(built.ok(), Some(true));
    }

    #[test]
    fn posting_outside_a_bundle_is_a_quiet_no_op() {
        super::post_completion("ubuntu.iso", 1 << 30);
    }
}
