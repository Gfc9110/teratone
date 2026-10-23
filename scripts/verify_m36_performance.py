#!/usr/bin/env python3
"""Check the declared 32-track/10-minute DSP gate from performance_probe JSON.
This checks recorded measurements; it does not certify listening, device-removal handling,
Windows, a native GUI, or every possible driver underrun. No third-party dependencies.
"""
import argparse
import json
from pathlib import Path


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('record',type=Path)
    parser.add_argument('--require-coreaudio',action='store_true')
    args=parser.parse_args()
    rows=[json.loads(line) for line in args.record.read_text().splitlines() if line.strip()]
    assert len(rows)==1, 'Expected one complete run'
    r=rows[0]
    assert (r['tracks'],r['synths'],r['drums'],r['samplers'],r['voices_per_synth'])==(32,16,8,8,8)
    assert r['sample_rate']==48000 and r['internal_block_frames']==256
    assert r['valid_continuous_window'] and 600<=r['seconds']<=601 and r['audio_seconds']>=599.8
    assert r['dsp_p99_ms']<3.2 and r['dsp_max_ms']<256/48 and r['dsp_over_budget']==0
    assert r['device_error']==0 and r['clipped_samples']==0
    if args.require_coreaudio:
        assert r['mode']=='native_device' and r['coreaudio_overload_notifications']==0
    print('PASS 32 mixed tracks / 8 voices per synth / 48kHz / 256 frames / continuous 10-minute DSP timing gate.')
    if args.require_coreaudio:print('PASS CoreAudio default output: no reported device errors or processor-overload notifications during this run.')
    print('Driver underruns beyond the exposed observations, listening and other platforms still require separate validation.')


if __name__=='__main__':main()
