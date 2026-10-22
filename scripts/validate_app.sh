#!/usr/bin/env bash
# This gate was authored but NOT executed in the delivery environment (Rust absent).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
command -v cargo >/dev/null || { echo 'Install the stable Rust toolchain before running the app validation gate.' >&2; exit 2; }
command -v cmake >/dev/null || { echo 'CMake is required.' >&2; exit 2; }
cd "$root/app"
if [ ! -f Cargo.lock ]; then
  echo 'No validated lockfile supplied: resolving dependencies on this host.'
  cargo generate-lockfile
fi
cargo test --locked --no-default-features
cargo test --locked
cargo build --locked --release --bin opendaw --bin opendaw-project
cargo run --locked --no-default-features --bin opendaw-project -- validate ../examples/First-session.opendaw
cargo run --locked --no-default-features --bin opendaw-project -- render ../examples/First-session.opendaw target/project-demo.wav
printf '\nValidate the UI and native audio on hardware; preserve Cargo.lock after successful validation.\n'
