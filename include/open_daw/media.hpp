#pragma once
#include "open_daw/engine.hpp"
#include <filesystem>
#include <fstream>

namespace daw {
struct SampleData {
    uint32_t sampleRate = 48000;
    std::vector<float> stereo; // Immutable after loading, shared across voices.
    size_t frames() const noexcept { return stereo.size() / 2; }
};
// Short-sample importer, not a streaming decoder. PCM16/24/32 and IEEE float32 WAV.
std::shared_ptr<const SampleData> loadWav(const std::filesystem::path& path,
                                        uint64_t maximumFrames = 48000 * 60);
struct Peak { float low = 0, high = 0; };
std::vector<Peak> waveformPeaks(const SampleData& sample, size_t framesPerPeak);

class SamplerNode final : public InstrumentNode {
public:
    SamplerNode(std::shared_ptr<const SampleData> data, uint8_t rootNote, bool oneShot);
    void prepare(double sampleRate, uint32_t maxFrames) override;
    void reset() noexcept override;
    void process(const ProcessContext&, std::span<const AudioBlock* const>, AudioBlock&) noexcept override;
private:
    struct Voice {
        double position = 0, increment = 1;
        float gain = 0;
        uint64_t id = 0, age = 0;
        uint32_t releaseLeft = 0;
        uint8_t note = 0;
        bool active = false, releasing = false;
    };
    std::shared_ptr<const SampleData> data_;
    std::array<Voice, 24> voices_{};
    double sampleRate_ = 48000;
    uint64_t age_ = 0;
    uint32_t releaseFrames_ = 240;
    uint8_t root_ = 60;
    bool oneShot_ = false;
    void event(const BlockNoteEvent&) noexcept;
};

// Bounded-memory PCM16 writer. Explicit finish() reports I/O failures.
class WavStreamWriter {
public:
    WavStreamWriter(const std::filesystem::path& path, uint32_t rate, uint64_t totalFrames);
    void write(std::span<const float> stereo);
    void finish();
private:
    std::ofstream out_;
    uint64_t total_ = 0, written_ = 0;
    bool finished_ = false;
};
}
