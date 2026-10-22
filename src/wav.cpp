#include "open_daw/engine.hpp"
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <limits>
#include <stdexcept>

namespace daw {
namespace {
void writeU16(std::ostream& out, uint16_t n) {
    out.put(static_cast<char>(n & 0xff));
    out.put(static_cast<char>((n >> 8) & 0xff));
}
void writeU32(std::ostream& out, uint32_t n) {
    for (int i = 0; i < 4; ++i) out.put(static_cast<char>((n >> (8 * i)) & 0xff));
}
}

void writeWavPcm16(const char* path, const std::vector<float>& samples,
                   uint32_t sampleRate, uint16_t channels) {
    if (channels == 0 || channels > 8 || sampleRate < 8000 || samples.size() % channels != 0 ||
        samples.size() > (std::numeric_limits<uint32_t>::max() - 36) / 2) {
        throw std::invalid_argument("Invalid WAV export settings");
    }
    const auto dataBytes = static_cast<uint32_t>(samples.size() * 2);
    std::ofstream out(path, std::ios::binary);
    if (!out) throw std::runtime_error("Unable to open WAV file");
    out.write("RIFF", 4);
    writeU32(out, 36 + dataBytes);
    out.write("WAVEfmt ", 8);
    writeU32(out, 16);            // PCM fmt chunk length
    writeU16(out, 1);             // PCM
    writeU16(out, channels);
    writeU32(out, sampleRate);
    writeU32(out, sampleRate * channels * 2);
    writeU16(out, static_cast<uint16_t>(channels * 2));
    writeU16(out, 16);
    out.write("data", 4);
    writeU32(out, dataBytes);
    for (float sample : samples) {
        if (!std::isfinite(sample)) sample = 0.0f;
        const float scaled = std::clamp(sample, -1.0f, 1.0f);
        const auto pcm = static_cast<int16_t>(std::lrint(scaled * (scaled >= 0 ? 32767.0f : 32768.0f)));
        writeU16(out, static_cast<uint16_t>(pcm));
    }
    if (!out) throw std::runtime_error("WAV file write failed");
}
} // namespace daw
