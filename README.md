# OpenDAW — interactive foundation, 0.2.0 source candidate

This increment retains the uploaded C++20 audio graph/DSP and adds a native engine
service, bounded transport/live-note control, sampling, streaming WAV export,
a C ABI, and a Rust/Slint application implementation.

**M3.2 local increment — 22 October 2026:** multitrack arrangement with fixed
headers, clip previews, drag reorder and a collapsible inspector. Mute/solo work
live and in export, preserve fader values, and participate in save/undo/redo.
Multiple solo tracks are cumulative; mute takes precedence. The app reads v1 and
v2, saving v2 with a v1 migration checkpoint. The original C ABI remains compatible
through additive mixer APIs; dependencies are unchanged. Validation: 24 headless
and 33 UI-enabled Rust tests, four native suites, software UI at 100/200%,
ASan/UBSan and focused TSan. Physical audio and Windows still need target-host tests.
Clip manipulation, vertical keyboard, expanded synth and drums remain later M3 steps.
See [M3 progress, validation and usage](docs/M3_IMPLEMENTATION_PROGRESS.md).

**macOS update — 22 October 2026:** the Slint window sizing declaration is fixed,
and both `scripts/validate_app.sh` and `scripts/validate_native.sh` now pass on
macOS 15.6.1 / Apple Silicon. `app/Cargo.lock` is included in local Git history.
The release UI also survived a five-second startup probe with `--silent`.
Physical audio and GUI interactions remain untested. See the
[local macOS validation record](docs/BUILD.md#local-macos-validation--22-october-2026).
The delivery-environment statements below describe the original package.

**Validation status matters:** the C++ implementation was built and tested on
Linux x86_64 with GCC and Clang, ASan/UBSan, and a focused ThreadSanitizer run.
**The Rust/Slint application was not compiled or run** because Rust and package
network access were unavailable in the authoring environment. macOS/CoreAudio and
Windows/WASAPI adapters are implemented as source but were not compiled or tested
on those platforms. No physical audio latency or graphical performance claims
are made. This is not a finished or shipping DAW.

## Run the verified native path

Requires a C++20 compiler and CMake 3.20 or newer; no downloaded C++ dependencies.
From this directory on Linux/macOS:

```sh
./scripts/validate_native.sh
./build/release/daw_session --render demo.wav --seconds 8
./build/release/daw_session --interactive --silent
```

`--silent` is explicitly a **no-audio simulation clock**, not an output fallback.
On macOS/Windows, after validating the platform build and device:

```sh
./build/release/daw_session --play --seconds 8
```

Windows PowerShell: `./scripts/validate_native.ps1`; the default Visual Studio
build places the executable in `build/native/Release/daw_session.exe`.

Interactive commands: `play`, `pause`, `stop`, `seek 2`, `loop 0 4`, `unloop`,
`on 60`, `off 60`, `gain 0 0.5`, `pan 0 -0.2`, `status`, `quit`.
The CLI's default example has two instrument tracks. `--sample short.wav` replaces
its lead instrument with the sampler.

## Validate and launch the application candidate

Requires Rust stable, CMake, a native C++ toolchain, dependency download access,
and platform GUI development libraries. See [BUILD.md](docs/BUILD.md).
The frontend is delivered as unvalidated source, not a prebuilt application.

```sh
./scripts/validate_app.sh
cd app
cargo run --locked --release --bin opendaw
# On Linux, native audio is not implemented in this increment:
cargo run --locked --release --bin opendaw -- --silent
```

There is deliberately **no fabricated Cargo.lock**. The validation script resolves
it on a connected host. Preserve and commit the lockfile only after validation;
the supplied source is not a reproducible locked Rust release yet.

## Implemented boundaries

- C++: `EngineService`, per-block sequencing, transport, note IDs, bounded controls,
  fixed-voice sine/sampler instruments, device ownership, offline export.
- C ABI: opaque engine handle, checked arguments, explicit lifetime and single
  control-thread contract. Audio does not call back into Rust or the UI.
- Rust source: versioned project document, command/inverse undo, independent editor
  interaction model, asset-copying persistence, native wrapper and controller.
- Slint source: transport, selected-track piano roll, mixer controls, note editing,
  audition pads, sample loading, project save/open, and export controls.
- Tests and evidence: original eight core checks, fourteen service checks, a pure C
  client, allocation probe, independent Python shared-library exercise, raw logs.

Structural score edits stop audio. Live gain/pan controls do not rebuild the graph.
This is intentional: stop/prepare/restart is the initial safe lifecycle, not live
lock-free topology swapping. The UI uses visible note primitives, not a custom
WGPU renderer. View construction still scans the selected track; it is not a
finished large-project performance solution.

## Important limits

Native output is provided as **unvalidated source for macOS and Windows**. Linux
currently supports offline processing and explicit silent-clock testing only.
Seek/loop boundaries reset voices; there is no sustained-note chasing. Project end
cuts tails. Sampling uses linear interpolation, not high-quality time-stretching.
The project model currently has constant tempo and note tracks, not audio clips.

Not implemented: recording, long-file streaming, external MIDI-device input,
CLAP/VST3/AU hosting or scanning, arbitrary bus/send routing in the service,
latency compensation, automation lanes, mobile/browser packaging, automatic crash
recovery, or native plugin-window embedding.

## Documentation

- [Implementation status and handoff](docs/IMPLEMENTATION_STATUS.md)
- [Architecture and ownership](docs/ARCHITECTURE.md)
- [Build and validation instructions](docs/BUILD.md)
- [Executed test report and limits](docs/TEST_REPORT.md)
- [Project format](docs/PROJECT_FORMAT.md)
- [Next acceptance gates](docs/NEXT_STEPS.md)
- [Dependency notices](THIRD_PARTY_NOTICES.md)

Original milestone documents are preserved under `docs/legacy/`; they describe
the historical starter, not the current completion status. The authored code
retains the original MIT license. Third-party dependencies have their own terms.
