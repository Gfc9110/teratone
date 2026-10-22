# Execution report — 2026-10-20

## Verified in available Linux environment

- CMake Release configured and compiled successfully with GCC 14.2 / C++20.
- `ctest --output-on-failure`: **1 test executable / 8 independently printed checks, all passed**.
  - Graph mixing of two sources.
  - Directed cycle detection.
  - Invalid connection rejection.
  - Sample-accurate note onset and block-size invariance (64/128/256/512 frames).
  - A4/440 Hz frequency check.
  - Polyphony, voice stealing and ADSR stress.
  - Runtime atomic gain/pan smoothing.
  - Demo WAV generation and header validation.
- Built and ran a second Debug configuration using AddressSanitizer and UndefinedBehaviorSanitizer: CTest passed.
- Executed offline demo: `--offline /mnt/data/open_daw_preview.wav --seconds 5 --rate 48000 --block 256`.
- Independently parsed the generated WAV: PCM 16-bit stereo, 48,000 Hz, 240,000 frames = 5 seconds, 960,044 bytes. RMS in first samples was nonzero; no error in file parsing.

## Not verified / incomplete

- WASAPI Windows compilation and real sound device playback. Windows-only backend source is present, but this environment has no Windows SDK/audio endpoint.
- Native cross-platform realtime drivers other than WASAPI.
- Automated proof that the entire realtime callback performs zero allocations; the implementation is designed for this but should be instrumented on Windows.
- Interactive note control, external MIDI devices, runtime graph swap, and multithread graph execution.
- CPU/latency under real Windows playback and long-duration stress.

**Status:** M1 baseline PASS on Linux; M2 graph/mixer PASS on Linux; M2 Windows native realtime VALIDATION PENDING.
