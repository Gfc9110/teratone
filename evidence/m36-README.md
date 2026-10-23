# Evidenze M3.6 — 23 ottobre 2026

Codice misurato: `c4a8242b6a7dd4d7e206d29008588f47bbd45b7f`.
Baseline: `50de0cf` (M3.5). [Rapporto e limiti](../docs/M3_6_64_TRACKS_VALIDATION.md).
Host/toolchain: [m36-host.txt](m36-host.txt). Nessun push o CI remoto eseguito.

- [Gate app finale](m36-app-validation.log): 36 unit + 9 integration headless,
  55 unit + 9 integration con UI, release e v1 validate/render.
- [Gate native finale](m36-native-validation.log) e [stdout CTest](m36-native-ctest.log):
  otto suite e shared ABI, audit positivo e zero allocazioni/lock intercettati nel render.
- [ASan/UBSan iniziale](m36-asan-initial.log): sei suite passate; service falliva
  perché verificava ancora il vecchio rifiuto della 17ª traccia. Dopo la correzione
  al limite 65: [service](m36-asan-service-final.log) passa. La
  [capacità finale](m36-asan-capacity-final.log) viene rieseguita dopo la policy memoria.
  LeakSanitizer disabilitato sul runtime Apple; non si dichiara copertura LSan.
- [TSan iniziale](m36-tsan-initial.log): capacità/drums passati, service aveva lo
  stesso vecchio assert; [service corretto](m36-tsan-service-final.log) e
  [capacità finale](m36-tsan-capacity-final.log) passano separatamente.
- [Memoria prima](m36-memory-before.log) e [dopo](m36-memory-final.log): stesso
  carico di 300 cicli; la soglia RSS di 32 MiB rimane invariata. La prima esecuzione
  fallisce, la finale passa. [leaks prima](m36-leaks-before.log) e
  [leaks finale](m36-leaks-final.log) sono processi distinti, senza sanitizers/interposer.
- [Stack Clang](m36-stack-usage.txt): sole funzioni del servizio, misure arm64.
- [UI software](m36-layout.log): vero controller, 64 tracce/100k note, 1.490 visibili,
  120 operazioni per ciascuna scala. PNG `m36-64-*` e altre regressioni `m36-*.png`.
  Le immagini a 2x contengono il doppio dei pixel per dimensione logica.
- [FFT](m36-spectra-verification.json), [generazione](m36-spectra-generation.log),
  [asset CC0 offline](m36-cc0-verification.log) e [regressione v1](m36-v1-regression.log).
- [Output device](m36-audio-output-devices.json), [build misura](m36-measurement-build.log),
  [smoke CoreAudio](m36-coreaudio-smoke.jsonl), [10 start/stop reali](m36-coreaudio-lifecycle.log).
- [Tentativo paced 32 non valido](m36-paced32-invalid-window.jsonl): finestra non
  continua, valori fuori soglia conservati; non è il gate finale.
  [64 paced preliminare](m36-paced64-preliminary.jsonl): sotto altro carico,
  nove blocchi fuori budget; non dimostra 64 tracce senza glitch.

[CoreAudio 32 tracce/600 s](m36-coreaudio32-600.jsonl) e
[gate automatico](m36-performance-acceptance.log): PASS, finestra continua,
p99 1,573 ms, massimo 3,014 ms, zero blocchi oltre budget, device error e notifiche
di processor overload. [CoreAudio 64 tracce/60 s](m36-coreaudio64-60.jsonl):
p99 2,002 ms, massimo 2,896 ms e nessun blocco oltre budget/errore/overload;
è una prova breve, non una certificazione di dieci minuti a 64 tracce.
[Exit code dei tre passi finali](m36-soak-status.json): tutti zero.
`device_underruns: null` significa dato non esposto, non zero. Le notifiche di
processor overload sono una misura distinta. L'ascolto, il buffer hardware a 256,
i restanti device test, Windows/Intel e la GUI nativa restano gate separati.
