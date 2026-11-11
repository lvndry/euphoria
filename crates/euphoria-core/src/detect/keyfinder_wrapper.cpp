// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include <cmath>
#include <cstring>
#include <exception>
#include <mutex>
#include <vector>

#include <keyfinder/audiodata.h>
#include <keyfinder/constants.h>
#include <keyfinder/keyfinder.h>
#include <keyfinder/toneprofiles.h>
#include <keyfinder/workspace.h>

// From FFTW's threads library. Declared here so the wrapper needs no FFTW
// headers.
extern "C" void fftw_make_planner_thread_safe(void);

namespace {

constexpr int kSuccess = 0;
constexpr int kLibraryException = -1;
constexpr int kUnknownException = -2;

void copy_message(const char* message, char* buffer, unsigned long capacity) {
    if (buffer == nullptr || capacity == 0) {
        return;
    }
    std::strncpy(buffer, message, capacity - 1);
    buffer[capacity - 1] = '\0';
}

// Makes libkeyfinder safe to call from several threads at once. Runs once,
// before any other use of libkeyfinder.
//
// libkeyfinder builds its tone profiles on first use without a lock, so they
// are built here. It also destroys FFTW plans without a lock while other
// threads create theirs, and FFTW allows only the running of plans to happen
// concurrently, so FFTW is told to lock its planner itself.
void prepare_libkeyfinder_once() {
    static std::once_flag prepared;
    std::call_once(prepared, [] {
        fftw_make_planner_thread_safe();
        KeyFinder::toneProfileMajor();
        KeyFinder::toneProfileMinor();
    });
}

}  // namespace

// The chromagram of one track: `hop_count` rows of BANDS magnitudes, one row
// per analysis frame, and how many input samples separate two frames.
struct EuphoriaChromagram {
    std::vector<double> magnitudes;
    unsigned long hop_count;
    unsigned long samples_per_hop;
};

// Computes the chromagram of mono audio. Returns a chromagram to release with
// euphoria_keyfinder_chromagram_free, or null after writing the exception
// message into error_message.
extern "C" EuphoriaChromagram* euphoria_keyfinder_chromagram_new(
    const float* samples,
    unsigned long sample_count,
    unsigned int frame_rate,
    char* error_message,
    unsigned long error_capacity) {
    try {
        prepare_libkeyfinder_once();
        KeyFinder::AudioData audio;
        audio.setFrameRate(frame_rate);
        audio.setChannels(1);
        audio.addToSampleCount(static_cast<unsigned int>(sample_count));
        for (unsigned long index = 0; index < sample_count; ++index) {
            audio.setSample(static_cast<unsigned int>(index), samples[index]);
        }
        KeyFinder::KeyFinder finder;
        KeyFinder::Workspace workspace;
        finder.progressiveChromagram(audio, workspace);
        finder.finalChromagram(workspace);

        auto chromagram = new EuphoriaChromagram();
        const KeyFinder::Chromagram* source = workspace.chromagram;
        chromagram->hop_count = source == nullptr ? 0 : source->getHops();
        chromagram->magnitudes.reserve(chromagram->hop_count * BANDS);
        for (unsigned int hop = 0; hop < chromagram->hop_count; ++hop) {
            for (unsigned int band = 0; band < BANDS; ++band) {
                chromagram->magnitudes.push_back(source->getMagnitude(hop, band));
            }
        }
        // The library analyses the audio at a lower rate, which it records on
        // the buffer it analysed, so the ratio is the factor it lowered by.
        const unsigned int analysed_rate = workspace.preprocessedBuffer.getFrameRate();
        const unsigned long downsample_factor =
            analysed_rate == 0
                ? 1
                : std::lround(static_cast<double>(frame_rate) / analysed_rate);
        chromagram->samples_per_hop = HOPSIZE * downsample_factor;
        return chromagram;
    } catch (const std::exception& exception) {
        copy_message(exception.what(), error_message, error_capacity);
        return nullptr;
    } catch (...) {
        copy_message("unknown exception", error_message, error_capacity);
        return nullptr;
    }
}

extern "C" unsigned long euphoria_keyfinder_chromagram_hop_count(
    const EuphoriaChromagram* chromagram) {
    return chromagram->hop_count;
}

extern "C" unsigned long euphoria_keyfinder_chromagram_samples_per_hop(
    const EuphoriaChromagram* chromagram) {
    return chromagram->samples_per_hop;
}

// Copies hop_count rows of euphoria_keyfinder_band_count() magnitudes.
extern "C" void euphoria_keyfinder_chromagram_copy(const EuphoriaChromagram* chromagram,
                                                   double* magnitudes) {
    std::memcpy(magnitudes, chromagram->magnitudes.data(),
                chromagram->magnitudes.size() * sizeof(double));
}

extern "C" void euphoria_keyfinder_chromagram_free(EuphoriaChromagram* chromagram) {
    delete chromagram;
}

extern "C" unsigned long euphoria_keyfinder_band_count() {
    return BANDS;
}

// Scores vector_count chroma vectors of BANDS magnitudes each against every
// key. Writes KEYS scores per vector, in libkeyfinder key code order: how
// closely the vector matches the profile of that key, from 0 to 1. Returns
// zero on success. On failure returns a negative code and writes the
// exception message into error_message.
extern "C" int euphoria_keyfinder_key_scores(const double* chroma_vectors,
                                             unsigned long vector_count,
                                             double* scores,
                                             char* error_message,
                                             unsigned long error_capacity) {
    try {
        prepare_libkeyfinder_once();
        const KeyFinder::ToneProfile major(KeyFinder::toneProfileMajor());
        const KeyFinder::ToneProfile minor(KeyFinder::toneProfileMinor());
        std::vector<double> chroma_vector(BANDS);
        for (unsigned long vector = 0; vector < vector_count; ++vector) {
            const double* first_band = chroma_vectors + vector * BANDS;
            chroma_vector.assign(first_band, first_band + BANDS);
            double* vector_scores = scores + vector * KEYS;
            for (int tonic = 0; tonic < SEMITONES; ++tonic) {
                vector_scores[tonic * 2] = major.cosineSimilarity(chroma_vector, tonic);
                vector_scores[tonic * 2 + 1] = minor.cosineSimilarity(chroma_vector, tonic);
            }
        }
        return kSuccess;
    } catch (const std::exception& exception) {
        copy_message(exception.what(), error_message, error_capacity);
        return kLibraryException;
    } catch (...) {
        copy_message("unknown exception", error_message, error_capacity);
        return kUnknownException;
    }
}
