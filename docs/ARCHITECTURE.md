# Architecture implemented in this increment

## Ownership and dependency direction

```text
Slint / main GUI thread
  -> bounded Rust request channel
Rust controller worker
  -> authoritative Project + inverse-command History
  -> musical-tick-to-frame preparation
  -> checked bridge (one Engine owner, !Send / !Sync)
C ABI / one native control producer
  -> EngineService (prepared graph, schedule, command queue)
  -> OutputDevice (owns/quiesces audio consumer)
C++ audio consumer
  -> block-relative note events -> SynthNode / SamplerNode
  -> GainPanNode -> MixerNode -> bounded stereo output

Offline export: separate worker + separate engine instance + streaming writer.
```

The C++ callback never invokes Rust closures, UI methods, file decoding, or graph
construction. Rust does not receive sample-by-sample audio via the bridge. C++ owns
processing objects and sample buffers. The user-editable project belongs to Rust;
the native SessionSpec is a derived playback description, not a second editor.
The legacy Graph and sine DSP remain independently usable.

## Real-time execution contract

Only one producer submits engine commands and only one consumer renders. The
Rust controller serializes UI requests. Future MIDI/device producers must not be
attached to the existing SPSC queue without separate serialization/queues.

The native queue accepts 1,024 entries, and at most 64 commands are consumed per
prepared block. There are at most 256 scheduled events per track in any prepared
block window, checked before activation. A fixed 320-event per-track buffer
covers scheduled plus live events. Over-dense projects are rejected rather than
silently truncating note-offs. Telemetry uses lock-free atomics on supported
64-bit desktop targets; values form an approximate snapshot, not a transaction.

Stop/panic have a priority atomic path. A failed note-off requests panic rather
than leaving a voice stuck. That emergency path discards pending native commands.
The application additionally cancels its UI backlog on Stop or queue saturation
and displays this fact; it is not a durable command-delivery mechanism.

Gain and pan use smoothed atomic targets read per processing block. They are not
sample-accurate automation. Queue submission acknowledges acceptance, not audible
completion; last-applied serial and telemetry can be used for observation.

Processing accepts variable callback sizes, subdividing into the prepared maximum.
Loop/EOF boundaries split processing without moving scheduled note offsets.
Non-finite DSP output is replaced with silence; output is clamped to [-1,1], with
clipping counters and raw peak telemetry. This is not a mastering limiter.

The allocation probe measures overridden C++ new/delete operations only. It does
not prove absence of every malloc, driver call, page fault, or scheduler delay.

## Transport policy

- Play starts at the current position, or restarts from zero after EOF.
- Pause keeps position but resets voices. Resume does not chase earlier notes.
- Stop silences and returns to zero; pending commands are discarded.
- Seek resets DSP/event cursors. Notes beginning before the new position are not
  reconstructed. The next note starting at that position can play.
- Loops require a range at least as long as the prepared block; crossing the end
  resets voices and jumps exactly to the loop start. Sustained audio is cut.
- EOF stops and resets voices. Export longer than the project writes trailing
  silence, not effect/instrument tails.
- Live audition can run while transport is stopped; musical position stays fixed.
- Nonzero scheduled note IDs are unique. The high bit is reserved for live IDs.
  The legacy SynthNode preloaded-event API remains for compatibility/testing.

## Structural changes and failure containment

A new graph is constructed/prepared outside the callback. The C ABI prepares the
candidate before stopping the old device; invalid loads leave the old native
session in place. Successful loads stop/join output before destroying the old
service and leave the device closed. Temporary memory can include both graphs
and both sets of media. No live topology swapping is claimed.

The application stops playback on score/tempo/instrument/history edits and marks
its playback description dirty. Play recompiles and reopens output. Gain/pan can
change live once compilation exists. Failed control operations are surfaced.

The output-device object is destroyed before its engine. Silent/WASAPI workers
are joined. CoreAudio output is stopped, uninitialized, and disposed. Platform
implementations require target-host testing, including callback quiescence.

There is one process. A UI hang does not intentionally schedule audio, but a
process crash can terminate audio. Plugin isolation and crash containment are not
implemented and must not be inferred from thread separation.

## Media and export

Short WAVs: PCM16/24/32 and IEEE float32, mono/stereo RIFF, 8–384 kHz. Other
containers, WAVE_FORMAT_EXTENSIBLE, compressed audio, RF64, and streaming imports
are not implemented. Import validates chunk bounds, byte rates, alignment, finite
samples, and frame limits. A load deduplicates identical path strings and enforces
128 MiB decoded sample storage. Per-file limit: 2,880,000 source frames (60 seconds
at 48 kHz; duration differs with sample rate). Path aliases are not content hashes.

Sampler: 24 fixed voices, deterministic oldest stealing, root pitch, gated or
one-shot behavior, linear interpolation, short gated release. Pitch changes alter
playback speed/duration. There is no independent time-stretch or anti-aliased
resampler. Waveform min/max extraction is a native utility, not a persistent cache
or a waveform editor in this UI.

Export builds an independent engine and writes small PCM16 stereo blocks. It does
not allocate the whole render. RIFF output must remain below 4 GiB. The low-level
native CLI/API truncates the chosen destination; the Rust application wrapper
exports to a sibling temporary file and replaces the destination after success.

## Presentation

Selection, viewport, drag previews, snapping and hit-testing live in editor.rs,
not in DSP or Slint objects. Persistent edits are committed at gesture completion.
The first UI uses Slint primitives: viewport culling and a 3,000-visible-note cap
avoid an object per note in the full document. The cap is displayed, not applied
to playback. Building the scene/hit test still scans the track and creates text
labels; spatial indexing, batching and WGPU are later profiling-driven work.

No graphical performance or memory benchmark has been run. Keyboard navigation,
custom-editor accessibility, docking, HiDPI, multi-monitor and touch behavior are
not complete or validated.

## Relevant files

- include/open_daw/service.hpp, src/service.cpp: sequencing/transport/execution.
- include/open_daw/api.h, src/api.cpp: ABI, preparation, ownership, errors.
- src/device.cpp: native/silent device lifecycle.
- src/media.cpp: sampler/import/export.
- app/src/project.rs: document, validation, command inverses.
- app/src/controller.rs: sole native control owner and application coordination.
- app/src/storage.rs: versioned folder persistence and export commit.
- app/src/editor.rs: interaction and render-neutral note scene.
- app/src/bridge.rs: checked Rust ABI wrapper.
- app/src/main.rs, app/ui/main.slint: presentation adapter.
