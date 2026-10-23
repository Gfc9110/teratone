#pragma once
#include "open_daw/engine.hpp"
#include "open_daw/media.hpp"
#include "open_daw/drums.hpp"
#include <optional>
#include "open_daw/spsc.hpp"
#include <filesystem>

namespace daw {
inline constexpr uint32_t MaxTracks = 16;
inline constexpr uint32_t MaxBlockFrames = 4096;
inline constexpr size_t ScheduledEventsPerTrack = 256;
inline constexpr size_t CommandsPerBlock = 64;
inline constexpr size_t BlockEventCapacity = ScheduledEventsPerTrack + CommandsPerBlock;
inline constexpr uint64_t LiveIdBit = uint64_t{1} << 63;

struct SessionNote {
    uint64_t id = 1, start = 0, end = 1;
    uint32_t track = 0;
    uint8_t pitch = 60, velocity = 100;
};
struct TrackSettings {
    float gain = 0.8f, pan = 0;
    std::shared_ptr<const SampleData> sample; // null -> independent oscillator synth
    uint8_t rootNote = 60;
    bool oneShot = false;
    bool mute = false, solo = false;
    SynthConfig synth;
    std::optional<DrumKit> drums;
};
struct SessionSpec {
    double sampleRate = 48000;
    uint32_t maxFrames = 256;
    uint64_t lengthFrames = 48000 * 8;
    std::vector<TrackSettings> tracks{TrackSettings{}};
    std::vector<SessionNote> notes;
};

struct TrackMix { float gain = 0.8f, pan = 0; bool mute = false, solo = false; };
struct MixSnapshot {
    std::array<TrackMix, MaxTracks> tracks{};
    uint64_t serial = 0;
    std::array<SynthConfig, MaxTracks> synths{};
    std::array<DrumControls, MaxTracks> drums{};
    bool updateDrums = false;
    bool updateSynths = false; // legacy mixer commands preserve the last synth snapshot
};

enum class CommandType : uint32_t { Play, Pause, Seek, Loop, NoteOn, NoteOff };
struct Command {
    CommandType type = CommandType::Play;
    uint32_t track = 0;
    uint64_t a = 0, b = 0, id = 0, serial = 0;
    uint8_t pitch = 60, velocity = 100;
    bool enabled = false;
    uint64_t liveEpoch = 0;
};
struct Telemetry {
    uint64_t position = 0, renderedFrames = 0, blocks = 0, lastApplied = 0;
    uint64_t rejectedCommands = 0, clippedSamples = 0;
    float peakLeft = 0, peakRight = 0;
    bool playing = false, looping = false;
};

// Prepared once, then one control producer + one audio consumer. No graph editing
// while callbacks run. A structural replacement creates a separate service and
// stops/joins the device before swapping/destroying the previous one.
class EngineService {
public:
    explicit EngineService(const SessionSpec& spec);
    EngineService(const EngineService&) = delete;
    EngineService& operator=(const EngineService&) = delete;
    bool submit(Command command) noexcept; // single control producer
    void stop() noexcept;                  // priority, cancels the queued backlog
    bool drumStatus(uint32_t track,DrumStatus& out) const noexcept;
    bool cancelLive() noexcept;            // priority preview-only cancellation, preserves scheduled voices
    void panic() noexcept;                 // priority silence, retains transport
    bool setGain(uint32_t track, float gain) noexcept;
    bool setPan(uint32_t track, float pan) noexcept;
    bool submitMix(MixSnapshot snapshot, uint64_t& serial) noexcept;
    uint64_t mixApplied() const noexcept { return mixApplied_.load(std::memory_order_acquire); }
    uint64_t mixSubmitted() const noexcept { return mixSerial_; } // control thread only
    void render(float* stereo, uint32_t frames) noexcept; // only audio consumer
    Telemetry telemetry() const noexcept; // approximate atomic fields, not transactional
    double sampleRate() const noexcept { return sampleRate_; }
    uint32_t maxFrames() const noexcept { return maxFrames_; }
    uint32_t trackCount() const noexcept { return trackCount_; }
    uint64_t lengthFrames() const noexcept { return length_; }
private:
    struct Scheduled { uint64_t frame; uint32_t track; BlockNoteEvent event; };
    Graph graph_;
    std::array<InstrumentNode*, MaxTracks> instruments_{};
    std::array<SynthNode*, MaxTracks> synths_{};
    std::array<DrumNode*, MaxTracks> drums_{};
    std::array<size_t, MaxTracks> padCounts_{};
    std::array<GainPanNode*, MaxTracks> gains_{};
    std::array<AudibilityNode*, MaxTracks> gates_{};
    SpscQueue<MixSnapshot, 9> mixQueue_; // eight coherent snapshots, independent of note traffic
    uint64_t mixSerial_ = 0;
    std::atomic<uint64_t> mixApplied_{0};
    std::array<std::array<BlockNoteEvent, BlockEventCapacity>, MaxTracks> events_{};
    std::array<size_t, MaxTracks> counts_{};
    std::vector<Scheduled> schedule_;
    SpscQueue<Command, 1025> commands_;
    std::atomic<uint32_t> emergency_{0};
    uint64_t liveProducerEpoch_ = 0, liveAudioEpoch_ = 0;
    std::atomic<uint64_t> liveDesiredEpoch_{0};
    void syncLiveEpoch(uint64_t epoch) noexcept;
    double sampleRate_;
    uint32_t maxFrames_, trackCount_;
    uint64_t length_, position_ = 0, loopBegin_ = 0, loopEnd_ = 0;
    uint64_t rendered_ = 0, blocks_ = 0, applied_ = 0, clipped_ = 0, serial_ = 0;
    size_t cursor_ = 0;
    bool playing_ = false, looping_ = false;
    struct AtomicTelemetry {
        std::atomic<uint64_t> position{0}, rendered{0}, blocks{0}, applied{0}, rejected{0}, clipped{0};
        std::atomic<float> peakL{0}, peakR{0};
        std::atomic<bool> playing{false}, looping{false};
    } meter_;
    void discontinuity(uint64_t frame) noexcept;
    void consumeMix() noexcept;
    void consumeCommands() noexcept;
    void block(float* stereo, uint32_t frames) noexcept;
};
void renderSessionToWav(const SessionSpec& spec, const std::filesystem::path& path, uint64_t frames);
}
