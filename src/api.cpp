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
#include <map>

struct od_engine {
    daw::SessionSpec spec;
    uint64_t generation = 1;
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
namespace {
daw::SynthConfig decodeSynth(const od_synth_v1& s) {
    daw::SynthConfig c{static_cast<daw::Waveform>(s.waveform), s.shape, s.pulse_width, s.attack, s.decay, s.sustain, s.release};
    if (s.struct_size != sizeof(od_synth_v1) || s.version != OD_SYNTH_V1_VERSION || !c.valid())
        throw std::invalid_argument("Invalid synth descriptor/version/range");
    return c;
}
daw::PadParameters decodePad(const od_pad_parameters_v1& p){
    daw::PadParameters c{p.gain,p.pan,p.tune_semitones,p.mute!=0,p.one_shot!=0,p.choke_group};
    if(p.mute>1 || p.one_shot>1 || !c.valid())throw std::invalid_argument("Invalid drum pad parameters");return c;
}
od_result loadSession(od_engine* e, const od_track* tracks, const od_track_v2* tracksV2,
                      size_t trackCount, const od_note* notes, size_t noteCount, uint64_t length, const od_track_v3* tracksV3 = nullptr, const od_track_v4* tracksV4 = nullptr) {

        if (!e || (!tracks && !tracksV2 && !tracksV3 && !tracksV4) || !trackCount || trackCount > daw::MaxTracks || (noteCount && !notes) || noteCount > 100000)
            return invalid("Invalid project arrays/limits");
        daw::SessionSpec candidate;
        candidate.sampleRate = e->spec.sampleRate; candidate.maxFrames = e->spec.maxFrames;
        candidate.lengthFrames = length; candidate.tracks.clear();
        std::unordered_map<std::string, std::shared_ptr<const daw::SampleData>> samples;
        uint64_t residentBytes = 0, encodedBytes = 0;
        std::map<std::pair<uintptr_t,size_t>,std::shared_ptr<const daw::SampleData>> memorySamples;
        for (size_t i = 0; i < trackCount; ++i) {
            if(tracksV4 && (tracksV4[i].struct_size!=sizeof(od_track_v4) || tracksV4[i].version!=4 || tracksV4[i].instrument_kind>2 ||
                tracksV4[i].pad_count>16 || (tracksV4[i].instrument_kind==2 ? !tracksV4[i].pad_count || !tracksV4[i].pads : tracksV4[i].pad_count!=0)))return invalid("Invalid v4 instrument/pad descriptor");
            const od_track_v3* v3=tracksV4 ? &tracksV4[i].base : tracksV3 ? &tracksV3[i] : nullptr;
            if (v3 && (v3->struct_size != sizeof(od_track_v3) || v3->version != OD_TRACK_V3_VERSION))
                return invalid("Invalid v3 track descriptor");
            const od_track_v2* v2 = v3 ? &v3->base : tracksV2 ? &tracksV2[i] : nullptr;
            if (v2 && (v2->struct_size != sizeof(od_track_v2) || v2->version != OD_TRACK_V2_VERSION ||
                v2->mute > 1 || v2->solo > 1)) return invalid("Invalid v2 track descriptor");
            const auto& t = v2 ? v2->base : tracks[i];
            if (!std::isfinite(t.gain) || !std::isfinite(t.pan) || t.root_note > 127 || t.one_shot > 1)
                return invalid("Invalid track parameters");
            daw::TrackSettings settings;
            settings.gain = std::clamp(t.gain, 0.0f, 2.0f); settings.pan = std::clamp(t.pan, -1.0f, 1.0f);
            settings.mute = v2 && v2->mute; settings.solo = v2 && v2->solo;
            if (v3) settings.synth = decodeSynth(v3->synth);
            settings.rootNote = static_cast<uint8_t>(t.root_note); settings.oneShot = t.one_shot != 0;
            const bool hasSample=t.sample_path_utf8 && t.sample_path_utf8[0];
            if(tracksV4 && ((tracksV4[i].instrument_kind==1)!=hasSample))return invalid("Instrument kind conflicts with sampler path");
            if(tracksV4 && tracksV4[i].instrument_kind==2){
                settings.drums.emplace();auto& kit=*settings.drums;kit.pads.reserve(tracksV4[i].pad_count);
                for(size_t pad=0;pad<tracksV4[i].pad_count;++pad){
                    const auto& d=tracksV4[i].pads[pad];
                    if(d.struct_size!=sizeof(od_drum_pad_v1) || d.version!=1 || d.midi_note>127 || (!d.wav_bytes && d.wav_size) || (d.wav_bytes && !d.wav_size))return invalid("Invalid drum sample descriptor");
                    daw::DrumPad prepared;prepared.midiNote=static_cast<uint8_t>(d.midi_note);prepared.parameters=decodePad(d.parameters);
                    if(d.wav_size){
                        const auto key=std::pair{reinterpret_cast<uintptr_t>(d.wav_bytes),d.wav_size};auto it=memorySamples.find(key);
                        if(it==memorySamples.end()){
                            constexpr uint64_t budget=128*1024*1024;
                            if(d.wav_size>budget-encodedBytes)return invalid("Drum WAV byte budget exceeded");encodedBytes+=d.wav_size;
                            auto sample=daw::loadWavMemory({d.wav_bytes,d.wav_size},std::min<uint64_t>(48000*60,(budget-residentBytes)/(2*sizeof(float))));
                            residentBytes+=sample->stereo.size()*sizeof(float);it=memorySamples.emplace(key,std::move(sample)).first;
                        }prepared.sample=it->second;
                    }kit.pads.push_back(std::move(prepared));
                }
            }
            if (hasSample) {
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
        if (e->generation == UINT64_MAX) return invalid("Session generation exhausted");
        auto prepared = std::make_unique<daw::EngineService>(candidate);
        e->device.stop(); // callback quiescence before old graph/resource destruction
        e->service = std::move(prepared);
        e->spec = std::move(candidate); ++e->generation;
        return OD_OK;

}
od_result setMix(od_engine* e, uint64_t generation, const od_track_mix_v1* mix, size_t count, uint64_t* outSerial, const od_parameters_v1* params = nullptr, const od_parameters_v2* drums = nullptr) {
    if (!e || !mix || !outSerial || count != e->spec.tracks.size() || generation != e->generation)
        return invalid("Invalid mixer count, generation or output");
    daw::MixSnapshot snapshot; snapshot.updateSynths = params != nullptr;
    if (params) for (size_t i = 0; i < count; ++i) snapshot.synths[i] = decodeSynth(params[i].synth);
    for (size_t i = 0; i < count; ++i) {
        const auto& t = mix[i];
        if (!std::isfinite(t.gain) || !std::isfinite(t.pan) || t.gain < 0 || t.gain > 2 || t.pan < -1 || t.pan > 1 || t.mute > 1 || t.solo > 1)
            return invalid("Invalid mixer values");
        snapshot.tracks[i] = {t.gain, t.pan, t.mute != 0, t.solo != 0};
    }
    if(drums){
        snapshot.updateDrums=true;
        for(size_t i=0;i<count;++i){
            if(drums[i].struct_size!=sizeof(od_parameters_v2) || drums[i].version!=2 || drums[i].pad_count!=(e->spec.tracks[i].drums ? e->spec.tracks[i].drums->pads.size() : 0))return invalid("Invalid drum parameter snapshot/count");
            for(size_t pad=0;pad<drums[i].pad_count;++pad)snapshot.drums[i].pads[pad]=decodePad(drums[i].pads[pad]);
        }
    }
    uint64_t serial = 0;
    if (!e->service->submitMix(snapshot, serial)) { setError("Mixer queue full; retain and retry the latest snapshot"); return OD_QUEUE_FULL; }
    for (size_t i = 0; i < count; ++i) {
        auto& t = e->spec.tracks[i]; t.gain = mix[i].gain; t.pan = mix[i].pan; t.mute = mix[i].mute != 0; t.solo = mix[i].solo != 0;
        if (params) t.synth = snapshot.synths[i];
        if(drums && t.drums)for(size_t pad=0;pad<t.drums->pads.size();++pad)t.drums->pads[pad].parameters=snapshot.drums[i].pads[pad];
    }
    *outSerial = serial; return OD_OK;
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
od_result od_load(od_engine* e, const od_track* tracks, size_t count, const od_note* notes, size_t noteCount, uint64_t length) noexcept {
    return protect([&] { return loadSession(e, tracks, nullptr, count, notes, noteCount, length); });
}
od_result od_load_v2(od_engine* e, const od_track_v2* tracks, size_t count, const od_note* notes, size_t noteCount, uint64_t length) noexcept {
    return protect([&] { return loadSession(e, nullptr, tracks, count, notes, noteCount, length); });
}
od_result od_load_v3(od_engine* e, const od_track_v3* tracks, size_t count, const od_note* notes, size_t noteCount, uint64_t length) noexcept {
    return protect([&] { return loadSession(e, nullptr, nullptr, count, notes, noteCount, length, tracks); });
}
od_result od_set_parameters_v1(od_engine* e, uint64_t generation, const od_parameters_v1* params, size_t count, uint64_t* serial) noexcept {
    return protect([&] {
        if (!params || count > daw::MaxTracks) return invalid("Invalid parameter array");
        std::array<od_track_mix_v1, daw::MaxTracks> mix{};
        for (size_t i = 0; i < count; ++i) mix[i] = params[i].mix;
        return setMix(e, generation, mix.data(), count, serial, params);
    });
}
od_result od_load_v4(od_engine* e,const od_track_v4* tracks,size_t count,const od_note* notes,size_t noteCount,uint64_t length) noexcept {
    return protect([&]{return loadSession(e,nullptr,nullptr,count,notes,noteCount,length,nullptr,tracks);});
}
od_result od_set_parameters_v2(od_engine* e,uint64_t generation,const od_parameters_v2* p,size_t count,uint64_t* serial) noexcept {
    return protect([&]{
        if(!p || count>daw::MaxTracks)return invalid("Invalid drum parameter array");
        std::array<od_parameters_v1,daw::MaxTracks> base{};std::array<od_track_mix_v1,daw::MaxTracks> mix{};
        for(size_t i=0;i<count;++i){base[i]=p[i].base;mix[i]=p[i].base.mix;}return setMix(e,generation,mix.data(),count,serial,base.data(),p);
    });
}
od_result od_read_drum_status_v1(const od_engine* e,uint64_t generation,uint32_t track,od_drum_status_v1* out) noexcept {
    error[0]='\0';daw::DrumStatus s;
    if(!e || !out || generation!=e->generation || !e->service->drumStatus(track,s))return invalid("Invalid drum status generation/track/output");
    out->generation=e->generation;out->pad_count=s.padCount;out->active_voices=s.active;std::copy(s.triggers.begin(),s.triggers.end(),out->triggers);return OD_OK;
}
od_result od_cancel_live(od_engine* e) noexcept {
    error[0]='\0';if(!e)return invalid("Null engine");return e->service->cancelLive() ? OD_OK : invalid("Live generation exhausted");
}
od_result od_set_mix_v1(od_engine* e, uint64_t generation, const od_track_mix_v1* mix, size_t count, uint64_t* serial) noexcept {
    return protect([&] { return setMix(e, generation, mix, count, serial); });
}
od_result od_read_mix_status_v1(const od_engine* e, od_mix_status_v1* out) noexcept {
    error[0] = '\0'; if (!e || !out) return invalid("Null mixer status output");
    *out = {e->generation, e->service->mixSubmitted(), e->service->mixApplied()}; return OD_OK;
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
        std::array<od_track_mix_v1, daw::MaxTracks> mix{};
        for (size_t i = 0; i < e->spec.tracks.size(); ++i) {
            const auto& t = e->spec.tracks[i]; mix[i] = {t.gain, t.pan, uint32_t(t.mute), uint32_t(t.solo)};
        }
        mix[track].gain = std::clamp(gain, 0.0f, 2.0f); mix[track].pan = std::clamp(pan, -1.0f, 1.0f);
        uint64_t serial = 0; return setMix(e, e->generation, mix.data(), e->spec.tracks.size(), &serial);
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
