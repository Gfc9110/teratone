#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
build="${1:-$root/build/release}"
cmake -S "$root" -B "$build" -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=ON
cmake --build "$build" --config Release --parallel 4
ctest --test-dir "$build" -C Release --output-on-failure
"$build/daw_session" --render "$build/native-demo.wav" --seconds 8
case "$(uname -s)" in
  Darwin) lib="$build/libopen_daw_api.dylib" ;;
  Linux) lib="$build/libopen_daw_api.so" ;;
  *) lib="" ;;
esac
if command -v python3 >/dev/null && [ -n "$lib" ]; then
  python3 "$root/scripts/verify_shared_api.py" --library "$lib"
fi
