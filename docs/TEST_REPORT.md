# Executed validation — 22 October 2026

## Scope and provenance

All executed checks ran on Linux x86_64 in the authoring container. The input was
Open_DAW_M1_M2_Starter.zip; provenance.json records its checksum. Native source was
extended, not replaced with another engine. Evidence contains raw configure/build/
test output and the observed environment. Build logs contain temporary absolute
paths from this environment, not requirements for the recipient.

## Results actually observed

| Run | Observed result |
|---|---|
| GCC 14.2 Release | 4/4 CTest executables passed |
| Clang 17 Release | 4/4 CTest executables passed |
| GCC Debug ASan + UBSan, leak detection enabled | 4/4 passed; no sanitizer finding in these runs |
| GCC Debug ThreadSanitizer | 14 focused service checks passed; no race report in this run |
| Pure C ABI caller | Compiled as C and linked/executed against the native implementation |
| Allocation probe | 10,001 render calls; intercepted C++ new=0 and delete=0 |
| Python shared-library ABI integration | 8 WAV variants, 25 randomized callback partitions, error/state/export/device checks passed |
| Native silent-clock CLI | Ran explicitly without hardware audio; normal teardown, zero reported clipping |
| Eight-second streaming export | PCM16, stereo, 48 kHz, 384,000 frames; independently parsed |

Four CTest executables are not four individual assertions: the original executable
contains eight checks and the service executable contains fourteen. The remaining
two are the C smoke client and allocation probe. Do not add the repeated compiler/
sanitizer runs and present them as distinct features tested.

## Original checks retained

Original graph behavior, stereo/gain/mixing, note scheduling, oscillator/ADSR,
voice limits, and PCM16 export tests remain in tests/tests.cpp. All eight pass in
each executed CTest configuration.

## Added service checks

1. Constructor and setter gain/pan validation agree.
2. Overlapping same-pitch notes have independent nonzero IDs.
3. Variable processing blocks give invariant output.
4. Transport play/pause/stop and explicit reset-on-seek semantics.
5. Loop boundaries and block partition invariance.
6. Bounded queue rejection and emergency note-off/panic.
7. Live audition without moving stopped musical transport.
8. Session limits and scheduled-event density rejection.
9. Two-thread SPSC ordering under 100,000 queued values.
10. Streaming export, waveform peaks and RIFF size guard.
11. Malformed WAV boundary tests and 100 random malformed-header smoke cases.
12. Sampler playback, pitch/release and engine integration.
13. C ABI failed-load preservation, independent export, consumer exclusion, teardown.
14. Concurrent controls, telemetry and silent-device lifecycle.

The shared-library Python script additionally imports PCM16/24/32 and float32 in
both mono and stereo, rejects a NaN sample without replacing the active session,
compares 25 seeded randomized callback partitions byte-for-byte with a reference,
checks a Unicode export filename and parses WAV metadata, and verifies that a
second headless consumer is rejected while the silent device is open.

## What the results do not prove

- The allocation probe overrides C++ new/delete forms. It does not intercept every
  malloc/free implementation, operating-system facility, plugin or audio driver.
- Sanitizers cover exercised code and schedules. A clean run is not proof that all
  possible memory errors or races are absent.
- There was no physical audio-device access, underrun measurement, round-trip
  latency measurement, sustained hardware stress session, or deadline percentile
  benchmark. SilentClock is a test clock, not a low-latency device benchmark.
- macOS/CoreAudio and Windows/WASAPI code was not compiled or run in this container.
- Rust, Cargo, rustfmt and Slint were unavailable; no Rust test, Slint compilation,
  launch, screenshot, rendering benchmark or application end-to-end test ran.
- The authored Rust tests, CI workflow, save/recovery protocol, UI interactions,
  cross-language Rust wrapper and target packaging still require execution.
- The example JSON was checked as JSON, not validated with the unbuilt Rust CLI.
- No plugins, recording, MIDI hardware, mobile platform or browser was tested.

## Reproduction

Use scripts/validate_native.sh or scripts/validate_native.ps1. Run the separate
ASan/UBSan and TSan commands in BUILD.md. Run verify_shared_api.py against a Release
shared library; loading an ASan library into ordinary Python requires additional
runtime setup and is not the procedure used here. Run validate_app.sh on a connected
host to begin the pending frontend validation gate.

The eight-second WAV is delivered separately as OpenDAW_Demo.wav; its checksum and
metadata are in evidence/demo-metadata.json. It demonstrates the native renderer,
not the Slint application or a hardware capture.

## Source-package check

A preliminary source archive was extracted into a new directory and configured,
built and tested with GCC Release from that extracted tree. All four CTest
executables passed. The final archive adds these package logs; no native or
frontend implementation source changed after this clean build.
