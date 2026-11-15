// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening Euphoria when the user logs in, with Live in the menu bar. macOS
//! 13 and later register the app itself as a login item; the user can see and
//! remove it under System Settings, General, Login Items.

/// True when this system can open the app at login.
#[cfg(target_os = "macos")]
pub(crate) fn is_supported() -> bool {
    objc2::runtime::AnyClass::get(c"SMAppService").is_some()
}

/// True when the system opens the app at login now.
#[cfg(target_os = "macos")]
pub(crate) fn is_enabled() -> bool {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    if !is_supported() {
        return false;
    }
    // SAFETY: the class exists, as checked, and the main app service is a plain object.
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn is_enabled() -> bool {
    false
}

/// Turns opening at login on or off. The error says what macOS refused, as
/// for an app that is not in a bundle.
#[cfg(target_os = "macos")]
pub(crate) fn set_enabled(enabled: bool) -> Result<(), String> {
    use objc2_service_management::SMAppService;

    if !is_supported() {
        return Err("Opening at login needs macOS 13 or later.".to_string());
    }
    // SAFETY: the class exists, as checked.
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|error| error.localizedDescription().to_string())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_enabled(_enabled: bool) -> Result<(), String> {
    Err("Opening at login is not available on this system.".to_string())
}

/// True when the system started the app because it is a login item. Only
/// meaningful while the app is starting.
#[cfg(target_os = "macos")]
pub(crate) fn launched_as_login_item() -> bool {
    use objc2_foundation::NSAppleEventManager;

    let code = |text: &[u8; 4]| u32::from_be_bytes(*text);
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
        return false;
    };
    // SAFETY: a plain getter of the event, with a four character code as its argument.
    let property: Option<objc2::rc::Retained<objc2_foundation::NSAppleEventDescriptor>> =
        unsafe { objc2::msg_send![&*event, paramDescriptorForKeyword: code(b"prdt")] };
    property.is_some_and(|property| property.enumCodeValue() == code(b"lgit"))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn launched_as_login_item() -> bool {
    false
}
