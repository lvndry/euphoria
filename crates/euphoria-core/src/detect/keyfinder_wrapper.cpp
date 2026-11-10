#include <keyfinder/audiodata.h>
#include <keyfinder/keyfinder.h>

extern "C" int euphoria_keyfinder_detect(const float* samples,
                                         unsigned long sample_count,
                                         unsigned int frame_rate) {
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
    } catch (...) {
        return -1;
    }
}
