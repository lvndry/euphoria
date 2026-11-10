// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include <cstring>
#include <exception>

#include <keyfinder/audiodata.h>
#include <keyfinder/keyfinder.h>

namespace {

constexpr int kLibraryException = -1;
constexpr int kUnknownException = -2;

void copy_message(const char* message, char* buffer, unsigned long capacity) {
    if (buffer == nullptr || capacity == 0) {
        return;
    }
    std::strncpy(buffer, message, capacity - 1);
    buffer[capacity - 1] = '\0';
}

}  // namespace

// Returns a libkeyfinder key code (0 to 24) on success. On failure returns a
// negative code and writes the exception message into error_message.
extern "C" int euphoria_keyfinder_detect(const float* samples,
                                         unsigned long sample_count,
                                         unsigned int frame_rate,
                                         char* error_message,
                                         unsigned long error_capacity) {
    try {
        KeyFinder::AudioData audio;
        audio.setFrameRate(frame_rate);
        audio.setChannels(1);
        audio.addToSampleCount(static_cast<unsigned int>(sample_count));
        for (unsigned long index = 0; index < sample_count; ++index) {
            audio.setSample(static_cast<unsigned int>(index), samples[index]);
        }
        KeyFinder::KeyFinder finder;
        return static_cast<int>(finder.keyOfAudio(audio));
    } catch (const std::exception& exception) {
        copy_message(exception.what(), error_message, error_capacity);
        return kLibraryException;
    } catch (...) {
        copy_message("unknown exception", error_message, error_capacity);
        return kUnknownException;
    }
}
