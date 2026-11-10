// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the `euphoria` binary with its data in a folder of its own, so tests
//! never read or write the cache of the person running them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A folder under the system's temporary folder, deleted when dropped.
pub struct TemporaryFolder(PathBuf);

impl TemporaryFolder {
    pub fn new() -> Self {
        static CREATED: AtomicUsize = AtomicUsize::new(0);
        let folder = std::env::temp_dir().join(format!(
            "euphoria-cli-test-{}-{}",
            std::process::id(),
            CREATED.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).expect("the temporary folder is created");
        Self(folder)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The `euphoria` binary with `EUPHORIA_DATA_DIR` pointing at a temporary
/// folder that lasts as long as this value.
pub struct Euphoria {
    data_directory: TemporaryFolder,
}

impl Euphoria {
    pub fn new() -> Self {
        Self {
            data_directory: TemporaryFolder::new(),
        }
    }

    pub fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments)
            .output()
            .expect("the euphoria binary runs")
    }

    pub fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_euphoria"));
        command
            .args(arguments)
            .env("EUPHORIA_DATA_DIR", self.data_directory.path());
        command
    }

    #[allow(dead_code)]
    pub fn data_directory(&self) -> &Path {
        self.data_directory.path()
    }

    #[allow(dead_code)]
    pub fn cache_path(&self) -> PathBuf {
        self.data_directory.path().join("analysis-cache.sqlite3")
    }
}

pub fn fixtures_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/fixtures")
}

#[allow(dead_code)]
pub fn standard_error(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
