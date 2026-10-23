#pragma once
#include "open_daw/media.hpp"
namespace daw {
inline constexpr size_t MaxDrumPads = 16;
struct PadParameters {
    float gain = 1, pan = 0, tune = 0;
    bool mute = false, oneShot = true;
    uint32_t chokeGroup = 0; // 0 = none; 1..16 are local to this instrument
    bool valid() const noexcept;
};
struct DrumPad {
    uint8_t midiNote = 35;
    std::shared_ptr<const SampleData> sample;
    PadParameters parameters;
};
struct DrumControls { std::array<PadParameters, MaxDrumPads> pads{}; };
struct DrumKit { std::vector<DrumPad> pads; };
struct DrumStatus {std::array<uint64_t,MaxDrumPads> triggers{};uint32_t active=0,padCount=0;};
class DrumNode final : public InstrumentNode {
public:
    static constexpr size_t MaxVoices = 64;
    explicit DrumNode(const DrumKit& kit);
    void prepare(double rate, uint32_t) override;
    void reset() noexcept override;
    void cancelLive() noexcept override;
    DrumStatus status() const noexcept;
    void applyParameters(const DrumControls&) noexcept; // audio owner only, validated on control side
    void process(const ProcessContext&, std::span<const AudioBlock* const>, AudioBlock&) noexcept override;
private:
    struct PadState {
        std::shared_ptr<const SampleData> sample; // held for prepared graph lifetime, never moved in rendering
        PadParameters parameters;
        float left = 0, right = 0, targetLeft = 0, targetRight = 0, gate = 1, gateStep = 0;
        uint32_t gateLeft = 0;
    };
    struct Voice {
        double position = 0, increment = 1;
        uint64_t id = 0, age = 0;
        size_t pad = 0;
        uint32_t choke = 0, attack = 0, attackLength = 1, releaseLeft = 0, tailLeft = 0, tailLength = 1, tailMask = 0;
        float velocity = 0, envelope = 0, releaseStart = 0;
        float lastL = 0, lastR = 0, tailL = 0, tailR = 0;
        bool active = false, releasing = false, oneShot = true;
    };
    std::array<PadState, MaxDrumPads> pads_{};
    std::array<int8_t,128> lookup_{};
    std::array<Voice, MaxVoices> voices_{};
    size_t padCount_ = 0;
    double sampleRate_ = 48000;
    uint64_t age_ = 0;
    std::array<std::atomic<uint64_t>,MaxDrumPads> triggers_{};
    std::atomic<uint32_t> active_{0};
    uint32_t attackFrames_ = 48, releaseFrames_ = 240;
    float smoothing_ = 0;
    void event(const BlockNoteEvent&) noexcept;
    void release(Voice&) noexcept;
    float tailGate(uint32_t mask) const noexcept;
    void startTail(Voice&,float left,float right,uint32_t mask,uint32_t frames) noexcept;
};
}
