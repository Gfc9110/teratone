#!/usr/bin/env python3
"""Exercise the delivered shared C ABI from an independent Python process.
No third-party Python packages. Does not validate Rust, Slint, or physical audio.
"""
from __future__ import annotations
import argparse
import ctypes as C
import math
from pathlib import Path
import random
import struct
import tempfile
import wave


class Track(C.Structure):
    _fields_ = [("gain", C.c_float), ("pan", C.c_float), ("sample_path", C.c_char_p),
                ("root_note", C.c_uint32), ("one_shot", C.c_uint32)]


class Note(C.Structure):
    _fields_ = [("id", C.c_uint64), ("start", C.c_uint64), ("end", C.c_uint64),
                ("track", C.c_uint32), ("pitch", C.c_uint32), ("velocity", C.c_uint32)]


class Telemetry(C.Structure):
    _fields_ = [(x, C.c_uint64) for x in
                ["position", "rendered", "blocks", "applied", "rejected", "clipped"]] + [
                    ("left", C.c_float), ("right", C.c_float), ("playing", C.c_uint32),
                    ("looping", C.c_uint32), ("device_open", C.c_uint32), ("device_error", C.c_uint32)]


def configure(path: Path):
    dll = C.CDLL(str(path.resolve()))
    signatures = {
        "od_abi_version": (C.c_uint32, []),
        "od_error_message": (C.c_char_p, []),
        "od_create": (C.c_void_p, [C.c_uint32, C.c_uint32]),
        "od_destroy": (None, [C.c_void_p]),
        "od_load": (C.c_int, [C.c_void_p, C.POINTER(Track), C.c_size_t, C.POINTER(Note), C.c_size_t, C.c_uint64]),
        "od_play": (C.c_int, [C.c_void_p]),
        "od_stop": (C.c_int, [C.c_void_p]),
        "od_render": (C.c_int, [C.c_void_p, C.POINTER(C.c_float), C.c_uint32]),
        "od_read_telemetry": (C.c_int, [C.c_void_p, C.POINTER(Telemetry)]),
        "od_export": (C.c_int, [C.c_void_p, C.c_char_p, C.c_uint64]),
        "od_device_start": (C.c_int, [C.c_void_p, C.c_uint32]),
        "od_device_stop": (C.c_int, [C.c_void_p]),
    }
    for name, (result, args) in signatures.items():
        fn = getattr(dll, name)
        fn.restype, fn.argtypes = result, args
    return dll


def fixture(path: Path, bits: int, channels: int, floating: bool = False, nan: bool = False):
    frames, rate = 4096, 48000
    data = bytearray()
    for i in range(frames):
        for channel in range(channels):
            value = 0.4 * math.sin(i * 2 * math.pi * (220 + 110 * channel) / rate)
            if floating:
                data += struct.pack("<f", float("nan") if nan and i == 0 else value)
            else:
                v = round(value * ((1 << (bits - 1)) - 1))
                data += v.to_bytes(bits // 8, "little", signed=True)
    align = channels * bits // 8
    fmt = struct.pack("<HHIIHH", 3 if floating else 1, channels, rate, rate * align, align, bits)
    content = b"WAVEfmt " + struct.pack("<I", len(fmt)) + fmt + b"data" + struct.pack("<I", len(data)) + data
    path.write_bytes(b"RIFF" + struct.pack("<I", len(content)) + content)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    args = parser.parse_args()
    dll = configure(args.library)
    assert dll.od_abi_version() == 1
    assert (C.sizeof(Track), C.sizeof(Note), C.sizeof(Telemetry)) == (24, 40, 72), "64-bit ABI layout mismatch"

    def ok(result):
        if result:
            raise AssertionError(f"Native error {result}: {dll.od_error_message().decode('utf-8', 'replace')}")

    def render(engine, count):
        block = (C.c_float * (count * 2))()
        ok(dll.od_render(engine, block, count))
        return bytes(block)

    with tempfile.TemporaryDirectory(prefix="opendaw-ffi-") as temp:
        root = Path(temp)
        notes = (Note * 3)(Note(1, 0, 2500, 0, 60, 110), Note(2, 31, 3600, 0, 60, 80), Note(3, 2701, 5000, 0, 67, 95))
        engine = dll.od_create(48000, 256)
        assert engine
        try:
            for channels in (1, 2):
                for bits, floating in ((16, False), (24, False), (32, False), (32, True)):
                    path = root / f"sample-{channels}-{bits}-{floating}.wav"
                    fixture(path, bits, channels, floating)
                    tracks = (Track * 1)(Track(0.6, 0, str(path).encode(), 60, 0))
                    ok(dll.od_load(engine, tracks, 1, notes, 3, 6000))
                    ok(dll.od_play(engine))
                    raw = render(engine, 6000)
                    values = struct.unpack(f"<{len(raw)//4}f", raw)
                    assert all(math.isfinite(v) for v in values)
                    assert max(abs(v) for v in values) > 0.01
            print("PASS: 8 WAV variants through the shared C ABI")
            bad = root / "nonfinite.wav"
            fixture(bad, 32, 1, True, True)
            bad_track = (Track * 1)(Track(0.6, 0, str(bad).encode(), 60, 0))
            assert dll.od_load(engine, bad_track, 1, notes, 3, 6000) != 0
            state = Telemetry()
            ok(dll.od_read_telemetry(engine, C.byref(state)))
            assert state.position == 6000, "Failed sample load changed active session"
            print("PASS: non-finite media rejected without replacing the active session")
            sine = (Track * 1)(Track(0.6, 0, None, 60, 0))
            ok(dll.od_load(engine, sine, 1, notes, 3, 6000))
            ok(dll.od_play(engine))
            reference = render(engine, 6000)
            rng = random.Random(42)
            for _ in range(25):
                ok(dll.od_load(engine, sine, 1, notes, 3, 6000)); ok(dll.od_play(engine))
                total, parts = 0, []
                while total < 6000:
                    size = min(rng.randint(1, 1000), 6000 - total)
                    parts.append(render(engine, size)); total += size
                assert b"".join(parts) == reference, "Block partition changed rendered audio"
            print("PASS: 25 randomized callback partitions are bit-identical")
            output = root / "render-音楽.wav"
            ok(dll.od_export(engine, str(output).encode(), 48000))
            with wave.open(str(output), "rb") as wav:
                assert (wav.getnchannels(), wav.getsampwidth(), wav.getframerate(), wav.getnframes()) == (2, 2, 48000, 48000)
            ok(dll.od_read_telemetry(engine, C.byref(state)))
            assert state.position == 6000
            print("PASS: Unicode export path, WAV metadata, and independent export state")
            ok(dll.od_device_start(engine, 1))
            out = (C.c_float * 16)()
            assert dll.od_render(engine, out, 8) == 3
            ok(dll.od_device_stop(engine))
            print("PASS: explicit silent device excludes a second render consumer")
        finally:
            dll.od_destroy(engine)
    print("Shared-library integration checks passed (not a Rust/UI or hardware test).")


if __name__ == "__main__":
    main()
