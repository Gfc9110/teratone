#include "open_daw/media.hpp"
#include <algorithm>
#include <bit>
#include <cmath>
#include <cstring>
#include <limits>

namespace daw {
namespace {
uint16_t u16(const unsigned char* p) { return uint16_t(p[0]) | uint16_t(p[1]) << 8; }
uint32_t u32(const unsigned char* p) {
    return uint32_t(p[0]) | uint32_t(p[1]) << 8 | uint32_t(p[2]) << 16 | uint32_t(p[3]) << 24;
}
void readExact(std::ifstream& in, void* p, size_t n) {
    in.read(static_cast<char*>(p), static_cast<std::streamsize>(n));
    if (!in) throw std::runtime_error("Truncated WAV file");
}
void put16(std::ostream& out, uint16_t value) {
    const char b[]{char(value & 255), char((value >> 8) & 255)}; out.write(b, 2);
}
void put32(std::ostream& out, uint32_t value) {
    const char b[]{char(value & 255), char((value >> 8) & 255), char((value >> 16) & 255), char(value >> 24)};
    out.write(b, 4);
}
}

std::shared_ptr<const SampleData> loadWav(const std::filesystem::path& path, uint64_t maximumFrames) {
    std::ifstream in(path, std::ios::binary);
    if (!in) throw std::runtime_error("Cannot open sample WAV");
    const auto fileBytes = std::filesystem::file_size(path);
    unsigned char header[12]; readExact(in, header, 12);
    if (std::memcmp(header, "RIFF", 4) || std::memcmp(header + 8, "WAVE", 4))
        throw std::invalid_argument("Only little-endian RIFF/WAVE samples are supported");
    const uint64_t riffEnd = uint64_t{u32(header + 4)} + 8;
    if (riffEnd < 12 || riffEnd > fileBytes) throw std::invalid_argument("Invalid RIFF length");
    uint16_t format = 0, channels = 0, bits = 0, align = 0;
    uint32_t rate = 0, byteRate = 0, dataSize = 0;
    uint64_t dataOffset = 0;
    bool haveFormat = false, haveData = false;
    for (uint64_t pos = 12; pos + 8 <= riffEnd;) {
        in.seekg(static_cast<std::streamoff>(pos));
        unsigned char chunk[8]; readExact(in, chunk, 8);
        const uint64_t size = u32(chunk + 4);
        if (size > riffEnd - pos - 8) throw std::invalid_argument("WAV chunk exceeds file boundary");
        if (!std::memcmp(chunk, "fmt ", 4) && !haveFormat) {
            if (size < 16) throw std::invalid_argument("Invalid WAV format chunk");
            unsigned char fmt[16]; readExact(in, fmt, 16);
            format = u16(fmt); channels = u16(fmt + 2); rate = u32(fmt + 4);
            byteRate = u32(fmt + 8); align = u16(fmt + 12); bits = u16(fmt + 14);
            haveFormat = true;
        } else if (!std::memcmp(chunk, "data", 4) && !haveData) {
            dataOffset = pos + 8; dataSize = static_cast<uint32_t>(size); haveData = true;
        }
        const auto next = pos + 8 + size + (size & 1);
        if (next > riffEnd) throw std::invalid_argument("Missing WAV chunk padding");
        pos = next;
    }
    if (!haveFormat || !haveData || (channels != 1 && channels != 2) || rate < 8000 || rate > 384000 ||
        !((format == 1 && (bits == 16 || bits == 24 || bits == 32)) || (format == 3 && bits == 32)) ||
        align != channels * (bits / 8) || byteRate != rate * align || dataSize % align != 0)
        throw std::invalid_argument("Unsupported or inconsistent WAV format (PCM16/24/32 or float32, mono/stereo)");
    const uint64_t frames = dataSize / align;
    if (!frames || frames > maximumFrames || frames > std::numeric_limits<size_t>::max() / 2)
        throw std::length_error("Sample is empty or exceeds the short-sample frame budget");
    auto data = std::make_shared<SampleData>();
    data->sampleRate = rate; data->stereo.resize(static_cast<size_t>(frames) * 2);
    in.seekg(static_cast<std::streamoff>(dataOffset));
    for (size_t frame = 0; frame < frames; ++frame) {
        float pair[2]{};
        for (uint16_t channel = 0; channel < channels; ++channel) {
            unsigned char b[4]{}; readExact(in, b, bits / 8);
            float value;
            if (format == 3) value = std::bit_cast<float>(u32(b));
            else if (bits == 16) value = std::bit_cast<int16_t>(u16(b)) / 32768.0f;
            else if (bits == 24) {
                const uint32_t v = uint32_t(b[0]) | uint32_t(b[1]) << 8 | uint32_t(b[2]) << 16;
                value = (static_cast<int32_t>(v ^ 0x800000u) - 0x800000) / 8388608.0f;
            } else value = static_cast<float>(std::bit_cast<int32_t>(u32(b)) / 2147483648.0);
            if (!std::isfinite(value)) throw std::invalid_argument("Non-finite sample data");
            pair[channel] = value;
        }
        data->stereo[2 * frame] = pair[0];
        data->stereo[2 * frame + 1] = pair[channels == 1 ? 0 : 1];
    }
    return data;
}

std::vector<Peak> waveformPeaks(const SampleData& sample, size_t step) {
    if (!step) throw std::invalid_argument("Waveform step must be positive");
    std::vector<Peak> peaks;
    for (size_t pos = 0; pos < sample.frames();) {
        const auto end = pos + std::min(step, sample.frames() - pos);
        Peak p;
        for (; pos < end; ++pos) {
            p.low = std::min({p.low, sample.stereo[2 * pos], sample.stereo[2 * pos + 1]});
            p.high = std::max({p.high, sample.stereo[2 * pos], sample.stereo[2 * pos + 1]});
        }
        peaks.push_back(p);
    }
    return peaks;
}

SamplerNode::SamplerNode(std::shared_ptr<const SampleData> data, uint8_t rootNote, bool oneShot)
    : data_(std::move(data)), root_(rootNote), oneShot_(oneShot) {
    if (!data_ || !data_->frames() || data_->stereo.size() % 2 || root_ > 127 ||
        data_->sampleRate < 8000 || data_->sampleRate > 384000)
        throw std::invalid_argument("Invalid sample instrument");
    for (float f : data_->stereo) if (!std::isfinite(f)) throw std::invalid_argument("Non-finite sample");
}
void SamplerNode::prepare(double sampleRate, uint32_t) {
    if (!std::isfinite(sampleRate) || sampleRate < 8000 || sampleRate > 384000)
        throw std::invalid_argument("Invalid sampler rate");
    sampleRate_ = sampleRate;
    releaseFrames_ = std::max(1u, static_cast<uint32_t>(sampleRate * 0.005));
    reset();
}
void SamplerNode::reset() noexcept { voices_.fill(Voice{}); age_ = 0; blockEvents_ = {}; }
void SamplerNode::event(const BlockNoteEvent& e) noexcept {
    if (!e.on || !e.velocity) {
        if (oneShot_) return;
        for (auto& v : voices_) if (v.active && (e.id ? v.id == e.id : v.note == e.note) && !v.releasing) {
            v.releasing = true; v.releaseLeft = releaseFrames_;
        }
        return;
    }
    auto* dest = &voices_[0];
    for (auto& v : voices_) {
        if (!v.active) { dest = &v; break; }
        if (v.age < dest->age) dest = &v;
    }
    *dest = Voice{};
    dest->active = true; dest->id = e.id; dest->note = e.note;
    dest->gain = (e.velocity / 127.0f) * 0.8f; dest->age = ++age_;
    dest->increment = (static_cast<double>(data_->sampleRate) / sampleRate_) *
        std::pow(2.0, (static_cast<int>(e.note) - static_cast<int>(root_)) / 12.0);
}
void SamplerNode::process(const ProcessContext& ctx, std::span<const AudioBlock* const>, AudioBlock& output) noexcept {
    size_t next = 0;
    for (uint32_t frame = 0; frame < ctx.frames; ++frame) {
        while (next < blockEvents_.size() && blockEvents_[next].offset <= frame) event(blockEvents_[next++]);
        float l = 0, r = 0;
        for (auto& v : voices_) {
            if (!v.active) continue;
            const auto a = static_cast<size_t>(v.position);
            if (a >= data_->frames()) { v.active = false; continue; }
            const auto b = std::min(a + 1, data_->frames() - 1);
            const float f = static_cast<float>(v.position - static_cast<double>(a));
            const float envelope = v.releasing ? static_cast<float>(v.releaseLeft) / releaseFrames_ : 1;
            l += (data_->stereo[2 * a] * (1 - f) + data_->stereo[2 * b] * f) * v.gain * envelope;
            r += (data_->stereo[2 * a + 1] * (1 - f) + data_->stereo[2 * b + 1] * f) * v.gain * envelope;
            v.position += v.increment;
            if (v.releasing && --v.releaseLeft == 0) v.active = false;
        }
        output.left[frame] = l; output.right[frame] = r;
    }
    blockEvents_ = {};
}

WavStreamWriter::WavStreamWriter(const std::filesystem::path& path, uint32_t rate, uint64_t totalFrames)
    : total_(totalFrames) {
    if (rate < 8000 || rate > 384000 || !totalFrames || totalFrames > (UINT32_MAX - 36ull) / 4)
        throw std::invalid_argument("Invalid WAV export size/rate (RIFF must remain below 4 GiB)");
    out_.open(path, std::ios::binary | std::ios::trunc);
    if (!out_) throw std::runtime_error("Cannot create WAV output");
    const auto bytes = static_cast<uint32_t>(totalFrames * 4);
    out_.write("RIFF", 4); put32(out_, bytes + 36); out_.write("WAVEfmt ", 8);
    put32(out_, 16); put16(out_, 1); put16(out_, 2); put32(out_, rate);
    put32(out_, rate * 4); put16(out_, 4); put16(out_, 16);
    out_.write("data", 4); put32(out_, bytes);
    if (!out_) throw std::runtime_error("WAV header write failed");
}
void WavStreamWriter::write(std::span<const float> stereo) {
    if (finished_ || stereo.size() % 2 || stereo.size() / 2 > total_ - written_)
        throw std::invalid_argument("Invalid WAV block or exceeded declared frame count");
    std::array<char, 8192> bytes{};
    size_t used = 0;
    for (const auto f : stereo) {
        const auto sample = static_cast<int16_t>(std::lrint((std::isfinite(f) ? std::clamp(f, -1.0f, 1.0f) : 0) * 32767.0f));
        const auto bits = std::bit_cast<uint16_t>(sample);
        bytes[used++] = static_cast<char>(bits & 255); bytes[used++] = static_cast<char>(bits >> 8);
        if (used == bytes.size()) { out_.write(bytes.data(), used); used = 0; }
    }
    if (used) out_.write(bytes.data(), static_cast<std::streamsize>(used));
    if (!out_) throw std::runtime_error("WAV audio write failed");
    written_ += stereo.size() / 2;
}
void WavStreamWriter::finish() {
    if (finished_ || written_ != total_) throw std::logic_error("Incomplete WAV export");
    out_.flush();
    if (!out_) throw std::runtime_error("WAV flush failed");
    out_.close();
    if (!out_) throw std::runtime_error("WAV close failed");
    finished_ = true;
}
}
