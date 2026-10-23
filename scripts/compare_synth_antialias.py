#!/usr/bin/env python3
"""M3.4 numerical spike: naive vs quadratic PolyBLEP vs harmonic-truncated tables.
Not a test of higher-order BLEP/BLAMP variants; triangle only in the table candidate.
NumPy is a development-only dependency. Production implementation is checked separately.
"""
import numpy as np
N=65536; T=8192
worst={'table':-999,'polyblep':-999}
for rate in [44100,48000,96000]:
 for hz in [110,440,1760,3520,7040]:
  k=round(hz*N/rate); f=k*rate/N; p=np.arange(N)*k/N%1; dt=k/N; h=int(np.nextafter(rate/(2*f),0))
  coeff=np.zeros(T//2+1,complex); j=np.arange(1,min(h,2047)+1); coeff[j]=1j*T/(np.pi*j); saw=np.fft.irfft(coeff,n=T)
  tri_coeff=np.zeros_like(coeff); odd=j[j%2==1]; tri_coeff[odd]=-1j*T*4/np.pi**2*(-1.)**((odd-1)//2)/odd**2; tri=np.fft.irfft(tri_coeff,n=T)
  def read(table,phase):
   q=(phase%1)*T; idx=q.astype(int); return table[idx]+(table[(idx+1)%T]-table[idx])*(q-idx)
  def blep(t):
   return np.where(t<dt,2*t/dt-(t/dt)**2-1,np.where(t>1-dt,((t-1)/dt)**2+2*(t-1)/dt+1,0))
  for wave in ['saw','triangle','pulse.1','pulse.5','pulse.9']:
   if wave=='saw': naive=2*p-1; table=read(saw,p); poly=naive-blep(p)
   elif wave=='triangle': naive=2/np.pi*np.arcsin(np.sin(2*np.pi*p)); table=read(tri,p); poly=None
   else:
    d=float(wave[5:]); norm=2*max(d,1-d); naive=(np.where(p<d,1.,-1.)-(2*d-1))/norm; table=(read(saw,p-d)-read(saw,p))/norm; poly=(np.where(p<d,1.,-1.)+blep(p)-blep((p-d)%1)-(2*d-1))/norm
   allowed=np.zeros(N//2+1,bool); allowed[np.arange(1,h+1)*k]=True
   def metric(y):
    power=abs(np.fft.rfft(y))**2; return 10*np.log10(max(power[~allowed].sum(),1e-30)/power[allowed].sum())
   db=metric(table); base=metric(naive); worst['table']=max(worst['table'],db)
   if poly is not None: worst['polyblep']=max(worst['polyblep'],metric(poly))
   assert db<=-50 and base-db>=20,(rate,hz,wave,db,base)
print(worst)
