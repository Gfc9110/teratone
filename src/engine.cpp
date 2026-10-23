#include "open_daw/engine.hpp"

#include <algorithm>
#include <cassert>
#include <cmath>
#include <functional>
#include <complex>
#include <mutex>
#include <limits>

namespace daw {
namespace {
constexpr double Tau = 6.283185307179586476925286766559;
constexpr float PiOver4 = 0.7853981633974483096f;
}

bool SynthConfig::valid() const noexcept {
    return static_cast<uint32_t>(waveform) <= 3 && std::isfinite(shape) && shape >= 0 && shape <= 1 &&
        std::isfinite(pulseWidth) && pulseWidth >= 0.05f && pulseWidth <= 0.95f &&
        std::isfinite(attack) && attack >= 0 && attack <= 10 && std::isfinite(decay) && decay >= 0 && decay <= 10 &&
        std::isfinite(sustain) && sustain >= 0 && sustain <= 1 && std::isfinite(release) && release >= 0 && release <= 20;
}
namespace {
// Radix-2 inverse FFT, used exclusively during prepare. No external DSP code or assets.
void inverseFFT(std::vector<std::complex<double>>& a) {
    const size_t n = a.size();
    for (size_t i = 1, j = 0; i < n; ++i) {
        size_t bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit; if (i < j) std::swap(a[i], a[j]);
    }
    for (size_t length = 2; length <= n; length <<= 1) {
        const std::complex<double> step = std::polar(1.0, Tau / length);
        for (size_t i = 0; i < n; i += length) {
            std::complex<double> w = 1;
            for (size_t j = 0; j < length / 2; ++j) {
                const auto u = a[i + j], v = a[i + j + length / 2] * w;
                a[i + j] = u + v; a[i + j + length / 2] = u - v; w *= step;
            }
        }
    }
}
}
std::shared_ptr<const OscillatorBank> OscillatorBank::shared(double rate) {
    // Bounded weak cache shares only immutable tables between live/export/prepared graphs.
    // The mutex/FFT and all shared_ptr ownership changes stay outside rendering.
    struct Entry { double rate = 0; std::weak_ptr<const OscillatorBank> bank; };
    static std::array<Entry, 4> cache;
    static size_t next = 0;
    static std::mutex mutex;
    const std::lock_guard lock(mutex);
    for (auto& entry : cache) if (entry.rate == rate) if (auto bank = entry.bank.lock()) return bank;
    auto bank = std::make_shared<OscillatorBank>(rate);
    cache[next] = {rate, bank}; next = (next + 1) % cache.size(); return bank;
}
OscillatorBank::OscillatorBank(double rate) : notes_(128), sampleRate_(rate) {
    if (!std::isfinite(rate) || rate < 8000 || rate > 384000) throw std::invalid_argument("Invalid oscillator sample rate");
    std::vector<std::complex<double>> bins(TableSize);
    const double pi = Tau / 2;
    for (size_t note = 0; note < 128; ++note) {
        const double f = 440 * std::pow(2.0, (static_cast<int>(note) - 69) / 12.0);
        const size_t harmonics = std::min<size_t>(2047, static_cast<size_t>(std::max(0.0, std::ceil(rate / (2 * f)) - 1)));
        for (int triangle = 0; triangle < 2; ++triangle) {
            std::fill(bins.begin(), bins.end(), std::complex<double>{});
            for (size_t h = 1; h <= harmonics; ++h) {
                double imaginary = 1.0 / (pi * h); // rising saw: -2/pi * sin(h*phase)/h
                if (triangle) imaginary = h % 2 ? -4.0 / (pi * pi * h * h) * (h % 4 == 1 ? 1 : -1) : 0;
                bins[h] = {0, imaginary}; bins[TableSize - h] = std::conj(bins[h]);
            }
            inverseFFT(bins); // coefficients already normalized; omit 1/N
            auto& table = triangle ? notes_[note].triangle : notes_[note].saw;
            for (size_t i = 0; i < TableSize; ++i) table[i] = static_cast<float>(bins[i].real());
            table[TableSize] = table[0];
        }
    }
}
float OscillatorBank::read(const std::array<float, TableSize + 1>& table, double phase) noexcept {
    if (phase < 0) phase += 1;
    if (phase >= 1) phase -= 1;
    const double position = phase * TableSize;
    const auto i = static_cast<size_t>(position);
    return table[i] + (table[i + 1] - table[i]) * static_cast<float>(position - i);
}
float OscillatorBank::sample(Waveform waveform, uint8_t note, double phase, float duty) const noexcept {
    const auto& t = notes_[note];
    switch (waveform) {
        case Waveform::Sine: return static_cast<float>(std::sin(Tau * phase));
        case Waveform::Saw: return read(t.saw, phase + 0.5); // fundamental aligned with sine morph
        case Waveform::Square: return (read(t.saw, phase - duty) - read(t.saw, phase)) / (2 * std::max(duty, 1 - duty));
        case Waveform::Triangle: return read(t.triangle, phase);
    }
    return 0;
}
void SynthNode::configure(const SynthConfig& config) {
    if (!config.valid()) throw std::invalid_argument("Invalid synth configuration");
    config_ = config; setADSR(config.attack, config.decay, config.sustain, config.release);
}
void SynthNode::applyConfig(const SynthConfig& config) noexcept {
    // Already validated by control-side contracts. ADSR changes affect future note-ons only.
    const bool changed = config.waveform != config_.waveform || config.shape != config_.shape || config.pulseWidth != config_.pulseWidth;
    config_ = config;
    attackSamples_ = static_cast<uint32_t>(std::round(config.attack * sampleRate_));
    decaySamples_ = static_cast<uint32_t>(std::round(config.decay * sampleRate_));
    releaseSamples_ = static_cast<uint32_t>(std::round(config.release * sampleRate_)); sustain_ = config.sustain;
    if (!changed) return;
    remaining_ = rampSamples_;
    for (size_t w = 0; w < 4; ++w) weightSteps_[w] = ((w == static_cast<size_t>(config.waveform) ? 1.0f : 0.0f) - weights_[w]) / remaining_;
    shapeStep_ = (config.shape - shape_) / remaining_; pulseStep_ = (config.pulseWidth - pulseWidth_) / remaining_;
}

void AudioBlock::allocate(size_t maxFrames) {
    left.assign(maxFrames, 0.0f);
    right.assign(maxFrames, 0.0f);
}

void AudioBlock::clear(size_t frames) noexcept {
    std::fill_n(left.data(), frames, 0.0f);
    std::fill_n(right.data(), frames, 0.0f);
}

void SynthNode::addEvent(NoteEvent event) {
    if (event.note > 127 || event.velocity > 127) throw std::invalid_argument("Invalid note event");
    events_.push_back(event);
    std::stable_sort(events_.begin(), events_.end(), [](const auto& a, const auto& b) {
        return a.frame < b.frame;
    });
}

void SynthNode::clearEvents() {
    events_.clear();
    nextEvent_ = 0;
}

void SynthNode::setADSR(float attack, float decay, float sustain, float release) {
    if (!std::isfinite(attack) || !std::isfinite(decay) ||
        !std::isfinite(sustain) || !std::isfinite(release)) {
        throw std::invalid_argument("ADSR parameters must be finite");
    }
    if (attack < 0 || decay < 0 || release < 0 || sustain < 0 || sustain > 1) {
        throw std::invalid_argument("Invalid ADSR parameters");
    }
    const auto toSamples = [this](float seconds) {
        return static_cast<uint32_t>(std::min(
            static_cast<double>(std::numeric_limits<uint32_t>::max() - 1),
            std::round(static_cast<double>(seconds) * sampleRate_)));
    };
    config_.attack = attack; config_.decay = decay; config_.sustain = sustain; config_.release = release;
    attackSamples_ = toSamples(attack);
    decaySamples_ = toSamples(decay);
    releaseSamples_ = toSamples(release);
    sustain_ = sustain;
}

void SynthNode::prepare(double sampleRate, uint32_t) {
    if (sampleRate < 8000 || sampleRate > 384000 || !std::isfinite(sampleRate)) {
        throw std::invalid_argument("Invalid sample rate");
    }
    if (!bank_ || bank_->sampleRate() != sampleRate) bank_ = OscillatorBank::shared(sampleRate);
    rampSamples_ = std::max(1u, static_cast<uint32_t>(std::round(sampleRate * 0.010)));
    // Retain seconds, not counts quantized at a previous sample rate. Prepare and
    // runtime targets must agree even for sub-sample/short envelopes at 44.1/96kHz.
    sampleRate_ = sampleRate;
    setADSR(config_.attack, config_.decay, config_.sustain, config_.release);
    reset();
}

void SynthNode::reset() noexcept {
    weights_.fill(0); weights_[static_cast<size_t>(config_.waveform)] = 1;
    shape_ = config_.shape; pulseWidth_ = config_.pulseWidth; remaining_ = 0;
    voices_.fill(Voice{});
    blockEvents_ = {};
    nextEvent_ = 0;
    voiceCounter_ = 0;
}

void SynthNode::noteOn(uint8_t note, uint8_t velocity, uint64_t id) noexcept {
    if (velocity == 0) { noteOff(note, id); return; }
    auto* destination = static_cast<Voice*>(nullptr);
    for (auto& voice : voices_) {
        if (voice.state == Phase::Off) { destination = &voice; break; }
    }
    if (!destination) {
        // Stable voice stealing: prefer lowest envelope; then oldest voice.
        destination = &voices_[0];
        for (auto& voice : voices_) {
            if (voice.level < destination->level ||
                (voice.level == destination->level && voice.age < destination->age)) {
                destination = &voice;
            }
        }
    }
    const double frequency = 440.0 * std::pow(2.0, (static_cast<int>(note) - 69) / 12.0);
    if (frequency >= sampleRate_ * 0.5) return; // explicit silence above Nyquist

    *destination = Voice{};
    destination->note = note;
    destination->id = id;
    destination->level = velocity / 127.0f; // Envelope also applied during synthesis.
    destination->velocityGain = destination->level;
    destination->phaseIncrement = frequency / sampleRate_;
    destination->age = ++voiceCounter_;
    destination->level = 0.0f;
    destination->attack = attackSamples_; destination->decay = decaySamples_; destination->release = releaseSamples_; destination->sustain = sustain_;
    destination->state = attackSamples_ > 0 ? Phase::Attack : (decaySamples_ > 0 ? Phase::Decay : Phase::Sustain);
    if (attackSamples_ == 0) destination->level = 1.0f;
    if (attackSamples_ == 0 && decaySamples_ == 0) destination->level = sustain_;
    // Keep velocity gain independent of ADSR envelope level.
    destination->velocityGain = velocity / 127.0f;
}

void SynthNode::noteOff(uint8_t note, uint64_t id) noexcept {
    // IDs isolate overlapping same-pitch notes. ID 0 retains legacy pitch semantics.
    for (auto& voice : voices_) {
        if ((id ? voice.id == id : voice.note == note) && voice.state != Phase::Off && voice.state != Phase::Release) {
            voice.state = Phase::Release;
            voice.envelopeSamples = 0;
        }
    }
}

float SynthNode::sampleVoice(Voice& voice) noexcept {
    if (voice.state == Phase::Off) return 0.0f;
    const auto velocity = voice.velocityGain;
    switch (voice.state) {
        case Phase::Attack:
            if (voice.attack == 0 || ++voice.envelopeSamples >= voice.attack) {
                voice.level = 1.0f;
                voice.envelopeSamples = 0;
                voice.state = voice.decay ? Phase::Decay : Phase::Sustain;
            } else {
                voice.level = static_cast<float>(voice.envelopeSamples) / voice.attack;
            }
            break;
        case Phase::Decay:
            if (voice.decay == 0 || ++voice.envelopeSamples >= voice.decay) {
                voice.level = voice.sustain;
                voice.envelopeSamples = 0;
                voice.state = Phase::Sustain;
            } else {
                voice.level = 1.0f - (1.0f - voice.sustain) *
                    (static_cast<float>(voice.envelopeSamples) / voice.decay);
            }
            break;
        case Phase::Sustain: voice.level = voice.sustain; break;
        case Phase::Release:
            if (voice.release == 0 || ++voice.envelopeSamples >= voice.release) {
                voice.level = 0.0f;
                voice.state = Phase::Off;
            } else {
                // Exponential-like attenuation for smooth releases starting at any stage.
                const float remain = 1.0f - 1.0f / (voice.release - voice.envelopeSamples + 1.0f);
                voice.level *= remain;
            }
            break;
        case Phase::Off: break;
    }
    const float sine = static_cast<float>(std::sin(Tau * voice.phase));
    float oscillator = sine;
    if (weights_[0] != 1) {
        oscillator = weights_[0] * sine;
        for (size_t w = 1; w < 4; ++w) if (weights_[w] != 0)
            oscillator += weights_[w] * (sine * (1 - shape_) + shape_ * bank_->sample(static_cast<Waveform>(w), voice.note, voice.phase, pulseWidth_));
    }
    const float value = oscillator * voice.level * velocity;
    voice.phase += voice.phaseIncrement;
    if (voice.phase >= 1.0) voice.phase -= std::floor(voice.phase);
    return value;
}

void SynthNode::process(const ProcessContext& ctx, std::span<const AudioBlock* const>,
                        AudioBlock& output) noexcept {
    size_t blockEvent = 0;
    for (uint32_t i = 0; i < ctx.frames; ++i) {
        if (remaining_) {
            for (size_t w = 0; w < 4; ++w) weights_[w] += weightSteps_[w];
            shape_ += shapeStep_; pulseWidth_ += pulseStep_;
            if (--remaining_ == 0) { weights_.fill(0); weights_[static_cast<size_t>(config_.waveform)] = 1; shape_ = config_.shape; pulseWidth_ = config_.pulseWidth; }
        }
        while (blockEvent < blockEvents_.size() && blockEvents_[blockEvent].offset <= i) {
            const auto& e = blockEvents_[blockEvent++];
            if (e.on && e.velocity) noteOn(e.note, e.velocity, e.id);
            else noteOff(e.note, e.id);
        }
        const uint64_t frame = ctx.startFrame + i;
        while (nextEvent_ < events_.size() && events_[nextEvent_].frame <= frame) {
            const auto& event = events_[nextEvent_++];
            if (event.on && event.velocity) noteOn(event.note, event.velocity, event.id);
            else noteOff(event.note, event.id);
        }
        float sample = 0.0f;
        for (auto& voice : voices_) sample += 0.20f * sampleVoice(voice);
        output.left[i] = sample;
        output.right[i] = sample;
    }
    blockEvents_ = {};
}

GainPanNode::GainPanNode(float gain, float pan)
    : targetGain_(std::clamp(gain, 0.0f, 2.0f)),
      targetPan_(std::clamp(pan, -1.0f, 1.0f)),
      currentGain_(std::clamp(gain, 0.0f, 2.0f)),
      currentPan_(std::clamp(pan, -1.0f, 1.0f)) {
    if (!std::isfinite(gain) || !std::isfinite(pan)) {
        throw std::invalid_argument("gain/pan not finite");
    }
}

void GainPanNode::setGain(float value) noexcept {
    if (std::isfinite(value)) targetGain_.store(std::clamp(value, 0.0f, 2.0f), std::memory_order_relaxed);
}
void GainPanNode::setPan(float value) noexcept {
    if (std::isfinite(value)) targetPan_.store(std::clamp(value, -1.0f, 1.0f), std::memory_order_relaxed);
}
void GainPanNode::prepare(double sampleRate, uint32_t) {
    smoothing_ = 1.0f - std::exp(-1.0f / static_cast<float>(sampleRate * 0.005));
    reset();
}
void GainPanNode::reset() noexcept {
    currentGain_ = targetGain_.load(std::memory_order_relaxed);
    currentPan_ = targetPan_.load(std::memory_order_relaxed);
}
void GainPanNode::process(const ProcessContext& ctx, std::span<const AudioBlock* const> inputs,
                          AudioBlock& output) noexcept {
    if (inputs.empty()) return;
    const auto* input = inputs[0];
    const auto gain = targetGain_.load(std::memory_order_relaxed);
    const auto pan = targetPan_.load(std::memory_order_relaxed);
    for (uint32_t i = 0; i < ctx.frames; ++i) {
        currentGain_ += smoothing_ * (gain - currentGain_);
        currentPan_ += smoothing_ * (pan - currentPan_);
        const float angle = (currentPan_ + 1.0f) * PiOver4;
        // Equal-power center (-3dB per channel).
        output.left[i] = input->left[i] * currentGain_ * std::cos(angle);
        output.right[i] = input->right[i] * currentGain_ * std::sin(angle);
    }
}

void AudibilityNode::prepare(double rate, uint32_t) {
    rampFrames_ = std::max(1u, static_cast<uint32_t>(std::round(rate * 0.005)));
    reset();
}
void AudibilityNode::reset() noexcept { current_ = target_; remaining_ = 0; step_ = 0; }
void AudibilityNode::setAudible(bool audible) noexcept {
    const float next = audible ? 1.0f : 0.0f;
    if (next != target_) { target_ = next; remaining_ = rampFrames_; step_ = (target_ - current_) / remaining_; }
}
void AudibilityNode::process(const ProcessContext& ctx, std::span<const AudioBlock* const> inputs, AudioBlock& output) noexcept {
    if (inputs.empty()) return;
    for (uint32_t i = 0; i < ctx.frames; ++i) {
        if (remaining_) { current_ += step_; if (--remaining_ == 0) current_ = target_; }
        output.left[i] = inputs[0]->left[i] * current_;
        output.right[i] = inputs[0]->right[i] * current_;
    }
}

void MixerNode::prepare(double, uint32_t) {}
void MixerNode::reset() noexcept {}
void MixerNode::process(const ProcessContext& ctx,
                        std::span<const AudioBlock* const> inputs, AudioBlock& output) noexcept {
    for (auto* input : inputs) {
        for (uint32_t i = 0; i < ctx.frames; ++i) {
            output.left[i] += input->left[i];
            output.right[i] += input->right[i];
        }
    }
}

size_t Graph::add(std::unique_ptr<AudioNode> node, std::vector<size_t> inputs) {
    if (prepared_) throw std::logic_error("Cannot mutate a prepared graph");
    if (!node) throw std::invalid_argument("AudioNode cannot be null");
    auto entry = std::make_unique<Entry>();
    entry->node = std::move(node);
    entry->inputs = std::move(inputs);
    entries_.push_back(std::move(entry));
    return entries_.size() - 1;
}
void Graph::setMaster(size_t nodeIndex) {
    if (prepared_) throw std::logic_error("Cannot change master on prepared graph");
    master_ = nodeIndex;
}

void Graph::prepare(double sampleRate, uint32_t maxFrames) {
    if (entries_.empty() || master_ >= entries_.size() || maxFrames == 0 ||
        sampleRate < 8000 || sampleRate > 384000 || !std::isfinite(sampleRate)) {
        throw std::invalid_argument("Invalid graph configuration");
    }
    prepared_ = false;
    const size_t count = entries_.size();
    std::vector<uint8_t> marks(count, 0);
    std::vector<size_t> sorted;
    std::function<void(size_t)> visit = [&](size_t index) {
        if (index >= count) throw std::invalid_argument("Invalid graph connection");
        if (marks[index] == 1) throw std::invalid_argument("Cycle in audio graph");
        if (marks[index] == 2) return;
        marks[index] = 1;
        for (const auto input : entries_[index]->inputs) visit(input);
        marks[index] = 2;
        sorted.push_back(index);
    };
    // Validate all nodes, not just those contributing to the master.
    for (size_t i = 0; i < count; ++i) visit(i);
    renderOrder_ = std::move(sorted);
    maxFrames_ = maxFrames;
    sampleRate_ = sampleRate;
    for (auto& entry : entries_) {
        entry->output.allocate(maxFrames);
        entry->node->prepare(sampleRate, maxFrames);
    }
    for (auto& entry : entries_) {
        entry->inputPointers.clear();
        entry->inputPointers.reserve(entry->inputs.size());
        for (const auto input : entry->inputs) {
            entry->inputPointers.push_back(&entries_[input]->output);
        }
    }
    prepared_ = true;
}

void Graph::reset() noexcept {
    for (auto& entry : entries_) entry->node->reset();
}

const AudioBlock& Graph::process(uint64_t startFrame, uint32_t frames) noexcept {
    // Caller contract: graph prepared; frames <= maxFrames; realtime thread only.
    assert(prepared_ && frames <= maxFrames_);
    const ProcessContext ctx{startFrame, frames, sampleRate_};
    for (auto index : renderOrder_) {
        auto& entry = *entries_[index];
        entry.output.clear(frames);
        entry.node->process(ctx, entry.inputPointers, entry.output);
    }
    return entries_[master_]->output;
}

std::vector<float> renderOffline(Graph& graph, uint64_t frames, uint32_t chunk) {
    if (!graph.prepared() || chunk == 0 || chunk > graph.maxFrames()) {
        throw std::invalid_argument("Graph not prepared, or invalid chunk");
    }
    if (frames > std::numeric_limits<size_t>::max() / 2) {
        throw std::length_error("Audio too large");
    }
    std::vector<float> interleaved(static_cast<size_t>(frames) * 2);
    graph.reset();
    for (uint64_t frame = 0; frame < frames; ) {
        const auto n = static_cast<uint32_t>(std::min<uint64_t>(chunk, frames - frame));
        const auto& output = graph.process(frame, n);
        for (uint32_t i = 0; i < n; ++i) {
            const auto offset = (static_cast<size_t>(frame) + i) * 2;
            interleaved[offset] = output.left[i];
            interleaved[offset + 1] = output.right[i];
        }
        frame += n;
    }
    return interleaved;
}

DemoGraph makeDemo(double sampleRate, uint32_t maxFrames) {
    DemoGraph demo;
    auto addSynthTrack = [&](std::vector<NoteEvent> notes, float gain, float pan,
                             GainPanNode*& gainPtr) {
        auto synth = std::make_unique<SynthNode>();
        synth->setADSR(0.008f, 0.10f, 0.50f, 0.15f);
        for (auto n : notes) synth->addEvent(n);
        auto synthIndex = demo.graph.add(std::move(synth));
        auto gainNode = std::make_unique<GainPanNode>(gain, pan);
        gainPtr = gainNode.get();
        return demo.graph.add(std::move(gainNode), {synthIndex});
    };
    auto f = [sampleRate](double seconds) -> uint64_t {
        return static_cast<uint64_t>(std::llround(seconds * sampleRate));
    };
    const auto trackA = addSynthTrack({
        {f(0.00), 60, 105, true}, {f(0.45), 60, 0, false},
        {f(0.50), 64, 95, true},  {f(0.95), 64, 0, false},
        {f(1.00), 67, 90, true},  {f(1.45), 67, 0, false},
        {f(1.50), 72, 110, true}, {f(2.15), 72, 0, false}
    }, 0.95f, -0.50f, demo.leftGain);
    const auto trackB = addSynthTrack({
        {f(0.00), 48, 100, true}, {f(1.00), 48, 0, false},
        {f(1.00), 55, 100, true}, {f(2.00), 55, 0, false},
        {f(2.50), 60, 100, true}, {f(3.50), 60, 0, false}
    }, 0.75f, 0.50f, demo.rightGain);
    demo.graph.setMaster(demo.graph.add(std::make_unique<MixerNode>(), {trackA, trackB}));
    demo.graph.prepare(sampleRate, maxFrames);
    return demo;
}

} // namespace daw
