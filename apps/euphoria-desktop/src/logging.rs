// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's log: warnings and errors go to the terminal, and the
//! `EUPHORIA_LOG` variable (`error`, `warn`, `info`, `debug` or `trace`)
//! picks how much. Unless it is set, libraries only report errors: their
//! warnings, such as a decoder skipping a few junk bytes at the start of a
//! file, describe files that still play fine.

use log::{LevelFilter, Log, Metadata, Record};

const DEFAULT_LEVEL: LevelFilter = LevelFilter::Warn;

struct TerminalLog {
    level: LevelFilter,
    quiet_libraries: bool,
}

impl TerminalLog {
    fn limit_for(&self, target: &str) -> LevelFilter {
        if self.quiet_libraries && !target.starts_with("euphoria") {
            LevelFilter::Error
        } else {
            self.level
        }
    }
}

impl Log for TerminalLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.limit_for(metadata.target())
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
    let setting = std::env::var("EUPHORIA_LOG").ok();
    let level = level_from(setting.as_deref());
    let log = Box::new(TerminalLog {
        level,
        quiet_libraries: setting.is_none(),
    });
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

    #[test]
    fn library_warnings_stay_quiet_by_default() {
        let log = TerminalLog {
            level: DEFAULT_LEVEL,
            quiet_libraries: true,
        };
        assert_eq!(
            log.limit_for("symphonia_core::formats::probe"),
            LevelFilter::Error
        );
        assert_eq!(
            log.limit_for("euphoria_desktop::output_stream"),
            LevelFilter::Warn
        );
        assert_eq!(log.limit_for("euphoria_core::audio"), LevelFilter::Warn);
    }

    #[test]
    fn a_chosen_level_applies_to_libraries_too() {
        let log = TerminalLog {
            level: LevelFilter::Debug,
            quiet_libraries: false,
        };
        assert_eq!(
            log.limit_for("symphonia_core::formats::probe"),
            LevelFilter::Debug
        );
    }
}
