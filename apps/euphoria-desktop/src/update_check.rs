// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asks the project's releases whether a newer version is out and, when it
//! is, writes one into the notice strip. Any failure is a quiet no-op, so a
//! machine without a connection never sees an error.

use std::time::Duration;

#[cfg_attr(test, allow(dead_code))]
const RELEASES_URL: &str = "https://api.github.com/repos/lvndry/euphoria/releases/latest";
#[cfg_attr(test, allow(dead_code))]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg_attr(test, allow(dead_code))]
const DEADLINE: Duration = Duration::from_secs(10);

/// Fetches the tag of the newest release, whatever the means of it are.
#[derive(Clone, Copy)]
pub(crate) struct UpdateChecker {
    pub(crate) fetch: fn() -> Result<String, String>,
}

impl Default for UpdateChecker {
    fn default() -> Self {
        Self {
            fetch: default_fetch,
        }
    }
}

#[cfg(not(test))]
fn default_fetch() -> Result<String, String> {
    UpdateChecker::fetch_latest()
}

// The tests open the app many times; keep them off the network so none of
// them waits on a release the machine cannot reach.
#[cfg(test)]
fn default_fetch() -> Result<String, String> {
    Err("no connection".to_string())
}

impl UpdateChecker {
    /// Reads the newest release tag from the project's releases.
    #[cfg_attr(test, allow(dead_code))]
    fn fetch_latest() -> Result<String, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(DEADLINE))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .build(),
            )
            .build()
            .into();
        let mut response = agent
            .get(RELEASES_URL)
            .header("accept", "application/vnd.github+json")
            .call()
            .map_err(|error| format!("could not reach the releases: {error}"))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| format!("could not read the releases: {error}"))?;
        let release: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("could not parse the releases: {error}"))?;
        release
            .get("tag_name")
            .and_then(|tag| tag.as_str())
            .map(str::to_string)
            .ok_or_else(|| "the release has no tag".to_string())
    }
}

/// Whether `latest` is newer than `current`, on the numbers of their
/// components: `v` prefixes are ignored and a longer list wins on an equal
/// prefix, so `0.1.10` is newer than `0.1.9` and `1.0.1` is newer than `1.0`.
pub(crate) fn newer_release(latest: &str, current: &str) -> bool {
    let parts = |tag: &str| {
        tag.strip_prefix('v')
            .unwrap_or(tag)
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .split('.')
            .map(|component| component.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let latest = parts(latest);
    let current = parts(current);
    let (longest, shorter) = if latest.len() >= current.len() {
        (&latest, &current)
    } else {
        (&current, &latest)
    };
    for (latest_part, current_part) in longest.iter().zip(shorter) {
        if latest_part != current_part {
            return latest_part > current_part;
        }
    }
    latest.len() > current.len()
}

/// The text for the notice strip when `latest` is out, or nothing to say.
pub(crate) fn decide(latest: &str, current: &str) -> Option<String> {
    (newer_release(latest, current)).then(|| {
        format!(
            "A newer version of Euphoria is available: {latest}. Update at \
             https://github.com/lvndry/euphoria/releases/latest"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{decide, newer_release};

    #[test]
    fn a_newer_release_earns_a_notice() {
        let notice = decide("0.2.0", "0.1.0").unwrap();
        assert_eq!(
            notice,
            "A newer version of Euphoria is available: 0.2.0. Update at \
             https://github.com/lvndry/euphoria/releases/latest"
        );
    }

    #[test]
    fn the_same_release_says_nothing() {
        assert_eq!(decide("0.1.0", "0.1.0"), None);
        assert_eq!(decide("v0.1.0", "0.1.0"), None);
    }

    #[test]
    fn an_older_release_says_nothing() {
        assert_eq!(decide("0.0.9", "0.1.0"), None);
    }

    #[test]
    fn the_v_prefix_is_ignored() {
        assert!(newer_release("v0.2.0", "0.1.0"));
        assert!(newer_release("0.2.0", "v0.1.0"));
    }

    #[test]
    fn components_are_numbers_not_words() {
        assert!(newer_release("0.1.10", "0.1.9"));
        assert!(newer_release("1.10.0", "1.9.0"));
    }

    #[test]
    fn the_longer_list_wins_on_an_equal_prefix() {
        assert!(newer_release("1.0.1", "1.0"));
        assert!(newer_release("1.0.1", "1.0.0"));
        assert!(!newer_release("1.0", "1.0.1"));
    }

    #[test]
    fn a_fetch_error_is_no_news() {
        let checker = super::testing::with_fetch(|| Err("no connection".to_string()));
        assert_eq!((checker.fetch)().ok(), None);
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::UpdateChecker;

    /// A checker whose reach for the releases is `fetch`, for the tests.
    pub(crate) fn with_fetch(fetch: fn() -> Result<String, String>) -> UpdateChecker {
        UpdateChecker { fetch }
    }
}
