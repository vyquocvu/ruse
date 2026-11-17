//! macOS/AppKit implementation details.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSWorkspace};
use objc2_foundation::{NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::PlatformError;
use crate::shortcuts::BrowserShortcut;

/// An installed AppKit event monitor plus the block it retains.
pub(crate) struct EventMonitor {
    object: Retained<AnyObject>,
    _block: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>,
}

impl Drop for EventMonitor {
    fn drop(&mut self) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        // SAFETY: main thread; the monitor was returned by AppKit.
        unsafe {
            NSEvent::removeMonitor(&self.object);
        }
    }
}

pub(crate) fn native_parent(
    window: &mut gpui::Window,
) -> Result<browser_engine::NativeParent, PlatformError> {
    let handle = HasWindowHandle::window_handle(window).map_err(|_| PlatformError::NotAppKit)?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => {
            // SAFETY: the AppKit handle is the GPUI content view owned by the
            // window; it outlives the engine while the window is open.
            Ok(unsafe { browser_engine::NativeParent::from_ns_view(appkit.ns_view.as_ptr()) })
        }
        _ => Err(PlatformError::NotAppKit),
    }
}

/// Makes GPUI's content view the window's first responder so GPUI receives
/// keyboard input (needed after the native web view had focus).
pub(crate) fn focus_content_view(window: &gpui::Window) -> Result<(), PlatformError> {
    if MainThreadMarker::new().is_none() {
        return Err(PlatformError::NotMainThread);
    }
    let handle = HasWindowHandle::window_handle(window).map_err(|_| PlatformError::NotAppKit)?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(PlatformError::NotAppKit);
    };
    // SAFETY: the AppKit handle is the GPUI content view, alive with the window.
    let view: &objc2_app_kit::NSView = unsafe { &*appkit.ns_view.as_ptr().cast() };
    if let Some(ns_window) = view.window() {
        ns_window.makeFirstResponder(Some(view));
        Ok(())
    } else {
        Err(PlatformError::NotAppKit)
    }
}

fn file_url(path: &std::path::Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

pub(crate) fn open_path(path: &std::path::Path) -> Result<(), PlatformError> {
    if MainThreadMarker::new().is_none() {
        return Err(PlatformError::NotMainThread);
    }
    let url = file_url(path);
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(PlatformError::OpenUrl(path.display().to_string()))
    }
}

pub(crate) fn reveal_path(path: &std::path::Path) -> Result<(), PlatformError> {
    if MainThreadMarker::new().is_none() {
        return Err(PlatformError::NotMainThread);
    }
    let url = file_url(path);
    let urls = objc2_foundation::NSArray::arrayWithObject(&*url);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    Ok(())
}

pub(crate) fn open_external_url(url: &str) -> Result<(), PlatformError> {
    if MainThreadMarker::new().is_none() {
        return Err(PlatformError::NotMainThread);
    }
    let ns_url = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| PlatformError::OpenUrl(url.to_owned()))?;
    let opened = NSWorkspace::sharedWorkspace().openURL(&ns_url);
    if opened {
        Ok(())
    } else {
        Err(PlatformError::OpenUrl(url.to_owned()))
    }
}

fn shortcut_for(event: &NSEvent) -> Option<BrowserShortcut> {
    let flags = event.modifierFlags();
    let command = flags.contains(NSEventModifierFlags::Command);
    let control = flags.contains(NSEventModifierFlags::Control);
    let option = flags.contains(NSEventModifierFlags::Option);
    let shift = flags.contains(NSEventModifierFlags::Shift);
    let key = event.keyCode();

    if control && !command && !option {
        return match key {
            48 => Some(if shift {
                BrowserShortcut::PreviousTab
            } else {
                BrowserShortcut::NextTab
            }),
            _ => None,
        };
    }

    if !command || control || option {
        return None;
    }

    let shortcut = match key {
        37 => BrowserShortcut::FocusLocation,
        33 => BrowserShortcut::Back,
        30 => BrowserShortcut::Forward,
        15 if shift => BrowserShortcut::HardReload,
        15 => BrowserShortcut::Reload,
        46 => BrowserShortcut::Stop,
        45 if shift => BrowserShortcut::NewPrivateWindow,
        17 if shift => BrowserShortcut::ReopenClosedTab,
        17 => BrowserShortcut::NewTab,
        13 => BrowserShortcut::CloseTab,
        2 => BrowserShortcut::ToggleBookmark,
        11 if shift => BrowserShortcut::ToggleBookmarks,
        3 => BrowserShortcut::FindInPage,
        38 if shift => BrowserShortcut::ToggleDownloads,
        8 if shift => BrowserShortcut::CopyUrl,
        40 => BrowserShortcut::CommandPalette,
        5 if shift => BrowserShortcut::FindPrevious,
        5 => BrowserShortcut::FindNext,
        24 if shift => BrowserShortcut::ZoomIn,
        27 => BrowserShortcut::ZoomOut,
        29 => BrowserShortcut::ZoomReset,
        43 => BrowserShortcut::OpenSettings,
        12 => BrowserShortcut::Quit,
        _ => return None,
    };
    Some(shortcut)
}

pub(crate) fn install_shortcut_monitor(
    sender: async_channel::Sender<BrowserShortcut>,
) -> Result<EventMonitor, PlatformError> {
    if MainThreadMarker::new().is_none() {
        return Err(PlatformError::NotMainThread);
    }

    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid NSEvent for the lifetime of the call.
        let event_ref = unsafe { event.as_ref() };
        if let Some(shortcut) = shortcut_for(event_ref) {
            match sender.try_send(shortcut) {
                Ok(()) => return std::ptr::null_mut(),
                // The app is shutting down: let the event through instead of
                // swallowing it silently.
                Err(error) => {
                    tracing::debug!(%error, "shortcut receiver closed");
                }
            }
        }
        event.as_ptr()
    });

    // SAFETY: main-thread AppKit call; the block returns a valid event pointer
    // or null and only handles the commands listed in `shortcut_for`.
    let object = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    }
    .ok_or(PlatformError::MonitorUnavailable)?;

    Ok(EventMonitor {
        object,
        _block: block,
    })
}

/// Synthetic keyboard events for automated end-to-end probes.
///
/// Real user key events are handled by the shortcut monitor; synthetic events
/// are dispatched directly through AppKit and exercise the GPUI/WebKit key
/// paths without depending on the physical keyboard.
pub mod testing {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSView};
    use objc2_foundation::{NSPoint, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use crate::PlatformError;

    /// Sends key-down events to the window's first responder.
    pub struct KeySender {
        view: *mut NSView,
    }

    // SAFETY: KeySender is only constructed and used on the main thread; the
    // pointer refers to the GPUI content view, which lives as long as the
    // window. It is never sent across threads because `*mut T` is `!Send`.
    impl KeySender {
        pub fn new(window: &gpui::Window) -> Result<Self, PlatformError> {
            let handle =
                HasWindowHandle::window_handle(window).map_err(|_| PlatformError::NotAppKit)?;
            match handle.as_raw() {
                RawWindowHandle::AppKit(appkit) => Ok(Self {
                    view: appkit.ns_view.as_ptr().cast(),
                }),
                _ => Err(PlatformError::NotAppKit),
            }
        }

        fn window_number(&self) -> Result<isize, PlatformError> {
            if MainThreadMarker::new().is_none() {
                return Err(PlatformError::NotMainThread);
            }
            // SAFETY: the view pointer is valid for the life of the window.
            let view: &NSView = unsafe { &*self.view };
            let window = view.window().ok_or(PlatformError::NotAppKit)?;
            Ok(window.windowNumber())
        }

        /// Sends one key-down event with optional Command/Shift modifiers.
        pub fn send_key(
            &self,
            key_code: u16,
            characters: &str,
            command: bool,
            shift: bool,
        ) -> Result<(), PlatformError> {
            let mtm = MainThreadMarker::new().ok_or(PlatformError::NotMainThread)?;
            let window_number = self.window_number()?;
            let mut flags = NSEventModifierFlags::empty();
            if command {
                flags |= NSEventModifierFlags::Command;
            }
            if shift {
                flags |= NSEventModifierFlags::Shift;
            }
            let chars = NSString::from_str(characters);
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                NSEventType::KeyDown,
                NSPoint { x: 0.0, y: 0.0 },
                flags,
                0.0,
                window_number as _,
                None,
                &chars,
                &chars,
                false,
                key_code,
            )
            .ok_or(PlatformError::MonitorUnavailable)?;
            NSApplication::sharedApplication(mtm).sendEvent(&event);
            Ok(())
        }

        pub fn type_text(&self, text: &str) -> Result<(), PlatformError> {
            for character in text.chars() {
                let mut buffer = [0u8; 4];
                self.send_key(0, character.encode_utf8(&mut buffer), false, false)?;
            }
            Ok(())
        }

        pub fn press_enter(&self) -> Result<(), PlatformError> {
            self.send_key(36, "\r", false, false)
        }
    }
}
