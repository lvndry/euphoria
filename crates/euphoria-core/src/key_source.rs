// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which detector found a key.

/// The detector that found a key, which sets how its confidence is rated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySource {
    /// libkeyfinder, which Euphoria used before drone-v2 and no longer
    /// ships. Only analyses stored by earlier versions carry it. Its score
    /// margin runs from 0 to about 0.1.
    KeyFinder,
    /// drone-v2, whose margin is the difference of two probabilities.
    Drone,
    /// stratum-dsp, in builds without the `drone` feature. It gives no
    /// margin.
    Stratum,
}

impl KeySource {
    /// The name the analysis cache stores.
    pub fn as_str(self) -> &'static str {
        match self {
            KeySource::KeyFinder => "libkeyfinder",
            KeySource::Drone => "drone-v2",
            KeySource::Stratum => "stratum-dsp",
        }
    }

    /// The source a stored name stands for, or `None` for a name this
    /// version does not know.
    pub fn from_stored(name: &str) -> Option<Self> {
        [KeySource::KeyFinder, KeySource::Drone, KeySource::Stratum]
            .into_iter()
            .find(|source| source.as_str() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_read_back_from_their_stored_names() {
        for source in [KeySource::KeyFinder, KeySource::Drone, KeySource::Stratum] {
            assert_eq!(KeySource::from_stored(source.as_str()), Some(source));
        }
        assert_eq!(KeySource::from_stored("other"), None);
    }
}
