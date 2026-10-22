#include "open_daw/engine.hpp"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <exception>
#include <iostream>
#include <string>
#include <thread>

#ifdef _WIN32
namespace daw::wasapi {
uint32_t defaultSampleRate();
void play(Graph& graph, double seconds);
}
#endif

namespace {
void usage() {
    std::cout << "Open DAW custom audio core demo\n"
              << "  daw_demo --offline <file.wav> [--seconds N] [--rate 48000] [--block 256]\n"
              << "  daw_demo --realtime [--seconds N]  (Windows WASAPI)\n";
}
}

int main(int argc, char** argv) {
    try {
        if (argc < 2) { usage(); return 0; }
        std::string mode;
        std::string filename = "demo.wav";
        double seconds = 5.0;
        uint32_t rate = 48000;
        uint32_t block = 256;
        for (int i = 1; i < argc; ++i) {
            const std::string arg = argv[i];
            if (arg == "--offline") {
                mode = "offline";
                if (i + 1 < argc && argv[i+1][0] != '-') filename = argv[++i];
            } else if (arg == "--realtime") {
                mode = "realtime";
            } else if (arg == "--seconds" && i + 1 < argc) {
                seconds = std::stod(argv[++i]);
            } else if (arg == "--rate" && i + 1 < argc) {
                rate = static_cast<uint32_t>(std::stoul(argv[++i]));
            } else if (arg == "--block" && i + 1 < argc) {
                block = static_cast<uint32_t>(std::stoul(argv[++i]));
            } else if (arg == "--help" || arg == "-h") {
                usage(); return 0;
            } else {
                throw std::invalid_argument("Unknown argument or missing value: " + arg);
            }
        }
        if (seconds <= 0.0 || seconds > 120.0 || !std::isfinite(seconds) ||
            rate < 8000 || rate > 384000 || block < 16 || block > 8192) {
            throw std::invalid_argument("Invalid time, sample rate or block size");
        }

        if (mode == "offline") {
            auto demo = daw::makeDemo(rate, block);
            const auto frames = static_cast<uint64_t>(std::llround(seconds * rate));
            const auto started = std::chrono::steady_clock::now();
            auto audio = daw::renderOffline(demo.graph, frames, block);
            daw::writeWavPcm16(filename.c_str(), audio, rate);
            const auto spent = std::chrono::duration<double>(
                std::chrono::steady_clock::now() - started).count();
            const auto peak = std::max_element(audio.begin(), audio.end(),
                [](float a, float b){ return std::abs(a) < std::abs(b); });
            std::cout << "Written: " << filename << " (" << frames << " frames, "
                      << rate << " Hz, block " << block << ", peak "
                      << (peak == audio.end() ? 0.0f : std::abs(*peak))
                      << ", render time " << spent << "s)\n";
        } else if (mode == "realtime") {
#ifdef _WIN32
            const auto deviceRate = daw::wasapi::defaultSampleRate();
            auto demo = daw::makeDemo(deviceRate, 512);
            std::cout << "WASAPI playback (default device @ " << deviceRate
                      << " Hz), duration " << seconds << " seconds\n";
            std::atomic<bool> active = true;
            std::thread controller([&]() {
                using namespace std::chrono_literals;
                int count = 0;
                while (active.load(std::memory_order_relaxed)) {
                    std::this_thread::sleep_for(125ms);
                    // Demonstrate safe control-thread edits to atomic parameters.
                    if (++count == 16) {
                        demo.leftGain->setPan(0.5f);
                        demo.rightGain->setPan(-0.5f);
                    }
                    if (count == 32) {
                        demo.leftGain->setGain(0.4f);
                    }
                }
            });
            try {
                daw::wasapi::play(demo.graph, seconds);
            } catch (...) {
                active.store(false);
                controller.join();
                throw;
            }
            active.store(false);
            controller.join();
#else
            throw std::runtime_error("Realtime WASAPI is only available on Windows in this prototype");
#endif
        } else {
            usage(); return 2;
        }
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "ERROR: " << e.what() << '\n';
        return 1;
    }
}
