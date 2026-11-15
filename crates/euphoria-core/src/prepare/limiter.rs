// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A look-ahead true-peak limiter, written to stay out of the way: it only
//! turns the gain down where an inter-sample peak would pass the ceiling,
//! and it turns it back up slowly.

use std::collections::VecDeque;

use crate::quality::peak::{HALF_TAPS, PHASES, TAPS, coefficients};

const LOOKAHEAD_SECONDS: f32 = 0.002;
const RELEASE_SECONDS: f32 = 0.08;
/// Frames after a frame that its inter-sample points depend on.
const INTERPOLATION_REACH: usize = HALF_TAPS;

pub(crate) struct TruePeakLimiter {
    channels: usize,
    ceiling: f32,
    lookahead: usize,
    release_coefficient: f32,
    coefficients: [[f32; TAPS]; PHASES],
    maximum_gain: f32,
    /// Interleaved frames from `base_frame` on, as pushed (virtual frames).
    frames: VecDeque<f32>,
    base_frame: u64,
    total_frames: u64,
    real_frames: u64,
    emitted_real_frames: u64,
    next_peak_frame: u64,
    previous_peak: f32,
    minimum_window: VecDeque<(u64, f32)>,
    next_minimum_frame: u64,
    minimum_ring: VecDeque<f32>,
    minimum_sum: f64,
    next_output_frame: u64,
    released_gain: f32,
    lowest_gain: f32,
}

impl TruePeakLimiter {
    pub(crate) fn new(channels: usize, sample_rate: u32, ceiling_db: f32) -> Self {
        let lookahead = ((sample_rate as f32 * LOOKAHEAD_SECONDS) as usize).max(2);
        let coefficients = coefficients();
        let maximum_gain = coefficients
            .iter()
            .map(|phase| phase.iter().map(|weight| weight.abs()).sum::<f32>())
            .fold(1.0_f32, f32::max);
        let mut limiter = Self {
            channels,
            ceiling: 10.0_f32.powf(ceiling_db / 20.0),
            lookahead,
            release_coefficient: (-1.0 / (RELEASE_SECONDS * sample_rate as f32)).exp(),
            coefficients,
            maximum_gain,
            frames: VecDeque::new(),
            base_frame: 0,
            total_frames: 0,
            real_frames: 0,
            emitted_real_frames: 0,
            next_peak_frame: 0,
            previous_peak: 0.0,
            minimum_window: VecDeque::new(),
            next_minimum_frame: 0,
            minimum_ring: VecDeque::new(),
            minimum_sum: 0.0,
            next_output_frame: 0,
            released_gain: 1.0,
            lowest_gain: 1.0,
        };
        let silence = vec![0.0; (lookahead - 1) * channels];
        limiter.append(&silence, false);
        limiter
    }

    /// The most the gain was ever turned down, in dB, as a positive number.
    pub(crate) fn reduction_db(&self) -> f32 {
        -20.0 * self.lowest_gain.log10()
    }

    pub(crate) fn push(&mut self, interleaved: &[f32]) -> Vec<f32> {
        self.append(interleaved, true);
        self.advance()
    }

    pub(crate) fn finish(&mut self) -> Vec<f32> {
        let silence = vec![0.0; (self.lookahead + INTERPOLATION_REACH + 1) * self.channels];
        self.append(&silence, false);
        self.advance()
    }

    fn append(&mut self, interleaved: &[f32], real: bool) {
        self.frames.extend(interleaved.iter().copied());
        let frames = (interleaved.len() / self.channels) as u64;
        self.total_frames += frames;
        if real {
            self.real_frames += frames;
        }
    }

    fn sample(&self, frame: i64, channel: usize) -> f32 {
        if frame < self.base_frame as i64 || frame >= self.total_frames as i64 {
            return 0.0;
        }
        self.frames[(frame as u64 - self.base_frame) as usize * self.channels + channel]
    }

    /// The highest value, in absolute terms, reached by the signal between
    /// `frame` and the frame after it, and on both.
    fn interval_peak(&self, frame: u64) -> f32 {
        let first = frame as i64 - (HALF_TAPS as i64 - 1);
        let mut peak = 0.0_f32;
        let mut window_peak = 0.0_f32;
        for channel in 0..self.channels {
            for tap in 0..TAPS {
                window_peak = window_peak.max(self.sample(first + tap as i64, channel).abs());
            }
        }
        if window_peak * self.maximum_gain <= self.ceiling {
            return window_peak;
        }
        for channel in 0..self.channels {
            peak = peak.max(self.sample(frame as i64, channel).abs());
            peak = peak.max(self.sample(frame as i64 + 1, channel).abs());
            for phase in &self.coefficients {
                let value: f32 = phase
                    .iter()
                    .enumerate()
                    .map(|(tap, weight)| weight * self.sample(first + tap as i64, channel))
                    .sum();
                peak = peak.max(value.abs());
            }
        }
        peak
    }

    fn advance(&mut self) -> Vec<f32> {
        let mut output = Vec::new();
        while self.next_peak_frame + (INTERPOLATION_REACH as u64) < self.total_frames {
            let frame = self.next_peak_frame;
            let peak = self.interval_peak(frame);
            let needed = peak.max(self.previous_peak);
            self.previous_peak = peak;
            let gain = if needed > self.ceiling {
                self.ceiling / needed
            } else {
                1.0
            };
            while self
                .minimum_window
                .back()
                .is_some_and(|(_, value)| *value >= gain)
            {
                self.minimum_window.pop_back();
            }
            self.minimum_window.push_back((frame, gain));
            self.next_peak_frame += 1;

            while self.next_minimum_frame + (self.lookahead as u64) <= self.next_peak_frame {
                let window_start = self.next_minimum_frame;
                while self
                    .minimum_window
                    .front()
                    .is_some_and(|(index, _)| *index < window_start)
                {
                    self.minimum_window.pop_front();
                }
                let minimum = self.minimum_window.front().map_or(1.0, |(_, value)| *value);
                self.minimum_ring.push_back(minimum);
                self.minimum_sum += f64::from(minimum);
                if self.minimum_ring.len() > self.lookahead {
                    let dropped = self.minimum_ring.pop_front().unwrap_or(1.0);
                    self.minimum_sum -= f64::from(dropped);
                }
                self.next_minimum_frame += 1;
                self.emit(&mut output);
            }
        }
        let keep_from = self
            .next_output_frame
            .min(self.next_peak_frame.saturating_sub(TAPS as u64));
        while self.base_frame < keep_from {
            for _ in 0..self.channels {
                self.frames.pop_front();
            }
            self.base_frame += 1;
        }
        output
    }

    fn emit(&mut self, output: &mut Vec<f32>) {
        let frame = self.next_output_frame;
        self.next_output_frame += 1;
        if frame + 1 < self.lookahead as u64 {
            return;
        }
        let smoothed = (self.minimum_sum / self.lookahead as f64) as f32;
        let recovering =
            self.released_gain * self.release_coefficient + (1.0 - self.release_coefficient);
        self.released_gain = smoothed.min(recovering);
        let is_real =
            frame + 1 >= self.lookahead as u64 && self.emitted_real_frames < self.real_frames;
        if !is_real {
            return;
        }
        self.emitted_real_frames += 1;
        self.lowest_gain = self.lowest_gain.min(self.released_gain);
        for channel in 0..self.channels {
            output.push(self.sample(frame as i64, channel) * self.released_gain);
        }
    }
}
