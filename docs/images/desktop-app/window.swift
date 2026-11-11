// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// Finds and drives the window of one process for make_screenshots.py, so that
// only the app instance the script started is ever captured or sent keys.
//
//   window id <pid>                      print the number of its main window
//   window key <pid> <keycode> [flags]   press a key, flags such as cmd,shift

import CoreGraphics
import Foundation

func mainWindowNumber(of pid: pid_t) -> Int? {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        return nil
    }
    for window in windows {
        let owner = window[kCGWindowOwnerPID as String] as? Int ?? -1
        let layer = window[kCGWindowLayer as String] as? Int ?? -1
        if owner == Int(pid) && layer == 0, let number = window[kCGWindowNumber as String] as? Int {
            return number
        }
    }
    return nil
}

let arguments = CommandLine.arguments
guard arguments.count >= 3, let pid = pid_t(arguments[2]) else {
    exit(2)
}
guard let windowNumber = mainWindowNumber(of: pid) else {
    exit(1)
}

switch arguments[1] {
case "id":
    print(windowNumber)
case "key":
    guard arguments.count >= 4, let code = UInt16(arguments[3]) else {
        exit(2)
    }
    var flags: CGEventFlags = []
    if arguments.count > 4 {
        for name in arguments[4].split(separator: ",") {
            switch name {
            case "cmd": flags.insert(.maskCommand)
            case "shift": flags.insert(.maskShift)
            default: break
            }
        }
    }
    let source = CGEventSource(stateID: .hidSystemState)
    for isDown in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: isDown) else {
            exit(1)
        }
        event.flags = flags
        event.postToPid(pid)
        usleep(40_000)
    }
default:
    exit(2)
}
