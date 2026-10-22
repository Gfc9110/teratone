#include "open_daw/engine.hpp"
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#define CHECK(condition) do { if (!(condition)) { throw std::runtime_error( \
    std::string(__FILE__) + ":" + std::to_string(__LINE__) + ": failed: " #condition); } } while (0)

namespace {
class Constant final : public daw::AudioNode {
public:
    explicit Constant(float value) : value_(value) {}
    void prepare(double, uint32_t) override {}
    void reset() noexcept override {}
    void process(const daw::ProcessContext& ctx, std::span<const daw::AudioBlock* const>,
                 daw::AudioBlock& output) noexcept override {
        for (uint32_t i = 0; i < ctx.frames; ++i) {
            output.left[i] = value_;
            output.right[i] = value_;
        }
    }
private:
    float value_;
};

void testGraphRouting() {
    daw::Graph graph;
    const auto a = graph.add(std::make_unique<Constant>(0.1f));
    const auto b = graph.add(std::make_unique<Constant>(0.2f));
    const auto mixer = graph.add(std::make_unique<daw::MixerNode>(), {a, b});
    graph.setMaster(mixer);
    graph.prepare(48000, 128);
    auto audio = daw::renderOffline(graph, 256, 64);
    CHECK(audio.size() == 512);
    CHECK(std::abs(audio[0] - 0.3f) < 0.00001f);
    CHECK(std::abs(audio[511] - 0.3f) < 0.00001f);
}

void testGraphCycle() {
    daw::Graph graph;
    graph.add(std::make_unique<daw::MixerNode>(), {1});
    graph.add(std::make_unique<daw::MixerNode>(), {0});
    graph.setMaster(1);
    bool caught = false;
    try { graph.prepare(48000, 128); }
    catch (const std::invalid_argument&) { caught = true; }
    CHECK(caught);
}

void testGraphInvalidInput() {
    daw::Graph graph;
    graph.add(std::make_unique<daw::MixerNode>(), {10});
    bool caught = false;
    try { graph.prepare(48000, 128); }
    catch (const std::invalid_argument&) { caught = true; }
    CHECK(caught);
}

std::vector<float> isolatedNote(uint32_t chunk) {
    daw::Graph graph;
    auto synth = std::make_unique<daw::SynthNode>();
    synth->setADSR(0.0f, 0.0f, 1.0f, 0.0f);
    synth->addEvent({100, 69, 127, true});
    synth->addEvent({48000, 69, 0, false});
    graph.setMaster(graph.add(std::move(synth)));
    graph.prepare(48000, chunk);
    return daw::renderOffline(graph, 24000, chunk);
}

void testEventPrecisionAndBlockInvariance() {
    auto ref = isolatedNote(64);
    CHECK(ref[2*99] == 0.0f);
    CHECK(ref[2*101] != 0.0f);
    for (auto n : {128u, 256u, 512u}) {
        auto alt = isolatedNote(n);
        CHECK(alt.size() == ref.size());
        for (size_t i = 0; i < alt.size(); ++i) {
            CHECK(std::abs(alt[i] - ref[i]) < 0.0000001f);
        }
    }
}

void testA440() {
    auto samples = isolatedNote(256);
    uint32_t positiveCrossings = 0;
    // Count 0.3 seconds after event, avoiding the initial sample.
    for (size_t i = 4801; i < 19200; ++i) {
        if (samples[2*(i-1)] <= 0 && samples[2*i] > 0) ++positiveCrossings;
    }
    const float estimated = positiveCrossings * 48000.0f / (19200.0f - 4801.0f);
    CHECK(std::abs(estimated - 440.0f) < 6.0f);
}

void testVoiceAndADSR() {
    daw::Graph graph;
    auto synth = std::make_unique<daw::SynthNode>();
    synth->setADSR(0.01f, 0.02f, 0.8f, 0.03f);
    for (int i = 0; i < 80; ++i) {
        synth->addEvent({static_cast<uint64_t>(i * 32), static_cast<uint8_t>(40 + i % 30), 110, true});
        synth->addEvent({static_cast<uint64_t>(i * 32 + 48), static_cast<uint8_t>(40 + i % 30), 0, false});
    }
    graph.setMaster(graph.add(std::move(synth)));
    graph.prepare(48000, 127);
    auto audio = daw::renderOffline(graph, 16000, 127);
    for (const auto sample : audio) CHECK(std::isfinite(sample));
    CHECK(std::abs(audio.back()) < 0.00001f);
}

void testGainPanLiveUpdate() {
    daw::Graph graph;
    const auto constant = graph.add(std::make_unique<Constant>(0.5f));
    auto gain = std::make_unique<daw::GainPanNode>(1.0f, 0.0f);
    auto* gainPtr = gain.get();
    graph.setMaster(graph.add(std::move(gain), {constant}));
    graph.prepare(48000, 256);
    graph.process(0, 256);
    gainPtr->setPan(-1.0f);
    gainPtr->setGain(0.5f);
    for (int i = 0; i < 20; ++i) graph.process((i+1)*256, 256);
    const auto& block = graph.process(21*256, 256);
    CHECK(std::abs(block.left[250] - 0.25f) < 0.0001f);
    CHECK(std::abs(block.right[250]) < 0.0001f);
}

void testDemoAndWaveHeader() {
    auto demo = daw::makeDemo(44100, 256);
    auto result = daw::renderOffline(demo.graph, 44100*4, 128);
    float peak = 0.0f;
    for (auto sample : result) {
        CHECK(std::isfinite(sample));
        peak = std::max(peak, std::abs(sample));
    }
    CHECK(peak > 0.01f);
    CHECK(peak < 1.0f);
    daw::writeWavPcm16("test_output.wav", result, 44100);
    std::ifstream in("test_output.wav", std::ios::binary);
    CHECK(static_cast<bool>(in));
    std::string header(44, '\0');
    in.read(header.data(), 44);
    CHECK(header.substr(0,4) == "RIFF");
    CHECK(header.substr(8,4) == "WAVE");
    CHECK(header.substr(36,4) == "data");
    in.seekg(0, std::ios::end);
    CHECK(in.tellg() == static_cast<std::streamoff>(44 + result.size()*2));
}
}

int main() {
    try {
        const auto run = [](const char* name, auto test) {
            test();
            std::cout << "PASS " << name << '\n';
        };
        run("graph routing", testGraphRouting);
        run("graph cycle", testGraphCycle);
        run("graph invalid connection", testGraphInvalidInput);
        run("sample-accurate MIDI and block invariance", testEventPrecisionAndBlockInvariance);
        run("440 Hz frequency", testA440);
        run("ADSR and voice stealing", testVoiceAndADSR);
        run("atomic gain/pan updates", testGainPanLiveUpdate);
        run("offline demo and WAV export", testDemoAndWaveHeader);
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "FAIL " << e.what() << '\n';
        return 1;
    }
}
