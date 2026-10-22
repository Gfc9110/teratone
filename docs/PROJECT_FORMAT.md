# Project format v1 (candidate Rust implementation)

```text
Song.opendaw/
  project.json
  assets/                 # immutable copied short samples
  recovery/previous.json  # prior validated document checkpoint
```

The example is examples/First-session.opendaw/project.json. This file was generated
and JSON-parsed in the delivery environment. Its Rust load/save/round-trip paths
have not been executed here.

Top-level fields: version=1, name, bpm, length_ticks, next_id, tracks. Time uses
960 ticks per quarter note, independent of device sample rate. Tempo is constant,
30–300 BPM; a tempo map is not implemented. Tracks have stable id, name, gain, pan,
instrument, and notes. Notes contain id, start, duration, pitch, velocity. All
persisted identities are nonzero, below 2^63 and below next_id; identities are not
reused after undo. Track and note identities are globally distinct.

Instrument variants:

```json
{"kind":"sine"}
```

```json
{"kind":"sampler","sample_path":"assets/sample-example.wav","root_note":60,"one_shot":false}
```

The saved sample path must be relative to assets/ with no traversal or absolute
components. Load resolves it to an absolute runtime path and checks canonical
containment. Save copies external media first, flushes files, retains the prior
validated manifest, then replaces project.json. A failed save can leave orphan
assets; it must not intentionally delete the previous checkpoint. Sample contents
must remain unchanged while copying: a size change is detected, but a concurrent
same-size content rewrite is not. There is no filesystem lock or sandbox against
another local process racing symlink/path changes.

Limits: 16 tracks, 100,000 notes, 24 hours, 32 MiB manifest, 128 MiB new-asset-copy
budget per save. The separate native loader caps decoded media at 128 MiB per
session; these are not interchangeable file/decoded-size limits. Temporary
prepared old/new sessions and independent exports can increase process memory
above those individual limits. No aggregate process RSS ceiling is implemented.

Undo stores inverse edits, not sample buffers, and is capped at 512 steps.
Consecutive changes to the same track's mixer coalesce until another edit; the
first implementation does not create time-based slider-gesture boundaries.
Drag previews commit one persistent note edit on release. Undo history is not
saved. Recovery is a previous saved manifest, not autosave or an operation journal.
To restore manually, close the app, keep copies of both manifests, then replace
project.json with recovery/previous.json; retain assets/.

Unknown schema versions and unknown fields are rejected. Migration functions,
plugin states, arrangement/audio clips, automation, routing documents and project
bundling are reserved for later schema versions; do not silently add them to v1.
