# OpenDAW — avanzamento M3

Aggiornamento del 22 ottobre 2026. Baseline iniziale:
`add95d9cfb7f9ffc38e43a6754365fac8a880a53` nel checkout
`/Users/giorgiofausto.canu/Downloads/OpenDAW_Interactive`.

## Primo incremento implementato: fondazione M3.1

- Elenco delle tracce nella sidebar, con nome, tipo,
  selezione e colore; lista e pannello hanno scroll per non perdere i controlli.
- Aggiunta di oscillatori, duplicazione anche dei sampler, rimozione, spostamento
  su/giù, rinomina e scelta di sei colori. Il limite resta 16, coerente con il
  motore attuale; l'ultima traccia non può essere eliminata.
- Istanze identificate separatamente dalla traccia; duplicazione profonda di
  configurazione, clip e note con nuovi ID. Il contenuto WAV immutabile può
  restare condiviso senza condividere voci o configurazioni modificabili.
- Comandi traccia, mix, import e audition con ID espliciti, senza usare l'indice
  visuale come identità. Selezione riparata dopo eliminazione, reorder e storia.
- Nuovo formato v2 con clip MIDI e tempo locale delle note; selettore delle clip
  esistenti nell'editor. Le vecchie note vengono migrate in una clip `Main` senza
  duplicare il contenuto in due strutture.
- Migrazione v1 in memoria e salvataggio v2 con checkpoint v1 dedicato,
  conservato anche dopo successivi salvataggi. Comando CLI `migrate` disponibile.
- Undo/redo delle operazioni traccia e ripristino della posizione originale delle
  note eliminate. I gesti separati sui cursori gain/pan hanno undo separati.
- Nome e colore, compreso il loro undo, non fermano l'audio. La selezione rilascia
  l'audition invece di inviare un panic globale. Gli edit strutturali continuano
  a fermare il dispositivo prima della ricompilazione.
- Il bridge appiattisce le clip con i corretti offset, conservando C ABI, grafo,
  DSP e ownership C++ esistenti. Nessuna dipendenza aggiunta o aggiornata.

## Verifica eseguita

Su macOS 15.6.1 / Apple Silicon:

- `scripts/validate_app.sh`: exit 0; 18 test senza UI, 21 con UI, build release
  di entrambi i binari, validazione e render del progetto esempio v1.
- `scripts/validate_native.sh build/macos-validation`: exit 0; 4 suite CTest,
  allocation probe e verifica della C ABI condivisa passati.
- Confronto WAV del progetto v1 migrato: identico byte per byte alla baseline,
  1.536.044 byte; SHA-256
  `c5e43d8f56299da06bdc6e2c167ba0690cb858f48cf70c2df676a86f4d501426`.
- Test del modello: 100 sequenze di operazioni/storia, ID e limiti, migrazione
  senza riscrittura del v1, backup v1 dopo salvataggi ripetuti, sampler duplicato
  portabile dopo rimozione della sorgente, posizioni delle clip e render dopo
  reorder, metadati live e comandi ritardati destinati alla traccia originale.
- Probe software del layout a 1280×820 e 1000×760: Play, Undo, aggiunta e
  duplicazione ricevono clic; Help/About apre, blocca il transport e si chiude.
  Immagini ispezionate per controllare leggibilità e allineamento.

Log e immagini hanno prefisso `evidence/m3-`. Questo non sostituisce una prova
interattiva completa su GUI nativa, audio fisico, Windows o macOS Intel, che non
sono stati verificati in questo incremento. Non sono dichiarate misure nuove di
latenza, prestazioni grandi sessioni o qualità sonora.

## Come provarlo

Salvare eventuali modifiche nell'app già aperta e rilanciare il binario ricostruito:

```sh
cargo run --manifest-path app/Cargo.toml --locked --release --bin opendaw
```

La sidebar contiene `+ Oscillator`, `Duplicate`, frecce, `Remove`, campo nome e
colore. Per un'altra istanza sampler, aggiungere una traccia e caricare il WAV su
quella traccia, oppure duplicare un sampler esistente. Un edit strutturale ferma
il playback: premere Play per riprendere. Aprire un v1 non cambia il file; Save
scrive v2 e conserva `recovery/pre-m3-v1.json`.

## Passi successivi e decisioni esplicite

M3 nel suo insieme non è completato. Restano:

1. M3.2: arrangement con header/corsie allineati, inspector, mute/solo reali,
   riordino con drag, telemetria e gestione/coalescing delle code più robusta.
2. M3.3: gestione grafica delle clip, tastiera verticale, notazione configurabile,
   zoom verticale e controllo velocity. Una traccia v2 senza clip è leggibile ma
   non permette inserimento di note finché non esiste una clip.
3. M3.4: forme d'onda e parametri/preset indipendenti con antialias misurato.
4. M3.5: kit multi-pad e asset CC0, import, choke e approvazione sonora. Nessun
   campione della ricerca è stato integrato nell'applicazione.
5. M3.6: aumento coordinato a 64 tracce, test prestazionali e hardware Mac/Windows.

Il formato v2 di questo incremento contiene solo i campi supportati. Campi mute,
pad, preset e riferimenti asset con hash verranno introdotti insieme alle relative
funzioni, con default/migrazioni espliciti o ulteriore versione se necessaria.
Il confronto DSP previsto in M3.0 è rinviato all'inizio di M3.4 perché questo
incremento non cambia oscillatori o API audio. La verifica baseline di M3.0 è
stata completata.

Sono preservati alcuni limiti del percorso originale: gli eventi UI ad alta
frequenza possono ancora saturare la coda e il percorso Stop cancella le azioni
pendenti con avviso; i miglioramenti di consegna/ack sono parte di M3.2. Gli asset
referenziati solo da undo non vengono raccolti in un archivio persistente della
storia: mantenere le sorgenti disponibili durante la sessione; gestione completa
con riferimenti immutabili prevista prima del gate asset M3.5.

L'handoff completo resta la specifica finale:
[tracce, piano roll, batteria e synth](handoff/M3_TRACKS_PIANOROLL_DRUMS_SYNTH.md).
