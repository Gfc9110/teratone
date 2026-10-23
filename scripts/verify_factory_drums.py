#!/usr/bin/env python3
"""Verify bundled CC0 files offline; --upstream additionally rechecks official fixed-revision sources.
Python standard library only. Network downloads use curl with normal TLS validation.
"""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import wave

ROOT = Path(__file__).resolve().parents[1] / 'assets/drums/vcsl'
REVISION = 'c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e'
RAW = f'https://raw.githubusercontent.com/sgossner/VCSL/{REVISION}/'
LICENSE_SHA = 'a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499'
README_SHA = '6f7214a188f106c917d6503748485413c3e47cf56bce69f82beb9d16aa0894a9'

def fetch(url):
    return subprocess.check_output(['curl', '--fail', '--silent', '--show-error', '--location', '--retry', '2', url])

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--upstream', action='store_true')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'manifest.json').read_text())
    assert manifest['upstream_commit'] == REVISION
    assert manifest['license_sha256'] == LICENSE_SHA and manifest['readme_sha256'] == README_SHA
    for local, source, digest in [('UPSTREAM_LICENSE','LICENSE',LICENSE_SHA), ('UPSTREAM_README.md','README.md',README_SHA)]:
        raw = (ROOT / local).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == digest, local
        if args.upstream:
            assert fetch(RAW + source) == raw, source
    tree = None
    if args.upstream:
        data = json.loads(fetch(f'https://api.github.com/repos/sgossner/VCSL/git/trees/{REVISION}?recursive=1'))
        assert not data.get('truncated')
        tree = {entry['path']:entry for entry in data['tree'] if entry['type']=='blob'}
    records = manifest['records']
    assert len(records) == 12 and len({r['midi_note'] for r in records}) == 12
    assert {p.name for p in ROOT.glob('*.wav')} == {r['local_file'] for r in records}
    total = 0
    for r in records:
        for required in ['asset_id','source_url','source_page_url','author_or_rightsholder','license_version','license_evidence_url','accessed_on','git_blob_sha1','license_scope_check']:
            assert r[required], (r['asset_id'],required)
        assert r['license'] == 'CC0-1.0' and r['upstream_revision'] == REVISION
        assert r['redistributable_with_project'] is True and r['included_in_application'] is True
        assert r['transformations'] == 'none; hash del file originale completo'
        assert r['source_url'].startswith(RAW) and REVISION in r['license_evidence_url']
        assert Path(r['local_file']).name == r['local_file']
        path = ROOT / r['local_file']; raw = path.read_bytes(); total += len(raw)
        assert len(raw) == r['file_bytes'] and hashlib.sha256(raw).hexdigest() == r['sha256'], path.name
        blob = hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()
        assert blob == r['git_blob_sha1']
        with wave.open(str(path),'rb') as wav:
            assert (wav.getnchannels(),wav.getsampwidth(),wav.getframerate()) == (2,2,44100)
            samples = array.array('h',wav.readframes(wav.getnframes()))
        if sys.byteorder != 'little': samples.byteswap()
        peak = max(abs(s) for s in samples); dc = sum(samples)/len(samples)/32768
        rms = math.sqrt(sum(s*s for s in samples)/len(samples))/32768
        assert 0 < peak < 32767 and abs(dc) < .005
        assert abs(20*math.log10(peak/32768)-r['peak_dbfs']) < .002
        assert abs(20*math.log10(rms)-r['rms_dbfs']) < .002
        onset = next(i//2 for i,s in enumerate(samples) if abs(s)>=peak*.01)/44.1
        if args.upstream:
            assert tree[r['source_path']]['sha'] == blob and fetch(r['source_url']) == raw
            ancestors = set(Path(r['source_path']).parents) - {Path('.')}
            for file in tree:
                if Path(file).parent in ancestors:
                    assert not any(word in Path(file).name.lower() for word in ['license','readme','copyright','notice']), file
        print(f"PASS MIDI {r['midi_note']:2} {r['asset_id']:12} bytes={len(raw):7} peak={r['peak_dbfs']:7.3f}dBFS onset1%={onset:5.2f}ms SHA256={r['sha256']}")
    print(f'PASS 12 unmodified CC0 assets, {total} encoded bytes; license/readme hashes verified. Human listening remains pending.')
    if args.upstream: print('PASS official fixed-revision files, Git blobs and absence of ancestor license overrides rechecked online.')

if __name__ == '__main__':
    main()
