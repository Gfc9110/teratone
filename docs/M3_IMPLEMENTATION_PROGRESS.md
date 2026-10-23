# OpenDAW — avanzamento M3

Aggiornamento del 22 ottobre 2026 nel checkout aggiornato
`/Users/giorgiofausto.canu/Downloads/OpenDAW_Interactive`.
La fondazione M3.1 è stata prima salvata nel commit `966a8c4`, come richiesto.
La baseline precedente era `add95d9cfb7f9ffc38e43a6754365fac8a880a53`.
Nessun recupero del vecchio ZIP e nessun aggiornamento delle dipendenze.

## M3.3 implementato — 23 ottobre 2026

Baseline `9897b1f`, poi gestione clip e piano roll nello stesso checkout aggiornato.

- Clip: creazione, rinomina, rimozione, duplicazione indipendente, drag tra tracce,
  resize non distruttivo, snap 1/16, selezione e anteprima. Estensione del progetto
  e operazione fanno un solo undo; ID e ordine conservati al redo.
- Tastiera verticale con nomi a sinistra, Do evidenziati e tooltip MIDI;
  scroll frazionario, scrollbar, zoom X/Y, ottave e Fit notes.
- MIDI 60 = C4 per default; C3/C4/C5 configurano la sola notazione di sessione.
  Velocity della nota selezionata modificabile da 1 a 127.
- Audition con token e destinazione originale, rilascio su exit/cancel/focus/cambio
  traccia e una via prioritaria separata dai comandi del documento.
- Verifica: 31 test headless / 45 con UI, build release, 4 suite native, probe UI
  100/200% con tre zoom verticali, save/reopen e confronto audio v1 identico.
  Nessuna modifica al C++, alle dipendenze o allo schema v2.

[Uso, decisioni, contratti e verifiche M3.3](M3_3_CLIPS_PIANO_ROLL.md).
Evidenze nuove `evidence/m33-*`; le sezioni seguenti conservano lo storico M3.2.

## M3.2 implementato: arrangement, inspector e mixer

- Arrangement con header e corsia nello stesso contenitore verticale: nome,
  colore, tipo d'istanza, selezione, mute, solo e stato di udibilità. Timeline,
  righello e cursore si spostano orizzontalmente mentre gli header restano fissi.
- Anteprime delle clip MIDI esistenti; clic sulla clip seleziona la stessa traccia
  e clip nell'editor. Il modello resta unico: nessuna copia modificabile delle note.
- Aggiunta, duplicazione, rimozione, rinomina, colori, frecce e trascinamento dal
  simbolo `≡`. Il drag ha soglia, indicatore di destinazione e scorrimento al bordo;
  il riordino viene registrato una sola volta al rilascio. `Show selected` rende
  raggiungibile la traccia selezionata. Restano 16 tracce e almeno una obbligatoria.
- Inspector richiudibile con schede Track / Project: nome, colore, istanza,
  uscita Master, volume/pan, mute/solo, ordine e sampler; progetto, tempo, save/open
  ed export nell'altra scheda. Arm non è mostrato perché non esiste registrazione.
- Il modello visuale delle tracce viene conservato e aggiorna solo le righe
  cambiate, salvo cambi di ordine/numero. Lo scroll non si azzera dopo un mix edit.
  Campi di testo non ancora confermati resistono agli aggiornamenti estranei;
  campi e slider si riallineano quando cambia la traccia selezionata.
- Mute/solo sono proprietà persistenti indipendenti dal fader e con undo/redo.
  Regola unica: `audible = !mute && (!any_solo || solo)`. Più solo restano udibili;
  mute prevale su solo. Anche un solo su una traccia muta esclude le tracce non solo.
- Ogni traccia ha un gate dopo GainPan, prima del Master. Il gate live fa una rampa
  lineare di 5 ms (240 frame a 48 kHz), fino a zero esatto. Le voci continuano a
  elaborare durante mute: unmute non riavvia note o inviluppi. Lo stato iniziale di
  un progetto caricato/export è già corretto dal primo campione.

Creazione/spostamento/ridimensionamento delle clip, tastiera verticale e velocity
sono state successivamente implementate in M3.3, come descritto sopra. Non sono stati implementati nuovi synth, pad, routing condiviso o
asset batteria. Gli edit strutturali continuano a fermare il playback.

## Contratti di consegna e real-time

Rust conserva il documento autorevole. Comandi UI identificati da request ID;
ACK con revisione, errore ed eventuale cancellazione del gesto/transport. Outbox
UI limitata a 256 richieste più 64 in volo, canale richieste 64 e risposte 8.
Move/viewport e aggiornamenti continui contigui dello stesso fader sono
coalescenti; fine gesto e comandi persistenti sono barriere. Una saturazione
rifiuta esplicitamente solo la nuova azione; quelle già accettate restano in coda.
I toggle conservano ogni clic anche se il modello UI attende ancora la risposta.

Stop/Panic usano una via atomica prioritaria e una nuova epoca per ignorare vecchi
Play/audition/gesti. **Non svuotano modifiche persistenti** come nome, ordine,
mute/solo, save e storia. Una release audition non accodabile richiede panic.
Il numero di azioni pendenti e la sincronizzazione del mixer sono visibili.

Il controller invia al C++ snapshot completi gain/pan/mute/solo, in ordine di slot
verificato rispetto agli ID compilati. ABI additiva: `od_load_v2`,
`od_set_mix_v1`, `od_read_mix_status_v1`; layout originali e ABI v1 conservati.
La generazione cambia dopo ogni load riuscito, così uno snapshot di una vecchia
sessione non può modificare gli slot nuovi. La coda mixer separata ha 8 posti e
viene letta al massimo 8 volte all'inizio di un blocco; applica il più recente
snapshot completo e pubblica il seriale applicato. Quel seriale indica target
applicati, non completamento della rampa o audio già arrivato all'altoparlante.

Se la coda è piena, il documento resta valido e dirty: il controller ritenta lo
snapshot completo più recente. Stop può svuotare la coda note/transport ma non
quella mixer. Dopo Stop, se manca un consumer, Play ricarica il documento desiderato
prima di aprire il dispositivo, evitando uno stato mix iniziale superato.
L'export usa sempre una propria istanza e il documento catturato all'avvio.
Il wrapper legacy `od_gain_pan` ora usa la stessa coda e può restituire
`OD_QUEUE_FULL`: un host deve controllare l'esito e ritentare.

Nessuna allocazione/deallocazione, lettura file, chiamata Rust/UI o mutex aggiunta
al callback. Preparazione, scene UI, conversione clip e persistenza restano sui
thread di controllo. L'unico consumer audio e stop/join prima del replace restano
invariati. Le istanze sampler condividono solo i dati WAV immutabili.

## Verifiche M3.2

Host: Apple M4 Pro, macOS 15.6.1 arm64; toolchain locale descritta in BUILD.md.

- Gate app: 24 test headless e 33 con UI, build release dei due binari, esempio v1
  validato e renderizzato. Nessuna modifica alla fixture v1.
- Gate native: quattro suite CTest, 18 controlli di servizio, shared-library
  integration e allocation probe. Nel probe, 10.001 render con note, seek,
  gate/mix e stop: zero `new/delete` C++ rilevati nel tratto monitorato.
- 64 combinazioni native mute/solo su tre tracce, 16 combinazioni nel bridge,
  silenzio iniziale e all-muted export, rampa, continuità delle voci, stop,
  saturazione, generazioni scadute e load transazionale.
- Round trip dei flag e default false nei v2 precedenti; undo/redo e duplicazione
  mantengono flag e fader; salvataggio/export atomici. Test delle code con burst di
  1.000 slider, ACK, Stop e modifiche persistenti rimaste pendenti.
- Software UI collegata al vero controller: 1280×820 e 1000×760 a 100%; 1000×760
  logici a 200%. Clic reali su mute/solo cumulativi, due toggle rapidi, Undo,
  selezione clip, inspector e hide/show. Sessione con 12–14 tracce: scroll,
  conservazione scroll dopo edit, drag con auto-scroll, undo in un passo,
  Add/Duplicate/Show selected, Help/About e blocco della finestra sottostante.
  Divisori header/corsia verificati nei pixel entro un pixel logico anche dopo scroll.
  Immagini renderizzate ispezionate; non è una prova della GUI nativa su hardware.
- ASan/UBSan: quattro suite passate. Il primo tentativo con LeakSanitizer è stato
  rifiutato dal runtime Apple (`detect_leaks` non supportato); eseguito con
  `detect_leaks=0`, senza dichiarare copertura leak. TSan separato: 18 controlli
  passati, inclusi snapshot mixer e consumer silent concorrenti.
- Regressione WAV v1: confronto byte per byte con il riferimento M3.1;
  1.536.044 byte, SHA-256
  `c5e43d8f56299da06bdc6e2c167ba0690cb858f48cf70c2df676a86f4d501426`.

Misura smoke su questo host (build release, non audio fisico): scene arrangement
p95 0,602 ms con 16 tracce / 100.000 note, massimo 400 rettangoli di thumbnail;
prepare 15,411 ms sul controllo; 5.000 chiamate ABI da 256 frame a 48 kHz con
snapshot periodici, p95 0,110 ms, massimo 0,392 ms. Il tempo ABI include il buffer
Rust allocato fuori dal callback. I valori sono un campione locale, non una
promessa su altri host o un benchmark dei frame UI.

Log e PNG nuovi: `evidence/m32-*`. I file `evidence/m3-*` restano lo storico M3.1
(18 test headless / 21 UI, quattro suite native e primo probe software).
Il probe `mixer_probe` misura separatamente scene con 16 tracce / 100.000 note e
render ABI headless; valori e carico esatto sono nel log. Include l'allocazione del
buffer Rust fuori dal callback e **non** certifica latenza hardware, p99 del solo
callback o il futuro gate 32 tracce × 8 voci / 64 tracce di M3.6.

## Come provarlo

Salvare il lavoro dell'eventuale app già aperta e avviare il binario ricostruito:

```sh
cargo run --manifest-path app/Cargo.toml --locked --release --bin opendaw
```

Clic su header/clip; `M` e `S` nell'header oppure Mute/Solo nell'inspector.
Trascinare `≡` per riordinare; il drag al bordo scorre la lista.
Usare la barra sotto le corsie o lo scroll orizzontale per la timeline.
Il pulsante Inspector recupera spazio. Save/Open/Export sono nella scheda Project;
Open continua a rifiutare la sostituzione di un documento con modifiche non salvate.
Gli edit di note, ordine, struttura e strumento fermano il playback: premere Play.
Mute/solo, volume/pan, nomi/colori e la loro storia possono aggiornarsi live.

Verifica software senza dispositivo fisico:

```sh
cargo run --manifest-path app/Cargo.toml --locked --example layout_probe
cargo run --manifest-path app/Cargo.toml --locked --no-default-features --release --example mixer_probe
```

## Limiti e passi successivi

M3 nel suo insieme resta aperto: M3.4 synth e antialias,
M3.5 kit/asset verificati e ascoltati, M3.6 64 tracce e matrice hardware.
Nessuna prova di audio fisico CoreAudio/WASAPI, sleep/wake, device removal,
Windows o macOS Intel svolta in M3.2. Il benchmark software non sostituisce
accessibilità completa, tastiera/drag nativi, multi-monitor o touch.
I thumbnail aggregano al massimo 32 rettangoli per clip e 256 clip visibili;
la limitazione è mostrata e non tronca playback/export. I righelli arrangement
sono generati dal viewport, anche oltre i primi 256 beat.

Gli asset referenziati solo dalla storia undo mantengono il precedente limite:
non sono raccolti in un archivio persistente; mantenere le sorgenti durante la
sessione. Non è stata introdotta pulizia automatica degli asset.
Il confronto antialias M3.0 resta da chiudere prima dei nuovi DSP M3.4.

Specifica finale: [handoff M3](handoff/M3_TRACKS_PIANOROLL_DRUMS_SYNTH.md).
