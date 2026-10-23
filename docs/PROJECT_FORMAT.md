# Project format v2 — M3.5

The application reads v1 and v2 and writes v2. Opening a v1 project migrates it
in memory without rewriting its source file. The example under
`examples/First-session.opendaw/project.json` intentionally remains v1 as a
compatibility fixture. The original ABI v1 remains compatible; the app uses additive v4 descriptors with synth and drum settings. Legacy descriptor layouts remain compatible.

```text
Song.opendaw/
  project.json
  assets/                 # copied short WAV files
  recovery/previous.json  # previous validated manifest
  recovery/pre-m3-v1.json # original v1 manifest when first saved as v2
```

## Current v2 model

Project fields: `version=2`, `name`, `bpm`, `length_ticks`, `next_id`, `tracks`.
Time uses 960 ticks per quarter note and constant tempo (30–300 BPM). Tracks now
own an independent instrument instance and MIDI clips; there is no duplicate
mutable `Track.notes` array.

```json
{
  "version": 2,
  "name": "Example",
  "bpm": 120.0,
  "length_ticks": 15360,
  "next_id": 100,
  "tracks": [{
    "id": 1,
    "name": "Lead",
    "color": "#62BFA6",
    "gain": 0.7,
    "pan": 0.0,
    "mute": false,
    "solo": false,
    "instrument": {"id": 2, "config": {"kind": "sine"}},
    "clips": [{
      "id": 3,
      "name": "Main",
      "start_tick": 0,
      "length_ticks": 15360,
      "notes": [{"id": 4, "start": 0, "duration": 720, "pitch": 60, "velocity": 100}]
    }]
  }]
}
```

The sampler configuration is:

```json
{"id": 2, "config": {"kind": "sampler", "sample_path": "assets/sample-example.wav", "root_note": 60, "one_shot": false}}
```

Track `mute` and `solo` are optional booleans with default false when absent,
including migrated v1 files and earlier v2 manifests. New saves write both.
Audibility is `!mute && (!any_solo || solo)`; gain/pan retain their nominal values.
Flags are undoable and duplicated with the independent instance. Playback and
export derive the same gate state. The synth/preset model below is implemented in M3.4;
drum pads and hashed asset references are implemented in M3.5 below. Unknown
fields and unsupported versions are rejected. The decoder uses separate strict
DTOs for v1 and v2 and rejects duplicate fields in the original JSON bytes.

All track, instance, clip, note and drum pad IDs are globally unique, nonzero, below 2^63
and below `next_id`. Track array order is visual/playback slot order, not identity.
Duplicating a track allocates fresh IDs for its instance, clips, notes and pads, while
copying their values. Undo/redo restores object identities and original positions;
`next_id` never goes backwards. Undo history is session-local, up to 512 steps.
Note deletion also restores its original array position on undo.

Clip starts are absolute project ticks. Note starts are local to their clip.
Clips must fit inside the project, and notes inside their clip; checked addition
rejects integer overflow. Overlapping clips sum their events with distinct note
IDs. The bridge compiles absolute start/end frames using the actual engine rate.
A note duration that rounds below one frame is rejected before native loading.
The editor selects one clip at a time. New tracks and migrated v1 tracks receive
one full-length Main clip. M3.3 adds clip CRUD and transfer/resize controls. A new
clip is four beats; duplication deep-copies notes with new IDs. Clip operations
may extend project length up to the validated 24-hour limit; that extension is
part of the same inverse command. Shrinking through notes is rejected. Ordinary
removal keeps project length. Empty tracks are valid and can create clips again.
Selected clip color follows its track; overlapping events sum with distinct IDs.

Velocity remains the existing integer 1–127, now editable. Keyboard notation and
per-clip viewport caches are session UI state, absent from project.json and undo.
C3/C4/C5 labels never transpose saved pitches or alter the compiled event score.

Limits remain 16 tracks (matching native capacity), 100,000 notes total, 4,096
clips, 24 hours, 32 MiB manifest and a 128 MiB new-asset-copy budget per save.
The native loader separately caps decoded sample storage at 128 MiB. Memory for
history, old/new prepared sessions and simultaneous export is additional.

## Migration and persistence

Migration preserves track/note IDs, pitch, velocity, tick values and mix. It
assigns deterministic colors and allocates new instance/clip IDs from `next_id`.
Legacy blank track names receive a displayable name. All content is validated
before loading; unsupported versions do not replace the current project.

On the first save over a valid v1 manifest, storage retains its original bytes
in both `recovery/previous.json` and `recovery/pre-m3-v1.json`. Later v2 saves can
replace `previous.json`, but preserve `pre-m3-v1.json`. The CLI also provides an
explicit `opendaw-project migrate PROJECT_DIR` command. The old application cannot
read v2: use the retained v1 manifest to recover the original checkpoint.

Samples retain the existing path contract: saved relative paths under `assets/`,
no traversal, and canonical containment checks on load/save. Runtime paths are
resolved to absolute paths. Save copies media first, flushes files, preserves the
validated prior manifest and atomically replaces `project.json`. Copies of one
canonical sample source are deduplicated during that save; instrument state stays
independent. A failed save may leave orphan asset files, but does not intentionally
remove the previous checkpoint. There is no lock against another local process
racing path/symlink changes, or proof against same-size rewrites of source media.
No asset garbage collection is introduced in this increment.

## M3.4 oscillator and owned presets

`kind: sine` remains an exact compatible default: sine, shape=1, pulse_width=0.5,
attack=0.006, decay=0.06, sustain=0.75, release=0.06. New parameters use:

```json
{
  "id": 3,
  "config": {
    "kind": "synth",
    "parameters": {
      "waveform": "square", "shape": 1.0, "pulse_width": 0.25,
      "attack": 0.008, "decay": 0.2, "sustain": 0.5, "release": 0.15
    },
    "preset": {
      "version": 1, "name": "Hollow Pulse", "source": "OpenDAW factory",
      "parameters": {
        "waveform": "square", "shape": 1.0, "pulse_width": 0.25,
        "attack": 0.008, "decay": 0.2, "sustain": 0.5, "release": 0.15
      }
    }
  }
}
```

The optional preset (`null`/absent allowed) is an owned versioned snapshot, not a
mutable reference. Parameters are authoritative; comparing them to the snapshot
derives the modified indicator. Duplication copies both with a fresh instance ID.
Preset files contain only the inner preset object and are limited to 4 KiB.
They omit notes, volume/pan and routing. Save is explicit and atomically replaces
the chosen file; Undo changes the document, not the external preset file.

Waveform is sine/saw/square/triangle. Shape [0,1] is a sine→selected-wave morph;
pulse_width [0.05,0.95] is active only for square. Attack/decay [0,10] and release
[0,20] are seconds, sustain [0,1]. All numeric values must be finite; all parameter
fields are required and unknown/duplicate fields are rejected. Each new voice
captures the whole ADSR. Oscillator targets ramp for 10ms on existing voices.
`kind: synth` is rejected in v1. Previous app versions reject this new v2 instrument
kind explicitly; backward reading is not promised. See M3_4_TRACK_SYNTH.md.

## M3.5 drum kits and verified assets

The new instrument configuration has `kind: drum_kit` and a strict owned `kit`.
The factory creates 16 pads (12 filled, four empty); a valid document may contain
1–16. Each pad has globally unique `id`, `name`, unique `midi_note` (0–127),
nullable `sample`, and complete `parameters`. A single-pad excerpt:

```json
{"kind":"drum_kit","kit":{"name":"Example","pads":[{
  "id":100,"name":"Kick","midi_note":35,
  "sample":{"kind":"factory","asset_id":"kick","sha256":"642aaf3375eaff1307973f7de3ccbf74d18e71fb3cf9412e021b08fc3bf1981c"},
  "parameters":{"gain":0.76164045,"pan":0.0,"tune_semitones":0.0,"mute":false,"one_shot":true,"choke_group":0}
}]}}
```

Gain 0–4, pan −1..1, tune −24..24 semitones, choke group 0–16 (0 disabled),
booleans mute/one_shot; all numeric values finite. Velocity stays in MIDI notes,
1–127. MIDI selects a sample without transposition; tune is separate. Empty and
unmapped notes are silent. One-shot ignores ordinary note-off; gated uses duration.
Tune/mode/choke are captured by each new voice. Choke groups are kit-local.

`factory` references only known embedded IDs with their exact original SHA-256.
Save copies every filled pad, including factory bytes, into
`assets/drum-<sha256>.wav`, deduplicates by verified content, and writes a `file`
reference: `kind`, `sample_path`, `sha256`, optional `origin` (factory asset ID).
When present, origin must match the registered factory hash. User imports have
no origin. Hashes are lowercase 64-digit hex. Load verifies **every referenced
file**, including separate paths claiming the same hash. Corrupt destination files
and asset symlinks are rejected rather than silently overwritten. The existing
relative assets/ containment contract applies. All user and factory drum files
are checked on load, native prepare and save; these hashes detect changes, not
adversarial concurrent filesystem races. Melodic sampler path semantics are unchanged.

A project containing factory-origin files receives `assets/VCSL-CC0-MANIFEST.json`
and `assets/VCSL-CC0-LICENSE.txt`. The manifest records all 12 originals, whether
or not this project uses all of them. App factory bytes are compiled into both UI
and headless binaries; no runtime download is required. The demo project under
`examples/Drum-kit.opendaw` uses factory references and becomes self-contained on Save.

Imported drum WAVs are staged immutably for session Undo, capped at 128 MiB across
unique imports. A failed candidate does not consume that cache. No automatic
media deletion; existing hashed assets and replaced files remain for history.
Save has a combined 128 MiB media budget. Native prepare separately caps encoded
drum input and combined decoded sample data at 128 MiB each; staging, transients,
old/new prepared sessions, oscillator banks and export add memory beyond these caps.
Old v1/v2 sources without drums remain supported with exact v1 audio regression.
New `drum_kit` is rejected by v1 and by older application versions, not silently
converted to sine. Full kit settings are persisted; separate kit-preset files,
velocity layers, step sequencing and shared plugin routing are not introduced.
