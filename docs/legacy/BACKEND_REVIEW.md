# OpenDAW backend review and revised architecture

**Reviewed:** 22 October 2026  
**Input:** `Open_DAW_M1_M2_Starter.zip`  
**Input SHA-256:** `4b6097308efbf623d790f41398ede6c5bc6be237f0c99e321f56141cfac03a2c`

## Executive decision

Keep the existing C++20 backend. It is a small, genuinely headless audio-engine prototype, not a Rust engine and not a Godot application. Its graph/DSP separation is worth preserving. No production source files were changed during this review.

For the stated desktop-plus-mobile direction, the recommended application architecture is **Rust application/document layer + Slint UI + a narrow native C interface + the existing C++20 engine**. This is a proposed integration, not something already implemented or benchmarked. A C++/Slint desktop application is a simpler alternative, but current Slint documentation limits its iOS support to Rust and describes WGPU low-level access as inaccessible from its C++ API. A Rust frontend avoids choosing that unsupported C++ platform path, but does not prove that the complete DAW or custom renderer already works on iOS. Qt Quick/C++ remains an alternative if Slint's rendering or native-window integration fails the prototype gates.

Do not rewrite the DSP in Rust, introduce CPAL just because the frontend uses Rust, or route audio buffers through GUI callbacks. Retain CMake/CTest for the native engine, and use Cargo for the proposed application layer.

## 1. What was actually inspected

All 13 archive entries were inspected, including the build configuration, MIT license, README, architecture/handoff/test documents, engine header, all four implementation files, and the existing test program. Extracted source files were compared byte-for-byte with the archive after testing; none were changed.

| Component | Implemented state | Source reference |
|---|---|---|
| Headless library | C++20 static library with engine and WAV writer | `CMakeLists.txt:1-8` |
| Audio buffers | Float32 planar stereo vectors | `include/open_daw/engine.hpp:27-43` |
| Graph | Dependency validation, cycle rejection, topological order, prepared buffers/input pointers | `src/engine.cpp:222-285` |
| Synth | Sine synthesis, ADSR, 24 voices, deterministic stealing, scheduled note events | `include/open_daw/engine.hpp:45-84`; `src/engine.cpp:24-170` |
| Mixer | Gain/pan atomics, smoothing, additive mixing | `src/engine.cpp:173-220` |
| Offline processing | Same `Graph::process()` path, returned as a complete interleaved vector | `src/engine.cpp:288-307` |
| WAV output | PCM16, sample clamping, RIFF output | `src/wav.cpp:20-47` |
| Device output | Windows WASAPI shared/event-driven demonstration; compiled only on Windows | `CMakeLists.txt:17-20`; `src/wasapi.cpp:97-190` |
| Godot | Historical design intention, no implementation dependency | `docs/HANDOFF.md:3-10`; `CMakeLists.txt` |

Not implemented in this archive: live MIDI queues/input, play/stop/seek/loop transport, editable session/document model, undo/redo, sample import, recording, samplers, plugins, waveform caches, GUI, device recovery, macOS/Linux/mobile native device adapters, or safe live graph replacement.

The existing README correctly calls M2 partial. The code should not be represented as a complete DAW backend or verified cross-platform real-time engine.

## 2. Independent execution results

Environment: Linux x86-64, GCC 14.2.0, CMake 3.31.6.

| Execution | Result | Evidence |
|---|---|---|
| Original Release build | PASS | `results/release-build.log` |
| Original Release CTest | PASS: one CTest executable with eight printed checks | `results/release-tests.log` |
| Debug build with AddressSanitizer and UndefinedBehaviorSanitizer | PASS | `results/sanitize-build.log`, `results/sanitize-tests.log` |
| Five-second offline demo | PASS: independently parsed PCM16 stereo WAV, 48 kHz, 240,000 frames, 960,044 bytes | `results/validation.json`, `results/demo.log` |
| Mixed block-size full demo | PASS: maximum sample difference 0 against the fixed-256-frame render | `results/behavior-probes.log` |
| Added zero-release note-off boundary cases | PASS: 42 combinations | `results/note-off-probes.log` |
| Replaceable C++ new/delete monitoring | 0 allocation and 0 deallocation calls in 1,734 monitored processing blocks / 240,000 frames | `results/allocation-probe.log` |
| Behavioral reproducers under sanitizers | Completed without sanitizer diagnostics | `results/behavior-probes-sanitize.log` |
| Windows compilation / WASAPI hardware playback | NOT EXECUTED | Linux environment; Windows-only source excluded from the local builds |
| Slint frontend / FFI / custom GPU renderer | NOT IMPLEMENTED OR BENCHMARKED | Architecture proposal only |

Mixed blocks were 1, 7, 127, 256, 511, 64, and 3 frames in a repeating pattern, with a shortened final block where necessary. The comparison used the existing two-synth demo over four seconds. The allocation probe used five seconds of simulated audio and gain/pan updates on the same monitored thread.

The added note-off cases used zero release, note-off frames 127, 128, 129, 255, 256, and 257, and block sizes 1, 7, 64, 127, 128, 256, and 512. These do not establish correctness for every ADSR or MIDI case.

**Scope of the allocation result:** this intercepts replaceable C++ `new`/`delete` calls, including array/aligned/nothrow forms, on the monitored thread. It does not intercept all direct C allocations, detect every lock or system call, measure scheduling jitter, prove hardware latency, or validate WASAPI. `std::atomic<float>::is_always_lock_free` was true on this build target; other targets need their own checks.

The behavior probe intentionally reproduces current shortcomings. Its successful exit means those observations were reproduced, not that the shortcomings satisfy final product requirements.

## 3. Findings and required changes

### F1 — Sequencing is currently inside the instrument

`SynthNode` owns a vector of scheduled events and a forward cursor, and consumes all events whose frame is at or before the current sample (`src/engine.cpp:24-29,157-165`). That works for the demonstrated forward render but is not a reusable DAW sequencer.

Move timeline scheduling to an engine-level sequencer. Pass bounded, preallocated, block-relative event views to processors. Introduce explicit note/parameter event types and stable identities. Bulk-load and sort event schedules once rather than sorting the whole vector after every inserted event.

### F2 — A frame argument is not a seek implementation

The oscillator phase, envelope state, and event cursor remain stateful. Passing a new `startFrame` does not reconstruct those states. A fresh synth processed directly at frame 100 differed from a sequential render at the same position by approximately 0.103492 in the focused probe.

This is a documented missing feature, not a failure of the current forward-only contract. Add explicit transport states and seek/loop handling, including event cursor repositioning and a defined note-chase/reset policy. Avoid replaying an arbitrarily large history of overdue events in a real-time callback.

### F3 — Same-pitch notes are not independently addressable

`noteOff(note)` intentionally releases all matching voices (`src/engine.cpp:105-112`). Two note-ons for the same pitch followed by the first note-off left no signal from the second occurrence in the probe.

Define internal note IDs and channel/port identity before building a piano roll and live-MIDI merger. For inputs without note IDs, document a matching policy. This is an explicit prototype behavior, not an accidental implementation error.

### F4 — Gain/pan construction and updates apply different range policies

The constructor checks finiteness but does not clamp ranges; setters clamp gain to [0,2] and pan to [-1,1] (`src/engine.cpp:173-184`).

With a unit source, construction using gain=5/pan=2 produced approximately L=-3.53553, R=3.53553. Applying those same values through the setters and resetting yielded approximately L=0, R=2.

Choose one policy and share validation across construction, project loading, and runtime commands. This is a reproduced consistency defect. The current hardcoded demo uses in-range values.

### F5 — Atomic parameter updates are not sample-accurate automation

Targets are sampled once per processing block (`src/engine.cpp:194-207`). Smoothing is useful for live control, but does not specify a musical automation event's sample offset. Retain atomics for suitable latest-value controls and add timestamped parameter events for automation.

### F6 — Prepared graph lifecycle needs an engine service

Prepared graphs reject adding nodes and changing the master. `reset()` resets processors but does not make topology mutable (`src/engine.cpp:222-285`). There is no remove/reconnect API or live-swap ownership protocol.

Initially stop processing, reconstruct a new graph off the audio thread, prepare, then resume. Later separate a mutable graph description from a compiled processing plan, publish ready plans at block boundaries, and retire old resources outside the callback. Preserve unchanged processor state where possible rather than reconstructing every instrument on every edit.

### F7 — Device output is a demo function, not a reusable service

`wasapi::play(Graph&, seconds)` is a synchronous fixed-duration loop using the default endpoint. Calling it directly from a GUI event handler would block that UI. Its chunking respects `graph.maxFrames()`, which is a useful existing boundary.

Refactor into a device adapter and engine lifecycle: open/configure, start, request stop, stop/join, error reporting, selected device, negotiated format, and telemetry. Keep waits for device readiness in the native output thread, not the UI. Audio processing must not wait for UI work. Validate appropriate Windows audio scheduling and device-recovery behavior on Windows hardware.

The code records callback count and maximum processing duration; it does not provide a validated underrun counter or round-trip latency measurement.

### F8 — Offline output currently scales memory with duration

`renderOffline()` allocates two floats per requested frame (`src/engine.cpp:288-307`). One hour at 48 kHz stereo float32 is 1,382,400,000 bytes of sample storage before other overhead.

Keep this vector-based helper for tests. Add a streaming offline renderer/writer for product export, with cancellation, progress, write-error handling, and an explicit large-file policy. A separate export instance is required for concurrent export; the current helper resets and mutates its graph.

### F9 — Node contract needs carefully scoped evolution

The current interface is a good source/processor/mixer base but lacks a general event bus, named input/output ports, latency/tail reporting, and explicit transport discontinuities. Add these as real features need them; retain stereo for the first working editor. Do not introduce a new public plugin ABI prematurely.

The gain/pan implementation scales the original left and right independently; it does not mix a right-channel signal into the left when panned left. That is usable for the current dual-mono synth, but stereo balance versus stereo panning needs an explicit product decision before stereo file tracks.

## 4. Proposed ownership architecture

```text
Slint UI + timeline view (Rust)
              |
Rust application core
  Project document, edits, undo, save/load, view state
              |
Narrow C ABI and checked Rust wrapper
  Commands/session descriptions -> native service
  Acknowledgements/telemetry     <- native service
              |
C++ EngineService (new)
  Sequencer, transport, graph compiler, resource ownership
              |
Existing C++ Graph + AudioNode processors
              |
Native device adapter / offline renderer
```

There is one main process initially. The application document is authoritative; compiled playback data is derived, not another editable project database.

The native engine owns its callback, DSP objects, sample storage, event buffers, and device lifetime. No Rust/UI callbacks run from its audio thread. The bridge handles control operations, immutable descriptions, and compact snapshots, not a per-sample message stream.

Use opaque engine handles and stable IDs across the boundary. Keep STL objects, references, exceptions, and allocator ownership out of the C ABI. Catch C++ exceptions before they reach Rust; prevent panic unwinding across native boundaries. Validate schemas, limits, and identifiers outside the callback.

All ordinary control commands should be serialized through one producer before an SPSC queue. A live MIDI input path needs its own appropriately designed producer/queue arrangement. Specify overflow policies, especially for note-off and stop/all-notes-off. Telemetry may drop intermediate updates; recording and structural commands must not disappear silently.

### Project structure

```text
native/
  engine/          # Existing code, reorganized incrementally
  engine-service/  # Transport, sequencer, lifecycle
  engine-api/      # C boundary
  platform/        # WASAPI first; other native adapters later
  tests/
app/
  project/         # Rust document and editing commands
  application/     # Session coordination
  engine-bridge/   # Rust native wrapper
  editor/          # Timeline interaction and render scene
  ui/              # Slint presentation
```

These are ownership boundaries, not a requirement to create many empty crates or libraries immediately.

### Rendering

Keep the timeline's hit-testing, selection, snapping, and scene construction separate from its GPU implementation. Draw only visible notes/clips; use waveform level-of-detail and cached geometry. Prototype with Slint's available primitives before making an external renderer compulsory.

A Rust/WGPU integration must use the same device and queue as Slint, and its currently unstable versioned API should be confined to one adapter. The documented iOS path uses Winit/Skia, so a desktop WGPU result does not prove iOS renderer interoperability. Gate this independently on hardware.

## 5. Implementation gates

1. **M2 completion/hardening:** resolve parameter validation, add reusable lifecycle, transport and bounded live-event queue; add targeted tests; compile Windows/MSVC and validate WASAPI on hardware. Record actual format, buffering and callback timings.
2. **Application integration proof:** build/link the C ABI and Rust wrapper, connect a Slint transport and mixer, validate UI/audio independence and shutdown. Prototype custom timeline rendering and native plugin-window handling; include an iOS feasibility build before committing to that target.
3. **First real editor:** project document, one instrument track, note editing, undo, save/reopen, deterministic offline export. Musical edits must reach the engine correctly without sample-rate-dependent project corruption.
4. **Audio assets:** import and share immutable assets, sampler, waveform cache, then streaming and recording. Preserve one engine pipeline.
5. **Plugins and wider platforms:** format adapters and isolated scanning; compatibility tests and optional runtime isolation later. Validate every native device adapter independently.

The next milestone should complete the interactive native-engine contract and prove the UI integration, not rewrite the engine or start with a full DAW interface.

## 6. Research references used for the stack decision

Retrieved on 22 October 2026. These references support framework capability statements, not claims that this project has already integrated those capabilities.

- Slint C++ API and CMake integration: https://docs.slint.dev/latest/docs/cpp/ and https://docs.slint.dev/latest/docs/cpp/cmake/
- Slint mobile language support: https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/general/
- Slint C++ graphics access: https://docs.slint.dev/latest/docs/cpp/api/slint/graphicsapi/
- Slint Rust WGPU integration: https://docs.slint.dev/latest/docs/rust/slint/wgpu_30/
- Slint iOS implementation: https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/
- Slint web implementation: https://docs.slint.dev/latest/docs/slint/guide/platforms/web/
- Microsoft user-mode audio / application thread scheduling: https://learn.microsoft.com/en-us/windows/win32/coreaudio/user-mode-audio-components
- Qt mobile alternatives: https://doc.qt.io/qt-6/ios.html and https://doc.qt.io/qt-6/android.html

## 7. Reproduction

The companion evidence bundle contains the unchanged source, standalone review probes, logs, validation metadata, and `REPRODUCE.sh`. The script targets Linux with CMake and a GCC/Clang-compatible C++20 compiler. No Windows/audio-device validation is implied. Binary executables and build directories are intentionally omitted.
