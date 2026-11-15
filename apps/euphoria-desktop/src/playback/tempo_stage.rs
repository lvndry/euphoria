// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tempo of a playing track: the decoded sound is played faster or
//! slower than it was recorded.
//!
//! With key lock on, the sound goes through the Signalsmith Stretch time
//! stretcher the decks use, so the pitch stays where it was. With key lock
//! off it is resampled, so the pitch follows the speed as on a turntable.
//! At exactly 100% the stage does nothing and holds no sound back.

use euphoria_core::shift::Stretch;

use super::PlaybackProblem;

/// How far from 1.0 a rate may be and still count as 100%.
const NEUTRAL_RATE_TOLERANCE: f64 = 1e-6;

/// The slowest and fastest rates a track source accepts.
const MINIMUM_RATE: f64 = 0.5;
const MAXIMUM_RATE: f64 = 2.0;

/// How fast a track plays and whether its pitch is kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlaybackTempo {
    /// Seconds of the track played per second heard: 1.0 is as recorded.
    pub(crate) rate: f64,
    /// Keep the pitch while the speed changes.
    pub(crate) key_lock: bool,
}

impl Default for PlaybackTempo {
    fn default() -> Self {
        Self {
            rate: 1.0,
            key_lock: true,
        }
    }
}

impl PlaybackTempo {
    /// The same tempo with the rate kept inside what a source can play.
    pub(crate) fn limited(self) -> Self {
        let rate = if self.rate.is_finite() {
            self.rate.clamp(MINIMUM_RATE, MAXIMUM_RATE)
        } else {
            1.0
        };
        Self { rate, ..self }
    }

    /// True when the track plays as recorded.
    pub(crate) fn is_neutral(self) -> bool {
        (self.rate - 1.0).abs() < NEUTRAL_RATE_TOLERANCE
    }
}

/// Which way the stage is changing the speed, for tests and the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TempoMode {
    /// The sound passes untouched.
    Bypass,
    /// Time-stretched, pitch kept.
    Stretching,
    /// Resampled, pitch follows.
    Resampling,
}

struct StretchLayer {
    stretch: Stretch,
    preroll: Vec<Vec<f32>>,
}

struct Stretching {
    layers: Vec<StretchLayer>,
    started: bool,
    to_drop: usize,
    input_latency: usize,
    output_latency: usize,
    heard_in: u64,
    made_out: u64,
    total_in: u64,
}

struct Resampling {
    position: f64,
    last_frame: Vec<f32>,
}

enum Engine {
    Bypass,
    Stretching(Stretching),
    Resampling(Resampling),
}

pub(super) struct TempoStage {
    channels: usize,
    layer_count: usize,
    sample_rate: u32,
    tempo: PlaybackTempo,
    engine: Engine,
}

impl TempoStage {
    pub(super) fn new(
        channels: usize,
        layer_count: usize,
        sample_rate: u32,
        tempo: PlaybackTempo,
    ) -> Result<Self, PlaybackProblem> {
        let mut stage = Self {
            channels,
            layer_count,
            sample_rate,
            tempo: tempo.limited(),
            engine: Engine::Bypass,
        };
        stage.rebuild()?;
        Ok(stage)
    }

    pub(super) fn tempo(&self) -> PlaybackTempo {
        self.tempo
    }

    pub(super) fn mode(&self) -> TempoMode {
        match self.engine {
            Engine::Bypass => TempoMode::Bypass,
            Engine::Stretching(_) => TempoMode::Stretching,
            Engine::Resampling(_) => TempoMode::Resampling,
        }
    }

    /// Track seconds per second heard, 1.0 while the sound passes untouched.
    pub(super) fn rate(&self) -> f64 {
        match self.engine {
            Engine::Bypass => 1.0,
            _ => self.tempo.rate,
        }
    }

    /// Forgets what is held, as after a seek, and follows the tempo.
    pub(super) fn rebuild(&mut self) -> Result<(), PlaybackProblem> {
        self.engine = if self.tempo.is_neutral() {
            Engine::Bypass
        } else if self.tempo.key_lock {
            let layers = (0..self.layer_count)
                .map(|_| {
                    Stretch::new(self.channels, self.sample_rate)
                        .map(|stretch| StretchLayer {
                            stretch,
                            preroll: vec![Vec::new(); self.channels],
                        })
                        .map_err(|_| PlaybackProblem::NotDecodable)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let input_latency = layers[0].stretch.input_latency();
            let output_latency = layers[0].stretch.output_latency();
            Engine::Stretching(Stretching {
                layers,
                started: false,
                to_drop: output_latency,
                input_latency,
                output_latency,
                heard_in: 0,
                made_out: 0,
                total_in: 0,
            })
        } else {
            Engine::Resampling(Resampling {
                position: 0.0,
                last_frame: vec![0.0; self.channels * self.layer_count],
            })
        };
        Ok(())
    }

    pub(super) fn apply(
        &mut self,
        samples: Vec<f32>,
        tempo: PlaybackTempo,
    ) -> Result<Vec<f32>, PlaybackProblem> {
        let tempo = tempo.limited();
        if tempo != self.tempo {
            self.tempo = tempo;
            self.rebuild()?;
        }
        let stride = self.channels * self.layer_count;
        let rate = self.tempo.rate;
        match &mut self.engine {
            Engine::Bypass => Ok(samples),
            Engine::Resampling(state) => Ok(state.resample(&samples, stride, rate)),
            Engine::Stretching(state) => {
                state.stretch(&samples, self.channels, self.layer_count, rate)
            }
        }
    }

    /// The sound still held, once the track has ended.
    pub(super) fn finish(&mut self) -> Result<Vec<f32>, PlaybackProblem> {
        let rate = self.tempo.rate;
        let channels = self.channels;
        let layer_count = self.layer_count;
        match &mut self.engine {
            Engine::Bypass | Engine::Resampling(_) => Ok(Vec::new()),
            Engine::Stretching(state) => state.finish(channels, layer_count, rate),
        }
    }
}

impl Resampling {
    fn resample(&mut self, samples: &[f32], stride: usize, rate: f64) -> Vec<f32> {
        let frames = samples.len() / stride;
        let mut output = Vec::with_capacity((samples.len() as f64 / rate) as usize + stride);
        while frames > 0 && self.position < (frames - 1) as f64 {
            let floor = self.position.floor();
            let fraction = (self.position - floor) as f32;
            for channel in 0..stride {
                let before = if floor < 0.0 {
                    self.last_frame[channel]
                } else {
                    samples[floor as usize * stride + channel]
                };
                let after = samples[(floor as i64 + 1) as usize * stride + channel];
                output.push(before + (after - before) * fraction);
            }
            self.position += rate;
        }
        if frames > 0 {
            self.last_frame
                .copy_from_slice(&samples[(frames - 1) * stride..frames * stride]);
            self.position -= frames as f64;
        }
        output
    }
}

fn layer_channels(
    samples: &[f32],
    channels: usize,
    layer_count: usize,
    layer: usize,
) -> Vec<Vec<f32>> {
    let stride = channels * layer_count;
    (0..channels)
        .map(|channel| {
            samples
                .chunks_exact(stride)
                .map(|frame| frame[layer * channels + channel])
                .collect()
        })
        .collect()
}

fn join_layer_channels(layers: &[Vec<Vec<f32>>], channels: usize, frames: usize) -> Vec<f32> {
    let mut joined = Vec::with_capacity(frames * channels * layers.len());
    for frame in 0..frames {
        for layer in layers {
            for channel in layer {
                joined.push(channel[frame]);
            }
        }
    }
    joined
}

impl Stretching {
    fn stretch(
        &mut self,
        samples: &[f32],
        channels: usize,
        layer_count: usize,
        rate: f64,
    ) -> Result<Vec<f32>, PlaybackProblem> {
        let mut inputs: Vec<Vec<Vec<f32>>> = (0..layer_count)
            .map(|layer| layer_channels(samples, channels, layer_count, layer))
            .collect();
        self.total_in += inputs[0][0].len() as u64;
        if !self.started {
            let wanted = self
                .input_latency
                .saturating_sub(self.layers[0].preroll[0].len());
            let taken = wanted.min(inputs[0][0].len());
            for (layer, input) in self.layers.iter_mut().zip(&mut inputs) {
                for (preroll, channel) in layer.preroll.iter_mut().zip(input.iter_mut()) {
                    preroll.extend(channel.drain(..taken));
                }
            }
            if self.layers[0].preroll[0].len() < self.input_latency {
                return Ok(Vec::new());
            }
            self.start(rate)?;
        }
        let frames = inputs[0][0].len();
        self.run(&inputs, frames, rate)
    }

    fn start(&mut self, rate: f64) -> Result<(), PlaybackProblem> {
        for layer in &mut self.layers {
            layer
                .stretch
                .seek(&layer.preroll, rate)
                .map_err(|_| PlaybackProblem::NotDecodable)?;
        }
        self.started = true;
        Ok(())
    }

    fn run(
        &mut self,
        inputs: &[Vec<Vec<f32>>],
        frames: usize,
        rate: f64,
    ) -> Result<Vec<f32>, PlaybackProblem> {
        if frames == 0 {
            return Ok(Vec::new());
        }
        self.heard_in += frames as u64;
        let wanted_total = (self.heard_in as f64 / rate).round() as u64;
        let made = (wanted_total.saturating_sub(self.made_out) as usize).max(1);
        self.made_out += made as u64;
        let channels = inputs[0].len();
        let mut outputs = Vec::with_capacity(self.layers.len());
        for (layer, input) in self.layers.iter_mut().zip(inputs) {
            let mut output = vec![vec![0.0; made]; channels];
            layer
                .stretch
                .process(input, &mut output)
                .map_err(|_| PlaybackProblem::NotDecodable)?;
            outputs.push(output);
        }
        let dropped = self.to_drop.min(made);
        self.to_drop -= dropped;
        for layer in &mut outputs {
            for channel in layer {
                channel.drain(..dropped);
            }
        }
        Ok(join_layer_channels(&outputs, channels, made - dropped))
    }

    fn finish(
        &mut self,
        channels: usize,
        layer_count: usize,
        rate: f64,
    ) -> Result<Vec<f32>, PlaybackProblem> {
        if self.total_in == 0 {
            return Ok(Vec::new());
        }
        let short = !self.started;
        if short {
            for layer in &mut self.layers {
                for channel in &mut layer.preroll {
                    channel.resize(self.input_latency, 0.0);
                }
            }
            self.start(rate)?;
        }
        let silence = vec![vec![vec![0.0; self.input_latency]; channels]; layer_count];
        let mut output = self.run(&silence, self.input_latency, rate)?;
        let mut flushed = Vec::with_capacity(self.layers.len());
        for layer in &mut self.layers {
            let mut tail = vec![vec![0.0; self.output_latency]; channels];
            layer
                .stretch
                .flush(&mut tail)
                .map_err(|_| PlaybackProblem::NotDecodable)?;
            flushed.push(tail);
        }
        output.extend(join_layer_channels(&flushed, channels, self.output_latency));
        if short {
            let keep = (self.total_in as f64 / rate).round() as usize;
            output.truncate(keep * channels * layer_count);
        }
        self.total_in = 0;
        Ok(output)
    }
}
