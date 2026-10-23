# Project format v2 — M3.4

The application reads v1 and v2 and writes v2. Opening a v1 project migrates it
in memory without rewriting its source file. The example under
`examples/First-session.opendaw/project.json` intentionally remains v1 as a
compatibility fixture. The original ABI v1 remains compatible; the app uses additive v3 descriptors with synth settings. Legacy descriptor layouts remain compatible.

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

The other currently supported configuration is:

```json
{"id": 2, "config": {"kind": "sampler", "sample_path": "assets/sample-example.wav", "root_note": 60, "one_shot": false}}
```

Track `mute` and `solo` are optional booleans with default false when absent,
including migrated v1 files and earlier v2 manifests. New saves write both.
Audibility is `!mute && (!any_solo || solo)`; gain/pan retain their nominal values.
Flags are undoable and duplicated with the independent instance. Playback and
export derive the same gate state. The synth/preset model below is implemented in M3.4; drum pads and
hashed asset references remain subsequent M3 increments. Unknown
fields and unsupported versions are rejected. The decoder uses separate strict
DTOs for v1 and v2 and rejects duplicate fields in the original JSON bytes.

All track, instance, clip and note IDs are globally unique, nonzero, below 2^63
and below `next_id`. Track array order is visual/playback slot order, not identity.
Duplicating a track allocates fresh IDs for its instance, clips and notes, while
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
