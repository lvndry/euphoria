// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs a command with a pseudo terminal in front of it, so the command
//! believes it is on a terminal and shows its progress.

use std::io::Read;
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// A command's terminal output and the status it exited with.
#[allow(dead_code)]
pub struct Terminal {
    /// Every byte the terminal wrote, progress included.
    pub output: String,
    /// Whether the command exited successfully.
    pub succeeded: bool,
}

/// Runs `command` in a pseudo terminal and returns what it wrote and how it
/// exited. A reader thread keeps draining the terminal while the command
/// runs, so no progress is lost at the end.
#[allow(dead_code)]
pub fn capture(command: &std::process::Command) -> Terminal {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("the pseudo terminal opens");

    let mut argv = vec![command.get_program().to_os_string()];
    argv.extend(command.get_args().map(std::ffi::OsStr::to_os_string));
    let mut builder = CommandBuilder::from_argv(argv);
    for (name, value) in command.get_envs() {
        builder.env(name, value.unwrap_or_default());
    }
    let mut child = pair
        .slave
        .spawn_command(builder)
        .expect("the command starts");

    let terminal = pair
        .master
        .try_clone_reader()
        .expect("the terminal reader clones");
    let collected = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let collected = Arc::clone(&collected);
        std::thread::spawn(move || {
            let mut terminal = terminal;
            let mut buffer = [0u8; 4096];
            loop {
                match terminal.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => collected
                        .lock()
                        .expect("the output lock")
                        .extend_from_slice(&buffer[..read]),
                }
            }
        })
    };
    drop(pair.slave);
    let status = child.wait().expect("the command's status is reported");
    reader.join().expect("the reader thread finishes");
    Terminal {
        output: String::from_utf8_lossy(&collected.lock().expect("the output lock")).into_owned(),
        succeeded: status.exit_code() == 0,
    }
}
