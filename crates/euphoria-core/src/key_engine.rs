// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The choice of key detector, and which detector found a key.

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::error::EuphoriaError;

/// Which detector decides the key of a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEngine {
    /// drone-v1 decides the key. libkeyfinder still measures the tuning and
    /// follows the key over time, and decides the key when drone-v1 finds
    /// none. The default when both are built in.
    Auto,
    /// drone-v1 alone: the cheapest, with no key over time.
    Drone,
    /// libkeyfinder alone, with tuning correction, as Euphoria analyzed keys
    /// before drone-v1.
    KeyFinder,
}

impl KeyEngine {
    /// Every engine, in the order help text lists them.
    pub const ALL: [KeyEngine; 3] = [KeyEngine::Auto, KeyEngine::Drone, KeyEngine::KeyFinder];

    /// The name a command line takes and the analysis cache records.
    pub fn as_str(self) -> &'static str {
        match self {
            KeyEngine::Auto => "auto",
            KeyEngine::Drone => "drone",
            KeyEngine::KeyFinder => "keyfinder",
        }
    }

    /// Whether this build has the detectors the engine needs.
    pub fn is_available(self) -> bool {
        match self {
            KeyEngine::Auto => true,
            KeyEngine::Drone => cfg!(feature = "drone"),
            KeyEngine::KeyFinder => cfg!(feature = "libkeyfinder"),
        }
    }

    fn code(self) -> u8 {
        match self {
            KeyEngine::Auto => 0,
            KeyEngine::Drone => 1,
            KeyEngine::KeyFinder => 2,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => KeyEngine::Drone,
            2 => KeyEngine::KeyFinder,
            _ => KeyEngine::Auto,
        }
    }
}

impl Display for KeyEngine {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for KeyEngine {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        KeyEngine::ALL
            .into_iter()
            .find(|engine| engine.as_str() == text)
            .ok_or_else(|| format!("unknown key engine {text:?}: use auto, drone or keyfinder"))
    }
}

static SELECTED_ENGINE: AtomicU8 = AtomicU8::new(0);

/// Chooses the key engine for every analysis this process makes from now
/// on. Set it before opening the analysis cache, which keeps the analyses of
/// each engine apart. Fails for an engine this build does not have.
pub fn set_key_engine(engine: KeyEngine) -> Result<(), EuphoriaError> {
    if !engine.is_available() {
        return Err(EuphoriaError::Detection {
            detector: "key engine",
            reason: format!("this build of Euphoria has no {engine} key engine"),
            source: None,
        });
    }
    SELECTED_ENGINE.store(engine.code(), Ordering::Relaxed);
    Ok(())
}

/// The key engine analyses use: [`KeyEngine::Auto`] unless
/// [`set_key_engine`] chose another.
pub fn key_engine() -> KeyEngine {
    KeyEngine::from_code(SELECTED_ENGINE.load(Ordering::Relaxed))
}

/// The detector that found a key, which sets how its confidence is rated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySource {
    /// libkeyfinder, whose score margin runs from 0 to about 0.1.
    KeyFinder,
    /// drone-v1, whose margin is the difference of two probabilities.
    Drone,
    /// stratum-dsp, in builds without libkeyfinder. It gives no margin.
    Stratum,
}

impl KeySource {
    /// The name the analysis cache stores.
    pub fn as_str(self) -> &'static str {
        match self {
            KeySource::KeyFinder => "libkeyfinder",
            KeySource::Drone => "drone-v1",
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
    fn engines_read_back_from_their_names() {
        for engine in KeyEngine::ALL {
            assert_eq!(engine.as_str().parse::<KeyEngine>(), Ok(engine));
            assert_eq!(KeyEngine::from_code(engine.code()), engine);
        }
        assert!("libkeyfinder".parse::<KeyEngine>().is_err());
    }

    #[test]
    fn sources_read_back_from_their_stored_names() {
        for source in [KeySource::KeyFinder, KeySource::Drone, KeySource::Stratum] {
            assert_eq!(KeySource::from_stored(source.as_str()), Some(source));
        }
        assert_eq!(KeySource::from_stored("other"), None);
    }

    #[test]
    fn the_default_engine_is_auto() {
        assert_eq!(key_engine(), KeyEngine::Auto);
    }
}
