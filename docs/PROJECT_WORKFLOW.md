# Project browser — 27 October 2026

The startup browser creates an empty project in a chosen parent folder, opens
existing project folders and lists the 12 most recently used projects. File and
the transport's Projects button reopen the browser. The inspector's Project tab
shows the active absolute path, Save, Save as and Open project folder.

Create and Save as reserve a new directory and reject existing destinations.
Switching projects validates and prepares the target before replacing session
state. Unsaved edits offer Save and continue, Discard and continue, or Cancel.
Failed target validation or saving preserves the current document. Opening a
project resets history, selection and playback; Save as retains the editing
session and copies referenced assets through the existing portable-save path.
User settings are separate from project contents; see README for the paths.

## Executed checks on macOS / Apple Silicon

- `cargo test --locked --no-default-features`: 38 unit + 9 integration tests pass.
- `cargo test --locked`: 60 unit + 9 integration tests pass.
- Locked release build: application, project CLI and both UI probes pass.
- `project_workflow_probe`: real production callbacks with isolated settings;
  pointer creation, blank content, absolute folder display, Save/Discard switching,
  recent-row opening, Save as, missing-project preservation and history reload.
  Welcome screens rendered at 1280×820 and 1000×760, at 100% and 200% scaling.
- `layout_probe`: existing arrangement, clips, piano roll, mixer, synth, drums,
  undo, audition and 64-track / 100,000-note scenarios pass at 100% and 200%.
  The largest observed response-to-paint p95 is 23.848 ms; largest frame p95 is
  12.209 ms. These are software-renderer probe measurements, not hardware latency.
- Release application startup: remained running for four seconds with `--silent`;
  only the probe process was terminated. The user's existing application was left
  running.

Raw evidence: `evidence/projects-{headless-tests,ui-tests,release-build,workflow-probe,layout-regression,startup-smoke}.log`.
Screenshots: `evidence/projects-welcome.png` and `evidence/projects-welcome-200pct.png`.
Native folder dialog interaction and opening Finder were compiled but were not
automated by these probes. Windows and Linux require target-host verification.
