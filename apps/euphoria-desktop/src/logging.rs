// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's log: warnings and errors go to the terminal, and the
//! `EUPHORIA_LOG` variable (`error`, `warn`, `info`, `debug` or `trace`)
//! picks how much.

use log::{LevelFilter, Log, Metadata, Record};

const DEFAULT_LEVEL: LevelFilter = LevelFilter::Warn;

struct TerminalLog {
    level: LevelFilter,
}

impl Log for TerminalLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "euphoria {} {}: {}",
                record.level().to_string().to_lowercase(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

pub(crate) fn level_from(setting: Option<&str>) -> LevelFilter {
    setting
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(DEFAULT_LEVEL)
}

pub(crate) fn init() {
    let level = level_from(std::env::var("EUPHORIA_LOG").ok().as_deref());
    let log = Box::new(TerminalLog { level });
    if log::set_boxed_logger(log).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_defaults_to_warnings() {
        assert_eq!(level_from(None), LevelFilter::Warn);
        assert_eq!(level_from(Some("nonsense")), LevelFilter::Warn);
    }

    #[test]
    fn the_level_can_be_chosen() {
        assert_eq!(level_from(Some("debug")), LevelFilter::Debug);
        assert_eq!(level_from(Some(" ERROR ")), LevelFilter::Error);
    }
}
