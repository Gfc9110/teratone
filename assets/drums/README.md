# OpenDAW internal factory drums

The 12 unmodified WAV files in `vcsl/` are from the official VCSL collection by
Versilian Studios LLC, fixed revision `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
They are distributed under **CC0 1.0 Universal**, independently of the application
code's MIT license. See `vcsl/UPSTREAM_LICENSE`, `vcsl/UPSTREAM_README.md` and
`vcsl/manifest.json` for each original URL, author/rightsholder, license evidence,
revision, access date, SHA-256 and redistribution assessment.

Run `python3 scripts/verify_factory_drums.py` from the repository root to check
bundled bytes, metadata and licensing evidence offline. Add `--upstream` to
recheck the fixed official GitHub tree and original files with normal TLS.
No runtime download occurs. Factory WAVs are embedded in the Rust binaries.

Technical QA passes; human listening/approval remains pending. The source
recordings have an acoustic/percussion character and retain their original
preroll. Playback gains are set per pad; WAV bytes are unchanged. No other audio
assets are approved for bundling by this increment. User imports are separate
from the verified factory collection.
