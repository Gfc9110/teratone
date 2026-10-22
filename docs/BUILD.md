# Build, run, and validation

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

The app starts with the two-track example. Select a track in the sidebar. Click
empty piano-roll space to insert, drag to move, drag the right edge to resize,
right-click or use Delete note to remove. Undo/Redo restores document edits.
Zoom/pan/octave buttons change the viewport; click the ruler to seek. Audition pads
send live note IDs. Mixer controls update the active engine without structural
rebuild. Note/tempo/instrument/history edits deliberately stop playback.

Enter a short WAV path, root note and one-shot option to load a sample. Enter a
project **folder** (not project.json) to Save/Open. Saving copies referenced samples
into assets/. Opening is refused while unsaved edits exist. Export writes an
independent PCM16 WAV. Close asks for explicit save/discard; save does not silently
close the window. Export completes before application teardown.

File dialogs, shortcuts, multi-selection, automated GUI tests, native packaging,
code signing and release installers are not included.

## CI configuration

The repository includes Linux/macOS/Windows native and frontend jobs plus Linux
ASan/UBSan. The workflow was authored only: it has not been pushed or executed.
Each frontend job resolves a candidate lockfile and uploads it for review. This
must be changed to a shared committed validated lockfile for reproducible releases.
CI compilation cannot replace device/audio or GUI interaction tests.
