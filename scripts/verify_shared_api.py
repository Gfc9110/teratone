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
        "od_track_capacity": (C.c_uint32, []),
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


class Mix(C.Structure):
    _fields_ = [('gain',C.c_float),('pan',C.c_float),('mute',C.c_uint32),('solo',C.c_uint32)]
class Synth(C.Structure):
    _fields_ = [('struct_size',C.c_uint32),('version',C.c_uint32),('waveform',C.c_uint32)] + [(x,C.c_float) for x in ['shape','pulse_width','attack','decay','sustain','release']]
class TrackV2(C.Structure):
    _fields_ = [('struct_size',C.c_uint32),('version',C.c_uint32),('base',Track),('mute',C.c_uint32),('solo',C.c_uint32)]
class TrackV3(C.Structure):
    _fields_ = [('struct_size',C.c_uint32),('version',C.c_uint32),('base',TrackV2),('synth',Synth)]
class PadParameters(C.Structure):
    _fields_ = [(x,C.c_float) for x in ['gain','pan','tune']] + [(x,C.c_uint32) for x in ['mute','one_shot','choke']]
class Pad(C.Structure):
    _fields_ = [('struct_size',C.c_uint32),('version',C.c_uint32),('midi',C.c_uint32),('parameters',PadParameters),('bytes',C.POINTER(C.c_uint8)),('size',C.c_size_t)]
class TrackV4(C.Structure):
    _fields_ = [('struct_size',C.c_uint32),('version',C.c_uint32),('base',TrackV3),('kind',C.c_uint32),('pad_count',C.c_uint32),('pads',C.POINTER(Pad))]
class ParametersV1(C.Structure):
    _fields_ = [('mix',Mix),('synth',Synth)]
class ParametersV2(C.Structure):
    _fields_ = [('struct_size',C.c_uint32),('version',C.c_uint32),('base',ParametersV1),('pad_count',C.c_uint32),('pads',PadParameters*16)]
class MixStatus(C.Structure):
    _fields_ = [(x,C.c_uint64) for x in ['generation','submitted','applied']]
class DrumStatus(C.Structure):
    _fields_ = [('generation',C.c_uint64),('pad_count',C.c_uint32),('active',C.c_uint32),('triggers',C.c_uint64*16)]

def verify_drums(dll,root,ok,render):
    for name,result,args in [
        ('od_load_v4',C.c_int,[C.c_void_p,C.POINTER(TrackV4),C.c_size_t,C.POINTER(Note),C.c_size_t,C.c_uint64]),
        ('od_read_mix_status_v1',C.c_int,[C.c_void_p,C.POINTER(MixStatus)]),
        ('od_set_parameters_v2',C.c_int,[C.c_void_p,C.c_uint64,C.POINTER(ParametersV2),C.c_size_t,C.POINTER(C.c_uint64)]),
        ('od_read_drum_status_v1',C.c_int,[C.c_void_p,C.c_uint64,C.c_uint32,C.POINTER(DrumStatus)]),
        ('od_cancel_live',C.c_int,[C.c_void_p]),
    ]:
        getattr(dll,name).restype=result;getattr(dll,name).argtypes=args
    assert [C.sizeof(t) for t in [TrackV2,TrackV3,PadParameters,Pad,TrackV4,ParametersV2,DrumStatus]] == [40,88,24,56,112,448,144]
    path=root/'drum.wav';fixture(path,24,2);raw=path.read_bytes();wav=(C.c_uint8*len(raw)).from_buffer_copy(raw)
    synth=Synth(C.sizeof(Synth),1,0,1,.5,.006,.06,.75,.06)
    pads=(Pad*2)(Pad(C.sizeof(Pad),1,35,PadParameters(1,0,0,0,1,0),wav,len(wav)),Pad(C.sizeof(Pad),1,42,PadParameters(1,0,0,0,1,1),wav,len(wav)))
    track=TrackV4(C.sizeof(TrackV4),4,TrackV3(C.sizeof(TrackV3),3,TrackV2(C.sizeof(TrackV2),2,Track(.5,0,None,60,0),0,0),synth),2,2,pads)
    notes=(Note*1)(Note(1,0,4000,0,35,100));engine=dll.od_create(48000,256);assert engine
    try:
        ok(dll.od_load_v4(engine,C.byref(track),1,notes,1,6000));ok(dll.od_play(engine));render(engine,256)
        status=MixStatus();ok(dll.od_read_mix_status_v1(engine,C.byref(status)));generation=status.generation
        # Invalid descriptors and WAV data leave the playing session and its generation intact.
        for owner,field,bad in [(track,'version',99),(track,'pad_count',17),(track,'kind',3),(pads[1],'midi',35),(pads[0],'version',99),(pads[0],'size',5),(pads[0].parameters,'gain',float('nan')),(pads[0].parameters,'mute',2)]:
            old=getattr(owner,field);setattr(owner,field,bad)
            assert dll.od_load_v4(engine,C.byref(track),1,notes,1,6000)!=0,(field,bad)
            setattr(owner,field,old);ok(dll.od_read_mix_status_v1(engine,C.byref(status)));assert status.generation==generation
            state=Telemetry();ok(dll.od_read_telemetry(engine,C.byref(state)));assert state.playing==1 and state.position==256
        # Borrowed input can be overwritten/freed immediately after successful synchronous prepare.
        C.memset(wav,0,len(wav));del wav
        values=struct.unpack('<1024f',render(engine,512));assert max(abs(x) for x in values)>.01
        parameters=ParametersV2();parameters.struct_size=C.sizeof(ParametersV2);parameters.version=2
        parameters.base=ParametersV1(Mix(.5,0,0,0),synth);parameters.pad_count=2
        for i in range(2):parameters.pads[i]=pads[i].parameters
        serial=C.c_uint64(999);assert dll.od_set_parameters_v2(engine,generation+1,C.byref(parameters),1,C.byref(serial))==1 and serial.value==999
        parameters.pads[0].choke=17;assert dll.od_set_parameters_v2(engine,generation,C.byref(parameters),1,C.byref(serial))==1;parameters.pads[0].choke=0
        for _ in range(8):ok(dll.od_set_parameters_v2(engine,generation,C.byref(parameters),1,C.byref(serial)))
        old=serial.value;assert dll.od_set_parameters_v2(engine,generation,C.byref(parameters),1,C.byref(serial))==2 and serial.value==old
        render(engine,256);ok(dll.od_set_parameters_v2(engine,generation,C.byref(parameters),1,C.byref(serial)))
        meter=DrumStatus();ok(dll.od_read_drum_status_v1(engine,generation,0,C.byref(meter)));assert meter.pad_count==2 and meter.triggers[0]==1
        assert dll.od_read_drum_status_v1(engine,generation+1,0,C.byref(meter))==1
        ok(dll.od_cancel_live(engine));render(engine,256);state=Telemetry();ok(dll.od_read_telemetry(engine,C.byref(state)));assert state.playing==1
        print('PASS: additive v4/v2 64-bit layouts, descriptor/WAV validation, borrowed-byte ownership, generation guards, full queue recovery, drum telemetry and preview cancellation through shared ABI')
    finally: dll.od_destroy(engine)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    args = parser.parse_args()
    dll = configure(args.library)
    assert dll.od_abi_version() == 1
    assert dll.od_track_capacity() == 64
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
        verify_drums(dll,root,ok,render)
    print("Shared-library integration checks passed (not a Rust/UI or hardware test).")


if __name__ == "__main__":
    main()
