#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>
#include <stdexcept>
#include <vector>

namespace daw {

struct NoteEvent {
    uint64_t frame = 0;
    uint8_t note = 60;
    uint8_t velocity = 100;
    bool on = true;
    uint64_t id = 0; // 0 = legacy pitch matching; nonzero = independent note occurrence.
};

struct ProcessContext {
    uint64_t startFrame = 0;
    uint32_t frames = 0;
    double sampleRate = 48000.0;
};

struct AudioBlock {
    std::vector<float> left;
    std::vector<float> right;

    void allocate(size_t maxFrames);
    void clear(size_t frames) noexcept;
};

class AudioNode {
public:
    virtual ~AudioNode() = default;
    virtual void prepare(double sampleRate, uint32_t maxFrames) = 0;
    virtual void reset() noexcept = 0;
    virtual void process(const ProcessContext& ctx,
                         std::span<const AudioBlock* const> inputs,
                         AudioBlock& output) noexcept = 0;
};

struct BlockNoteEvent {
    uint32_t offset = 0;
    uint8_t note = 60;
    uint8_t velocity = 100;
    bool on = true;
    uint64_t id = 0;
};

class InstrumentNode : public AudioNode {
public:
    virtual void cancelLive() noexcept {} // audio owner: cancel previews without resetting the score
    // The owner keeps this span alive until process() returns; sorted by offset.
    // Set every block. No allocation or ownership transfer.
    void setBlockEvents(std::span<const BlockNoteEvent> events) noexcept { blockEvents_ = events; }
protected:
    std::span<const BlockNoteEvent> blockEvents_{};
};

enum class Waveform : uint32_t { Sine, Saw, Square, Triangle };
struct SynthConfig {
    Waveform waveform = Waveform::Sine;
    float shape = 1, pulseWidth = 0.5f;
    float attack = 0.006f, decay = 0.06f, sustain = 0.75f, release = 0.06f;
    bool valid() const noexcept;
};
// Immutable, shared only within a prepared session. Construction and FFTs are control-side.
class OscillatorBank {
public:
    static constexpr size_t TableSize = 8192;
    explicit OscillatorBank(double sampleRate);
    static std::shared_ptr<const OscillatorBank> shared(double sampleRate); // control-side cache, never audio
    double sampleRate() const noexcept { return sampleRate_; }
    float sample(Waveform waveform, uint8_t note, double phase, float pulseWidth) const noexcept;
private:
    struct Tables { std::array<float, TableSize + 1> saw{}, triangle{}; };
    std::vector<Tables> notes_;
    double sampleRate_;
    static float read(const std::array<float, TableSize + 1>& table, double phase) noexcept;
};

class SynthNode final : public InstrumentNode {
public:
    static constexpr size_t MaxVoices = 24;

    // Legacy demo scheduling only. EngineService uses block events instead.
    // Setup-only operations: do not call from audio thread.
    void addEvent(NoteEvent event);
    void clearEvents();
    void setADSR(float attackSeconds, float decaySeconds,
                 float sustainLevel, float releaseSeconds);
    void configure(const SynthConfig& config); // setup only; validates
    void setBank(std::shared_ptr<const OscillatorBank> bank) { bank_ = std::move(bank); } // setup only
    void cancelLive() noexcept override;
    void applyConfig(const SynthConfig& config) noexcept; // AUDIO OWNER ONLY, block boundary
    void prepare(double sampleRate, uint32_t maxFrames) override;
    void reset() noexcept override;
    void process(const ProcessContext& ctx, std::span<const AudioBlock* const> inputs,
                 AudioBlock& output) noexcept override;

private:
    enum class Phase : uint8_t { Off, Attack, Decay, Sustain, Release };
    struct Voice {
        uint8_t note = 0;
        uint64_t id = 0;
        float level = 0.0f;
        float velocityGain = 0.0f;
        double phase = 0.0;
        double phaseIncrement = 0.0;
        uint64_t age = 0;
        uint32_t envelopeSamples = 0;
        Phase state = Phase::Off;
        uint32_t attack = 0, decay = 0, release = 0;
        float sustain = 0;

    };
    std::shared_ptr<const OscillatorBank> bank_;
    SynthConfig config_{Waveform::Sine, 1, 0.5f, 0.01f, 0.05f, 0.7f, 0.1f}; // legacy standalone defaults
    std::array<float, 4> weights_{1, 0, 0, 0}, weightSteps_{};
    float shape_ = 1, pulseWidth_ = 0.5f, shapeStep_ = 0, pulseStep_ = 0;
    uint32_t rampSamples_ = 480, remaining_ = 0;
    std::array<Voice, MaxVoices> voices_{};
    std::vector<NoteEvent> events_;
    size_t nextEvent_ = 0;
    double sampleRate_ = 48000.0;
    uint64_t voiceCounter_ = 0;
    uint32_t attackSamples_ = 480;
    uint32_t decaySamples_ = 2400;
    uint32_t releaseSamples_ = 4800;
    float sustain_ = 0.7f;

    void noteOn(uint8_t note, uint8_t velocity, uint64_t id) noexcept;
    void noteOff(uint8_t note, uint64_t id) noexcept;
    float sampleVoice(Voice& voice) noexcept;
};

class GainPanNode final : public AudioNode {
public:
    GainPanNode(float gain = 1.0f, float pan = 0.0f);
    void setGain(float value) noexcept;
    void setPan(float value) noexcept;
    void prepare(double sampleRate, uint32_t maxFrames) override;
    void reset() noexcept override;
    void process(const ProcessContext& ctx, std::span<const AudioBlock* const> inputs,
                 AudioBlock& output) noexcept override;
private:
    std::atomic<float> targetGain_;
    std::atomic<float> targetPan_;
    float currentGain_ = 1.0f;
    float currentPan_ = 0.0f;
    float smoothing_ = 0.0f;
};

// Target changes happen only on the audio consumer at a block boundary.
// This gate is separate from the nominal track fader and reaches exact silence.
class AudibilityNode final : public AudioNode {
public:
    explicit AudibilityNode(bool audible) : current_(audible ? 1.0f : 0.0f), target_(current_) {}
    void setAudible(bool audible) noexcept;
    void prepare(double sampleRate, uint32_t) override;
    void reset() noexcept override;
    void process(const ProcessContext&, std::span<const AudioBlock* const>, AudioBlock&) noexcept override;
private:
    float current_, target_, step_ = 0;
    uint32_t rampFrames_ = 240, remaining_ = 0;
};

class MixerNode final : public AudioNode {
public:
    void prepare(double sampleRate, uint32_t maxFrames) override;
    void reset() noexcept override;
    void process(const ProcessContext& ctx, std::span<const AudioBlock* const> inputs,
                 AudioBlock& output) noexcept override;
};

// Graph compilation (topological sort, allocations) happens off the audio thread.
// Once prepared, do NOT mutate node structure until audio is stopped.
class Graph {
public:
    size_t add(std::unique_ptr<AudioNode> node, std::vector<size_t> inputs = {});
    void setMaster(size_t nodeIndex);
    void prepare(double sampleRate, uint32_t maxFrames);
    void reset() noexcept;
    const AudioBlock& process(uint64_t startFrame, uint32_t frames) noexcept;
    uint32_t maxFrames() const noexcept { return maxFrames_; }
    double sampleRate() const noexcept { return sampleRate_; }
    bool prepared() const noexcept { return prepared_; }

private:
    struct Entry {
        std::unique_ptr<AudioNode> node;
        std::vector<size_t> inputs;
        std::vector<const AudioBlock*> inputPointers;
        AudioBlock output;
    };
    std::vector<std::unique_ptr<Entry>> entries_;
    std::vector<size_t> renderOrder_;
    size_t master_ = 0;
    uint32_t maxFrames_ = 0;
    double sampleRate_ = 48000.0;
    bool prepared_ = false;
};

// Example program: two simultaneous MIDI tracks routed via independent gain/pan
// processors into a master bus. Pointers stay valid while DemoGraph owns the nodes.
struct DemoGraph {
    Graph graph;
    GainPanNode* leftGain = nullptr;
    GainPanNode* rightGain = nullptr;
};
DemoGraph makeDemo(double sampleRate, uint32_t maxFrames);

void writeWavPcm16(const char* path, const std::vector<float>& interleaved,
                   uint32_t sampleRate, uint16_t channels = 2);
std::vector<float> renderOffline(Graph& graph, uint64_t frames, uint32_t chunk);

} // namespace daw
