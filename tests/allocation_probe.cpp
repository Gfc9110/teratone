#include "open_daw/service.hpp"
#include <cstdlib>
#include <iostream>
#include <new>
#include <vector>
#ifdef __APPLE__
#include <dlfcn.h>
#include <pthread.h>
#endif
#ifdef _WIN32
#include <malloc.h>
#endif
namespace {
thread_local bool monitor = false;
thread_local size_t allocations = 0, deallocations = 0;
void* alloc(size_t n) { if (monitor) ++allocations; if (auto* p = std::malloc(n ? n : 1)) return p; throw std::bad_alloc(); }
void freep(void* p) noexcept { if (p && monitor) ++deallocations; std::free(p); }
void* aligned(size_t n, size_t alignment) {
    if (monitor) ++allocations;
#ifdef _WIN32
    void* p = _aligned_malloc(n ? n : 1, alignment);
#else
    void* p = nullptr; if (posix_memalign(&p, alignment, n ? n : 1) != 0) p = nullptr;
#endif
    if (!p) throw std::bad_alloc(); return p;
}
void freeAligned(void* p) noexcept {
    if (p && monitor) ++deallocations;
#ifdef _WIN32
    _aligned_free(p);
#else
    std::free(p);
#endif
}
}
void* operator new(size_t n) { return alloc(n); }
void* operator new[](size_t n) { return alloc(n); }
void operator delete(void* p) noexcept { freep(p); }
void operator delete[](void* p) noexcept { freep(p); }
void operator delete(void* p, size_t) noexcept { freep(p); }
void operator delete[](void* p, size_t) noexcept { freep(p); }
void* operator new(size_t n, std::align_val_t a) { return aligned(n, static_cast<size_t>(a)); }
void* operator new[](size_t n, std::align_val_t a) { return aligned(n, static_cast<size_t>(a)); }
void operator delete(void* p, std::align_val_t) noexcept { freeAligned(p); }
void operator delete[](void* p, std::align_val_t) noexcept { freeAligned(p); }
void operator delete(void* p, size_t, std::align_val_t) noexcept { freeAligned(p); }
void operator delete[](void* p, size_t, std::align_val_t) noexcept { freeAligned(p); }
void* operator new(size_t n, const std::nothrow_t&) noexcept { try { return alloc(n); } catch (...) { return nullptr; } }
void* operator new[](size_t n, const std::nothrow_t&) noexcept { try { return alloc(n); } catch (...) { return nullptr; } }
void operator delete(void* p, const std::nothrow_t&) noexcept { freep(p); }
void operator delete[](void* p, const std::nothrow_t&) noexcept { freep(p); }
void* operator new(size_t n, std::align_val_t a, const std::nothrow_t&) noexcept { try { return aligned(n, static_cast<size_t>(a)); } catch (...) { return nullptr; } }
void* operator new[](size_t n, std::align_val_t a, const std::nothrow_t&) noexcept { try { return aligned(n, static_cast<size_t>(a)); } catch (...) { return nullptr; } }
void operator delete(void* p, std::align_val_t, const std::nothrow_t&) noexcept { freeAligned(p); }
void operator delete[](void* p, std::align_val_t, const std::nothrow_t&) noexcept { freeAligned(p); }
int main() {
#ifdef __APPLE__
    struct AuditCounts {uint64_t allocations,deallocations,blockingLocks;};
    auto beginAudit=reinterpret_cast<void(*)()>(dlsym(RTLD_DEFAULT,"od_rt_audit_begin"));
    auto endAudit=reinterpret_cast<AuditCounts(*)()>(dlsym(RTLD_DEFAULT,"od_rt_audit_end"));
    if(std::getenv("OD_REQUIRE_RT_AUDIT")&&(!beginAudit||!endAudit))return 2;
    if(beginAudit&&endAudit){
        pthread_mutex_t lock=PTHREAD_MUTEX_INITIALIZER;void*(*volatile mallocCall)(size_t)=&std::malloc;void(*volatile freeCall)(void*)=&std::free;
        beginAudit();void* p=mallocCall(17);freeCall(p);pthread_mutex_lock(&lock);pthread_mutex_unlock(&lock);const auto test=endAudit();pthread_mutex_destroy(&lock);
        if(!test.allocations||!test.deallocations||!test.blockingLocks){std::cerr<<"Interposer positive control failed\n";return 3;}
        std::cout<<"PASS malloc/free/pthread-lock interposer positive control\n";
    }
#endif
    daw::SessionSpec spec; spec.lengthFrames = 48000; spec.tracks.resize(daw::MaxTracks);
    auto sample = std::make_shared<daw::SampleData>(); sample->stereo.resize(2048, 0.05f);
    spec.tracks[1].sample = sample;
    spec.tracks[2].drums.emplace();
    for (uint8_t pitch : {35,42,46}) {
        daw::DrumPad pad;pad.midiNote=pitch;pad.sample=sample;pad.parameters.chokeGroup=pitch==35?0:1;
        spec.tracks[2].drums->pads.push_back(pad);
    }
    spec.notes = {{1, 0, 12000, 0, 60, 100}, {2, 1000, 14000, 0, 60, 100}, {3, 500, 2000, 1, 72, 100}};
    for(uint32_t t=3;t<daw::MaxTracks;++t){spec.tracks[t]=spec.tracks[t%3];spec.tracks[t].gain=.005f;spec.notes.push_back({100+t,0,12000,t,static_cast<uint8_t>(t%3==2?35:60),80});}
    daw::EngineService engine(spec); std::vector<float> out(512);
    daw::Command loop{daw::CommandType::Loop}; loop.enabled = true; loop.b = 48000;
    engine.submit(loop); engine.submit({daw::CommandType::Play});
#ifdef __APPLE__
    if(beginAudit&&endAudit)beginAudit();
#endif
    monitor = true;
    for (size_t block = 0; block < 10000; ++block) {
        engine.setGain(0, static_cast<float>(block % 100) / 100);
        if (block % 200 == 0) { daw::Command seek{daw::CommandType::Seek}; seek.a = 0; engine.submit(seek); }
        if (block % 50 == 0) {
            daw::Command note{daw::CommandType::NoteOn}; note.id = daw::LiveIdBit | 1; engine.submit(note);
        }
        if (block % 50 == 10) {
            daw::Command note{daw::CommandType::NoteOff}; note.id = daw::LiveIdBit | 1; engine.submit(note);
        }
        if (block % 4 == 0) {
            daw::MixSnapshot mix; mix.tracks[0].gain = 0.5f; mix.tracks[1].gain = 0.7f;
            mix.tracks[0].mute = block % 8 == 0; mix.tracks[1].solo = block % 12 == 0;
            uint64_t serial = 0; mix.updateSynths = true; mix.synths[0].waveform = static_cast<daw::Waveform>((block / 4) % 4);
            mix.synths[0].shape = static_cast<float>(block % 100) / 100; mix.synths[0].pulseWidth = block % 8 ? .05f : .95f;
            mix.synths[0].release = block % 12 ? 0 : 20;
            engine.submitMix(mix, serial);
        }
        if(block % 3 == 0) {
            daw::Command hit{daw::CommandType::NoteOn};hit.track=2;hit.pitch=block%6?42:46;hit.id=daw::LiveIdBit|2;
            for(int i=0;i<80;++i)engine.submit(hit); // bounded overload and voice stealing
            daw::MixSnapshot mix;mix.updateDrums=true;mix.drums[2].pads[1].pan=block%2 ? -1.f : 1.f;
            mix.drums[2].pads[1].mute=block%4==0;uint64_t serial=0;engine.submitMix(mix,serial);
        }
        if(block % 7 == 0)engine.cancelLive();
        engine.render(out.data(), 256);
        daw::DrumStatus drums;(void)engine.drumStatus(2,drums);
        (void)engine.telemetry();
    }
    engine.stop(); engine.render(out.data(), 256);
    monitor = false;
#ifdef __APPLE__
    if(beginAudit&&endAudit){const auto count=endAudit();std::cout<<"Runtime audit: allocation_calls="<<count.allocations<<" free_calls="<<count.deallocations<<" blocking_lock_calls="<<count.blockingLocks<<'\n';if(count.allocations||count.deallocations||count.blockingLocks)return 4;}
#endif
    std::cout << "Monitored 10001 renders: new=" << allocations << " delete=" << deallocations << '\n';
    return (allocations || deallocations) ? 1 : 0;
}
