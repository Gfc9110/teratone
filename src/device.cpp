#include "open_daw/device.hpp"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstring>
#include <future>
#include <thread>
#include <vector>

#if defined(__APPLE__)
#include <TargetConditionals.h>
#if TARGET_OS_OSX
#include <AudioUnit/AudioUnit.h>
#define OD_COREAUDIO 1
#endif
#elif defined(_WIN32)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <audioclient.h>
#include <mmdeviceapi.h>
#include <avrt.h>
#endif

namespace daw {
struct OutputDevice::Impl {
    EngineService* engine = nullptr;
    bool opened = false;
    DeviceMode mode = DeviceMode::Native;
    std::atomic<bool> quit{false};
    std::atomic<uint32_t> failure{0};
    std::thread worker;
#if OD_COREAUDIO
    AudioUnit unit = nullptr;
    static OSStatus callback(void* p, AudioUnitRenderActionFlags*, const AudioTimeStamp*, UInt32, UInt32 n, AudioBufferList* out) {
        auto& self = *static_cast<Impl*>(p);
        if (out && out->mNumberBuffers == 1 && out->mBuffers[0].mData &&
            out->mBuffers[0].mNumberChannels == 2 && out->mBuffers[0].mDataByteSize >= uint64_t{n} * 2 * sizeof(float)) {
            self.engine->render(static_cast<float*>(out->mBuffers[0].mData), n);
        } else {
            if (out) for (UInt32 i = 0; i < out->mNumberBuffers; ++i)
                if (out->mBuffers[i].mData) std::memset(out->mBuffers[i].mData, 0, out->mBuffers[i].mDataByteSize);
            self.failure.store(1, std::memory_order_relaxed);
        }
        return noErr;
    }
#endif
#if defined(_WIN32)
    HANDLE stopEvent = nullptr;
    template<class T> struct Com {
        T* p = nullptr;
        ~Com() { if (p) p->Release(); }
        T* operator->() const { return p; }
    };
    static void check(HRESULT h, const char* operation) {
        if (FAILED(h)) throw std::runtime_error(std::string(operation) + " (HRESULT " + std::to_string(static_cast<uint32_t>(h)) + ")");
    }
    void windowsWorker(std::promise<void> ready) noexcept {
        bool signaled = false;
        bool comStarted = false;
        HANDLE audioEvent = nullptr, mmcss = nullptr;
        try {
            const auto init = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
            check(init, "COM init"); comStarted = true;
            // Nested scope ensures all COM interfaces are released BEFORE CoUninitialize.
            {
                Com<IMMDeviceEnumerator> enumerator;
                Com<IMMDevice> endpoint;
                Com<IAudioClient> client;
                Com<IAudioRenderClient> render;
                check(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
                    __uuidof(IMMDeviceEnumerator), reinterpret_cast<void**>(&enumerator.p)), "Audio enumerator");
                check(enumerator->GetDefaultAudioEndpoint(eRender, eMultimedia, &endpoint.p), "Default endpoint");
                check(endpoint->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                    reinterpret_cast<void**>(&client.p)), "Audio client");
                WAVEFORMATEX format{};
                format.wFormatTag = WAVE_FORMAT_IEEE_FLOAT; format.nChannels = 2;
                format.nSamplesPerSec = static_cast<DWORD>(engine->sampleRate());
                format.wBitsPerSample = 32; format.nBlockAlign = 8; format.nAvgBytesPerSec = format.nSamplesPerSec * 8;
                check(client->Initialize(AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    0, 0, &format, nullptr), "Shared stereo output");
                UINT32 buffer = 0; check(client->GetBufferSize(&buffer), "Buffer size");
                check(client->GetService(__uuidof(IAudioRenderClient), reinterpret_cast<void**>(&render.p)), "Render interface");
                audioEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
                if (!audioEvent) throw std::runtime_error("Cannot create audio event");
                check(client->SetEventHandle(audioEvent), "Set audio event");
                BYTE* output = nullptr;
                check(render->GetBuffer(buffer, &output), "Prime buffer");
                check(render->ReleaseBuffer(buffer, AUDCLNT_BUFFERFLAGS_SILENT), "Prime silence");
                check(client->Start(), "Start output");
                DWORD task = 0;
                mmcss = AvSetMmThreadCharacteristicsW(L"Pro Audio", &task);
                ready.set_value(); signaled = true;
                const HANDLE waits[]{stopEvent, audioEvent};
                while (true) {
                    const DWORD wait = WaitForMultipleObjects(2, waits, FALSE, 2000);
                    if (wait == WAIT_OBJECT_0) break;
                    if (wait != WAIT_OBJECT_0 + 1) { failure.store(WAIT_TIMEOUT); break; }
                    UINT32 padding = 0;
                    HRESULT h = client->GetCurrentPadding(&padding);
                    if (FAILED(h) || padding > buffer) { failure.store(FAILED(h) ? static_cast<uint32_t>(h) : 1); break; }
                    const auto available = buffer - padding;
                    if (!available) continue;
                    h = render->GetBuffer(available, &output);
                    if (FAILED(h)) { failure.store(static_cast<uint32_t>(h)); break; }
                    engine->render(reinterpret_cast<float*>(output), available);
                    h = render->ReleaseBuffer(available, 0);
                    if (FAILED(h)) { failure.store(static_cast<uint32_t>(h)); break; }
                }
                client->Stop();
            }
        } catch (...) {
            failure.store(1);
            if (!signaled) { try { ready.set_exception(std::current_exception()); } catch (...) {} }
        }
        if (mmcss) AvRevertMmThreadCharacteristics(mmcss);
        if (audioEvent) CloseHandle(audioEvent);
        if (comStarted) CoUninitialize();
    }
#endif
};
OutputDevice::OutputDevice() : impl_(std::make_unique<Impl>()) {}
OutputDevice::~OutputDevice() { stop(); }
bool OutputDevice::open() const noexcept { return impl_->opened; }
uint32_t OutputDevice::errorCode() const noexcept { return impl_->failure.load(std::memory_order_relaxed); }
void OutputDevice::start(EngineService& service, DeviceMode mode) {
    stop();
    auto& s = *impl_;
    s.engine = &service; s.mode = mode; s.quit.store(false); s.failure.store(0);
    try {
        if (mode == DeviceMode::SilentClock) {
            // Explicit test/demo clock. Never masquerades as a physical audio device.
            std::vector<float> scratch(size_t{service.maxFrames()} * 2);
            s.worker = std::thread([&s, scratch = std::move(scratch)]() mutable {
                const auto period = std::chrono::duration_cast<std::chrono::steady_clock::duration>(
                    std::chrono::duration<double>(s.engine->maxFrames() / s.engine->sampleRate()));
                auto next = std::chrono::steady_clock::now();
                while (!s.quit.load(std::memory_order_acquire)) {
                    s.engine->render(scratch.data(), s.engine->maxFrames());
                    next += period;
                    std::this_thread::sleep_until(next);
                }
            });
        } else {
#if OD_COREAUDIO
            const auto check = [](OSStatus code, const char* op) {
                if (code != noErr) throw std::runtime_error(std::string(op) + " (CoreAudio " + std::to_string(code) + ")");
            };
            AudioComponentDescription desc{};
            desc.componentType = kAudioUnitType_Output;
            desc.componentSubType = kAudioUnitSubType_DefaultOutput;
            desc.componentManufacturer = kAudioUnitManufacturer_Apple;
            const auto component = AudioComponentFindNext(nullptr, &desc);
            if (!component) throw std::runtime_error("CoreAudio default output unavailable");
            check(AudioComponentInstanceNew(component, &s.unit), "Create output unit");
            AudioStreamBasicDescription format{};
            format.mSampleRate = service.sampleRate(); format.mFormatID = kAudioFormatLinearPCM;
            format.mFormatFlags = kAudioFormatFlagsNativeFloatPacked;
            format.mBytesPerPacket = 8; format.mFramesPerPacket = 1; format.mBytesPerFrame = 8;
            format.mChannelsPerFrame = 2; format.mBitsPerChannel = 32;
            check(AudioUnitSetProperty(s.unit, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input,
                0, &format, sizeof(format)), "Set stereo stream format");
            UInt32 maximum = MaxBlockFrames;
            check(AudioUnitSetProperty(s.unit, kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global,
                0, &maximum, sizeof(maximum)), "Set maximum callback size");
            AURenderCallbackStruct callback{Impl::callback, &s};
            check(AudioUnitSetProperty(s.unit, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input,
                0, &callback, sizeof(callback)), "Set audio callback");
            check(AudioUnitInitialize(s.unit), "Initialize output");
            check(AudioOutputUnitStart(s.unit), "Start output");
#elif defined(_WIN32)
            s.stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
            if (!s.stopEvent) throw std::runtime_error("Cannot create shutdown event");
            std::promise<void> ready;
            auto startup = ready.get_future();
            s.worker = std::thread([&s, ready = std::move(ready)]() mutable { s.windowsWorker(std::move(ready)); });
            startup.get(); // startup errors propagate; no waiting in the rendering callback
#else
            throw std::runtime_error("Native output adapter not implemented on this platform; --silent is an explicit no-audio clock");
#endif
        }
        s.opened = true;
    } catch (...) { stop(); throw; }
}
void OutputDevice::stop() noexcept {
    auto& s = *impl_;
    s.quit.store(true, std::memory_order_release);
#if defined(_WIN32)
    if (s.stopEvent) SetEvent(s.stopEvent);
#endif
#if OD_COREAUDIO
    if (s.unit) {
        AudioOutputUnitStop(s.unit);
        AudioUnitUninitialize(s.unit);
        AudioComponentInstanceDispose(s.unit);
        s.unit = nullptr;
    }
#endif
    if (s.worker.joinable()) s.worker.join();
#if defined(_WIN32)
    if (s.stopEvent) { CloseHandle(s.stopEvent); s.stopEvent = nullptr; }
#endif
    s.opened = false; s.engine = nullptr;
}
}
