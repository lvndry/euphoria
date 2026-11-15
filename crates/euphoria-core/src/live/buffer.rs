// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::NonZeroU32;

use crate::audio::AudioBuffer;

/// The most recent audio of a stream, mono, kept in memory only. Older audio
/// is overwritten as new audio arrives.
#[derive(Debug, Clone)]
pub struct RollingBuffer {
    samples: Vec<f32>,
    write_position: usize,
    filled: usize,
    sample_rate: NonZeroU32,
}

impl RollingBuffer {
    /// A buffer that holds the last `capacity_seconds` of audio at `sample_rate`.
    pub fn new(sample_rate: NonZeroU32, capacity_seconds: f32) -> Self {
        let capacity = ((capacity_seconds * sample_rate.get() as f32) as usize).max(1);
        Self {
            samples: vec![0.0; capacity],
            write_position: 0,
            filled: 0,
            sample_rate,
        }
    }

    /// Samples per second.
    pub fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    /// How much audio the buffer holds now, in seconds.
    pub fn seconds(&self) -> f32 {
        self.filled as f32 / self.sample_rate.get() as f32
    }

    /// How many samples the buffer holds now.
    pub fn len(&self) -> usize {
        self.filled
    }

    /// True when the buffer holds no audio.
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Appends audio, dropping the oldest when the buffer is full.
    pub fn push(&mut self, samples: &[f32]) {
        let capacity = self.samples.len();
        let samples = if samples.len() > capacity {
            &samples[samples.len() - capacity..]
        } else {
            samples
        };
        let first_part = samples.len().min(capacity - self.write_position);
        self.samples[self.write_position..self.write_position + first_part]
            .copy_from_slice(&samples[..first_part]);
        let rest = samples.len() - first_part;
        self.samples[..rest].copy_from_slice(&samples[first_part..]);
        self.write_position = (self.write_position + samples.len()) % capacity;
        self.filled = (self.filled + samples.len()).min(capacity);
    }

    /// Forgets all audio.
    pub fn clear(&mut self) {
        self.write_position = 0;
        self.filled = 0;
    }

    /// The last `seconds` of audio, or all of it when the buffer holds less.
    pub fn latest(&self, seconds: f32) -> AudioBuffer {
        let wanted = ((seconds * self.sample_rate.get() as f32) as usize).min(self.filled);
        let capacity = self.samples.len();
        let start = (self.write_position + capacity - wanted) % capacity;
        let mut samples = Vec::with_capacity(wanted);
        let first_part = wanted.min(capacity - start);
        samples.extend_from_slice(&self.samples[start..start + first_part]);
        samples.extend_from_slice(&self.samples[..wanted - first_part]);
        AudioBuffer::new(samples, self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> NonZeroU32 {
        NonZeroU32::new(10).unwrap()
    }

    #[test]
    fn it_returns_what_was_pushed_in_order() {
        let mut buffer = RollingBuffer::new(rate(), 2.0);
        buffer.push(&[1.0, 2.0, 3.0]);
        buffer.push(&[4.0]);
        assert_eq!(buffer.latest(10.0).samples(), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(buffer.latest(0.2).samples(), [3.0, 4.0]);
    }

    #[test]
    fn it_keeps_only_the_latest_capacity_when_it_wraps() {
        let mut buffer = RollingBuffer::new(rate(), 1.0);
        let first: Vec<f32> = (0..7).map(|value| value as f32).collect();
        let second: Vec<f32> = (7..13).map(|value| value as f32).collect();
        buffer.push(&first);
        buffer.push(&second);
        assert_eq!(buffer.len(), 10);
        let expected: Vec<f32> = (3..13).map(|value| value as f32).collect();
        assert_eq!(buffer.latest(5.0).samples(), expected.as_slice());
    }

    #[test]
    fn a_push_longer_than_the_buffer_keeps_its_tail() {
        let mut buffer = RollingBuffer::new(rate(), 1.0);
        let long: Vec<f32> = (0..25).map(|value| value as f32).collect();
        buffer.push(&long);
        let expected: Vec<f32> = (15..25).map(|value| value as f32).collect();
        assert_eq!(buffer.latest(1.0).samples(), expected.as_slice());
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut buffer = RollingBuffer::new(rate(), 1.0);
        buffer.push(&[1.0; 8]);
        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.seconds(), 0.0);
        assert!(buffer.latest(1.0).samples().is_empty());
    }
}
