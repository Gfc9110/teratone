# Remaining acceptance gates

These are remaining work, not background tasks or promises of completion.

## 1. Prove the application build on the target Mac

Resolve and preserve Cargo.lock; run all Rust tests, compile Slint and launch the
interface. Correct actual compiler/platform findings rather than weakening tests.
Run formatting and lint review. Build and test native CoreAudio on the same Mac.
Check repeated Play/Pause/Stop, failed imports, edit/undo/save/reopen, sample-file
copying, export while playing, and shutdown during queued input. Test sample-path
Unicode, duplicate same-pitch notes, tiny/long notes, resizing and HiDPI.

Gate: an actual usable one-instrument workflow with observed audio and round-trip
project persistence. Source code alone does not satisfy this gate.

## 2. Validate Windows and implement Linux native output

Compile Windows adapters and test WASAPI with multiple devices. Add an ALSA,
PipeWire or portable-device-library adapter on Linux behind OutputDevice rather
than moving device ownership to the GUI. Device selection/change recovery, input
recording and full-duplex synchronization are separate tasks.

Gate: clean build matrix and physical default-device playback, lifecycle stress,
measured underruns/latency with documented hardware/configuration.

## 3. Measure large-project editor behavior

Profile representative note counts and viewport sizes, frame times, memory and
idle redraws. Add a spatial index and incremental scene updates if needed. Compare
Slint renderer choices before introducing a custom GPU adapter. Test the intended
mobile-compatible path separately. Add keyboard navigation and semantic access to
custom-drawn editing content. Do not claim 60/120 Hz from a toolkit selection.

## 4. Extend the DAW model and engine

Add real tracks/clips and arrangements, tempo maps, automation, arbitrary routing,
latency compensation, voice-tail policies, sample asset IDs and waveform caches.
Add long-file read-ahead and recording with bounded writer buffers and explicit
failure recovery. Improve sampler interpolation before calling it production
quality. Live graph publication requires a separate resource-retirement design.

## 5. Add external plugins deliberately

Prototype one CLAP and one VST3 host, including actual native editor-window
integration. Add a scanner helper process before scanning arbitrary plugins in the
main process. Implement state save/load, parameters, buses, transport, latency and
threading contracts. Runtime plugin isolation remains separate from scanner
isolation. Test licenses and packaging requirements before redistribution.

## 6. Mobile/browser and release engineering

Prove the native core build/lifecycle on each platform; use adaptive interfaces,
not a shrunken desktop UI. Add Apple audio-session/Audio Unit and Android lifecycle
integration where required. Browser AudioWorklet/WASM is its own adapter and scope.
Introduce signed installers, dependency notices, reproducible locked builds,
crash recovery and platform-specific reliability tests before release.
