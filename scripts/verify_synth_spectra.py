#!/usr/bin/env python3
"""Analyze samples from synth_tests --spectra DIR (NumPy; no production dependency).
Coherent one-second FFT at exactly 44.1/48/96kHz, no window/guard bins; integer harmonics strictly below Nyquist.
Compares the real SynthNode to an independent additive reference and naive oscillators.
"""
import argparse
import csv
import json
from pathlib import Path
import numpy as np


def run(root):
    rows=[]
    for c in csv.DictReader((root/'cases.csv').open()):
        n=int(c['frames']); k=int(c['bin']); wave=int(c['wave']); duty=float(c['duty'])
        y=np.fromfile(root/c['file'],dtype='<f4').astype(float)/0.2
        assert len(y)==n and np.isfinite(y).all()
        phase=(np.arange(n)*k/n)%1
        harmonics=np.arange(1,(n//2-1)//k+1)
        allowed=np.zeros(n//2+1,bool); allowed[harmonics*k]=True
        coeff=np.zeros(n//2+1,complex)
        if wave==1:
            naive=2*((phase+.5)%1)-1
            coeff[harmonics*k]=1j*n/(np.pi*harmonics)*(-1.)**harmonics
        elif wave==2:
            norm=2*max(duty,1-duty)
            naive=(np.where(phase<duty,1.,-1.)-(2*duty-1))/norm
            coeff[harmonics*k]=1j*n/(np.pi*harmonics)*(np.exp(-2j*np.pi*harmonics*duty)-1)/norm
        else:
            naive=2/np.pi*np.arcsin(np.sin(2*np.pi*phase))
            odd=harmonics[harmonics%2==1]
            coeff[odd*k]=-1j*n*4/np.pi**2*(-1.)**((odd-1)//2)/odd**2
        reference=np.fft.irfft(coeff,n=n)
        def metric(v):
            power=abs(np.fft.rfft(v))**2
            return float(10*np.log10(max(power[~allowed].sum(),1e-30)/power[allowed].sum()))
        spur=metric(y); base=metric(naive)
        error=float(10*np.log10(max(np.mean((y-reference)**2),1e-30)/np.mean(reference**2)))
        # Same gate above 4kHz, fixed by the M3.4 spike before accepting the algorithm.
        assert spur<=-50 and base-spur>=20,(c,spur,base)
        assert error<=-50,(c,error)
        rows.append(dict(c,spurious_dbr=spur,naive_dbr=base,improvement_db=base-spur,reference_error_dbr=error))
    assert len(rows)==75
    summary={'cases':len(rows),'worst_spurious_dbr':max(r['spurious_dbr'] for r in rows),'minimum_improvement_db':min(r['improvement_db'] for r in rows),'worst_reference_error_dbr':max(r['reference_error_dbr'] for r in rows),'gate':'<= -50 dBr and >= 20 dB improvement, including 7040 Hz','method':'One-second coherent captures; exact MIDI pitches 110/440/1760/3520/7040Hz at exactly 44.1/48/96kHz; no window, no excluded guard bins; independent additive reference, DC included as spurious'}
    (root/'results.json').write_text(json.dumps(dict(summary=summary,cases=rows),indent=2)+'\n')
    print(json.dumps(summary,indent=2))


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('directory',type=Path);run(parser.parse_args().directory)
