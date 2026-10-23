# Architecture implemented in this increment

## Ownership and dependency direction

```text
Slint / main GUI thread
  -> bounded UI outbox + Rust request channel / ACKs
Rust controller worker
  -> authoritative Project + inverse-command History
  -> musical-tick-to-frame preparation
  -> checked bridge (one Engine owner, !Send / !Sync)
C ABI / one native control producer
  -> EngineService (prepared graph, schedule, note queue + coherent mixer queue)
  -> OutputDevice (owns/quiesces audio consumer)
C++ audio consumer
  -> block-relative note events -> SynthNode / SamplerNode
  -> GainPanNode -> AudibilityNode (5 ms gate) -> MixerNode -> bounded stereo output

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
The independent 8-entry mixer queue survives this discard. The application retains
durable document edits on Stop/saturation. A UI outbox (256 pending, 64 in flight)
retries full channels, coalesces only contiguous transient controls, and reports a
rejected new action explicitly. ACKs carry request identity/revision/error; a stop
epoch invalidates old transport/audition/gestures. A new Play after Stop waits for
priority quiescence before dispatch. This is bounded in-memory delivery, not an
on-disk command journal.

A complete gain/pan/mute/solo snapshot is copied through an SPSC queue with at
most 8 reads per block. Generation guards slots after load; the latest complete
snapshot sets all targets coherently before rendering. Gain/pan keep their nominal
values and smoothing. Mute/solo derive `!mute && (!any_solo || solo)` and a separate
5 ms linear gate reaching exact silence. Voices keep advancing while muted. Initial
load/export gates are correct from sample zero. The legacy C++ setGain/setPan
methods remain available; the application and C ABI use coherent snapshots.
These are block-boundary targets, not sample-accurate automation. Submitted/applied
serials distinguish acceptance from consumption, not completion of the ramp. Full
queues preserve the Rust document and trigger retry. Play reloads desired mix if
there was no consumer after Stop. The old od_gain_pan API can now report queue full.

Processing accepts variable callback sizes, subdividing into the prepared maximum.
Loop/EOF boundaries split processing without moving scheduled note offsets.
Non-finite DSP output is replaced with silence; output is clamped to [-1,1], with
clipping counters and raw peak telemetry. This is not a mastering limiter.

The allocation probe measures overridden C++ new/delete operations only. It does
not prove absence of every malloc, driver call, page fault, or scheduler delay.

## Transport policy

- Play starts at the current position, or restarts from zero after EOF.
- Pause keeps position but resets voices. Resume does not chase earlier notes.
- Stop silences and returns to zero; pending native transport/note commands are
  discarded. Desired mixer state and durable UI document edits are retained.
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

The application stops playback on score/tempo/instrument/structural history edits
and marks its playback description dirty. Play recompiles and reopens output.
Gain/pan/mute/solo and metadata, including their history, can change live once
compilation exists. Failed control operations are surfaced.

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

Note selection, viewport transforms, note previews, snapping and hit-testing live
in editor.rs. The Slint arrangement holds a transient clip-drag ghost; the
controller/project validate the final ID/tick transaction on release. Persistent edits are committed at gesture completion.
The first UI uses Slint primitives: viewport culling and a 3,000-visible-note cap
avoid an object per note in the full document. The cap is displayed, not applied
to playback. Building the scene/hit test still scans the track and creates text
labels; spatial indexing, batching and WGPU are later profiling-driven work.

M3.2 adds bounded arrangement previews (32 aggregate rectangles/clip, 256 visible
clips with an explicit warning) and preserves the track model across unrelated
updates. Header/lane rows share vertical scroll; horizontal offset is applied only
to lanes and ruler. A software interaction probe checks 100/200% layouts and the
real controller. A 16-track/100k-note headless smoke benchmark is recorded; it is
not a hardware audio/UI-frame performance certificate. Keyboard navigation,
custom-editor accessibility, docking, multi-monitor and touch remain incomplete.

## Relevant files

- include/open_daw/service.hpp, src/service.cpp: sequencing/transport/execution.
- include/open_daw/api.h, src/api.cpp: ABI, preparation, ownership, errors.
- src/device.cpp: native/silent device lifecycle.
- src/media.cpp: sampler/import/export.
- app/src/project.rs: document, validation, command inverses.
- app/src/controller.rs: sole native control owner and application coordination.
- app/src/storage.rs: versioned folder persistence and export commit.
- app/src/editor.rs: interaction and render-neutral note scene.
- app/src/arrangement.rs: bounded clip/track scene and viewport culling.
- app/src/mailbox.rs: bounded UI outbox, coalescing, priority stop epoch and ACKs.
- app/src/ui_support.rs: stable Slint models and selected-track form synchronization.
- app/src/bridge.rs: checked Rust ABI wrapper.
- app/src/main.rs, app/ui/main.slint: presentation adapter.

## M3.1 track and clip foundation — 22 October 2026

The Rust document is now schema v2: stable track/instance/clip/note IDs,
track colors and clip-local note times. Separate strict DTOs migrate v1 in memory.
The controller identifies tracks by ID; numeric slots are derived only at the
bridge. Structural edits retain stop/prepare behavior. Metadata changes and their
undo preserve playback. Gain/pan changes can continue live; slider release ends
an undo gesture. Selection releases live audition notes instead of resetting all
scheduled voices. Export and native preparation flatten clips to absolute frames.
The C++ graph, DSP and C ABI v1 are unchanged in this increment.

In M3.1, the sidebar exposed track CRUD/order/properties and the editor could
select existing MIDI clips. M3.2 replaces that presentation with arrangement rows
and inspector, and adds persistent live mute/solo. M3.3 adds clip manipulation,
vertical keyboard, velocity, notation and scoped audition. M3.4 implements the
expanded synth described below; M3.5 adds the internal multi-pad drums below. See M3_IMPLEMENTATION_PROGRESS.md
for exact scope, evidence and remaining limitations; PROJECT_FORMAT.md documents
the implemented v2 fields rather than the broader proposed handoff model.

## M3.3 clip editing and piano keyboard — 23 October 2026

Clip transactions preserve content, global IDs, array order and any automatic
project-length extension in a single undo. Resize rejects note truncation. Transfer
keeps clip/note identities and uses the destination track's instrument. No duplicate
mutable score is added; export flattens absolute starts with the same bridge.

Viewport uses `(127-pitch)*row_height-scroll_y`; visible key rows and note rectangles
come from this shared transform. The grid TouchArea starts after the keyboard.
Gesture requests contain track/clip/token and the displayed viewport. A canceled
or old gesture cannot commit into another selection. A bounded cache remembers
viewports by clip ID; notation changes only text and remains session-local.

Audition maps gesture tokens to original track/instance/pitch and native occurrence
IDs, with explicit live velocity. RELEASE has a separate epoch from Stop and
invalidates pending audition-ons without discarding document edits or new Play.
Slint focus/cancel/exit and selection lifecycle release the original token.
Stable key models keep a held key alive across unrelated ACK/views. C++ DSP and
callback execution are unchanged; the Rust bridge uses the existing live-note ABI.

See M3_3_CLIPS_PIANO_ROLL.md and evidence/m33-* for measured scope and limitations.

## M3.4 per-track oscillator and parameter snapshots — 23 October 2026

TrackSettings owns SynthConfig. SynthNode owns 24 fixed voices, each capturing
its ADSR at note-on; runtime applyConfig is called only by the audio owner at a
block boundary. Waveform weights and shape/PWM ramp for 10ms without resetting
phase. Sine preserves the previous sin path and level. Saw and triangle use
8192+1-point per-MIDI tables with harmonics strictly below Nyquist (cap 2047);
PWM is a DC-corrected, nominally peak-normalized difference of two saw reads.
Fundamentals at/above Nyquist produce silence. Static spectral gates are measured;
arbitrary time modulation is not claimed to be strictly bandlimited.

The immutable 8.001MiB bank is shared between prepared tracks/live/export. A bounded
four-entry weak cache has a control-side mutex and FFT construction only. Rendering
never calls the cache or changes shared_ptr ownership. Graph reset changes voices
and ramp values only. Allocation and lifetime remain outside the callback.

The eight-entry SPSC carries optional synth arrays alongside mixer data. Consumer
coalescing retains the latest complete synth snapshot even if followed by a legacy
mixer snapshot, then applies both before note-ons. od_load_v3 and
od_set_parameters_v1 are additive; descriptor size/version and all ranges are
validated. The accepted export spec is updated only on successful enqueue. The
existing generation/submitted/applied status covers both mix and synth targets.
Rust sends full snapshots and retries the latest document on saturation; it checks
the compiled (track ID, instance ID) order before deriving slots.

app/src/synth.rs defines strict owned parameter/preset values. Project Edit::Synth
is realtime, preserves the previous instrument/preset for inverse history, and
coalesces only the same field of the same instance until FinishMix. Factory/preset
application is one history step. User preset files use atomic storage replacement.
The Sound inspector tab and software probe share the production UI input adapter.
See M3_4_TRACK_SYNTH.md for formulas, reproducible FFT tests, benchmarks and limits.

## M3.5 internal drums — 23 October 2026

`app/src/drums.rs` owns strict kit/pad/sample metadata and embedded verified CC0
bytes. The Rust document remains authoritative; pad commands use track, instance
and pad IDs. New `include/open_daw/drums.hpp` / `src/drums.cpp` implement a 128-key
lookup and 64 fixed voice slots per kit, sample-rate conversion, velocity, local
choke, gated/one-shot and per-pad smoothed mix. Voice/tail state is independent;
only immutable decoded samples are shared across instances.

The additive v4 load borrows WAV byte spans synchronously and prepares ownership
on the control thread. Rust checks raw SHA-256 before content deduplication;
C++ reuses decoded data for repeated prepared spans under a combined media budget.
The v2 parameter snapshot extends the existing eight-slot POD SPSC with pad arrays;
consumer coalescing retains synth/drum targets across legacy mix commands. The
same generation/submitted/applied receipt covers the complete target. Callback
never opens a file, acquires the control-side cache mutex or changes shared sample
ownership. Replaced graphs are destroyed after device stop/join.

Preview cancellation has its own native epoch, bypassing a full note queue and
preserving scheduled voices. One-shot previews are included even after the UI's
ordinary release. Trigger/active-voice atomics are polled by the control owner;
UI pulse/mute models are distinct from stable interactive pad rows.

`DrumStaging` retains imported content for session Undo; project storage copies
all filled pads to hash-addressed relative assets and includes CC0 provenance.
Structural drum edits decode a candidate before stopping/mutating the active
project; failed imports roll back their new staging file. Export uses an
independent engine from the captured document, as before. See
[M3.5 contracts, DSP, evidence and limitations](M3_5_DRUM_MACHINE.md).

## M3.6 capacity, prepared storage and measurement

The Rust document and native service support 64 independent tracks. The UI gets
the limit from the presenter; the bridge checks additive `od_track_capacity()`
before creating a backend handle. Legacy ABI layouts and project schema v2 are
unchanged. Pad arrays still have 16 entries. The loader rejects excess tracks
transactionally, preserving the existing service.

Fixed 320-event rows are allocated as a vector during prepare; the bounded mix
queue is owned by a unique_ptr. Neither grows or changes ownership in render.
On the measured arm64 build EngineService is 61,568 bytes; the 64-track event
storage is 327,680 bytes and mix queue 207,680 bytes on the prepared heap.
Old/new graphs and independent export can overlap on the control side.

After successful preparation and old-owner release, macOS returns unused malloc
pages every 16 preparations through `malloc_zone_pressure_relief`. The cadence
is thread-local so recreating handles does not bypass it. This is control-only,
not callback cleanup or a new cross-platform allocator requirement. Active media,
history and export owners are preserved. See the M3.6 memory experiment.

Factory bytes embedded immutably are hashed once per unique content per prepare.
Every File reference is still checked before deduplication, even when its claimed
hash matches an already loaded Factory or another path. Instance parameters,
voices and choke groups are never shared with the immutable sample data.

`OD_MEASURE_RENDER` is an opt-in build used only by performance_probe. It
preallocates a bounded measurement vector and records internal block durations;
normal app and library builds have no profiling clocks or vector. A separate
Mac test-only interposer audits heap and pthread lock calls with a positive
control. Neither tool is a production audio dependency. Results, scope and the
remaining device/OS gates are in [M3.6 validation](M3_6_64_TRACKS_VALIDATION.md).
