# Project format v2 — M3.2

The application reads v1 and v2 and writes v2. Opening a v1 project migrates it
in memory without rewriting its source file. The example under
`examples/First-session.opendaw/project.json` intentionally remains v1 as a
compatibility fixture. The original ABI v1 remains compatible; the app uses additive versioned mixer descriptors.

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
export derive the same gate state. A preset/synth-parameter model, drum pads and
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
The current editor selects one existing clip at a time. New tracks and migrated
v1 tracks receive one full-length `Main` clip. M3.2 shows existing clips in aligned
arrangement lanes and selects them for editing. Clip creation/movement/resizing
controls follow in M3.3.

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
