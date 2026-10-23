# OpenDAW M3.3 — clip e piano roll

23 ottobre 2026. Baseline verificata: `9897b1f` (M3.2), working tree iniziale
pulito, checkout `/Users/giorgiofausto.canu/Downloads/OpenDAW_Interactive`.
Implementazione incrementale sul codice aggiornato; dipendenze e motore C++ invariati.

## Funzioni e uso

- `+ Clip` crea una clip MIDI vuota da quattro beat dopo la clip selezionata;
  senza selezione usa la posizione progetto. Doppio clic su una parte vuota della
  corsia crea una clip in quel punto, con snap a 1/16.
- `Duplicate` nella barra **CLIPS** copia la clip subito dopo l'originale, con
  nuovi ID della clip e di tutte le note. Lo strumento resta quello della traccia;
  le note sono indipendenti. La duplicazione di una traccia continua invece a
  creare un'istanza di strumento indipendente, come in M3.1.
- `Remove` elimina la clip selezionata e le sue note. Si può arrivare a una traccia
  senza clip e crearne una nuova. Il campo a destra rinomina la clip con Invio.
- Trascinare il corpo di una clip per spostarla, anche su un'altra traccia;
  trascinare il bordo destro per ridimensionarla. Il riquadro di anteprima non
  modifica il documento fino al rilascio. Cancel/perdita del focus annulla il gesto.
  Il trascinamento al bordo scorre la vista. La clip selezionata è disegnata sopra
  eventuali clip sovrapposte e viene resa raggiungibile orizzontalmente.
- La tastiera verticale a sinistra contiene nomi delle note, tasti chiari/scuri,
  Do evidenziati e tooltip con numero MIDI. Il colore dei Do prosegue nella griglia.
  Un clic/pressione fa audition dello strumento della traccia, senza registrare.
- Scroll verticale con rotella/trackpad e scrollbar; barra orizzontale per il tempo
  locale della clip. `X−/X+` e `Y−/Y+` separano i due zoom, `Oct−/Oct+` spostano
  l'ottava e `Fit notes` centra/adatta la vista entro i limiti di zoom.
- `Middle C` sceglie C3/C4/C5: il default è **MIDI 60 = C4**. Cambiano solo le
  etichette; pitch salvato, frequenza, velocity ed export restano invariati.
- Selezionare una nota e usare `Velocity`, da 1 a 127. Le modifiche partecipano
  alla storia e al salvataggio; come gli altri edit della partitura fermano il
  playback. Nomi di tracce/clip e mix possono continuare live.

## Decisioni, modello e storia

Il formato rimane v2: questi dati erano già rappresentabili in `MidiClip` e
`Note.velocity`. Nessun campo futuro di synth/drum è stato introdotto. Il colore
clip segue la traccia. MIDI clip sovrapposte sommano gli eventi; non sono take lanes,
comping o audio clip. Il selettore delle clip permette di raggiungere le sovrapposte.

La durata del progetto si estende quando add/duplica/spostamento/resize supera
l'estremo corrente, entro il limite già validato di 24 ore. Estensione e operazione
clip formano **un solo passo undo**; redo ripristina gli stessi ID e lo stesso
ordine. Una normale eliminazione non accorcia automaticamente la sessione.
Il ridimensionamento che taglierebbe una nota è rifiutato con spiegazione; non
viene effettuato trimming distruttivo. Tentativi di edit strutturali possono
fermare il playback anche se la validazione rifiuta il cambiamento.

`AddClip`, `DuplicateClip`, `RemoveClip`, `MoveClip`, `ResizeClip`, `ClipName`,
`Velocity` e le rispettive inverse verificano limiti, ID, destinazione e somme
prima di mutare il documento. Le inverse conservano posizione nell'array e durata
precedente del progetto. `AddNote` alloca il nuovo ID nella stessa transazione;
un inserimento non valido non consuma un ID. Rimangono 16 tracce, 4.096 clip e
100.000 note. Non è stata aumentata implicitamente la capacità nativa.

Viewport e notazione sono stato della sessione UI, fuori dalla storia musicale e
dal manifest. Il viewport viene ricordato per ID clip, anche dopo trasferimento;
la cache ha al massimo 4.096 elementi. La notazione è comune alla sessione e non
viene conservata tra riavvii. I limiti di zoom sono 12–32 px per semitono e
24–256 px per beat; una partitura con estensione superiore richiede scroll.

## Coordinate e audition

`editor::Viewport` è la trasformazione comune a scene, tastiera, hit test,
inserimento e drag. La coordinata verticale è
`y(p) = (127 - p) * row_height - scroll_y`, con offset frazionario e pitch 0–127.
I rettangoli delle note hanno un margine di un pixel nella stessa riga del tasto.
Il TouchArea del piano roll comincia **dopo** la tastiera e invia coordinate locali:
la tastiera non può essere interpretata come una posizione di inserimento.
Righello, snap e playhead usano l'origine locale; il cursore sottrae l'inizio
assoluto della clip. I modelli dei tasti restano stabili durante hold/ACK/telemetria.

Un gesto di note editing invia ID traccia/clip, token e viewport visualizzato.
Move/Up si riferiscono a quel gesto e non possono finire su una nuova selezione.
Ogni audition ha token, traccia, istanza, pitch e velocity espliciti; la release
recupera la destinazione originale dal token. Due note live dello stesso pitch
possono avere ID nativi indipendenti. Le release ripetute sono innocue.

Uscita dalla finestra, cancel, focus perso, cambio selezione, rimozione della traccia
e chiusura rilasciano le note. Una via prioritaria `RELEASE` con epoca audition
separata invalida gli On ancora in coda senza cancellare Play o edit persistenti.
Saturare una release richiede il panic già previsto. Il normale note-off lascia
terminare il breve inviluppo di release dello strumento; non è uno stop del brano.

Il controller Rust resta l'unico produttore nativo e il proprietario del documento.
Preparazione/replace avvengono dopo stop/join; export usa un motore separato.
Nessun callback Rust/UI, filesystem, allocazione o lock è stato aggiunto al callback
C++. Il bridge usa il parametro velocity già esistente nella C ABI.

## Verifiche eseguite

Host: Apple M4 Pro, macOS 15.6.1 arm64; Slint 1.18.1 e lockfile invariati.

- `validate_app.sh`: **31 test senza UI / 45 con UI**, entrambi i binari release,
  progetto v1 validato e renderizzato.
- `validate_native.sh`: **4 suite**, 18 controlli servizio e integrazione C ABI;
  allocation probe con 10.001 render e zero `new/delete` C++ rilevati.
- Modello: estensione e undo/redo in un passo, ID/ordine/contenuto dopo trasferimento,
  rifiuto di crop/overflow/ID/range invalidi senza mutazioni parziali del documento,
  limite 4.096 clip e recupero di una traccia vuota.
- Save/reopen di una traccia con due clip e 20 note e sua duplicazione indipendente;
  modifica velocity e trasferimento della copia senza cambiare l'originale.
- Audio: clip duplicate/trasferite/sovrapposte contro la partitura appiattita,
  nessuna nota persa; velocity live esplicita modifica il suono; release raggiunge
  silenzio. WAV v1 identico byte per byte al riferimento M3.1/M3.2:
  1.536.044 byte, SHA-256
  `c5e43d8f56299da06bdc6e2c167ba0690cb858f48cf70c2df676a86f4d501426`.
- Coordinate unitarie MIDI 0/60/127, righe parziali e quattro altezze; C3/C4/C5
  cambiano soltanto label; NaN/Inf e posizioni negative rifiutati.
- Probe software collegato al vero controller: clic e drag clip, resize, undo/redo,
  doppio clic su corsia vuota, inserimento MIDI 60, velocity, salvataggio;
  audition hold/release, PointerExited, focus perso e cambio traccia.
  Regressioni M3.2 su header, mix, inspector, scroll e riordino ancora passate.
- Layout 1280×820 e 1000×760 a 100%, 1000×760 logici a 200%; tre zoom verticali
  renderizzati (12/20/32 px), centri tasto/nota allineati entro un pixel logico.
  PNG ispezionati, comprese label dei Do e note ai bordi.
- Smoke benchmark release su progetto da 16 tracce/100.000 note: scena piano+
  tastiera p95 **0,021 ms**, massimo 300 note / 25 tasti nel viewport misurato;
  arrangement p95 0,753 ms. Non è una misura del frame UI o della latenza hardware.
  Dati e carico completo sono nel log performance.

Evidenze: `evidence/m33-*`. Gli storici M3.1/M3.2 sono conservati.
C++/C ABI non modificati; ASan/UBSan/TSan restano le prove registrate in M3.2,
non nuove esecuzioni M3.3. Audio fisico, Windows, macOS Intel, multi-monitor,
touch e accessibilità completa richiedono ancora prove sui rispettivi sistemi.

## Riproduzione e stato M3

```sh
./scripts/validate_app.sh
./scripts/validate_native.sh build/macos-validation
cargo run --manifest-path app/Cargo.toml --locked --example layout_probe
cargo run --manifest-path app/Cargo.toml --locked --no-default-features --release --example mixer_probe
cargo run --manifest-path app/Cargo.toml --locked --release --bin opendaw
```

Riavviare il binario ricostruito dopo aver salvato il lavoro dell'eventuale app già
aperta. Restano M3.4 (synth/antialias), M3.5 (kit e asset approvati) e M3.6
(capacità, prestazioni e matrice hardware). L'handoff resta la specifica complessiva;
questo incremento non dichiara completato tutto M3.
