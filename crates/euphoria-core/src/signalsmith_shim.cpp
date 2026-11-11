// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// A C interface to Signalsmith Stretch for src/signalsmith.rs. Every function
// checks its pointers and lengths and returns a status instead of letting a
// C++ exception reach Rust.

#include <new>

#include "signalsmith-stretch/signalsmith-stretch.h"

namespace {

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

// The seed of the random phase the stretcher starts from, fixed so that the
// same input always gives the same output.
constexpr long SEED = 0x5eed;

}  // namespace

extern "C" {

struct euphoria_stretch {
    Stretch stretch;
    int channels;

    euphoria_stretch(int channels) : stretch(SEED), channels(channels) {}
};

enum {
    EUPHORIA_STRETCH_OK = 0,
    EUPHORIA_STRETCH_INVALID = -1,
    EUPHORIA_STRETCH_FAILED = -2,
};

euphoria_stretch *euphoria_stretch_new(int channels, float sample_rate) noexcept {
    if (channels <= 0 || !(sample_rate > 0)) return nullptr;
    try {
        auto *stretch = new euphoria_stretch(channels);
        stretch->stretch.presetDefault(channels, sample_rate);
        return stretch;
    } catch (...) {
        return nullptr;
    }
}

void euphoria_stretch_free(euphoria_stretch *stretch) noexcept {
    delete stretch;
}

int euphoria_stretch_input_latency(const euphoria_stretch *stretch) noexcept {
    if (stretch == nullptr) return EUPHORIA_STRETCH_INVALID;
    return stretch->stretch.inputLatency();
}

int euphoria_stretch_output_latency(const euphoria_stretch *stretch) noexcept {
    if (stretch == nullptr) return EUPHORIA_STRETCH_INVALID;
    return stretch->stretch.outputLatency();
}

int euphoria_stretch_set_transpose_semitones(euphoria_stretch *stretch, float semitones) noexcept {
    if (stretch == nullptr) return EUPHORIA_STRETCH_INVALID;
    try {
        stretch->stretch.setTransposeSemitones(semitones);
        return EUPHORIA_STRETCH_OK;
    } catch (...) {
        return EUPHORIA_STRETCH_FAILED;
    }
}

static bool channels_present(const float *const *channels, int count) {
    if (channels == nullptr) return false;
    for (int channel = 0; channel < count; ++channel) {
        if (channels[channel] == nullptr) return false;
    }
    return true;
}

int euphoria_stretch_seek(
    euphoria_stretch *stretch,
    const float *const *inputs,
    int channels,
    int input_samples,
    double playback_rate) noexcept {
    if (stretch == nullptr || channels != stretch->channels || input_samples < 0) {
        return EUPHORIA_STRETCH_INVALID;
    }
    if (input_samples > 0 && !channels_present(inputs, channels)) return EUPHORIA_STRETCH_INVALID;
    try {
        stretch->stretch.seek(inputs, input_samples, playback_rate);
        return EUPHORIA_STRETCH_OK;
    } catch (...) {
        return EUPHORIA_STRETCH_FAILED;
    }
}

int euphoria_stretch_process(
    euphoria_stretch *stretch,
    const float *const *inputs,
    int input_samples,
    float *const *outputs,
    int output_samples,
    int channels) noexcept {
    if (stretch == nullptr || channels != stretch->channels || input_samples < 0
        || output_samples < 0) {
        return EUPHORIA_STRETCH_INVALID;
    }
    if (input_samples > 0 && !channels_present(inputs, channels)) return EUPHORIA_STRETCH_INVALID;
    if (output_samples > 0 && !channels_present(outputs, channels)) {
        return EUPHORIA_STRETCH_INVALID;
    }
    try {
        stretch->stretch.process(inputs, input_samples, outputs, output_samples);
        return EUPHORIA_STRETCH_OK;
    } catch (...) {
        return EUPHORIA_STRETCH_FAILED;
    }
}

int euphoria_stretch_flush(
    euphoria_stretch *stretch,
    float *const *outputs,
    int output_samples,
    int channels) noexcept {
    if (stretch == nullptr || channels != stretch->channels || output_samples < 0) {
        return EUPHORIA_STRETCH_INVALID;
    }
    if (output_samples > 0 && !channels_present(outputs, channels)) {
        return EUPHORIA_STRETCH_INVALID;
    }
    try {
        stretch->stretch.flush(outputs, output_samples);
        return EUPHORIA_STRETCH_OK;
    } catch (...) {
        return EUPHORIA_STRETCH_FAILED;
    }
}

}  // extern "C"
