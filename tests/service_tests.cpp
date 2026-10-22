#include "open_daw/service.hpp"
#include "open_daw/api.h"
#include "open_daw/device.hpp"
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>
#include <fstream>
#include <iostream>
#include <limits>
#include <random>
#include <thread>
#include <vector>

#define CHECK(c) do { if (!(c)) throw std::runtime_error(std::string(__FILE__) + ":" + std::to_string(__LINE__) + ": " #c); } while (0)
namespace {
template<class F> void throws(F f) { bool caught = false; try { f(); } catch (const std::exception&) { caught = true; } CHECK(caught); }
float energy(const std::vector<float>& v, size_t a = 0, size_t b = SIZE_MAX) {
    float sum = 0; for (size_t i = a; i < std::min(b, v.size()); ++i) sum += std::abs(v[i]); return sum;
}
daw::SessionSpec score() {
    daw::SessionSpec s; s.lengthFrames = 4096; s.maxFrames = 256;
    s.notes = {{1, 100, 900, 0, 60, 100}, {2, 600, 1600, 0, 60, 100}, {3, 2200, 3600, 0, 67, 90}};
    return s;
}
std::vector<float> render(daw::EngineService& e, uint32_t frames) {
    std::vector<float> result(size_t{frames} * 2); e.render(result.data(), frames); return result;
}
void gainConsistency() {
    daw::Graph graph;
    auto a = std::make_unique<daw::GainPanNode>(5, 2);
    auto b = std::make_unique<daw::GainPanNode>(); b->setGain(5); b->setPan(2);
    a->prepare(48000, 64); b->prepare(48000, 64);
    daw::AudioBlock in, outA, outB; in.allocate(64); outA.allocate(64); outB.allocate(64);
    std::fill(in.left.begin(), in.left.end(), 1); std::fill(in.right.begin(), in.right.end(), 1);
    const daw::AudioBlock* ptr = &in;
    a->process({0, 64, 48000}, {&ptr, 1}, outA); b->process({0, 64, 48000}, {&ptr, 1}, outB);
    CHECK(outA.left == outB.left && outA.right == outB.right);
    throws([] { daw::GainPanNode bad(std::numeric_limits<float>::quiet_NaN(), 0); });
}
void independentNotes() {
    daw::Graph graph; auto synth = std::make_unique<daw::SynthNode>(); auto* p = synth.get();
    synth->setADSR(0, 0, 1, 0); graph.setMaster(graph.add(std::move(synth))); graph.prepare(48000, 256);
    const daw::BlockNoteEvent events[]{{0, 60, 100, true, 1}, {20, 60, 100, true, 2},
        {100, 60, 0, false, 1}, {200, 60, 0, false, 2}};
    p->setBlockEvents(events); const auto& out = graph.process(0, 256);
    float middle = 0;
    for (size_t i = 101; i < 199; ++i) middle += std::abs(out.left[i]);
    CHECK(middle > 1); for (size_t i = 200; i < 256; ++i) CHECK(out.left[i] == 0);
}
void blockInvariance() {
    auto s = score(); daw::EngineService a(s), b(s);
    CHECK(a.submit({daw::CommandType::Play})); CHECK(b.submit({daw::CommandType::Play}));
    auto reference = render(a, 4096); std::vector<float> out(8192);
    const uint32_t sizes[]{1, 7, 255, 257, 511, 64, 3}; uint32_t done = 0; size_t k = 0;
    while (done < 4096) { auto n = std::min(sizes[k++ % 7], 4096 - done); b.render(out.data() + 2 * done, n); done += n; }
    CHECK(reference == out); CHECK(energy(out) > 10);
    CHECK(a.telemetry().position == 4096 && !a.telemetry().playing);
}
void transport() {
    auto s = score(); daw::EngineService e(s);
    CHECK(energy(render(e, 128)) == 0); CHECK(e.telemetry().position == 0);
    daw::Command seek{daw::CommandType::Seek}; seek.a = 1700;
    CHECK(e.submit(seek)); CHECK(e.submit({daw::CommandType::Play}));
    CHECK(energy(render(e, 400)) == 0); // no chasing notes that began before the seek
    CHECK(energy(render(e, 400)) > 1);
    CHECK(e.submit({daw::CommandType::Pause})); CHECK(energy(render(e, 128)) == 0);
    CHECK(e.telemetry().position == 2500 && !e.telemetry().playing);
    e.stop(); render(e, 128); CHECK(e.telemetry().position == 0);
    seek.a = s.lengthFrames + 1; CHECK(!e.submit(seek));
}
void loops() {
    auto s = score(); s.notes = {{1, 128, 300, 0, 60, 100}, {2, 450, 620, 0, 67, 100}};
    daw::EngineService a(s), b(s);
    daw::Command loop{daw::CommandType::Loop}; loop.enabled = true; loop.a = 128; loop.b = 640;
    CHECK(a.submit(loop) && b.submit(loop)); a.submit({daw::CommandType::Play}); b.submit({daw::CommandType::Play});
    auto reference = render(a, 4096); std::vector<float> out(8192);
    for (uint32_t i = 0; i < 4096; i += 7) b.render(out.data() + 2 * i, std::min(7u, 4096 - i));
    CHECK(reference == out); CHECK(a.telemetry().position == 128 && a.telemetry().looping);
    loop.b = loop.a + 100; CHECK(!a.submit(loop));
}
void overloadAndPanic() {
    daw::SessionSpec s; daw::EngineService e(s);
    size_t accepted = 0; while (e.submit({daw::CommandType::Play})) ++accepted;
    CHECK(accepted == 1024); CHECK(e.telemetry().rejectedCommands == 1);
    e.stop(); CHECK(energy(render(e, 128)) == 0); CHECK(!e.telemetry().playing);
    daw::Command note{daw::CommandType::NoteOn}; note.id = daw::LiveIdBit | 9; note.pitch = 67;
    CHECK(e.submit(note)); CHECK(energy(render(e, 256)) > 0);
    while (e.submit({daw::CommandType::Play})) {}
    note.type = daw::CommandType::NoteOff; CHECK(!e.submit(note)); // reserved emergency path
    CHECK(energy(render(e, 256)) == 0); CHECK(!e.telemetry().playing);
}
void liveWhileStopped() {
    daw::SessionSpec s; daw::EngineService e(s);
    daw::Command n{daw::CommandType::NoteOn}; n.id = daw::LiveIdBit | 1;
    CHECK(e.submit(n)); CHECK(energy(render(e, 1024)) > 10); CHECK(e.telemetry().position == 0);
    n.type = daw::CommandType::NoteOff; CHECK(e.submit(n)); render(e, 4096);
    CHECK(energy(render(e, 256)) == 0);
}
void validation() {
    auto s = score();
    s.notes.push_back(s.notes[0]); throws([&] { daw::EngineService e(s); });
    s = score(); s.notes[0].id = daw::LiveIdBit; throws([&] { daw::EngineService e(s); });
    s = score(); s.notes[0].end = 1000000; throws([&] { daw::EngineService e(s); });
    s = score(); s.maxFrames = 0; throws([&] { daw::EngineService e(s); });
    s = score(); s.notes.clear();
    for (uint64_t id = 1; id <= 257; ++id) s.notes.push_back({id, 0, 1000, 0, 60, 100});
    throws([&] { daw::EngineService e(s); });
    s = score(); s.tracks.resize(17); throws([&] { daw::EngineService e(s); });
}
void queueStress() {
    daw::SpscQueue<uint64_t, 67> q; constexpr uint64_t total = 100000;
    std::atomic<bool> failed{false};
    std::thread producer([&] { for (uint64_t i = 0; i < total; ++i) while (!q.push(i)) std::this_thread::yield(); });
    for (uint64_t i = 0; i < total; ++i) { uint64_t n = 0; while (!q.pop(n)) std::this_thread::yield(); if (n != i) failed = true; }
    producer.join(); CHECK(!failed.load());
}
void streamingExport() {
    auto s = score(); daw::renderSessionToWav(s, "service_test.wav", 4096);
    CHECK(std::filesystem::file_size("service_test.wav") == 44 + 4096 * 4);
    auto sample = daw::loadWav("service_test.wav"); CHECK(sample->frames() == 4096 && sample->sampleRate == 48000);
    CHECK(energy(sample->stereo) > 10);
    auto peaks = daw::waveformPeaks(*sample, 256); CHECK(peaks.size() == 16);
    throws([&] { daw::loadWav("service_test.wav", 100); });
    throws([&] { daw::waveformPeaks(*sample, 0); });
    throws([] { daw::WavStreamWriter out("invalid_size.wav", 48000, UINT64_MAX); });
    CHECK(!std::filesystem::exists("invalid_size.wav"));
    { daw::WavStreamWriter out("short_write.wav", 48000, 10); throws([&] { out.finish(); }); }
}
void malformedWave() {
    { std::ofstream out("bad.wav", std::ios::binary); out << "not a wave"; }
    throws([] { daw::loadWav("bad.wav"); });
    std::ifstream source("service_test.wav", std::ios::binary);
    std::vector<char> bytes((std::istreambuf_iterator<char>(source)), {});
    CHECK(bytes.size() > 44); bytes[40] = char(0xff); bytes[41] = char(0xff); bytes[42] = char(0xff); bytes[43] = char(0x7f);
    { std::ofstream out("bad.wav", std::ios::binary); out.write(bytes.data(), bytes.size()); }
    throws([] { daw::loadWav("bad.wav"); });
    std::mt19937 rng(12345);
    for (int trial = 0; trial < 100; ++trial) {
        std::vector<char> fuzz(128); for (auto& c : fuzz) c = static_cast<char>(rng() & 255);
        { std::ofstream out("fuzz.wav", std::ios::binary); out.write(fuzz.data(), fuzz.size()); }
        throws([] { daw::loadWav("fuzz.wav"); });
    }
}
void samplerPitchAndRelease() {
    auto data = std::make_shared<daw::SampleData>(); data->stereo.resize(2000);
    for (size_t i = 0; i < 1000; ++i) data->stereo[2*i] = data->stereo[2*i+1] = static_cast<float>(i) / 1000;
    daw::SamplerNode sampler(data, 60, false); sampler.prepare(48000, 512);
    daw::AudioBlock out; out.allocate(512);
    const daw::BlockNoteEvent events[]{{0, 72, 127, true, 1}};
    sampler.setBlockEvents(events); sampler.process({0, 512, 48000}, {}, out);
    CHECK(std::abs(out.left[100] - 0.16f) < 1e-6f); // octave-up: reads source frame 200
    CHECK(out.left[501] == 0); // source exhausted at 2x rate
    sampler.reset(); const daw::BlockNoteEvent release[]{{0, 60, 127, true, 2}, {100, 60, 0, false, 2}};
    sampler.setBlockEvents(release); sampler.process({0, 512, 48000}, {}, out); CHECK(out.left[400] == 0);
    auto s = score(); s.tracks[0].sample = data; daw::EngineService engine(s);
    engine.submit({daw::CommandType::Play}); CHECK(energy(render(engine, 4096)) > 0);
}
void ffiLifecycle() {
    CHECK(od_abi_version() == OD_ABI_VERSION); CHECK(!od_create(1, 256)); CHECK(std::strlen(od_error_message()) > 0);
    od_engine* e = od_create(48000, 256); CHECK(e);
    const od_track track{0.8f, 0, nullptr, 60, 0};
    const od_note notes[]{{1, 100, 1000, 0, 60, 100}, {2, 500, 1400, 0, 60, 100}};
    CHECK(od_load(e, &track, 1, notes, 2, 4096) == OD_OK);
    CHECK(od_seek(e, 4097) == OD_INVALID); CHECK(od_live_note(e, 0, 1, 60, 100, 1) == OD_INVALID);
    CHECK(od_play(e) == OD_OK); std::vector<float> buffer(1024); CHECK(od_render(e, buffer.data(), 512) == OD_OK);
    CHECK(energy(buffer) > 0); od_telemetry t{}; CHECK(od_read_telemetry(e, &t) == OD_OK && t.position == 512);
    od_note bad = notes[0]; bad.end_frame = 10;
    CHECK(od_load(e, &track, 1, &bad, 1, 4096) == OD_INVALID);
    CHECK(od_read_telemetry(e, &t) == OD_OK && t.position == 512); // failed load is non-destructive
    CHECK(od_gain_pan(e, 0, 9, 4) == OD_OK); CHECK(od_gain_pan(e, 2, 1, 0) == OD_INVALID);
    CHECK(od_export(e, "ffi_export.wav", 4096) == OD_OK);
    CHECK(od_read_telemetry(e, &t) == OD_OK && t.position == 512); // independent export instance
    CHECK(od_device_start(e, 1) == OD_OK); CHECK(od_render(e, buffer.data(), 128) == OD_BUSY);
    std::this_thread::sleep_for(std::chrono::milliseconds(30));
    CHECK(od_read_telemetry(e, &t) == OD_OK && t.device_open && t.blocks > 2);
    CHECK(od_load(e, &track, 1, notes, 2, 4096) == OD_OK); // stops/joins before replacement
    CHECK(od_read_telemetry(e, &t) == OD_OK && !t.device_open && t.position == 0);
    CHECK(od_device_start(e, 1) == OD_OK); od_destroy(e); // destruction joins an active callback owner
    od_destroy(nullptr); CHECK(od_stop(nullptr) == OD_INVALID);
}
void concurrentControl() {
    auto s = score(); s.lengthFrames = 10000000;
    daw::EngineService engine(s); daw::OutputDevice device;
    device.start(engine, daw::DeviceMode::SilentClock);
    engine.submit({daw::CommandType::Play});
    for (int i = 0; i < 10000; ++i) {
        engine.setGain(0, static_cast<float>(i % 100) / 100);
        engine.setPan(0, static_cast<float>(i % 200 - 100) / 100);
        const auto t = engine.telemetry(); CHECK(std::isfinite(t.peakLeft));
        if (i % 100 == 0) { daw::Command c{daw::CommandType::Seek}; c.a = i; engine.submit(c); }
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
    device.stop(); CHECK(engine.telemetry().blocks > 0);
}
}
int main() {
    try {
        const auto run = [](const char* name, auto test) { test(); std::cout << "PASS " << name << '\n'; };
        run("constructor/setter validation consistency", gainConsistency);
        run("overlapping note occurrence IDs", independentNotes);
        run("variable-block scheduling invariance", blockInvariance);
        run("play/pause/stop/seek reset policy", transport);
        run("loop boundaries and block invariance", loops);
        run("bounded queue overflow and emergency note-off", overloadAndPanic);
        run("live audition without moving transport", liveWhileStopped);
        run("session and event-density validation", validation);
        run("SPSC two-thread ordering stress", queueStress);
        run("streaming export and waveform peaks", streamingExport);
        run("malformed WAV bounds and fuzz smoke", malformedWave);
        run("sampler pitch/release and engine integration", samplerPitchAndRelease);
        run("C ABI transactional load/export/lifecycle", ffiLifecycle);
        run("concurrent controls, telemetry, device stop", concurrentControl);
        std::cout << "14 interactive checks passed\n"; return 0;
    } catch (const std::exception& e) { std::cerr << "FAIL " << e.what() << '\n'; return 1; }
}
