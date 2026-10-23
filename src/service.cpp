#include "open_daw/service.hpp"
#include <algorithm>
#include <cassert>
#include <cmath>
#include <limits>
#include <unordered_set>

namespace daw {
static_assert(std::atomic<uint64_t>::is_always_lock_free,
              "This prototype requires lock-free 64-bit atomics (64-bit desktop targets)");
static_assert(std::atomic<float>::is_always_lock_free);
static_assert(std::atomic<uint32_t>::is_always_lock_free);

EngineService::EngineService(const SessionSpec& spec)
    : sampleRate_(spec.sampleRate), maxFrames_(spec.maxFrames),
      trackCount_(static_cast<uint32_t>(spec.tracks.size())), length_(spec.lengthFrames) {
    if (!std::isfinite(sampleRate_) || sampleRate_ < 8000 || sampleRate_ > 384000 ||
        maxFrames_ == 0 || maxFrames_ > MaxBlockFrames || spec.tracks.empty() ||
        spec.tracks.size() > MaxTracks || length_ == 0 ||
        length_ > static_cast<uint64_t>(sampleRate_ * 60 * 60 * 24) || spec.notes.size() > 100000) {
        throw std::invalid_argument("Invalid session limits: 1-16 tracks, 1-4096 block frames, <=24 hours, <=100000 notes");
    }
    std::unordered_set<uint64_t> ids;
    ids.reserve(spec.notes.size());
    schedule_.reserve(spec.notes.size() * 2);
    for (const auto& n : spec.notes) {
        if (!n.id || (n.id & LiveIdBit) || !ids.insert(n.id).second ||
            n.track >= trackCount_ || n.pitch > 127 || n.velocity == 0 || n.velocity > 127 ||
            n.start >= n.end || n.end > length_) {
            throw std::invalid_argument("Invalid note range, track, MIDI value or duplicate/reserved ID");
        }
        schedule_.push_back({n.start, n.track, {0, n.pitch, n.velocity, true, n.id}});
        schedule_.push_back({n.end, n.track, {0, n.pitch, 0, false, n.id}});
    }
    std::sort(schedule_.begin(), schedule_.end(), [](const Scheduled& a, const Scheduled& b) {
        if (a.frame != b.frame) return a.frame < b.frame;
        if (a.event.on != b.event.on) return !a.event.on; // off before on at a boundary
        if (a.track != b.track) return a.track < b.track;
        return a.event.id < b.event.id;
    });
    // Reject over-dense scores BEFORE they reach the callback. No truncation of note-offs.
    for (uint32_t track = 0; track < trackCount_; ++track) {
        std::vector<uint64_t> frames;
        for (const auto& e : schedule_) if (e.track == track) frames.push_back(e.frame);
        size_t begin = 0;
        for (size_t end = 0; end < frames.size(); ++end) {
            while (frames[end] - frames[begin] >= maxFrames_) ++begin;
            if (end - begin + 1 > ScheduledEventsPerTrack)
                throw std::invalid_argument("Event density exceeds 256 scheduled events per track per block");
        }
    }
    std::vector<size_t> inputs;
    inputs.reserve(trackCount_);
    const bool anySolo = std::any_of(spec.tracks.begin(), spec.tracks.end(), [](const auto& t) { return t.solo; });
    const auto bank = OscillatorBank::shared(sampleRate_);
    for (uint32_t i = 0; i < trackCount_; ++i) {
        const auto& settings = spec.tracks[i];
        std::unique_ptr<InstrumentNode> instrument;
        if (settings.sample) {
            instrument = std::make_unique<SamplerNode>(settings.sample, settings.rootNote, settings.oneShot);
        } else {
            auto synth = std::make_unique<SynthNode>();
            synth->setBank(bank); synth->configure(settings.synth); synths_[i] = synth.get();
            instrument = std::move(synth);
        }
        instruments_[i] = instrument.get();
        auto source = graph_.add(std::move(instrument));
        auto gain = std::make_unique<GainPanNode>(settings.gain, settings.pan);
        gains_[i] = gain.get();
        const auto mixed = graph_.add(std::move(gain), {source});
        auto gate = std::make_unique<AudibilityNode>(!settings.mute && (!anySolo || settings.solo));
        gates_[i] = gate.get();
        inputs.push_back(graph_.add(std::move(gate), {mixed}));
    }
    graph_.setMaster(graph_.add(std::make_unique<MixerNode>(), std::move(inputs)));
    graph_.prepare(sampleRate_, maxFrames_);
}

bool EngineService::submit(Command c) noexcept {
    bool valid = true;
    switch (c.type) {
        case CommandType::Play: case CommandType::Pause: break;
        case CommandType::Seek: valid = c.a <= length_; break;
        case CommandType::Loop:
            valid = !c.enabled || (c.a < c.b && c.b <= length_ && c.b - c.a >= maxFrames_);
            break;
        case CommandType::NoteOn: case CommandType::NoteOff:
            valid = c.track < trackCount_ && c.pitch <= 127 && c.velocity <= 127 &&
                    c.id != LiveIdBit && (c.id & LiveIdBit) &&
                    (c.type != CommandType::NoteOn || c.velocity != 0);
            break;
        default: valid = false; break;
    }
    if (valid) {
        c.serial = ++serial_;
        if (commands_.push(c)) return true;
    }
    meter_.rejected.fetch_add(1, std::memory_order_relaxed);
    // An undeliverable release must not leave a voice sounding indefinitely.
    if (c.type == CommandType::NoteOff) panic();
    return false;
}
void EngineService::stop() noexcept { emergency_.fetch_or(2, std::memory_order_release); }
void EngineService::panic() noexcept { emergency_.fetch_or(1, std::memory_order_release); }
bool EngineService::setGain(uint32_t track, float gain) noexcept {
    if (track >= trackCount_ || !std::isfinite(gain)) return false;
    gains_[track]->setGain(gain); return true;
}
bool EngineService::setPan(uint32_t track, float pan) noexcept {
    if (track >= trackCount_ || !std::isfinite(pan)) return false;
    gains_[track]->setPan(pan); return true;
}

bool EngineService::submitMix(MixSnapshot snapshot, uint64_t& serial) noexcept {
    for (uint32_t i = 0; i < trackCount_; ++i) {
        const auto& t = snapshot.tracks[i];
        if (!std::isfinite(t.gain) || !std::isfinite(t.pan) || t.gain < 0 || t.gain > 2 || t.pan < -1 || t.pan > 1 ||
            (snapshot.updateSynths && !snapshot.synths[i].valid())) {
            meter_.rejected.fetch_add(1, std::memory_order_relaxed); return false;
        }
    }
    if (mixSerial_ == std::numeric_limits<uint64_t>::max()) {
        meter_.rejected.fetch_add(1, std::memory_order_relaxed); return false;
    }
    snapshot.serial = mixSerial_ + 1;
    if (!mixQueue_.push(snapshot)) { meter_.rejected.fetch_add(1, std::memory_order_relaxed); return false; }
    serial = ++mixSerial_; return true;
}
void EngineService::consumeMix() noexcept {
    MixSnapshot next, latest; bool changed = false, synthChanged = false;
    std::array<SynthConfig, MaxTracks> synths{};
    // Bounded even if the producer continues filling the queue.
    for (size_t n = 0; n < 8 && mixQueue_.pop(next); ++n) {
        latest = next; changed = true;
        if (next.updateSynths) { synths = next.synths; synthChanged = true; }
    }
    if (!changed) return;
    bool anySolo = false;
    for (uint32_t i = 0; i < trackCount_; ++i) anySolo |= latest.tracks[i].solo;
    for (uint32_t i = 0; i < trackCount_; ++i) {
        const auto& t = latest.tracks[i];
        gains_[i]->setGain(t.gain); gains_[i]->setPan(t.pan);
        gates_[i]->setAudible(!t.mute && (!anySolo || t.solo));
        if (synthChanged && synths_[i]) synths_[i]->applyConfig(synths[i]);
    }
    mixApplied_.store(latest.serial, std::memory_order_release);
}

void EngineService::discontinuity(uint64_t frame) noexcept {
    position_ = frame;
    graph_.reset(); // O(prepared nodes + fixed voices), never reconstructs objects.
    counts_.fill(0);
    cursor_ = static_cast<size_t>(std::lower_bound(schedule_.begin(), schedule_.end(), frame,
        [](const Scheduled& e, uint64_t f) { return e.frame < f; }) - schedule_.begin());
}
void EngineService::consumeCommands() noexcept {
    counts_.fill(0);
    const auto emergency = emergency_.exchange(0, std::memory_order_acquire);
    if (emergency) {
        commands_.discard();
        if (emergency & 2) { playing_ = false; discontinuity(0); }
        else discontinuity(position_);
        return;
    }
    Command c;
    for (size_t i = 0; i < CommandsPerBlock && commands_.pop(c); ++i) {
        applied_ = c.serial;
        switch (c.type) {
            case CommandType::Play:
                if (position_ >= length_) discontinuity(0);
                if (looping_ && (position_ < loopBegin_ || position_ >= loopEnd_)) discontinuity(loopBegin_);
                playing_ = true;
                break;
            case CommandType::Pause: playing_ = false; discontinuity(position_); break;
            case CommandType::Seek:
                discontinuity(c.a);
                if (looping_ && (position_ < loopBegin_ || position_ >= loopEnd_)) looping_ = false;
                break;
            case CommandType::Loop:
                looping_ = c.enabled;
                if (looping_) {
                    loopBegin_ = c.a; loopEnd_ = c.b;
                    if (position_ < loopBegin_ || position_ >= loopEnd_) discontinuity(loopBegin_);
                }
                break;
            case CommandType::NoteOn: case CommandType::NoteOff:
                assert(counts_[c.track] < BlockEventCapacity);
                events_[c.track][counts_[c.track]++] = {0, c.pitch, c.velocity, c.type == CommandType::NoteOn, c.id};
                break;
        }
    }
}

void EngineService::block(float* stereo, uint32_t frames) noexcept {
    consumeMix(); // Stop/panic cancels note commands, never the desired mixer state.
    consumeCommands();
    uint32_t done = 0;
    float peakL = 0, peakR = 0;
    while (done < frames) {
        if (playing_ && looping_ && position_ >= loopEnd_) discontinuity(loopBegin_);
        if (playing_ && !looping_ && position_ >= length_) {
            playing_ = false;
            discontinuity(length_);
        }
        uint32_t n = frames - done;
        if (playing_) {
            const auto boundary = looping_ ? loopEnd_ : length_;
            n = static_cast<uint32_t>(std::min<uint64_t>(n, boundary - position_));
            const uint64_t end = position_ + n;
            while (cursor_ < schedule_.size() && schedule_[cursor_].frame < end) {
                const auto& e = schedule_[cursor_++];
                assert(e.frame >= position_ && counts_[e.track] < BlockEventCapacity);
                auto event = e.event;
                event.offset = static_cast<uint32_t>(e.frame - position_);
                events_[e.track][counts_[e.track]++] = event;
            }
        }
        for (uint32_t track = 0; track < trackCount_; ++track)
            instruments_[track]->setBlockEvents({events_[track].data(), counts_[track]});
        const auto& audio = graph_.process(position_, n);
        for (uint32_t i = 0; i < n; ++i) {
            const float l = std::isfinite(audio.left[i]) ? audio.left[i] : 0;
            const float r = std::isfinite(audio.right[i]) ? audio.right[i] : 0;
            peakL = std::max(peakL, std::abs(l)); peakR = std::max(peakR, std::abs(r));
            clipped_ += static_cast<uint64_t>(std::abs(l) > 1) + static_cast<uint64_t>(std::abs(r) > 1);
            stereo[2 * (done + i)] = std::clamp(l, -1.0f, 1.0f);
            stereo[2 * (done + i) + 1] = std::clamp(r, -1.0f, 1.0f);
        }
        if (playing_) position_ += n;
        done += n;
        counts_.fill(0); // live events apply only to this block's first segment
    }
    // Reaching an exact endpoint also updates transport state without waiting for another callback.
    if (playing_ && !looping_ && position_ == length_) { playing_ = false; discontinuity(length_); }
    if (playing_ && looping_ && position_ == loopEnd_) discontinuity(loopBegin_);
    rendered_ += frames; ++blocks_;
    meter_.position.store(position_, std::memory_order_relaxed);
    meter_.rendered.store(rendered_, std::memory_order_relaxed);
    meter_.blocks.store(blocks_, std::memory_order_relaxed);
    meter_.applied.store(applied_, std::memory_order_relaxed);
    meter_.clipped.store(clipped_, std::memory_order_relaxed);
    meter_.peakL.store(peakL, std::memory_order_relaxed);
    meter_.peakR.store(peakR, std::memory_order_relaxed);
    meter_.playing.store(playing_, std::memory_order_relaxed);
    meter_.looping.store(looping_, std::memory_order_relaxed);
}
void EngineService::render(float* stereo, uint32_t frames) noexcept {
    assert(stereo || frames == 0);
    for (uint32_t done = 0; done < frames;) {
        const auto n = std::min(maxFrames_, frames - done);
        block(stereo + size_t{2} * done, n);
        done += n;
    }
}
Telemetry EngineService::telemetry() const noexcept {
    Telemetry t;
    t.position = meter_.position.load(std::memory_order_relaxed);
    t.renderedFrames = meter_.rendered.load(std::memory_order_relaxed);
    t.blocks = meter_.blocks.load(std::memory_order_relaxed);
    t.lastApplied = meter_.applied.load(std::memory_order_relaxed);
    t.rejectedCommands = meter_.rejected.load(std::memory_order_relaxed);
    t.clippedSamples = meter_.clipped.load(std::memory_order_relaxed);
    t.peakLeft = meter_.peakL.load(std::memory_order_relaxed);
    t.peakRight = meter_.peakR.load(std::memory_order_relaxed);
    t.playing = meter_.playing.load(std::memory_order_relaxed);
    t.looping = meter_.looping.load(std::memory_order_relaxed);
    return t;
}
void renderSessionToWav(const SessionSpec& spec, const std::filesystem::path& path, uint64_t frames) {
    // Independent instance: the caller's live DSP state is never reset or shared.
    EngineService engine(spec);
    WavStreamWriter writer(path, static_cast<uint32_t>(spec.sampleRate), frames);
    std::vector<float> scratch(size_t{spec.maxFrames} * 2);
    engine.submit({CommandType::Play});
    for (uint64_t done = 0; done < frames;) {
        const auto n = static_cast<uint32_t>(std::min<uint64_t>(spec.maxFrames, frames - done));
        engine.render(scratch.data(), n);
        writer.write({scratch.data(), size_t{n} * 2});
        done += n;
    }
    writer.finish();
}
}
