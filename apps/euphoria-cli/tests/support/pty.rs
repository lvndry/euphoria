// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs a command with a pseudo terminal in front of it, so the command
//! believes it is on a terminal and shows its progress.

use std::io::Read;

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
/// exited.
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

    let mut terminal = pair
        .master
        .try_clone_reader()
        .expect("the terminal reader clones");
    drop(pair.slave);
    let mut terminal_output = Vec::new();
    let _ = terminal.read_to_end(&mut terminal_output);
    let status = child.wait().expect("the command's status is reported");
    Terminal {
        output: String::from_utf8_lossy(&terminal_output).into_owned(),
        succeeded: status.exit_code() == 0,
    }
}
