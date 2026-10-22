#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <audioclient.h>
#include <mmdeviceapi.h>
#include <ksmedia.h>
#include <combaseapi.h>

#include "open_daw/engine.hpp"
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <string>

namespace daw::wasapi {
namespace {
void verify(HRESULT value, const char* description) {
    if (FAILED(value)) throw std::runtime_error(
        std::string(description) + " HRESULT=" + std::to_string(static_cast<uint32_t>(value)));
}

template <class Interface> struct ComPtr {
    Interface* ptr = nullptr;
    ~ComPtr() { if (ptr) ptr->Release(); }
    Interface* operator->() const noexcept { return ptr; }
    ComPtr(const ComPtr&) = delete;
    ComPtr& operator=(const ComPtr&) = delete;
    ComPtr() = default;
};

struct ComInitialize {
    bool shouldUninit = false;
    ComInitialize() {
        const auto result = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
        if (result == S_OK || result == S_FALSE) shouldUninit = true;
        else verify(result, "CoInitializeEx");
    }
    ~ComInitialize() { if (shouldUninit) CoUninitialize(); }
};

struct MixFormat {
    WAVEFORMATEX* format = nullptr;
    ~MixFormat() { CoTaskMemFree(format); }
};

struct DeviceObjects {
    ComPtr<IMMDeviceEnumerator> enumerator;
    ComPtr<IMMDevice> device;
    ComPtr<IAudioClient> client;
    MixFormat mix;

    DeviceObjects() {
        verify(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
            __uuidof(IMMDeviceEnumerator), reinterpret_cast<void**>(&enumerator.ptr)),
            "CoCreateInstance(MMDeviceEnumerator)");
        verify(enumerator->GetDefaultAudioEndpoint(eRender, eMultimedia, &device.ptr),
               "GetDefaultAudioEndpoint");
        verify(device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
               reinterpret_cast<void**>(&client.ptr)), "Activate(IAudioClient)");
        verify(client->GetMixFormat(&mix.format), "GetMixFormat");
    }
};

enum class PcmKind { Float32, Signed16 };
PcmKind checkFormat(const WAVEFORMATEX* fmt) {
    if (fmt->wBitsPerSample == 32 && (
         fmt->wFormatTag == WAVE_FORMAT_IEEE_FLOAT ||
         (fmt->wFormatTag == WAVE_FORMAT_EXTENSIBLE &&
          IsEqualGUID(reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(fmt)->SubFormat,
                      KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)))) return PcmKind::Float32;
    if (fmt->wBitsPerSample == 16 && (
         fmt->wFormatTag == WAVE_FORMAT_PCM ||
         (fmt->wFormatTag == WAVE_FORMAT_EXTENSIBLE &&
          IsEqualGUID(reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(fmt)->SubFormat,
                      KSDATAFORMAT_SUBTYPE_PCM)))) return PcmKind::Signed16;
    throw std::runtime_error("Device mix format currently unsupported (requires float32 or PCM16)");
}

struct ScopedEvent {
    HANDLE handle = nullptr;
    ~ScopedEvent() { if (handle) CloseHandle(handle); }
};
}

uint32_t defaultSampleRate() {
    ComInitialize com;
    DeviceObjects endpoint;
    return endpoint.mix.format->nSamplesPerSec;
}

void play(Graph& graph, double seconds) {
    ComInitialize com;
    DeviceObjects endpoint;
    const auto* format = endpoint.mix.format;
    const auto kind = checkFormat(format);
    const uint32_t rate = format->nSamplesPerSec;
    const uint32_t channels = format->nChannels;
    if (channels < 2 || channels > 32 || graph.sampleRate() != rate) {
        throw std::runtime_error("Unsupported WASAPI configuration or device rate changed");
    }
    verify(endpoint.client->Initialize(AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 0, 0, endpoint.mix.format, nullptr),
        "IAudioClient.Initialize(shared event-driven)");
    UINT32 bufferFrames = 0;
    verify(endpoint.client->GetBufferSize(&bufferFrames), "GetBufferSize");
    ComPtr<IAudioRenderClient> writer;
    verify(endpoint.client->GetService(__uuidof(IAudioRenderClient),
        reinterpret_cast<void**>(&writer.ptr)), "GetService(IAudioRenderClient)");
    ScopedEvent event;
    event.handle = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event.handle) throw std::runtime_error("CreateEvent failed");
    verify(endpoint.client->SetEventHandle(event.handle), "SetEventHandle");

    // Initialize endpoint with silence before playback begins.
    BYTE* ptr = nullptr;
    verify(writer->GetBuffer(bufferFrames, &ptr), "GetBuffer (prime)");
    verify(writer->ReleaseBuffer(bufferFrames, AUDCLNT_BUFFERFLAGS_SILENT), "ReleaseBuffer (prime)");

    uint64_t frame = 0;
    const uint64_t totalFrames = static_cast<uint64_t>(std::llround(seconds * rate));
    uint64_t callbackCount = 0;
    std::chrono::nanoseconds maxProcessing{};
    const auto start = std::chrono::steady_clock::now();
    verify(endpoint.client->Start(), "IAudioClient.Start");
    try {
        while (frame < totalFrames) {
            const auto wait = WaitForSingleObject(event.handle, 1000);
            if (wait != WAIT_OBJECT_0) throw std::runtime_error("WASAPI callback timeout/failure");
            UINT32 padding = 0;
            verify(endpoint.client->GetCurrentPadding(&padding), "GetCurrentPadding");
            if (padding > bufferFrames) throw std::runtime_error("Invalid WASAPI padding");
            const UINT32 ready = bufferFrames - padding;
            if (!ready) continue;
            BYTE* dest = nullptr;
            verify(writer->GetBuffer(ready, &dest), "GetBuffer");
            const auto t0 = std::chrono::steady_clock::now();
            UINT32 completed = 0;
            while (completed < ready) {
                const UINT32 frames = std::min(graph.maxFrames(), ready - completed);
                const auto& block = graph.process(frame, frames);
                for (UINT32 i = 0; i < frames; ++i) {
                    const auto ix = static_cast<size_t>(completed + i) * channels;
                    const float left = (frame + i < totalFrames) ? block.left[i] : 0.0f;
                    const float right = (frame + i < totalFrames) ? block.right[i] : 0.0f;
                    if (kind == PcmKind::Float32) {
                        auto* out = reinterpret_cast<float*>(dest);
                        out[ix] = left;
                        out[ix + 1] = right;
                        for (UINT32 c = 2; c < channels; ++c) out[ix+c] = 0.0f;
                    } else {
                        auto* out = reinterpret_cast<int16_t*>(dest);
                        out[ix] = static_cast<int16_t>(std::lrint(std::clamp(left, -1.0f, 1.0f) * 32767.0f));
                        out[ix + 1] = static_cast<int16_t>(std::lrint(std::clamp(right, -1.0f, 1.0f) * 32767.0f));
                        for (UINT32 c = 2; c < channels; ++c) out[ix+c] = 0;
                    }
                }
                completed += frames;
                frame += frames;
            }
            verify(writer->ReleaseBuffer(ready, 0), "ReleaseBuffer");
            const auto elapsed = std::chrono::steady_clock::now() - t0;
            if (elapsed > maxProcessing) maxProcessing = elapsed;
            ++callbackCount;
        }
        // The final packet may still be buffered by WASAPI. Let the endpoint drain
        // it before stopping, rather than cutting off the final milliseconds.
        while (true) {
            UINT32 remaining = 0;
            verify(endpoint.client->GetCurrentPadding(&remaining), "GetCurrentPadding(drain)");
            if (remaining == 0) break;
            const auto wait = WaitForSingleObject(event.handle, 1000);
            if (wait != WAIT_OBJECT_0) throw std::runtime_error("WASAPI drain timeout/failure");
        }
        verify(endpoint.client->Stop(), "IAudioClient.Stop");
    } catch (...) {
        endpoint.client->Stop();
        throw;
    }
    const auto wall = std::chrono::duration<double>(
        std::chrono::steady_clock::now() - start).count();
    std::cout << "WASAPI: " << rate << " Hz, " << channels << " channels, device buffer "
              << bufferFrames << " frames; callbacks " << callbackCount
              << ", max process " << std::chrono::duration<double, std::milli>(maxProcessing).count()
              << " ms, wall " << wall << "s\n";
}
} // namespace daw::wasapi
#endif
