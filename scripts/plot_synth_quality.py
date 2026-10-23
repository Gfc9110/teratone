#!/usr/bin/env python3
"""Optional development-only plots (NumPy + Matplotlib). No role in runtime or acceptance gates."""
import argparse
import json
from pathlib import Path
import wave
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('results',type=Path)
parser.add_argument('motion',type=Path)
parser.add_argument('destination',type=Path)
args=parser.parse_args()
d=json.loads(args.results.read_text())
fig,axes=plt.subplots(1,2,figsize=(11,4),layout='constrained')
for waveform,label,marker in [(1,'Saw','o'),(2,'Square/PWM','s'),(3,'Triangle','^')]:
    rows=[r for r in d['cases'] if int(r['wave'])==waveform]
    for i,key in enumerate(['spurious_dbr','reference_error_dbr']):
        axes[i].scatter([float(r['frequency']) for r in rows],[r[key] for r in rows],s=24,marker=marker,label=label,alpha=.65)
for ax in axes:
    ax.set_xscale('log');ax.set_xlabel('Fundamental (Hz)');ax.set_ylabel('dBr')
    ax.axhline(-50,color='red',ls='--',label='Acceptance -50 dBr');ax.grid(alpha=.2)
axes[0].set_title('Energy outside permitted harmonics')
axes[1].set_title('Error vs independent additive reference')
axes[0].legend(fontsize=8)
fig.suptitle('OpenDAW M3.4: 75 coherent captures at 44.1 / 48 / 96 kHz')
fig.savefig(args.destination/'m34-synth-spectra.png',dpi=140);plt.close(fig)
with wave.open(str(args.motion),'rb') as f:
    audio=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,2)[:,0]/32768.
    rate=f.getframerate()
fig,ax=plt.subplots(figsize=(11,4),layout='constrained')
with np.errstate(divide='ignore'):
    ax.specgram(audio,NFFT=2048,Fs=rate,noverlap=1536,cmap='magma',vmin=-110,vmax=-30)
ax.set_ylim(0,rate/2);ax.set_xlabel('Time (s)');ax.set_ylabel('Hz')
ax.set_title('MIDI 45→117 stepped sweep · waveform crossfades and animated PWM (0.1–0.9)')
fig.savefig(args.destination/'m34-synth-motion.png',dpi=140);plt.close(fig)
print('Saved FFT quality plot and motion spectrogram')
