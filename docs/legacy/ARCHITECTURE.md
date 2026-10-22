# Audio engine architecture — M1/M2

- **Runtime:** C++20 with CMake; audio engine has no UI/game engine dependency. Future Godot app can connect through GDExtension without placing Godot on the audio thread.
- **Audio data:** float32 planar stereo (`AudioBlock`). Backend handles format conversion/interleaving.
- **Time:** absolute frame counter (`uint64_t`). Synth events are sorted before playback and processed at their exact frame even when changing the DSP block length.
- **Graph:** `Graph` owns `AudioNode`s. Before playback, `prepare()` validates connections, rejects cycles, sorts dependencies, allocates buffers and resolves input pointers. `process()` only clears preallocated buffers and runs nodes in topological order. Graph mutation while prepared is prohibited.
- **Synth:** 24 voices, sine oscillator, note velocity, ADSR, deterministic voice stealing. `NoteEvent` is offline-scheduled for M1/M2; a realtime lock-free MIDI event ring and transport controller are TODO.
- **Mixer:** `GainPanNode` atomic target parameters with 5 ms smoothing, equal-power panning. `MixerNode` adds multiple audio inputs. Master has no limiter, and can exceed ±1 depending on graph; WAV export clamps, while raw float output preserves overs.
- **Device:** `wasapi.cpp` demonstrates shared event-driven output; event signals request filling native buffers, potentially broken into multiple DSP chunks <= configured `maxFrames`. Graph is sample-rate-matched to device; float32/PCM16 output formats only; no guaranteed requested buffer size. Requires real Windows validation.
- **Threading:** graph compilation and changes of node topology off audio thread. Atomic gain/pan targets can be changed by another thread. Playback-time MIDI queue, graph hot swapping and safe deferred destruction of nodes are deferred, not secretly claimed supported.
- **Sample-based instruments:** a future `SampleAsset` contains decoded immutable audio and metadata. A `SamplePlayerNode` keeps fractional read position and interpolates between samples. A `DrumSamplerNode` maps note numbers to pads/one-shots; a `ChromaticSamplerNode` maps pitches around root notes. Import/decoding happens outside audio callback; long clips will need streaming buffers.
- **Headless:** `renderOffline` executes exactly the same graph as native output. Deterministic unit tests can run without sound hardware.

## Known limitations of this starter

1. Realtime WASAPI has not been built or played in the Linux execution environment.
2. Sine synthesis uses direct `std::sin`; additional oscillators, modulation and anti-aliasing are future DSP work.
3. Scheduled MIDI events require a fixed sequence; MIDI input, control commands and transport are future work.
4. Runtime sample rate changes, device hot swap and streaming media are not implemented.
5. WASAPI shared-mode can negotiate formats/channels not supported by this simple backend.
6. Full graph swap/reconfiguration during playback is not supported.
7. Input/output realtime latency and device underruns still need measurement on Windows.
