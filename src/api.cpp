#include "open_daw/path.hpp"
#include "open_daw/api.h"
#include "open_daw/service.hpp"
#include "open_daw/device.hpp"
#include <algorithm>
#include <cmath>
#include <string>
#include <cstring>
#include <array>
#include <unordered_map>

struct od_engine {
    daw::SessionSpec spec;
    std::unique_ptr<daw::EngineService> service;
    daw::OutputDevice device; // destroyed first, before service
};
namespace {
thread_local std::array<char, 512> error{};
void setError(const char* text) noexcept {
    if (!text) text = "Native error";
    std::strncpy(error.data(), text, error.size() - 1);
    error.back() = '\0';
}
template<class F> od_result protect(F&& function) noexcept {
    try { error[0] = '\0'; return function(); }
    catch (const std::invalid_argument& e) { setError(e.what()); return OD_INVALID; }
    catch (const std::exception& e) { setError(e.what()); return OD_ERROR; }
    catch (...) { setError("Unknown native engine error"); return OD_ERROR; }
}
od_result invalid(const char* reason) { setError(reason); return OD_INVALID; }
od_result command(od_engine* e, daw::Command c) {
    if (!e) return invalid("Null engine");
    if (!e->service->submit(c)) { setError("Command rejected (invalid arguments or full queue)"); return OD_QUEUE_FULL; }
    return OD_OK;
}
}
extern "C" {
uint32_t od_abi_version() noexcept { return OD_ABI_VERSION; }
const char* od_error_message() noexcept { return error.data(); }
od_engine* od_create(uint32_t rate, uint32_t block) noexcept {
    od_engine* result = nullptr;
    protect([&] {
        auto e = std::make_unique<od_engine>();
        e->spec.sampleRate = rate; e->spec.maxFrames = block;
        e->service = std::make_unique<daw::EngineService>(e->spec);
        result = e.release(); return OD_OK;
    });
    return result;
}
void od_destroy(od_engine* e) noexcept { delete e; }
od_result od_load(od_engine* e, const od_track* tracks, size_t trackCount, const od_note* notes,
                  size_t noteCount, uint64_t length) noexcept {
    return protect([&] {
        if (!e || !tracks || !trackCount || trackCount > daw::MaxTracks || (noteCount && !notes) || noteCount > 100000)
            return invalid("Invalid project arrays/limits");
        daw::SessionSpec candidate;
        candidate.sampleRate = e->spec.sampleRate; candidate.maxFrames = e->spec.maxFrames;
        candidate.lengthFrames = length; candidate.tracks.clear();
        std::unordered_map<std::string, std::shared_ptr<const daw::SampleData>> samples;
        uint64_t residentBytes = 0;
        for (size_t i = 0; i < trackCount; ++i) {
            const auto& t = tracks[i];
            if (!std::isfinite(t.gain) || !std::isfinite(t.pan) || t.root_note > 127 || t.one_shot > 1)
                return invalid("Invalid track parameters");
            daw::TrackSettings settings;
            settings.gain = std::clamp(t.gain, 0.0f, 2.0f); settings.pan = std::clamp(t.pan, -1.0f, 1.0f);
            settings.rootNote = static_cast<uint8_t>(t.root_note); settings.oneShot = t.one_shot != 0;
            if (t.sample_path_utf8 && t.sample_path_utf8[0]) {
                std::string key(t.sample_path_utf8);
                auto it = samples.find(key);
                if (it == samples.end()) {
                    constexpr uint64_t budget = 128 * 1024 * 1024;
                    const auto remainingFrames = (budget - residentBytes) / (2 * sizeof(float));
                    auto sample = daw::loadWav(daw::utf8Path(key), std::min<uint64_t>(48000 * 60, remainingFrames));
                    residentBytes += sample->stereo.size() * sizeof(float);
                    it = samples.emplace(key, std::move(sample)).first;
                }
                settings.sample = it->second;
            }
            candidate.tracks.push_back(std::move(settings));
        }
        candidate.notes.reserve(noteCount);
        for (size_t i = 0; i < noteCount; ++i) {
            const auto& n = notes[i];
            if (n.pitch > 127 || n.velocity > 127) return invalid("Invalid note MIDI value");
            candidate.notes.push_back({n.id, n.start_frame, n.end_frame, n.track,
                static_cast<uint8_t>(n.pitch), static_cast<uint8_t>(n.velocity)});
        }
        auto prepared = std::make_unique<daw::EngineService>(candidate);
        e->device.stop(); // callback quiescence before old graph/resource destruction
        e->service = std::move(prepared);
        e->spec = std::move(candidate);
        return OD_OK;
    });
}
od_result od_play(od_engine* e) noexcept { return protect([&] { return command(e, {daw::CommandType::Play}); }); }
od_result od_pause(od_engine* e) noexcept { return protect([&] { return command(e, {daw::CommandType::Pause}); }); }
od_result od_seek(od_engine* e, uint64_t frame) noexcept {
    return protect([&] {
        if (!e || frame > e->spec.lengthFrames) return invalid("Seek outside session");
        daw::Command c{daw::CommandType::Seek}; c.a = frame; return command(e, c);
    });
}
od_result od_loop(od_engine* e, uint32_t enabled, uint64_t begin, uint64_t end) noexcept {
    return protect([&] {
        if (!e || enabled > 1 || (enabled && (begin >= end || end > e->spec.lengthFrames || end - begin < e->spec.maxFrames)))
            return invalid("Loop must span at least one prepared block inside the project");
        daw::Command c{daw::CommandType::Loop}; c.enabled = enabled != 0; c.a = begin; c.b = end; return command(e, c);
    });
}
od_result od_live_note(od_engine* e, uint32_t track, uint64_t id, uint32_t pitch, uint32_t velocity, uint32_t on) noexcept {
    return protect([&] {
        if (!e || track >= e->spec.tracks.size() || !(id & daw::LiveIdBit) || id == daw::LiveIdBit ||
            pitch > 127 || velocity > 127 || on > 1 || (on && !velocity)) return invalid("Invalid live note or non-live ID");
        daw::Command c{on ? daw::CommandType::NoteOn : daw::CommandType::NoteOff};
        c.track = track; c.id = id; c.pitch = static_cast<uint8_t>(pitch); c.velocity = static_cast<uint8_t>(velocity);
        return command(e, c);
    });
}
od_result od_stop(od_engine* e) noexcept { return protect([&] { if (!e) return invalid("Null engine"); e->service->stop(); return OD_OK; }); }
od_result od_panic(od_engine* e) noexcept { return protect([&] { if (!e) return invalid("Null engine"); e->service->panic(); return OD_OK; }); }
od_result od_gain_pan(od_engine* e, uint32_t track, float gain, float pan) noexcept {
    return protect([&] {
        if (!e || track >= e->spec.tracks.size() || !std::isfinite(gain) || !std::isfinite(pan)) return invalid("Invalid gain/pan");
        e->service->setGain(track, gain); e->service->setPan(track, pan);
        e->spec.tracks[track].gain = std::clamp(gain, 0.0f, 2.0f);
        e->spec.tracks[track].pan = std::clamp(pan, -1.0f, 1.0f);
        return OD_OK;
    });
}
od_result od_read_telemetry(const od_engine* e, od_telemetry* out) noexcept {
    // Allocation-free polling on the control side. The fields form an approximate snapshot.
    error[0] = '\0';
    if (!e || !out) return invalid("Null engine or telemetry output");
    const auto t = e->service->telemetry();
    *out = {t.position, t.renderedFrames, t.blocks, t.lastApplied, t.rejectedCommands, t.clippedSamples,
        t.peakLeft, t.peakRight, static_cast<uint32_t>(t.playing), static_cast<uint32_t>(t.looping),
        static_cast<uint32_t>(e->device.open()), e->device.errorCode()};
    return OD_OK;
}
od_result od_render(od_engine* e, float* out, uint32_t frames) noexcept {
    error[0] = '\0';
    if (!e || (!out && frames) || frames > 1048576) return invalid("Invalid headless render arguments");
    if (e->device.open()) { setError("Device owns the render consumer; stop it before headless rendering"); return OD_BUSY; }
    e->service->render(out, frames); return OD_OK;
}
od_result od_device_start(od_engine* e, uint32_t silent) noexcept {
    return protect([&] {
        if (!e || silent > 1) return invalid("Invalid device request");
        if (e->device.open()) return OD_OK;
        e->device.start(*e->service, silent ? daw::DeviceMode::SilentClock : daw::DeviceMode::Native);
        return OD_OK;
    });
}
od_result od_device_stop(od_engine* e) noexcept { return protect([&] { if (!e) return invalid("Null engine"); e->device.stop(); return OD_OK; }); }
od_result od_export(od_engine* e, const char* path, uint64_t frames) noexcept {
    return protect([&] {
        if (!e || !path || !path[0]) return invalid("Invalid export path");
        daw::renderSessionToWav(e->spec, daw::utf8Path(path), frames);
        return OD_OK;
    });
}
}
