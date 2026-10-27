# Build, run, and validation

## M3.6 local verification — 23 October 2026

Implementation `c4a8242`, baseline M3.5 `50de0cf`; same macOS arm64 toolchain and
locked dependencies. Capacity is 64 throughout the model, UI, bridge and engine.
Current gates: 45 headless / 64 UI-enabled Rust tests (including integrations),
release binaries, eight native suites on macOS (seven without the Mac interposer),
shared C ABI, callback allocation/lock audit, sanitizer reruns, 75 FFT cases,
100,000-note/64-track software UI stress and byte-identical v1 audio.
Memory is checked over 300 rebuild/edit/export cycles plus a separate macOS leaks
run. The [M3.6 report](M3_6_64_TRACKS_VALIDATION.md) records exact results and limits.

Normal builds contain no render profiling clocks. To build the separate probe:

```sh
cmake -S . -B build/m36-measure -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_TESTING=OFF -DOD_MEASURE_RENDER=ON
cmake --build build/m36-measure --config Release --target performance_probe --parallel 4
# macOS default output; measured internal DSP blocks, no hardware-buffer change:
caffeinate -i build/m36-measure/performance_probe 32 600 --native > /tmp/m36-32.jsonl
python3 scripts/verify_m36_performance.py --require-coreaudio /tmp/m36-32.jsonl
build/m36-measure/performance_probe 64 60 --native
build/m36-measure/performance_probe --lifecycle
```

Keep the host awake and avoid other stress tests during measurement. The strict
32-track gate checks a continuous 600-second window, eight voices per synth,
48 kHz/256-frame internal blocks, p99 <3.2 ms and no DSP block over budget;
CoreAudio mode also requires zero reported processor-overload notifications.
This does not certify a physical 256-frame I/O buffer, listening or every possible
driver underrun. Windows uses the Release/performance_probe.exe target without
caffeinate or the CoreAudio-only flag. CI compiles this target on its OS matrix;
no remote CI run or Windows/Intel validation is claimed here.
The dated records below retain their original results.

## M3.5 local verification — 23 October 2026

Updated checkout baseline `ebc0e83`; same macOS arm64 toolchain as M3.4. Added
sha2 0.10.9 with six locked transitive packages, without upgrading existing ones.
The 12 original CC0 WAVs under assets/drums/vcsl are compile-time inputs to both
binaries; copy the whole source tree for builds. Runtime factory playback needs
no network or path outside the binary. `python3 scripts/verify_factory_drums.py`
is a dependency-free offline hash/license/WAV gate; `--upstream` additionally
checks official fixed-revision sources via curl and normal TLS validation.

Local gates: 36 headless unit tests + six integration, 55 UI-enabled + the same
six integration, both release binaries, six native suites, extended C/shared ABI,
zero C++ new/delete over 10,001 monitored renders, real-controller software UI
100/200%, v1 byte-identical output. CI adds the offline factory verification to
its existing OS build/test matrix; CI has not been pushed/run for this increment.
Detailed commands, benchmark and target-hardware/listening limits:
[M3.5 validation record](M3_5_DRUM_MACHINE.md).


## M3.4 increment — 23 October 2026

Current gates: 36 headless / 52 UI-enabled Rust tests and both release binaries;
five native CTest suites, shared ABI integration, ASan/UBSan, focused TSan,
10,001-render C++ allocation probe and the real-controller software UI probe.
75 coherent FFT cases pass the −50 dBr / 20 dB improvement gates, including 7040 Hz.
The v1 audio fixture is byte-identical. See [M3.4 report](M3_4_TRACK_SYNTH.md) for
commands, versioned ABI, presets, DSP choices, performance and `evidence/m34-*`.
Physical audio/listening, Windows and macOS Intel remain untested. No Rust lockfile
updates; NumPy is only used by the development/CI spectral gate.

## M3.3 increment — 23 October 2026

Historical M3.3 gates: 31 headless / 45 UI-enabled Rust tests, both release binaries,
v1 validate/render and four native suites. The real-controller software probe adds
clip CRUD/drag/resize, vertical keyboard, notation, velocity and release on window
exit/focus loss. Three vertical zooms and 100/200% layouts were rendered and
inspected. The v1 WAV remains byte-identical. See [M3.3 report](M3_3_CLIPS_PIANO_ROLL.md)
and `evidence/m33-*`. Native C++ is unchanged; the sanitizer results below describe
M3.2. Physical audio, Windows and macOS Intel remain untested.

## M3.2 increment — 22 October 2026

Historical M3.2 gates: 24 headless / 33 UI-enabled Rust tests, release binaries, v1
migration/render, four native suites (18 service checks) and shared-library checks.
The software probe uses the real silent controller at 1280×820, 1000×760 and
1000×760 logical at 200%; covers mixer, inspector, selection, scrolling and drag
reorder. ASan/UBSan and separate focused TSan pass. Apple LeakSanitizer is not
supported here: use `ASAN_OPTIONS=detect_leaks=0:halt_on_error=1` on this host.
Logs and snapshots: `evidence/m32-*`; see [M3 progress](M3_IMPLEMENTATION_PROGRESS.md)
for scope and limitations. Physical audio, Windows and macOS Intel are untested.
Historical records below retain their original counts.

## Local macOS validation — 22 October 2026

Validated in `/Users/giorgiofausto.canu/Downloads/OpenDAW_Interactive` on macOS
15.6.1, arm64, with Rust/Cargo 1.99.1, CMake 4.4.4, AppleClang 17 and Xcode
Command Line Tools. The package under `app/` is the Cargo workspace root and
contains the `opendaw-app` package, `opendaw` UI and `opendaw-project` CLI.
Slint and slint-build are both locked to 1.18.1; their versions and the existing
lockfile did not need changing.

The original `scripts/validate_app.sh` exited with status 101 in `cargo test
--locked`, when the Slint build step reported:

```text
error: Cannot specify both 'height' and 'min-height'
error: Cannot specify both 'width' and 'min-width'
```

`app/ui/main.slint` specified fixed `width`/`height` together with minimum sizes.
It now uses `preferred-width: 1280px` and `preferred-height: 820px`, preserving
the minimum sizes and allowing resizing, as described in the
[Slint Window documentation](https://docs.slint.dev/latest/docs/slint/reference/window/window/).
No changes to the validation script, Cargo dependencies, build script or C++
backend were necessary. Backend file hashes were checked after validation.

Executed checks:

- `bash scripts/validate_app.sh`: exit 0; all 11 Rust tests passed with UI disabled
  and again with UI enabled; both release binaries built; the example project
  validated (2 tracks, 10 notes) and rendered to `app/target/project-demo.wav`.
- `bash scripts/validate_native.sh build/macos-validation`: exit 0; all four CTest
  suites, WAV render and Python shared-library checks passed.
- `app/target/release/opendaw --silent`: the process remained running for five
  seconds and was then deliberately terminated. This was a startup probe only.

Logs are in `evidence/macos-app-validation-before.log`,
`evidence/macos-app-validation-after.log`, `evidence/macos-native-validation.log`
and `evidence/macos-ui-startup.log`. The `.gitignore` excludes build directories,
generated test audio and local backups; the validated Cargo.lock is tracked.
`MANIFEST.sha256`, `provenance.json` and older evidence describe the original
delivery and have been retained as historical records.

To validate and launch from the project root:

```sh
./scripts/validate_app.sh
cargo run --manifest-path app/Cargo.toml --locked --release --bin opendaw
```

Use `-- --silent` at the end of the launch command for a no-audio clock.
GUI interaction, physical audio playback, device changes, Intel macOS,
Windows/Linux builds and hosted CI were not tested in this local run.
The remaining sections retain the original delivery instructions and limits.

### Toolbar visibility and About attribution follow-up

A screenshot of the running app exposed a layout error that the startup probe
could not detect: the transport header and editing toolbar had no vertical
position binding, so Slint centered them behind the piano roll. Both now have
explicit top-left positions. Play, Pause, Stop and Loop appear beside the OpenDAW
logo, with Undo/Redo and view controls in the row above the ruler.

The sidebar's AboutSlint widget has moved into Help → About OpenDAW. This retains
the widget in a dialog accessible from the top-level menu, an attribution option
in [Slint's royalty-free license](https://slint.dev/agreements/slint-royalty-free-license.pdf).
On macOS, Help is in the system menu bar while the app is active. The dialog uses
the standard widget palette so its attribution remains readable in light or dark
mode.

The full application validation script passed again. A separate software-rendered
probe also checked that Play and Undo receive pointer clicks at 1280×820 and
1000×760, Help opens the About dialog, the modal blocks the transport, and Close
dismisses it. This does not test the native macOS menu or physical audio output.
The probe is guarded by the UI feature so headless tests still work:

```sh
cd app
cargo run --locked --example layout_probe
```

It writes PPM images under `evidence/`. PNG snapshots, the probe's output and the
full validation log are saved there as `macos-toolbar-1280.png`,
`macos-toolbar-1000.png`, `macos-about.png`, `macos-layout-probe.log` and
`macos-toolbar-validation.log`. Save unsaved edits, close the running app and
relaunch the rebuilt binary to load the new interface.

## Native C++ — independently executable

CMake >=3.20 and a C++20 compiler. Tested here: Linux x86_64, GCC 14.2, Clang 17,
CMake 3.31.6. No downloaded native third-party libraries are required.

```sh
cmake -S . -B build/release -DCMAKE_BUILD_TYPE=Release
cmake --build build/release --parallel 4
ctest --test-dir build/release --output-on-failure
./build/release/daw_session --render demo.wav --seconds 8
./build/release/daw_session --interactive --silent
```

`daw_demo` is the historical starter executable. `daw_session` uses the new service.
The shared library is `libopen_daw_api.so` (Linux), `libopen_daw_api.dylib` (macOS),
or `open_daw_api.dll` (Windows). The public C contract is include/open_daw/api.h.

```sh
python3 scripts/verify_shared_api.py --library build/release/libopen_daw_api.so
```

For sanitizers on a supported native toolchain:

```sh
cmake -S . -B build/asan -DCMAKE_BUILD_TYPE=Debug -DOD_SANITIZE=ON
cmake --build build/asan --parallel 4
ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 \
  ctest --test-dir build/asan --output-on-failure

cmake -S . -B build/tsan -DCMAKE_BUILD_TYPE=Debug -DOD_THREAD_SANITIZE=ON
cmake --build build/tsan --target service_tests --parallel 4
./build/tsan/service_tests
```

ASan/UBSan and TSan must be separate builds. Only the focused service suite was
executed under TSan in this delivery. Hardware/device paths were not exercised.

## macOS native validation gate

Use Xcode Command Line Tools and CMake, then `./scripts/validate_native.sh`.
The CoreAudio implementation targets macOS, not iOS. Test actual default-output
playback with `./build/release/daw_session --play`, repeated stop/start, system
output changes, sample-rate changes, sleep/wake and device removal. DefaultOutput
and client-format configuration may need host-specific correction; source
presence is not a portability certificate.

## Windows native validation gate

Use Visual Studio 2022 C++ tools and CMake. In a developer PowerShell:

```powershell
./scripts/validate_native.ps1
./build/native/Release/daw_session.exe --play --seconds 8
```

The script assumes Visual Studio's multi-configuration layout. WASAPI uses shared,
event-driven output, a dedicated COM-owning worker, optional MMCSS registration,
and explicit shutdown joining. No ASIO, exclusive mode, device picker or input
recording is implemented. Validate on the target machine before relying on it.

## Rust application candidate — not compiled here

Install stable Rust and the native toolchain. Linux builds also need the GUI/system
libraries in .github/workflows/validate.yml. The build script invokes CMake and
links the static native engine. It supports matching host/target desktop builds;
mobile and cross-compilation deliberately fail instead of guessing toolchains.

The first connected build must resolve an actual lockfile:

```sh
./scripts/validate_app.sh
```

Equivalent manual sequence:

```sh
cd app
cargo generate-lockfile
cargo test --locked --no-default-features
cargo test --locked
cargo build --locked --release --bin opendaw --bin opendaw-project
cargo run --locked --release --bin opendaw
```

On Linux use `-- --silent` for the UI because native Linux output is not yet
implemented. No silent fallback is automatic on any platform. To test project
logic without GUI compilation:

```sh
cd app
cargo run --locked --no-default-features --bin opendaw-project -- demo ../MySong.opendaw
cargo run --locked --no-default-features --bin opendaw-project -- validate ../MySong.opendaw
cargo run --locked --no-default-features --bin opendaw-project -- render ../MySong.opendaw ../MySong.wav
```

These are validation instructions, not a claim that the frontend compiled in the
authoring environment. Preserve the resulting Cargo.lock after fixing any build
failures and passing tests on the supported host matrix. Apply `cargo fmt --all`
and review formatting before committing; rustfmt was not available here.

## Application interaction implemented in source

The app starts with the two-track example. Use the CLIPS toolbar or double-click
an empty lane to create a MIDI clip. Drag the body between tracks or its right edge
to resize; clipping existing notes is refused. Clip changes and any automatic
project extension undo together. The piano keyboard shows note names and auditions
the track without recording. Use X/Y zoom, scrollbars, Fit notes, Middle C notation
and the selected-note Velocity control. See the M3.3 report for exact semantics.

The app starts with the two-track example. Select an arrangement header or MIDI clip. The inspector shows its instance and
Master output. Mute/solo work in both headers and inspector; solo is cumulative
and mute takes precedence. Drag `≡` to reorder, or use the inspector arrows.
Show selected reveals the selected header; Inspector toggles the panel. Click
empty piano-roll space to insert, drag to move, drag the right edge to resize,
and right-click to select a note for contextual actions. Shift- or Ctrl/Command-click
toggles notes in a selection; drag a selected note's body to move the group, or use
Delete note to remove it. The velocity control applies to the focused note. Undo/Redo
restores document edits. Fit notes saves the previous
viewport for Restore, separately for each clip; repeated Fit actions keep the
original view. Zoom/pan/octave buttons change the viewport; click the ruler to seek. Audition pads
send live note IDs. Mixer controls update the active engine without structural
rebuild. Note/tempo/instrument/structural history edits deliberately stop playback;
mixer and metadata history can continue live.

Enter a short WAV path, root note and one-shot option to load a sample. Enter a
project **folder** (not project.json) in the Project inspector tab to Save/Open. Saving copies referenced samples
into assets/. Opening is refused while unsaved edits exist. Export writes an
independent PCM16 WAV. Close asks for explicit save/discard; save does not silently
close the window. Export completes before application teardown.

Software-rendered interaction probes include `layout_probe`, `workspace_probe`,
`context_menu_probe`, `midi_view_probe`, and `view_state_probe`. They cover
keyboard track selection and rename, multi-selection, MIDI references and project
view restoration as well as the existing editing workflows. Full native GUI
automation, native file dialogs, packaging, code signing and release installers
are not covered. See `docs/ui-redesign.md` for the latest results and limits.

## CI configuration

The repository includes Linux/macOS/Windows native and frontend jobs plus Linux
ASan/UBSan. The workflow was authored only: it has not been pushed or executed.
Each frontend job resolves a candidate lockfile and uploads it for review. This
must be changed to a shared committed validated lockfile for reproducible releases.
CI compilation cannot replace device/audio or GUI interaction tests.
