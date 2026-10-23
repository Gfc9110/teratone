#include "open_daw/path.hpp"
#include "open_daw/service.hpp"
#include "open_daw/device.hpp"
#include <chrono>
#include <cmath>
#include <iostream>
#include <sstream>
#include <thread>

int main(int argc, char** argv) {
    try {
        bool play = false, silent = false, interactive = false;
        std::string output, sample;
        double seconds = 8;
        for (int i = 1; i < argc; ++i) {
            const std::string arg = argv[i];
            if (arg == "--render" && i + 1 < argc) output = argv[++i];
            else if (arg == "--sample" && i + 1 < argc) sample = argv[++i];
            else if (arg == "--seconds" && i + 1 < argc) seconds = std::stod(argv[++i]);
            else if (arg == "--play") play = true;
            else if (arg == "--silent") silent = true;
            else if (arg == "--interactive") interactive = true;
            else throw std::invalid_argument("Unknown/incomplete option: " + arg);
        }
        if (!std::isfinite(seconds) || seconds <= 0 || seconds > 3600) throw std::invalid_argument("Seconds must be in (0,3600]");
        if (output.empty() && !play && !interactive) {
            std::cout << "OpenDAW interactive engine 0.2\n"
                         "  daw_session --render demo.wav [--seconds 8] [--sample short.wav]\n"
                         "  daw_session --play [--silent] [--seconds 8]\n"
                         "  daw_session --interactive [--silent] [--sample short.wav]\n"
                         "Native playback: macOS/CoreAudio, Windows/WASAPI source paths.\n"
                         "--silent runs a NO-AUDIO simulation clock on any supported desktop.\n";
            return 0;
        }
        daw::SessionSpec spec;
        spec.lengthFrames = static_cast<uint64_t>(seconds * spec.sampleRate);
        spec.tracks.resize(2);
        spec.tracks[0].gain = 0.7f; spec.tracks[0].pan = -0.2f;
        spec.tracks[1].gain = 0.5f; spec.tracks[1].pan = 0.2f;
        if (!sample.empty()) spec.tracks[0].sample = daw::loadWav(daw::utf8Path(sample));
        constexpr uint8_t melody[]{60, 64, 67, 72, 67, 64, 62, 67};
        uint64_t id = 1;
        for (uint64_t beat = 0; beat * 24000 < spec.lengthFrames; ++beat) {
            const auto start = beat * 24000;
            spec.notes.push_back({id++, start, std::min(start + 18000, spec.lengthFrames), 0, melody[beat % 8], 100});
            if (beat % 4 == 0) spec.notes.push_back({id++, start, std::min(start + 84000, spec.lengthFrames), 1,
                static_cast<uint8_t>(beat % 8 ? 43 : 48), 85});
        }
        if (!output.empty()) {
            daw::renderSessionToWav(spec, daw::utf8Path(output), spec.lengthFrames);
            std::cout << "Exported " << spec.lengthFrames << " stereo frames to " << output << '\n';
        }
        if (play || interactive) {
            daw::EngineService engine(spec);
            daw::OutputDevice device;
            device.start(engine, silent ? daw::DeviceMode::SilentClock : daw::DeviceMode::Native);
            std::cout << (silent ? "SILENT simulation: no hardware output\n" : "Native output opened\n");
            if (play) {
                engine.submit({daw::CommandType::Play});
                std::this_thread::sleep_for(std::chrono::duration<double>(seconds + 0.1));
            }
            if (interactive) {
                std::cout << "Commands: play, pause, stop, seek SECONDS, loop START END, unloop,\n"
                             "on NOTE, off NOTE, gain TRACK VALUE, pan TRACK VALUE, status, quit\n";
                for (std::string line; std::cout << "> " && std::getline(std::cin, line);) {
                    std::istringstream in(line); std::string word; in >> word;
                    daw::Command command;
                    bool send = true;
                    if (word == "quit") break;
                    if (word == "play") command.type = daw::CommandType::Play;
                    else if (word == "pause") command.type = daw::CommandType::Pause;
                    else if (word == "stop") { engine.stop(); send = false; }
                    else if (word == "seek") {
                        double where = -1; in >> where;
                        if (!in || !std::isfinite(where) || where < 0 || where > seconds) { std::cout << "Invalid seek\n"; continue; }
                        command.type = daw::CommandType::Seek; command.a = static_cast<uint64_t>(where * spec.sampleRate);
                    } else if (word == "loop" || word == "unloop") {
                        command.type = daw::CommandType::Loop; command.enabled = word == "loop";
                        if (command.enabled) {
                            double a = -1, b = -1; in >> a >> b;
                            if (!in || !std::isfinite(a) || !std::isfinite(b) || a < 0 || b <= a || b > seconds) { std::cout << "Invalid loop\n"; continue; }
                            command.a = static_cast<uint64_t>(a * spec.sampleRate); command.b = static_cast<uint64_t>(b * spec.sampleRate);
                        }
                    } else if (word == "on" || word == "off") {
                        int note = -1; in >> note;
                        if (!in || note < 0 || note > 127) { std::cout << "Invalid note\n"; continue; }
                        command.type = word == "on" ? daw::CommandType::NoteOn : daw::CommandType::NoteOff;
                        command.pitch = static_cast<uint8_t>(note); command.velocity = 100;
                        command.id = daw::LiveIdBit | static_cast<uint64_t>(note + 1);
                    } else if (word == "gain" || word == "pan") {
                        uint32_t track = 0; float value = 0; in >> track >> value;
                        if (!in || !(word == "gain" ? engine.setGain(track, value) : engine.setPan(track, value))) std::cout << "Invalid control\n";
                        send = false;
                    } else if (word == "status") {
                        const auto t = engine.telemetry();
                        std::cout << "frame=" << t.position << " playing=" << t.playing << " blocks=" << t.blocks
                                  << " peak=" << t.peakLeft << "," << t.peakRight << " rejected=" << t.rejectedCommands
                                  << " device_error=" << device.errorCode() << '\n'; send = false;
                    } else { std::cout << "Unknown command\n"; send = false; }
                    if (send && !engine.submit(command)) std::cout << "Command rejected\n";
                }
            }
            device.stop();
            const auto t = engine.telemetry();
            std::cout << "Processed " << t.blocks << " blocks; clipped samples=" << t.clippedSamples << '\n';
            if (device.errorCode()) throw std::runtime_error("Native output reported an error");
        }
        return 0;
    } catch (const std::exception& e) { std::cerr << "OpenDAW: " << e.what() << '\n'; return 1; }
}
