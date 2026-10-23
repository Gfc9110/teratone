# OpenDAW — Handoff M3: tracce, piano roll, batteria e synth per traccia

**Data:** 22 ottobre 2026, Europe/Rome. **Stato:** specifica di lavoro, nessuna funzionalità M3 implementata in questo incarico.

**Baseline ispezionata:** `/Users/giorgiofausto.canu/Downloads/OpenDAW_Interactive`, branch `main`, commit `add95d9cfb7f9ffc38e43a6754365fac8a880a53` — `Restore visible transport controls and move Slint attribution to About`. Il working tree era pulito prima della creazione di questo handoff. Il vecchio ZIP non è stato usato come fonte del codice corretto.

**Obiettivo:** rendere riconoscibile e utilizzabile una sessione multitraccia, con istanze indipendenti dello stesso strumento, editor delle note leggibile, kit di percussioni interno e synth configurabile per traccia, conservando backend C++20 e frontend Rust/Slint.

## Stato di esecuzione aggiornato — 23 ottobre 2026

La specifica e l'audit sotto conservano la baseline del 22 ottobre. Nel repository
aggiornato sono poi implementate M3.1–M3.5 e **M3.6, capacità fino a 64 tracce**
(codice `c4a8242`, baseline M3.5 `50de0cf`). I limiti correnti e i risultati misurati
sono nel [rapporto M3.6](../M3_6_64_TRACKS_VALIDATION.md) e nell'
[avanzamento M3](../M3_IMPLEMENTATION_PROGRESS.md); non dedurli dalle descrizioni
storiche «attuale» della sezione 2.

Il software verifica 64 tracce indipendenti, 100.000 note, UI software 100/200%,
save/undo/export, code, allocazioni/lock intercettati, sanitizers e regressioni FFT/v1.
Il rapporto distingue le misure audio native dai tentativi preliminari, il blocco
DSP interno dal buffer hardware e RSS da heap attivo/budget media. Nessuna soglia
prestazionale è considerata soddisfatta sulla sola compilazione.

L'utente accetta per questa iterazione il kit di 12 suoni CC0 già verificati,
**senza averlo ascoltato** e con possibilità di sostituzione. Il manifest registra
la scelta provvisoria; il gate di ascolto non è segnato come eseguito.

**Il DoD complessivo resta aperto**: Windows x64/WASAPI, Mac Intel, GUI nativa
(menu/focus/Tab/accessibilità), ascolto, I/O hardware a 256, 44,1 kHz reale,
sleep/wake e cambio/rimozione device richiedono ancora i rispettivi test.
Le checkbox originali sono criteri di specifica, non un'attestazione automatica
di chiusura. Proseguire in piccoli step e conservare le evidenze già raccolte.

## 1. Come usare questo handoff

Il coding agent deve partire dal repository aggiornato, leggere le istruzioni locali e confrontare il commit e gli hash di `M3_BASELINE_AUDIT.json` con i file effettivi. Se il repository non è accessibile, richiedere quello aggiornato; non ricostruire la baseline dal precedente archivio. Se ci sono modifiche successive, ispezionarle e adattare questa specifica senza sovrascriverle.

Le sezioni «Verificato» descrivono il codice letto. Le sezioni «Proposto» e le milestone descrivono lavoro futuro. I nomi di comandi e strutture M3 sono contratti proposti, non API già disponibili. Le soglie prestazionali sono obiettivi da misurare, non risultati ottenuti.

Allegati di questo documento:

- [Audit della baseline e hash di 37 file](M3_BASELINE_AUDIT.json).
- [Log di validazione applicazione](M3_BASELINE_APP_VALIDATION.log) e [log di validazione nativa](M3_BASELINE_NATIVE_VALIDATION.log).
- [Registro completo dei 12 asset verificati](M3_ASSETS_VERIFIED.json) e [versione CSV](M3_ASSETS_VERIFIED.csv).

Il pacchetto di handoff contiene documentazione e prove; non contiene WAV, nuovi preset eseguibili o modifiche alle funzionalità. I WAV sono stati scaricati soltanto in un'area temporanea di ricerca per verificarne i byte. La loro integrazione e l'approvazione sonora spettano alla milestone dedicata.

## 2. Baseline reale — verificato nel codice

### 2.1 Struttura e responsabilità

| Componente attuale | File reali e simboli verificati | Stato e limite rilevante |
|---|---|---|
| Build nativa C++20 | `CMakeLists.txt` | Target `open_daw_core`, `open_daw_engine`, `open_daw_api_static`, `open_daw_api`, `daw_demo`, `daw_session`. |
| Grafo e DSP | `include/open_daw/engine.hpp`, `src/engine.cpp`: `Graph`, `InstrumentNode`, `SynthNode`, `GainPanNode`, `MixerNode` | Grafo preparato fuori dal callback; synth sinusoidale a 24 voci. `SynthNode::setADSR` esiste come configurazione di setup, ma non come controllo UI/runtime. Gain/pan hanno smoothing di circa 5 ms. |
| Servizio di esecuzione | `include/open_daw/service.hpp`, `src/service.cpp`: `SessionSpec`, `TrackSettings`, `EngineService` | Crea un nuovo `SynthNode` oppure `SamplerNode` per ciascuna traccia, poi un gain/pan e il master. `MaxTracks = 16`. ADSR del synth fissato dal costruttore del servizio. |
| Sampler e media | `include/open_daw/media.hpp`, `src/media.cpp`: `SampleData`, `SamplerNode`, `loadWav`, `WavStreamWriter` | Un campione per strumento, 24 voci, root note, modalità gated/one-shot, interpolazione lineare. Non è un kit multi-pad. |
| C ABI | `include/open_daw/api.h`, `src/api.cpp`: `od_engine`, `od_load`, `od_live_note`, `od_gain_pan`, `od_export` | ABI v1, handle opaco e un solo thread di controllo. `od_track` distingue sine/sample tramite il percorso del campione; non possiede un tipo synth/drum esplicito. |
| Package Rust | `app/Cargo.toml`, `app/build.rs`, `app/src/lib.rs` | Un package `opendaw-app`, libreria `opendaw_app`, binari `opendaw` e `opendaw-project`. Non esistono crate separati per project/core/plugin. Slint e slint-build sono fissati a 1.18.1; `Cargo.lock` esiste ed è tracciato. |
| Documento e storia | `app/src/project.rs`: `Project`, `Track`, `Instrument`, `Note`, `Edit`, `History` | Schema 1, 960 PPQ, tempo costante, `Track.notes`, varianti `Sine` e `Sampler`. ID stabili globali, undo/redo a inverse fino a 512 passi. Mancano clip, colori, mute/solo e comandi CRUD delle tracce. |
| Coordinamento | `app/src/controller.rs`: `Request`, `ViewState`, `State`, `spawn` | Il worker possiede documento, storia e wrapper nativo. La selezione è un indice di traccia; mix/audition implicano la traccia selezionata. Le modifiche strutturali fermano l'audio e richiedono ricompilazione. |
| Interazione editor | `app/src/editor.rs`: `Viewport`, `Gesture`, `NoteRect`, `label` | Culling e hit test, snap a 1/16, righe fisse di 16 px, massimo 3.000 rettangoli visibili. `label(60)` produce `C4`. La costruzione della scena scansiona le note della traccia. |
| Persistenza/export | `app/src/storage.rs`: `load`, `save`, `export_atomic`; `app/src/bridge.rs`: `Engine` | Salvataggio con asset copiati e sostituzione del manifest; checkpoint precedente. Export su worker e motore indipendente. Il bridge preserva la durata degli array/stringhe nelle chiamate sincrone. |
| UI | `app/ui/main.slint`, `app/src/main.rs` | Selezione traccia in `ComboBox`, mix della selezionata, piano roll con etichette dentro le note, pulsanti ottava/zoom e otto pad di audition melodica. Non ci sono tastiera verticale, arrangement multitraccia o inspector per tipo di strumento. |
| Dispositivi | `src/device.cpp`, `include/open_daw/device.hpp` | Percorsi CoreAudio/macOS, WASAPI/Windows e clock silenzioso. `src/wasapi.cpp` appartiene anche al percorso demo storico: per M3 seguire il lifecycle del servizio in `device.cpp`. |

**Punto essenziale:** il tipo di strumento non è già limitato a una sola traccia. `Project::demo()` ha due tracce `Sine`, e `EngineService` costruisce due synth indipendenti. M3 deve esporre creazione/duplicazione, aggiungere configurazioni persistenti per istanza e provare l'isolamento; non introdurre un singleton condiviso.

### 2.2 Contratti e limiti da preservare

- Massimo attuale: 16 tracce, 100.000 note per progetto, 24 ore, 32 MiB di manifest. La durata è in tick; il motore riceve frame derivati al sample rate scelto.
- Coda nativa SPSC da 1.024 comandi; consumo massimo di 64 comandi per blocco. Fino a 256 eventi schedulati per traccia in ogni finestra di blocco, verificati prima dell'attivazione; buffer eventi per traccia da 320 elementi.
- Gli eventi note hanno offset nel blocco e ID distinti anche a pitch uguale. Il bit alto identifica le note live. Note-off prima dei note-on schedulati sullo stesso frame.
- Stop/panic hanno un percorso atomico prioritario. Il fallimento di un note-off attiva panic. Non perdere questa garanzia aggiungendo pad o tastiera.
- I canali Rust UI→controller e controller→UI sono limitati rispettivamente a 64 e 8 messaggi. La saturazione UI oggi può cancellare azioni pendenti. M3 deve rendere affidabili i nuovi edit e limitare il traffico dei gesti.
- `od_load` prepara il candidato, ferma e chiude il dispositivo prima di sostituire/distruggere il servizio. Un errore di validazione nativo conserva la sessione precedente; il controller può aver già fermato il playback.
- Campioni immutabili possono condividere i dati decodificati; attualmente il loader deduplica per stringa di percorso, non per hash. Il budget di 128 MiB decodificati è distinto dai 128 MiB di nuove copie su disco per salvataggio.
- WAV importabili: RIFF little-endian, PCM16/24/32 o float32, mono/stereo, 8–384 kHz; niente formati compressi o WAVE_FORMAT_EXTENSIBLE. Limite per file: 2.880.000 frame sorgente, non «60 secondi» a qualsiasi frequenza.
- Seek, pause, loop ed EOF resettano le voci; non esiste note chasing. Le code sonore sono troncate a fine progetto. L'uscita viene limitata a [-1,1] e conta i campioni oltre scala; non è presente un limiter da mastering.
- Non esistono registrazione, input MIDI esterno, hosting CLAP/VST3/AU, scansione plugin, routing bus/send nel servizio o compensazione della latenza. Non presentarli come prerequisiti già risolti.

### 2.3 Controlli rieseguiti il 22 ottobre 2026

Su macOS 15.6.1, arm64:

| Controllo eseguito | Risultato osservato |
|---|---|
| `bash scripts/validate_app.sh` | Exit 0; 11 test Rust passati senza feature UI e gli stessi 11 con UI; build release dei due binari; esempio validato con 2 tracce e 10 note; rendering WAV riuscito. |
| `bash scripts/validate_native.sh build/macos-validation` | Exit 0; 4 suite CTest passate; rendering demo e verifiche Python della libreria condivisa passati. |
| Stato Git dopo i controlli, prima dell'handoff | Nessuna modifica tracciata al codice. |

Questa sessione non ha testato interazione GUI, ascolto sull'hardware, Windows, macOS Intel o Linux. `docs/BUILD.md` contiene inoltre prove storiche locali del layout, separate dai controlli qui rieseguiti. Alcuni paragrafi del README e dei documenti precedenti riportano ancora i limiti dell'ambiente originario: non usarli per dedurre che Rust sia tuttora non compilabile.

## 3. Perimetro M3 e comportamento di prodotto — proposto

### 3.1 Tracce sempre riconoscibili

Usare il [manuale ufficiale Waveform: Edit Tab](https://tracktion.github.io/waveform_manual/latest/edit-tab/) come riferimento funzionale per header, corsie, selezione e proprietà contestuali. Disegnare componenti e icone originali OpenDAW; non copiare immagini, loghi, asset, skin o sorgenti proprietarie.

Richieste M3:

1. Arrangement con una riga per traccia, header sempre visibile a sinistra e corsia clip allineata. Il menu a tendina non deve più essere il punto principale di gestione.
2. Header con nome modificabile, colore, tipo di strumento, selezione/focus distinguibili e pulsanti mute/solo. Nome, colore e ordine persistono e partecipano a undo/redo.
3. Pulsante evidente «+ Traccia» con scelta Oscillatore / Sampler / Batteria. Menu contestuale con rinomina, colore, duplica, elimina, sposta su/giù; drag di riordino con indicatore di destinazione e auto-scroll. Le alternative da menu rendono il riordino utilizzabile senza drag.
4. Duplicazione completa e indipendente: nome «… copia», nuovi ID, stesse note/clip e valori di preset iniziali. I dati audio immutabili possono essere condivisi. Eliminazione annullabile in un passo; impedire l'eliminazione dell'ultima traccia per conservare l'invariante corrente di almeno una traccia.
5. Scroll verticale unico per header e corsie; scroll orizzontale della timeline senza spostare gli header. Scroll e selezione devono restare coerenti durante riordino, undo e ridimensionamento.
6. Inspector contestuale della traccia selezionata: nome/colore, strumento/preset, parametri pertinenti, gain/pan e destinazione «Master». Il pannello può richiudersi sulle finestre strette; la selezione resta visibile.
7. Una corsia di clip MIDI per traccia in M3, con nome, colore, miniatura delle note e apertura nel piano roll. Inserimento, spostamento, duplicazione ed eliminazione di clip; nessuna promessa di comping, take lanes, audio clip o editor di routing in questa iterazione.

**Arm:** non mostrare un pulsante di registrazione funzionante. L'input/recording non è implementato nella baseline. Omettere Arm oppure renderlo disabilitato con descrizione «Registrazione non disponibile». L'audition della traccia selezionata è una funzione distinta. Abilitare Arm soltanto in una milestone futura con input, monitoraggio, destinazione e test di registrazione reali.

**Mute/solo:** solo cumulativo. Una traccia è udibile se `!mute && (!any_solo || solo)`. Mute prevale su solo. Conservare gain nominale e gate di udibilità come valori distinti; mutare non deve scrivere zero nel fader persistente. Il gate si applica con rampa di circa 5 ms. Continuare a consumare gli eventi e aggiornare le voci anche sulle tracce mute per evitare note bloccate; non introdurre risparmi CPU prima di avere una politica di riattivazione verificata. Export e playback usano la medesima regola. Non sono previsti solo-safe o bus in M3.

**Numero di tracce:** prima completare il flusso alle 16 attuali; poi portare a 64 il limite funzionale di progetto/motore/UI con un intervento dedicato e misurazioni. Un numero di tracce ammesso non garantisce qualsiasi polifonia su qualunque computer. Mostrare limite e carico in modo comprensibile; non annunciare «illimitato».

### 3.2 Stesso tipo, istanze diverse

Separare esplicitamente:

- **Tipo:** oscillatore, sampler o drum kit; scegliibile su numerose tracce.
- **Istanza:** configurazione posseduta dalla singola traccia, con un proprio ID, parametri, stato delle voci e riferimento descrittivo al preset.
- **Preset:** valori copiati nell'istanza. Modificare una traccia non modifica un preset factory o un'altra istanza che usa lo stesso preset. Salvare il preset utente è un'azione esplicita.
- **Asset immutabile:** WAV o tabella DSP condivisibile per memoria; non contiene cursori di playback, inviluppi o mixer delle altre tracce.

La condivisione intenzionale di **una stessa istanza di plugin** tra più tracce è una funzione futura diversa. Richiede un modello di instrument rack, routing MIDI con porte/canali, routing audio e uscite multiple, gestione di mute/solo, ownership, automazione e salvataggio dello stato condiviso. Non implementarla attraverso un ID uguale assegnato accidentalmente a due tracce. Non introdurre hosting plugin per soddisfare il requisito di istanze interne multiple.

### 3.3 Piano roll con riferimento musicale a sinistra

- Tastiera verticale fissa a sinistra, tasti chiari/scuri e riga per ogni semitono. Nomi come `C4`, `C#4`, ottava leggibile e Do evidenziati anche sulla griglia. Tooltip con numero MIDI.
- Default `MIDI 60 = C4`, già usato da `editor::label`. Preferenza configurabile «Do centrale: C3 / C4 / C5»: cambia soltanto la notazione, mai pitch salvato, frequenza, mapping drum o MIDI. Formula proposta: `octave = floor(pitch/12) - 1 + display_octave_offset`, default offset 0.
- Nome su tutti i tasti quando lo spazio lo consente; almeno ogni Do sempre identificabile a zoom minimo. Mostrare il nome completo della nota selezionata nell'inspector anche per rettangoli troppo corti per un'etichetta interna.
- Scroll verticale continuo, scrollbar e pulsanti ottava; zoom verticale e orizzontale separati. «Adatta alle note» non modifica il documento. Ricordare viewport per traccia/clip nella sessione UI, senza aggiungere passi alla storia musicale.
- Un'unica trasformazione di coordinate deve servire disegno, tastiera, hit test, drag e inserimento. Se `r` è l'altezza di riga e `p_top` la nota superiore: `y(p) = (p_top - p) * r - scroll_subpixel`. Sottrarre la larghezza della tastiera prima di passare x agli strumenti di editing. Gestire clipping e righe parziali senza creare pitch fuori da 0–127.
- Ruler, snap, playhead e rettangoli usano la stessa origine orizzontale. Quando si edita una clip, il piano roll mostra tempo locale della clip e il suo inizio assoluto è indicato nella toolbar; il playhead deriva dal tempo progetto meno l'inizio della clip.
- Click/pressione su un tasto: audition dello strumento della traccia. Release, uscita dalla finestra, perdita del focus, cancel, cambio traccia, rimozione della traccia e chiusura dell'app devono rilasciare le note. ID live per ogni gesto, non soltanto `pitch + 1` come nel controller attuale. Non registrare note nel progetto facendo audition.
- Audition e preview dei pad rispettano gain, mute e solo della traccia; indicare quando questi impediscono l'ascolto. Un eventuale monitor di preview che li scavalchi richiede un controllo e un percorso distinti, non impliciti.
- In modalità batteria, sostituire le etichette melodiche con nome del pad e numero GM; mantenere la corrispondenza numerica con le note, anche se le righe vengono filtrate ai pad disponibili. Evidenziare chiaramente la modalità.
- Preservare culling e cap di sicurezza; in un caso oltre 3.000 note visibili mostrare densità/avviso e richiesta di zoom. Mai troncare gli eventi audio né offrire hit test che contraddica la scena. Ottimizzare l'indice delle note solo se le misure mostrano che la scansione non rispetta il budget UI.

Riferimento di comportamento aggiuntivo: [manuale ufficiale Waveform: MIDI Editing](https://tracktion.github.io/waveform_manual/latest/midi-editing/). Il layout ASCII seguente è una proposta originale, non una riproduzione del prodotto di riferimento.

### 3.4 Drum machine interna

M3 introduce uno strumento multi-pad, non una traccia sampler per ciascun colpo:

- Griglia fino a 16 pad, 12 inizialmente popolati dalla selezione verificata; almeno 8 devono superare la selezione sonora prima di chiamarlo kit factory pronto. Pad vuoti chiaramente distinti e utilizzabili con import dell'utente.
- Ogni pad ha nome, numero nota MIDI, campione sostituibile, gain, pan, mute, tuning di base e modalità one-shot/gated. Preview del pad, indicazione di trigger e controllo velocity. Il mixer di traccia agisce dopo il mix dei pad.
- Sequenziamento con le note esistenti, velocity 1–127 e relativa modifica nell'inspector/editor. Il numero MIDI seleziona il pad, non deve trasporre automaticamente l'intero campione come nel sampler melodico. L'eventuale tuning del pad è separato.
- Mapping GM per le posizioni occupate, riportato nell'appendice. Un kit di 12 suoni non è un'implementazione completa o certificata General MIDI; le altre note sono non assegnate e producono silenzio. Un futuro import/export MIDI userà il canale percussivo 10, indice 9 nelle API a base zero; oggi non esiste tale input esterno.
- Choke group opzionale per pad; almeno un gruppo hi-hat configurato per closed/pedal/open nel kit iniziale. Un nuovo colpo rilascia con rampa breve le voci precedenti del gruppo nella stessa istanza. Definire ordine deterministico per trigger simultanei; gruppi locali al kit, mai globali tra tracce.
- Pool di voci preallocato per istanza; proposta iniziale 64 voci complessive per kit, non 24 per ogni pad. Stealing deterministico, preferendo voci in rilascio/deboli, con rampa anticlick. Numero definitivo subordinato a misura CPU/memoria.
- Import/sostituzione WAV fuori dal callback; decodifica e validazione prima del commit. Percorsi portabili, copia in `assets/`, undo ripristinabile anche dopo cambio traccia. Nessun download automatico di campioni nel callback o durante l'avvio audio.
- Nessun time stretching, browser commerciale o step sequencer separato richiesto per chiudere M3. Una vista ritmica a step può seguire dopo il completamento del kit e della velocity.

Il riferimento tecnico per le assegnazioni è [MEI: nomi MIDI e percussioni](https://music-encoding.org/guidelines/v5/data-types/data.MIDINAMES.html); [Microsoft documenta il legame con lo standard MMA](https://learn.microsoft.com/en-us/windows/win32/multimedia/standard-midi-key-assignments). I nomi delle ottave possono variare tra applicazioni; il contratto interoperabile è il numero della nota.

### 3.5 Synth a oscillatore per traccia

Interpretazione operativa di «strumento onda»: un oscillatore polifonico per istanza in M3. Oscillatori multipli, detune/unison, sync, FM, filtri, modulazioni e automazione sono una fase successiva da valutare separatamente.

| Parametro proposto | Intervallo e semantica |
|---|---|
| `waveform` | `sine`, `saw`, `square`, `triangle`; forma riconoscibile a shape neutro. |
| `shape` | 0–1: morph lineare normalizzato tra sine e la forma selezionata; default 1. Su sine il controllo è disabilitato. Nessun wavefolder non lineare implicito. |
| `pulse_width` | 0,05–0,95, default 0,5; applicabile a square, che diventa un impulso a duty variabile. Disabilitato sulle altre forme. Correggere DC e normalizzazione in modo documentato. |
| Attack / Decay / Release | Secondi nel documento: 0–10 / 0–10 / 0–20; UI in ms/s con scala adatta. Zero valido con percorso esplicito, senza divisioni per zero. |
| Sustain | 0–1. |
| Volume / pan | Riutilizzare gain di traccia 0–2 e pan -1…1; UI può mostrare dB. Evitare due controlli di volume identici con semantica diversa. |
| Preset | Snapshot versionato di valori, nome e provenienza; indicatore «modificato». Il preset non contiene note e non crea legami mutabili tra tracce. |

La migrazione da `Sine` mantiene forma sine, valori ADSR effettivamente usati dal servizio (A 0,006 s, D 0,06 s, S 0,75, R 0,06 s), gain/pan e note. Non usare i valori del demo storico come preset di migrazione. Preservare per quanto possibile il livello attuale del synth e verificare una regressione sonora sul render v1.

Requisiti DSP:

- Stato di fase, inviluppo, note ID, velocity e stealing indipendenti per voce e per istanza. Nessuna variabile DSP mutabile condivisa tra tracce.
- Saw, square/PWM e triangle devono avere una strategia antialias verificata. Candidati: correzioni bandlimited per discontinuità e derivate, oppure tabelle multibanda preparate fuori dall'audio. Una saw ottenuta solo dal modulo di fase e una square ottenuta solo da una soglia non soddisfano il requisito.
- Fare un breve confronto misurato prima di fissare l'algoritmo. La scelta deve rispettare anche PWM estremo, triangle, frequenze alte e cambi di forma, non soltanto una saw a 440 Hz. Riferimento primario: [Välimäki, Pekonen e Nam, JASA 2012](https://mac.kaist.ac.kr/pubs/ValimakiPeknenNam-jasa2012.pdf). È un riferimento algoritmico, non un'autorizzazione a copiare codice di terzi senza verificarne la licenza.
- Applicare target coerenti all'inizio del blocco. Smoothing per sample di shape/pulse width e gain/pan, inizialmente 5–20 ms secondo parametro. Cambi discreti di waveform con crossfade breve mantenendo la fase; nessuna riallocazione al cambio.
- ADSR completo acquisito dalla voce al note-on: le voci già attive completano il proprio inviluppo, le nuove usano il nuovo snapshot. Questo evita salti arbitrari a metà attacco/release. UI e documento devono dichiarare questa semantica. Nessuna chiamata concorrente a `SynthNode::setADSR` sul nodo in elaborazione.
- Clamp/validazione dei valori finiti, guardie sul sample rate e comportamento esplicito per fondamentali sopra Nyquist nei sample rate bassi: silenzio per quella voce, senza alias o loop non limitati. Per i target app a 44,1/48/96 kHz verificare tutta la gamma MIDI 0–127.
- Preset e controlli devono valere anche nel motore indipendente di export, non solo nei target live del nodo attivo.

## 4. Wireframe di lavoro

### 4.1 Arrangement e inspector

```text
+--------------------------------------------------------------------------------------+
| OpenDAW   [Play] [Pause] [Stop] [Loop]    120 BPM    003:01       [Save] [Export]        |
+----------------------------+--------------------------------------+------------------+
| TRACCE             [+]     |  1       2       3       4       5    | INSPECTOR        |
+----------------------------+--------------------------------------+------------------+
| > [blu] Lead A             | [ Intro A ===== ] [ Tema A ======== ] | Lead A           |
|   Oscillatore [M] [S] [...]|                                      | Colore [blu]     |
|   Gain ----o  Pan --o--    |                                      | Sine / Pad soft  |
+----------------------------+--------------------------------------+ Wave [saw v]     |
|   [verde] Lead B           |          [ Risposta B ============== ]| Shape ----o      |
|   Oscillatore [M] [S] [...]|                                      | ADSR ...         |
+----------------------------+--------------------------------------+ Gain / Pan       |
|   [ambra] Batteria         | [ Groove 1 ==== ] [ Groove 2 ======= ]| Output: Master   |
|   Kit interno [M] [S] [...]|                                      |                  |
+----------------------------+--------------------------------------+------------------+
| header fissi               | <------ scroll orizzontale ------->  | pannello chiudibile|
+----------------------------+--------------------------------------+------------------+
| EDITOR: Intro A / Lead A                [Fit note] [Zoom X - +] [Zoom Y - +]           |
| Tastiera | piano roll della clip selezionata                                           |
+--------------------------------------------------------------------------------------+
| stato: salvato / modifica struttura ferma playback       Master L/R    [Panic]         |
+--------------------------------------------------------------------------------------+
```

Header e corsie condividono altezza e scroll verticale; il ruler rimane fisso. Su 1000×760 l'inspector può richiudersi e l'editor si ridimensiona, senza coprire transport o header. Conservare Help → About OpenDAW e l'attribuzione Slint già sistemati; non usare l'area di editing per informazioni sullo stack tecnico.

### 4.2 Piano roll

```text
| Intro A — inizio progetto 1:01 | tempo locale clip | Snap 1/16 | Do centrale [C4 v] |
|          | 1                  2                  3                  4             |
| D4   62  |--------------------+------------------+------------------+-------------|
| C#4  61  |  tasto scuro       |                  |                  |             |
| C4   60  |== [nota C4 ========] ====================== Do evidenziato =============|
| B3   59  |                    | [nota B3 =====]  |                  |             |
| A#3  58  |  tasto scuro       |                  |                  |             |
| A3   57  |--------------------+------------------+------------------+-------------|
| ...      |       griglia, note e tasti usano le stesse coordinate y                |
|          | [velocity della nota selezionata: 100]                       scroll Y  |
```

La tastiera resta ferma durante lo scroll orizzontale. La linea `C4` corrisponde esattamente al rettangolo MIDI 60, qualunque sia lo zoom. Il campo numero MIDI può essere un tooltip per non appesantire la vista.

### 4.3 Kit e mixer pad

```text
| Batteria / Percussion Kit 01       [Preset v]  [Salva preset]     Velocity [100] |
| [35 Bass drum] [38 Snare]   [42 Hat closed] [46 Hat open]                        |
| [44 Hat pedal] [45 Low tom] [50 High tom]   [49 Crash]                           |
| [54 Tamb.]     [56 Cowbell] [60 Hi bongo]   [75 Claves]                          |
| [vuoto +]      [vuoto +]   [vuoto +]       [vuoto +]                            |
| Pad selezionato: Hat open / MIDI 46 / Choke 1 / One-shot                        |
| [Preview] [Importa/Sostituisci WAV]   Gain ---o  Pan --o--  Tune 0 st  [Mute]    |
| Sorgente / licenza / stato file    -> dettagli asset                            |
| Mix pad -> mix della traccia -> Master                                         |
```

## 5. Modelli dati e persistenza — proposto

### 5.1 Documento M3

Definire uno schema finale **v2** e un migratore puro v1→v2. Introdurre il modello clip fin dall'incremento dati, inizialmente con una clip di durata progetto per ogni vecchia traccia: in questo modo la UI può evolvere in step senza mantenere due sorgenti delle note. Le varianti strumento v2 diventano utilizzabili soltanto quando il rispettivo motore è implementato; caricare una variante non supportata deve dare un errore chiaro, mai riprodurla come sine senza avviso.

Schema concettuale; non è codice già presente:

```text
Project { version=2, name, bpm, length_ticks, next_id, tracks[] }
Track   { id, name, color, gain, pan, mute, solo, instrument, clips[] }
InstrumentInstance { id, preset_ref?, config }
InstrumentConfig = Oscillator { waveform, shape, pulse_width, adsr }
                 | Sampler    { asset, root_note, one_shot }
                 | DrumKit    { name, pads[] }
MidiClip { id, name, color?, start_tick, length_ticks, notes[] }
Note     { id, start, duration, pitch, velocity }  // start locale alla clip
Pad      { id, midi_note, name, asset?, gain, pan, mute,
           tune_semitones, one_shot, choke_group? }
AssetRef { relative_path, sha256, provenance? }
PresetRef { id, display_name, version, origin }   // descrittivo, valori copiati
```

Regole:

- ID numerici di tracce, istanze, clip, pad e note tutti distinti nel progetto, non nulli e sotto 2^63. `next_id` cresce anche dopo undo; undo/redo ripristinano gli ID dell'oggetto esistente, una nuova duplicazione ne genera altri.
- L'ordine di `tracks[]` è l'ordine visuale persistente. Non usare l'indice come identità nei messaggi UI.
- Una traccia possiede una sola istanza M3. Una clip appartiene a una traccia; le sue note sono locali. Non conservare anche `Track.notes` come copia mutabile.
- Clip e note con somme controllate contro overflow: `clip.start + clip.length <= project.length` e `note.start + note.duration <= clip.length`. Per M3 il ridimensionamento che taglierebbe note può essere rifiutato con spiegazione; non introdurre trimming distruttivo implicito.
- Clip sovrapposte consentite: gli eventi si sommano e mantengono ID distinti. Nessun legame di pattern condiviso implicito quando si duplica.
- Pad con note MIDI uniche nella stessa istanza, massimo 16 pad in M3; tabella runtime di lookup a 128 slot preparata fuori dall'audio. Pad senza asset non produce suono, ma resta editabile.
- Convalidare nomi, colori, valori finiti e range prima della mutazione. Conservare i limiti di note/durata e aggiungere quelli espliciti di clip/pad: proposta 4.096 clip totali, massimo 64 tracce a milestone conclusa.
- Selezione, focus, scroll, preview dei drag e note live sono stato UI/sessione, non eventi di undo. La preferenza sulla notazione delle ottave è dell'applicazione: usare un archivio preferenze esplicito o un nuovo campo versionato, senza infilare campi ignoti nel formato v1 con `deny_unknown_fields`.

### 5.2 Migrazione e salvataggio

1. Leggere e validare un DTO v1 distinto; non tentare prima di deserializzare v2 confidando nei default.
2. Conservare gli ID di tracce/note, tick, velocity, gain e pan; allocare nuovi ID per clip/istanze con `next_id`. Impostare colori deterministici, mute/solo false e synth compatibile con il servizio attuale.
3. Per i sampler, risolvere il percorso sotto `assets/` con le difese già presenti; calcolare hash e costruire `AssetRef` fuori dall'audio. Nessun cambio di pitch o ricampionamento necessario per la migrazione.
4. Validare l'intero v2 prima di sostituire lo stato corrente. Caricare v1 non lo riscrive automaticamente: salvare v2 deve rendere visibile il cambio formato e conservare il checkpoint v1.
5. Adeguare `storage::save`: oggi legge il manifest precedente direttamente come `Project` e lo valida con una sola versione. Per il primo save v2 deve accettare e preservare un precedente v1 valido usando il decoder versionato, altrimenti il checkpoint impedirebbe l'aggiornamento.
6. Estendere risoluzione/copia asset a tutti i pad, preset sampler/drum e relativi riferimenti. Deduplicare i byte immutabili per hash quando disponibile; hash uguale non significa stato DSP condiviso. Copiare prima gli asset, poi sostituire atomicamente il manifest, con rollback del documento in memoria in caso di errore.
7. Gli asset restano immutabili; undo del cambio campione ripristina il riferimento precedente. Non cancellare file ancora usati da storia, export o progetto aperto. Garbage collection esplicita fuori da M3 se non indispensabile.
8. Provare errore I/O, disco pieno/simulato, file mancante o alterato, UTF-8/Unicode e percorsi Windows. Non attribuire automaticamente una licenza CC0 ai campioni importati dall'utente; provenienza `user_import`, redistribuibilità non verificata.

Gli asset factory devono essere risolti dal pacchetto/installazione, senza dipendere da `/Downloads` o da URL online. Al salvataggio di una sessione portabile, copiarli in `assets/` oppure usare una risoluzione factory versionata con fallback esplicito. Preferenza M3: copia nel progetto per apertura autosufficiente. Conservare il relativo registro delle licenze quando si distribuisce il progetto demo.

### 5.3 Proiezione verso il motore

Il documento è l'unica autorità di editing. Il controller genera una `SessionSpec` derivata:

- Costruisce mappa stabile `TrackId -> engine_slot` e una `session_generation` nuova a ogni compilazione riuscita.
- Appiattisce le clip: `absolute_tick = clip.start_tick + note.start`; converte start/end con `Project::frame_at` al sample rate effettivo, senza accumulare rounding per blocco.
- Mantiene gli ID delle note e il controllo della densità prima dell'attivazione. Se rounding produce durata nulla, gestire con regola esplicita e test, senza inviare eventi invalidi al motore.
- Prepara configurazioni e campioni per ogni istanza, più il gate mute/solo derivato. Il grafo rimane `strumento -> gain/pan/gate -> master`; il kit miscela internamente i pad prima dell'uscita di traccia.
- Export deriva lo stesso snapshot persistente, su un'istanza distinta: nessun prelievo delle voci del dispositivo live. Gesti non confermati e audition non entrano nel rendering.

## 6. Contratti di comando, ABI e real-time

### 6.1 UI → controller → documento

I nomi seguenti sono proposti. Implementarli nelle responsabilità esistenti di `Request`/`Edit`/`History`, senza creare nuovi crate per principio.

| Intento | Contratto proposto | Effetto e undo |
|---|---|---|
| Selezione | `SelectTrack { track_id }`, `SelectClip { track_id, clip_id }` | Solo UI. Cancella/rilascia audition precedente; niente modifica documento. |
| Aggiunta | `AddTrack { kind, after_id? }` | Valida limite; assegna track/instance ID. Una voce in storia; nuova traccia selezionata. |
| Rimozione | `RemoveTrack { track_id }` | Inversa conserva dati e posizione. Selezione passa a vicina valida; niente riferimenti pendenti. |
| Duplicazione | `DuplicateTrack { track_id }` | Copia profonda della configurazione e delle note/clip con nuovi ID; inverse rimuove la copia. |
| Proprietà | `RenameTrack`, `SetTrackColor`, `MoveTrack { id, before_id? }` | Nome/colore senza rebuild audio. Riordino inizialmente con stop/recompile perché il bridge attuale usa indici. |
| Mix | `SetTrackMix { track_id, gain, pan }`, `SetTrackMute/Solo` | Target live a blocchi; gate separato dal fader. Un undo per gesto o click. |
| Clip | `Add/Remove/Move/DuplicateClip { track_id, clip_id, ... }` | Transazioni con tutti gli ID/range validati. Spostamento/duplica/elimina annullabili in un passo. |
| Synth | `SetSynthParams { track_id, instance_id, params, gesture_id }` | Snapshot completo coerente, range verificati, parametri nell'undo e nell'export. |
| Preset | `ApplyPreset { track_id, instance_id, preset_snapshot }` | Un solo passo anche per molti parametri; nessun collegamento mutabile al preset sorgente. |
| Kit | `ReplacePadSample`, `SetPadParams`, `RenamePad`, `SetPadMidiNote` | Identità esplicite; import prima validato fuori RT, poi commit atomico. |
| Audition | `AuditionOn/Off { track_id, instance_id, token, pitch, velocity }` | Effimero, non entra in storia; release relativo alla destinazione originale del token. |
| Storia | `Undo`, `Redo` | Restituisce impatto sull'audio e nuova selezione valida. Non recuperare indici divenuti obsoleti. |

Richiesta accompagnata da `request_id` e contesto della revisione; risposta `Applied { request_id, revision, impact }` oppure `Rejected { request_id, reason }`. La UI non deve mostrare successo definitivo per un edit rifiutato o perdere l'ID target perché la selezione è cambiata nel frattempo. Il controller serializza le modifiche e valida prima della mutazione.

Introdurre un impatto esplicito: `UiOnly`, `Metadata`, `RealtimeParameters`, `Structural`. Non passare ogni cambio di nome attraverso `State::edit`, che oggi chiude il dispositivo. Per gli interventi strutturali mantenere inizialmente il lifecycle sicuro di stop/prepare/restart manuale; mostrare lo stato «Modifica applicata; premi Play per riprendere».

Per i controlli continui: `BeginGesture` / aggiornamenti coalescenti / `EndGesture`, un'inversa sul valore iniziale. Due gesti distinti devono produrre due undo, anche sulla stessa traccia: il coalescing attuale del mix non ha questo confine. Undo e redo ripristinano snapshot completi. Un edit non valido lascia invariati progetto e storia.

Se il documento è già aggiornato ma l'invio audio fallisce, segnare `engine_unsynced`, mostrare lo stato e riapplicare lo snapshot completo dal controller; non perdere il comando persistente. Il percorso prioritario Stop può cancellare il backlog audio, ma al successivo Play occorre riallineare i parametri al documento. Distinguere «accettato in coda» da «applicato nel motore».

### 6.2 Estensione C ABI

La struct `od_track` v1 non può rappresentare tutti gli strumenti M3. Proposta: contratto v2 con descrittori espliciti, `struct_size`, `version`, tag del tipo e dati POD per synth/pad; caricamento sincrono che copia quanto serve e valida conteggi, percorsi, null, range e versione.

- Non aggiungere campi reinterpretando silenziosamente il layout v1. Preferire nuove entry point, ad esempio `od_load_v2` e `od_set_instrument_params_v2`, mantenendo identificabile il vecchio contratto. Questi nomi non esistono nella baseline.
- Definire esplicitamente la versione dichiarata da `od_abi_version` e aggiornare insieme wrapper Rust, client C/Python e test di layout. Se si passa a versione 2, il vecchio wrapper che pretende `== 1` deve rifiutarla: non dichiarare compatibilità binaria automatica.
- I descrittori di caricamento possono riferire array/stringhe validi per la durata della chiamata; il core copia e carica fuori RT. Il callback non conserva puntatori a `Vec`, `CString`, oggetti Slint o stack Rust.
- I comandi runtime contengono solo slot/ID controllati, generazione, seriale e valori di dimensione fissa. Nessuna stringa o percorso attraversa la coda audio. La mappa ID→slot appartiene al controller; generazioni obsolete sono rifiutate.
- Uno snapshot synth o di mix deve essere letto coerentemente a inizio blocco. Preferire un comando POD completo nella SPSC; non leggere campi correlati come atomiche scollegate, e non usare un seqlock che faccia spin senza limite nel callback.
- I target persistenti devono aggiornare anche la descrizione usata dall'export nativo: oggi `od_gain_pan` aggiorna `e->spec`. Evitare che nuove funzioni modifichino solo il nodo live lasciando `od_export` con preset obsoleti.
- I risultati distinguono argomento invalido, coda piena, handle occupato e errore di preparazione. Telemetria/ack con generazione e ultimo seriale applicato; gli indicatori di livello restano approssimati e non devono essere usati come una transazione.

### 6.3 Invarianti audio obbligatorie

```text
UI Slint
  -> richieste limitate, ID espliciti, aggiornamenti di gesto coalescenti
worker Rust / modello + storia + persistenza
  -> un solo produttore C ABI
control C++ / validazione + caricamento + preparazione
  -> snapshot POD / SPSC limitata / target pronti
inizio blocco audio C++
  -> consumo limitato -> smoothing -> voci -> mix -> telemetria atomica

structural edit:
  candidato preparato fuori RT -> stop/quiesce del consumer
  -> sostituzione -> distruzione vecchi oggetti sul controllo -> Play esplicito
```

Nel callback: **nessuna allocazione o deallocazione, filesystem, decodifica, logging, chiamata UI/Rust, join, attesa, mutex bloccante o ricostruzione del grafo**. Non distruggere l'ultimo `shared_ptr` di un campione nel callback; mantenere owner stabili fino alla quiescenza. Anche una sostituzione apparentemente economica può liberare memoria.

Mantenere un solo consumer: render headless e dispositivo non elaborano contemporaneamente lo stesso motore. Non aggiungere un secondo produttore alla SPSC per la preview dei pad: UI e futuri input devono essere serializzati dal controller o da code con un contratto diverso, progettato a parte.

Applicare i parametri live ai confini di blocco; le note schedulate mantengono l'offset campione. Gestire overflow senza perdere release: target continui conservabili/coalescenti sul controllo, note-off affidabile o panic prioritario, contatore e messaggio di errore. Non smaltire l'intera coda con un ciclo non limitato nel callback.

Per arrivare a 64 tracce, modificare in modo coordinato validazioni Rust/native, capacità e messaggi di errore. Verificare la dimensione degli array `events_`: la semplice moltiplicazione di `MaxTracks` può aumentare molto oggetti sullo stack, particolarmente nei test Windows. Spostare l'eventuale storage necessario su heap preallocato durante prepare, senza introdurre allocazioni runtime. Valutare memoria di due sessioni durante replace e del motore di export simultaneo, non solo del grafo attivo.

## 7. Ricerca asset e regole di inclusione

### 7.1 Fonte selezionata e prove

Fonte ufficiale: [VCSL, repository dell'autore/editore](https://github.com/sgossner/VCSL). Il [README alla revisione fissata](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/README.md) identifica Versilian Studios LLC e consente l'uso dei suoni anche in software commerciale. Il [LICENSE alla stessa revisione](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE) è **CC0 1.0 Universal**. [Creative Commons descrive i diritti di copia, modifica e distribuzione, anche commerciale](https://creativecommons.org/publicdomain/zero/1.0/).

Per ciascuno dei 12 file dell'appendice sono stati verificati: presenza nella tree Git non troncata, download dei byte originali dalla revisione fissata, lunghezza e Git blob SHA-1 corrispondenti, SHA-256 completo, header e dati PCM. Nella tree ispezionata non risultano licenze locali alternative per le cartelle selezionate. I relativi file ricadono quindi nella dichiarazione CC0 della raccolta. Non si afferma una verifica forense della titolarità oltre alla fonte ufficiale.

- Revisione upstream: `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e` (il commit è il riferimento di versione degli asset).
- Data accesso e verifica: **2026-10-22** per ogni record.
- Autore/titolare dichiarato per questi record: **Versilian Studios LLC**. Non inventare un esecutore individuale: non risulta accreditato per singolo WAV nelle fonti lette.
- Licenza per ogni record: **CC0-1.0**, versione **1.0 Universal**. Redistribuibile con OpenDAW secondo la dichiarazione ufficiale; non richiesto un pagamento royalty. Conservare comunque provenienza, licenza e hash per tracciabilità, senza suggerire endorsement.
- SHA-256 del file LICENSE originale: `a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499`.
- Il JSON allegato ripete URL, autore/titolare, licenza/versione, evidenza, data, hash e decisione di redistribuzione **per ciascun file**. Gli hash si riferiscono al contenitore WAV originale completo, non al solo PCM o a una conversione.

La selezione è un kit acustico/percussivo candidato. Le misure mostrano WAV PCM stereo 44,1 kHz/16 bit, nessun campione digitale a fondo scala e picchi con margine. Questo non dimostra da solo alta qualità percepita, assenza di clipping analogico o coerenza timbrica: **non è stato effettuato ascolto umano in questa sessione**. Kick orchestrale e crash da clash cymbal, in particolare, vanno valutati rispetto al carattere desiderato prima del branding come kit moderno. Non promettere timbri clap/808/ride non presenti nella selezione.

### 7.2 Fonti non ammesse al bundle factory CC0

| Fonte consultata | Esito per questa iterazione |
|---|---|
| [99Sounds: licenza ufficiale](https://99sounds.org/license/) | Royalty-free, ma con restrizioni sulla redistribuzione dei file. Non è CC0; nessun file incluso nella whitelist. |
| [MusicRadar / SampleRadar: pagina ufficiale](https://www.musicradar.com/news/tech/free-music-samples-royalty-free-loops-hits-and-multis-to-download-sampleradar) | Consente uso musicale ma vieta redistribuzione dei campioni. Nessun file incluso nella whitelist. |
| Altri download senza prova puntuale | Nessuna inclusione basata soltanto su «free», aggregatori, risultati di ricerca o licenza di un repository che non copra esplicitamente gli asset. |

«Free to download», «royalty-free», «senza pagamento» e «CC0/public domain» non sono sinonimi. Per il kit distribuito da OpenDAW usare solo whitelist CC0 verificata. Qualsiasi file con autore, licenza o contenuto non verificabile resta fuori; offrire import volontario dal disco dell'utente, senza copiarlo in esempi/release pubbliche.

### 7.3 Gate di qualità e integrazione futura

Prima di includere un WAV nella release:

1. Riscaricare dall'URL immutabile e confrontare SHA-256. Un mismatch blocca quel file: nessuna sostituzione silenziosa da `master`.
2. Far passare i byte attraverso il vero `loadWav` del progetto e un render C ABI. La verifica RIFF di ricerca non sostituisce questo test.
3. Ascoltare il colpo isolato e in un pattern a velocity 32/64/100/127: onset, click, rumore, code, immagine stereo, ruolo GM e omogeneità del kit. Registrare esito per ogni file e chi lo ha ascoltato. Non dichiarare qualità approvata finché manca questo passaggio.
4. Regolare il gain del pad, non normalizzare distruttivamente l'originale. Confrontare i loudness percepiti: i picchi attuali variano molto. Se si taglia silenzio, cambia sample rate o applica un fade, mantenere hash originale e derivato, ricetta e versione della trasformazione.
5. Conservare manifest e testo CC0 nel futuro bundle asset/avvisi; i WAV mantengono la propria provenienza distinta dalla licenza MIT del codice. Nessun logo esterno necessario.
6. Integrare almeno 8 e al massimo 16 suoni approvati. Obiettivo preferito: tutti i 12 candidati; se alcuni falliscono, escluderli e lasciare pad importabili, oppure trovare e verificare nuovi file allo stesso livello. Un kit factory con meno di 8 suoni non chiude il gate asset di M3.

I 12 file originali sommano **5.600.134 byte**. La decodifica float stereo richiede circa il doppio dei dati PCM16, oltre agli oggetti e alle altre sessioni. I valori per file e i livelli misurati sono nel JSON/CSV.

## 8. Milestone incrementali e dipendenze

Ogni milestone deve restare compilabile, verificabile e reversibile. Prima preservare il percorso corrente, poi aggiungere una sola capacità per volta. Nessun refactor generale del grafo, del renderer o del workspace Rust.

| Passo | Consegna concreta | Dipendenze e gate di uscita |
|---|---|---|
| **M3.0 — baseline e decisioni** | Confermare commit/toolchain, fixture v1 e render di riferimento; fissare semantica di mute/solo, ADSR e shape; confronto DSP antialias breve. | Tutti i gate attuali passano. Le decisioni sono annotate; non aggiornare dipendenze per comodità. |
| **M3.1 — documento e tracce** | Modello v2 con identità d'istanza e clip; migratore v1; add/remove/duplicate/rename/color/order; selezione per ID; undo di transazioni; ancora 16 tracce. UI inizialmente semplice ma operativa. | M3.0. Round trip e migrazione, primo save sopra manifest v1, isolamento di due istanze e ripristino dopo undo. L'editor corrente continua a mostrare la clip migrata. |
| **M3.2 — header e mixer** | Header/corsie allineati, selezione, scroll, inspector; mute/solo reali a blocchi, prima estensione ABI minima necessaria; gesti e ack coerenti. | M3.1. Nessun controllo fittizio; nomi/colori non fermano l'audio; test di mix ed export equivalenti. |
| **M3.3 — clip e tastiera** | CRUD/spostamento/duplica clip nell'arrangement; tastiera verticale, label/Do, audition robusta, zoom X/Y, scroll e notazione configurabile; velocity edit. | M3.1–2. Coordinate uniche e test visivi alle due dimensioni minime. Nessuna nota persa nel flattening. |
| **M3.4 — synth per traccia** | Quattro forme, shape/PWM, ADSR, snapshot runtime coerenti, smoothing e preset; configurazione in prepare e in export. | M3.0 DSP + M3.1–2. Misure antialias e anticlick, tre tracce stesso tipo con suoni e note indipendenti. |
| **M3.5 — drum engine e kit** | Nodo kit interno, pool di voci, lookup GM, pad/mixer/preview/import, choke; pacchetto di almeno 8 asset verificati e ascoltati. | M3.1–3 + contratto strumenti M3.4. Prima engine con campioni sintetici di test, poi UI, poi asset factory. Nessun download richiesto a runtime. |
| **M3.6 — 64 tracce e rilascio** | Limiti coordinati, memoria/stack controllati, fixture grande, prove performance/UX, build e audio reali Mac/Windows, documentazione finale. | M3.1–5. Tutti i criteri obbligatori e DoD; pubblicare misure e limiti residui. |

M3.3 e M3.4 hanno alcune attività indipendenti; mantenerle come incrementi distinti per ridurre il rischio. Asset approvati e contenuti factory possono avanzare dopo il modello pad; nessun ritardo della ricerca giustifica l'inclusione di campioni non verificati.

## 9. File e target candidati reali

Questi percorsi esistono nella baseline ispezionata. I candidati indicano dove intervenire, non impongono di modificare ogni file.

| File / target esistente | Intervento candidato |
|---|---|
| `app/src/project.rs` | Track/instance/clip/pad/config, validazioni, edit inverse e transazioni/gesti. |
| `app/src/storage.rs` | Decoder/migratore versionato, checkpoint v1/v2, tutti i riferimenti asset, preset e save/export atomico. |
| `app/src/controller.rs` | Richieste con ID espliciti, selezioni valide, mappe slot/generazione, impatti edit, coalescing e stato engine sincronizzato. |
| `app/src/editor.rs` | Coordinate tastiera/griglia, viewport X/Y, notazione, scene delle clip e hit test. |
| `app/ui/main.slint` | Header, arrangement, tastiera, inspector synth/drum e layout responsivo. Componenti Slint aggiuntivi solo se migliorano la leggibilità, dichiarandoli nuovi. |
| `app/src/main.rs` | Binding callback/modelli per ID, differenze di stato, rilascio audition su lifecycle/focus e adattamento UI. |
| `app/src/bridge.rs` | Layout e validazione ABI v2, snapshot di parametri, flatten delle clip, velocity live esplicita. |
| `app/src/project_cli.rs` | Validazione/migrazione/render dei formati supportati e fixture; non creare un secondo parser. |
| `include/open_daw/engine.hpp`, `src/engine.cpp` | Config synth, nuove forme antialias, voci e smoothing; conservare contratti `Graph`/`InstrumentNode`. |
| `include/open_daw/media.hpp`, `src/media.cpp` | Riutilizzo import/campioni immutabili e logica voci per il kit; non simulare il kit con molte istanze sampler complete. |
| `include/open_daw/service.hpp`, `src/service.cpp` | Tipi strumento espliciti, costruzione per istanza, comandi parametri, gate e limiti/preallocazione. |
| `include/open_daw/api.h`, `src/api.cpp` | Contratti ABI nuovi, validazione/copied lifetime, snapshot export e gestione errori. |
| `include/open_daw/spsc.hpp` | Da preservare se possibile; rivedere solo se indispensabile e con prove del contratto SPSC. |
| `src/device.cpp`, `include/open_daw/device.hpp` | Soltanto eventuali correzioni necessarie emerse da lifecycle e test target; non riscrivere adattatori durante l'aggiunta delle tracce. |
| `tests/tests.cpp`, `tests/service_tests.cpp`, `tests/api_smoke.c` | Unità DSP, sessione, ABI, indipendenza/GM/export e casi invalidi. |
| `tests/allocation_probe.cpp` | Estendere scenario alle forme e al kit; includere cambi parametri, choke, overflow e stop. |
| `scripts/verify_shared_api.py` | Nuova ABI/layout e round trip dei nuovi strumenti; conservare controlli sui WAV e sulle partizioni callback. |
| `app/examples/layout_probe.rs` | Estendere test render/hit UI agli header, alla tastiera e ai nuovi pannelli; mantenere guardia feature UI. |
| `app/build.rs`, `CMakeLists.txt` | Aggiungere sorgenti solo se estratte in nuovi file esplicitamente dichiarati. Il build Rust continua a collegare le librerie native statiche. |
| `app/Cargo.toml`, `app/Cargo.lock` | Nessun nuovo crate richiesto a priori. Cambi dipendenze solo motivati e con lockfile validato. |
| `.github/workflows/validate.yml` | Usare lockfile tracciato e gate Mac/Windows/Linux. Oggi il job frontend esegue ancora `cargo generate-lockfile`: correggere il gate di riproducibilità senza aggiornare indiscriminatamente le versioni. |
| `docs/ARCHITECTURE.md`, `docs/PROJECT_FORMAT.md`, `docs/BUILD.md`, `THIRD_PARTY_NOTICES.md` | Documentare stato finale, migrazione, nuove semantiche, asset e prove per piattaforma. |

Un eventuale file dedicato al drum node, ai preset o alla migrazione è **nuovo da creare**, non un modulo corrente. Prima usare le separazioni esistenti; estrarre quando c'è codice concreto da isolare. Nessun crate `audio-core`, `track-manager` o `plugin-host` risulta presente: non basare il piano su questi nomi.

## 10. Verifica e criteri di accettazione misurabili

Tutti i criteri seguenti sono futuri. La baseline supera soltanto i controlli della sezione 2.3.

### 10.1 Unit e property test

- **Identità e storia:** almeno 100 sequenze generate di add/duplicate/reorder/remove/note edit/undo/redo, con validazione dopo ogni operazione. Nessun ID duplicato o riutilizzato da una nuova creazione; undo completo ripristina il contenuto iniziale, eccetto il contatore monotono `next_id`.
- **Migrazione:** fixture v1 sine e sampler; conservazione esatta di pitch, tick, velocity e mix. Primo save v2 sopra v1 riuscito, checkpoint v1 ricaricabile, versione futura rifiutata senza alterare il progetto aperto.
- **Coordinate:** MIDI 0/60/127, righe parziali, HiDPI e almeno 3 zoom verticali; differenza fra centro tasto e centro nota <1 pixel logico. Nessuna inserzione dalla zona tastiera. C3/C4/C5 cambiano soltanto label.
- **Comandi:** input NaN/Inf, range errati, ID non esistente, slot/generazione vecchi, pad duplicati, clip fuori range, durate overflow e 65a traccia rifiutati senza mutazioni parziali.
- **Gesti:** un drag di 100 aggiornamenti produce un solo undo; due drag separati producono due undo. Preset completo applicato/annullato in un passo.
- **DSP:** ADSR zero e ai limiti, release durante attack/decay, note sovrapposte allo stesso pitch, stealing e reset deterministici; nessun NaN/Inf in tutte le forme/range.

### 10.2 Integration e audio

| Caso | Risultato richiesto |
|---|---|
| Indipendenza istanze | Almeno 3 tracce oscillatore con forme/preset/note diversi e 2 tracce dello stesso kit. Modificare/mutare/cancellare A non cambia valori, inviluppi o choke di B. Confrontare render isolato di B prima/dopo; non usare solo screenshot dei controlli. |
| Duplicazione | Duplicare una traccia con almeno 2 clip e 20 note; tutti gli ID nuovi; modifiche a note, preset e pad della copia non toccano l'originale. Save/reopen conserva la distinzione. |
| Mute/solo | Verificare tutte le combinazioni di 3 tracce, compreso mute+solo sulla stessa e più solo; gain nominale inalterato, comportamento coerente in export. |
| Scheduling | Start/end delle note alle posizioni compilate, errore massimo 1 frame dalla conversione tick→frame. Nessuna perdita con confini blocco, loop e clip sovrapposte. |
| Kit GM | Ognuno dei 12 numeri in whitelist attiva il campione associato; note non assegnate producono silenzio. Velocity 32/64/100/127 dà ampiezza crescente senza variazione di pitch. |
| Choke | Hat open seguito da closed/pedal: la voce precedente entra nella rampa al campione dell'evento e termina entro 5 ms di rampa; su trigger UI aggiungere la latenza di coda/blocco. Nessun choke fra istanze. |
| Audition lifecycle | 100 cicli press/release e cambio traccia, focus loss, cancel, remove/undo; nessuna voce ancora attiva dopo release/panic e relativa rampa. Ripetere con coda satura. |
| Import e portabilità | Sostituzione/undo/save/reopen di almeno 3 pad; progetto copiato in altra directory e aperto senza sorgenti esterne. WAV malformato, asset mancante o hash alterato non sostituiscono silenziosamente il vecchio pad. |
| Export | Stesso snapshot statico renderizzato live-headless/offline con partizioni 64/128/256/511/1024 e almeno 25 partizioni casuali: confronto bit-identico sullo stesso target se l'algoritmo lo consente, altrimenti errore assoluto massimo dichiarato e motivato ≤1e-6 prima di PCM16. Confrontare PCM16 con tolleranza di 1 LSB. |
| Export durante playback | Il cursore e le voci live non vengono resettati; l'export usa lo snapshot acquisito all'avvio anche se successivamente si modifica il progetto. Campioni mantenuti vivi fino al termine. |

**Antialias:** render senza inviluppo/transienti, frequenze coerenti con i bin FFT prossime a 110/440/1760/3520 Hz e una sopra 4 kHz, a 44,1/48/96 kHz; saw, square/PWM 0,1/0,5/0,9 e triangle. Usare riferimento additivo limitato alle armoniche sotto Nyquist, fase/ampiezza allineate e guard band dichiarate. Misurare energia spuria fuori dalle armoniche ammesse, non THD che conteggerebbe le armoniche desiderate. Gate proposto: energia alias/spuria ≤-50 dBr fino a 4 kHz e miglioramento ≥20 dB rispetto alla forma ingenua; per frequenze superiori fissare e registrare il limite dopo lo spike, prima di accettare l'algoritmo. Se una forma fallisce, migliorare l'algoritmo o il banco multibanda; non eliminare il test. Verificare separatamente sweep e PWM in movimento con ascolto e spettrogramma.

**Smoothing/anticlick:** su tono sostenuto cambiare gain, pan, shape, duty e waveform. Verificare che la transizione duri il tempo configurato ±1 blocco e non produca salti dovuti a reset di fase o target incoerenti. Per un gate gain su segnale costante 0,25, con rampa minima 5 ms a 48 kHz, differenza fra campioni consecutivi attribuibile al gate ≤0,002. Ascolto necessario anche per note stealing/choke; non imporre un limite assoluto di differenza tra campioni alle normali forme d'onda.

### 10.3 UX e layout

- A 1280×820 e 1000×760, 100% e 200% scaling: transport, almeno un header, selezione e accesso inspector sempre visibili; nessuna sovrapposizione o controllo senza area cliccabile. Il probe deve verificare hit test, oltre alle immagini renderizzate.
- Con 64 tracce, header e corsie restano allineati entro 1 pixel logico dopo scroll dall'inizio alla fine, riordino e undo. La traccia selezionata è raggiungibile con «mostra selezione» senza perdere il focus editor.
- Un utilizzatore deve poter creare due synth dello stesso tipo, rinominarli/colorarli, duplicarne uno, cambiare il preset della copia e riordinarli senza editare JSON o percorsi di codice. Script di prova completabile in ≤2 minuti dopo aver letto i nomi dei comandi.
- Do e ottave leggibili; la nota selezionata è identificabile anche se lunga pochi pixel. Tastiera, grid e ruler non scorrono su assi diversi per errore.
- Navigazione Tab, attivazione da tastiera dei comandi traccia e focus visibile; non affidare mute/solo/selezione al solo colore. Etichette accessibili per nuovi controlli.
- Se la registrazione non esiste, Arm assente/disabilitato. Il silenzio esplicito `--silent` rimane distinguibile dal playback fisico.
- Conservare Help/About e validare di nuovo il menu nativo macOS; il probe software precedente non lo copre.

### 10.4 Performance e real-time

Registrare macchina, OS/architettura, build release, sample rate, dimensione buffer, numero di voci e sessione esatta. Non pubblicare un generico «nessuna latenza».

| Scenario riproducibile | Gate proposto |
|---|---|
| Callback 48 kHz, 256 frame; 32 tracce, 8 voci attive per synth sulle tracce synth, mix synth/kit/sampler e campioni residenti | 10 minuti su Mac Apple Silicon e PC Windows x64 di riferimento: p99 del lavoro DSP <60% del budget di blocco (3,2 ms), massimo <5,33 ms nel test isolato, zero underrun osservati nella prova dispositivo. Pubblicare separatamente ritardi scheduler/driver. |
| 64 tracce funzionali | Create/save/load/reorder/render di tutte le tracce; test stress audio separato con carico dichiarato. Se la macchina non regge, riportare il limite pratico; nessun crash, overflow o corruzione. |
| Zero heap nel rendering | Estendere allocation probe a ≥10.000 blocchi con synth/kit, parametri, choke, seek e overflow: zero `new/delete`. Integrare audit/strumentazione per malloc/free e lock dove disponibile; il probe attuale da solo non prova tutta la sicurezza RT. |
| UI grande | 100.000 note totali, 64 tracce e ≤1.500 note nel viewport: risposta p95 di selezione/scroll/zoom ≤50 ms, frame UI p95 ≤16,7 ms sulla macchina dichiarata; nessun blocco >100 ms durante i gesti. |
| Coda e controlli | Burst di 1.000 aggiornamenti slider dal livello UI: coalescing, ultimo valore applicato, Stop prioritario entro 2 blocchi da quando raggiunge il servizio; nessuna release persa. Misurare separatamente ritardo UI/controller. |
| Memoria | Sessione standard con kit candidato: dati decodificati entro il budget esplicito 128 MiB; nessuna crescita continua dopo 100 cicli di cambio preset/duplica/elimina/undo. Riportare picco RSS con vecchia+nuova sessione e export; non confonderlo con il budget media. |

Le soglie sono criteri da verificare, non una promessa già valida sul computer dell'utente. Non aggiungere clock o logging costoso a ogni sample per misurarle: usare una build di misura con raccolta limitata e aggregazione fuori dal callback.

### 10.5 Matrice piattaforme

- **macOS arm64:** build Rust/UI e nativa; CoreAudio reale, start/stop ripetuti, sleep/wake e cambio/rimozione dispositivo, nuove UI e menu. Provare 44,1/48 kHz lato hardware quando disponibili.
- **Windows x64:** Visual Studio/MSVC, build native/Rust/Slint, WASAPI shared, stop/join, percorsi Unicode, save/replace, layout 100/200% e suono reale. Non dedurre esito Windows dal passaggio macOS.
- **macOS Intel:** dichiarare separatamente copertura e risultato; se non disponibile, non chiamarlo validato. Non richiedere un cross-build al `build.rs` che oggi lo rifiuta.
- **Linux:** regressione headless/export, build UI e clock silenzioso; nessuna pretesa di nuovo backend audio Linux in M3. ASan/UBSan e, in build separata, TSan sul servizio dove supportati.
- La CI è una prova di build/test, non di hardware. Pin del lockfile comune, log associati al commit M3 e risultati marcati pass/fail/non eseguito.

## 11. Rischi e decisioni aperte

| Tema | Decisione raccomandata / verifica residua |
|---|---|
| «Personalizzazione onda» | M3: singolo oscillatore, quattro forme e morph/PWM definiti. Confermare in seguito se servono 2–3 oscillatori; non ampliare ora il motore senza un requisito separato. |
| Carattere del kit | Selezione acustica/percussiva documentata. Ascolto per decidere se kick e crash soddisfano il risultato richiesto; per una batteria elettronica trovare altre fonti CC0 puntuali. |
| Qualità synth/CPU | Spike antialias prima dell'UI definitiva; scegliere con misure, non solo con la semplicità della formula. Soglia oltre 4 kHz da chiudere e registrare in M3.0. |
| Aumento tracce | Passare 16→64 soltanto dopo UI/ownership corrette; controllare array, stack Windows, densità eventi e doppie preparazioni. |
| ABI e formato progetto | Migrazione v1→v2 e negoziazione ABI esplicite. Documento v1 precedente resta recuperabile. Nessun layout FFI reinterpretato. |
| Backpressure | Nuovi comandi persistenti non possono sparire con i Move del mouse. Coalescing e acknowledgment, stato di sincronizzazione engine visibile e ripristino dopo Stop. |
| Undo e asset | Storia riferisce asset immutabili; delete fisico differito. Duplicazione non condivide parametri mutabili. |
| Mix/mute/export | Gate indipendente dal fader; stessa derivazione di solo per playback ed export. Preset live aggiornano anche la configurazione esportabile. |
| Monitoraggio per traccia | Master meter già presente, meter per traccia non presente. Aggiungerlo solo con telemetria preallocata e frequenza UI limitata, oppure lasciare un indicatore trigger chiaramente distinto dal meter audio. |
| Registrazione/routing/plugin | Fuori M3; nessun Arm attivo, host plugin o collegamento condiviso fittizio. Output iniziale al master. |
| Persistenza su altre piattaforme | Provare sostituzione atomica e Unicode su Windows; non cambiare il protocollo di salvataggio insieme a refactor estranei. |
| Layout e attribuzione | Evitare regressione dei transport già corretti e mantenere AboutSlint. Screenshot/probe non sostituiscono il controllo del menu e dell'audio fisico. |

Le decisioni indispensabili di M3.0 possono essere prese dal coding agent entro il perimetro qui definito e annotate. Rivolgersi all'utente per preferenze sonore o ampliamenti reali del perimetro, senza bloccare gli step tecnici indipendenti.

## 12. Istruzioni operative al coding agent

1. Ispeziona nuovamente repository, istruzioni e modifiche locali. Non toccare eventuali file sincronizzati sotto `sources/` del progetto ChatGPT. Usa il checkout aggiornato indicato in apertura.
2. Riproduci i gate attuali e conserva una fixture/render v1. Non riaprire la correzione di `validate_app.sh` già risolta senza una regressione concreta.
3. Implementa una milestone per volta, con commit piccoli e descrizioni del comportamento verificabile. Prima dati/contratti e test mirati, poi binding e UI del singolo flusso.
4. Conserva il confine Rust controller / ABI / servizio C++; niente audio elaborato da Slint o callback Rust, nessuna allocazione RT. Mantieni stop/prepare per struttura finché non esiste una sostituzione live progettata e provata separatamente.
5. Dopo ogni incremento esegui i test pertinenti e i gate esistenti. Dopo cambi FFI esegui anche client C, bridge Rust e Python; dopo layout estendi il probe e ispeziona le immagini.
6. Non cambiare contemporaneamente renderer, grafo, package Rust e formati per ragioni stilistiche. Nuove classi/file solo quando servono alla funzione in lavorazione. Non aggiornare Slint/Rust/dependency lockfile come parte implicita della feature.
7. Non spacciare stub per funzioni: un pad deve suonare, mute deve agire sul motore, un preset deve salvare e riaprire. Nascondi o disabilita esplicitamente ciò che non è disponibile.
8. Per gli asset usa esclusivamente URL/hash whitelist, registra trasformazioni e ascolto. Nessun sample da fonte non verificata in repository, test pubblici o release. Per i test DSP preferisci segnali generati e deterministici.
9. Aggiorna questo handoff con decisioni chiuse e documenti di architettura/formato con lo stato effettivamente implementato. Riporta test non eseguiti e piattaforme mancanti senza dedurre compatibilità.
10. A fine milestone consegna: comportamento cambiato, file principali, prova di salvataggio/undo/export, test eseguiti e limiti residui. Non dichiarare M3 completo prima del DoD.

## 13. Definition of Done M3

- [ ] Repository aggiornato identificato; nessuna regressione dei gate baseline, del transport visibile o di Help/About.
- [ ] Header, nomi/colori, selezione, aggiunta/rimozione/duplica, ordine, scroll e inspector operativi; mute/solo reali; Arm coerente con le capacità effettive.
- [ ] Fino a 64 tracce valide; numerose istanze dello stesso tipo provate indipendenti, incluse note, preset, voci e choke.
- [ ] Clip MIDI persistenti e modificabili, migrazione v1, undo/redo atomico e save/reopen portabile verificati, compreso checkpoint del vecchio formato.
- [ ] Tastiera e nomi note a sinistra, Do/ottave, scroll/zoom, notazione configurabile e audition con release robusta; griglia e hit test allineati.
- [ ] Synth sine/saw/square/triangle, shape/PWM, ADSR, gain/pan e preset per istanza, smoothing e antialias misurati; export coerente.
- [ ] Kit interno multi-pad con mapping numerico GM, velocity, naming/preview, sample replace/import e mixer di base; choke verificato e locale all'istanza.
- [ ] Almeno 8 e fino a 16 suoni factory con prova ufficiale CC0/public domain, hash, versione, autore, data, redistribuibilità e ascolto approvato per ogni file; nessun asset incerto incluso.
- [ ] Nessuna allocazione/deallocazione, I/O o lock bloccante nei percorsi audio nuovi; capacità limitate e comportamento overflow/Stop provati.
- [ ] Export indipendente e persistente, test unit/integration/UX/performance della sezione 10 e misure associate al commit finale.
- [ ] Build e test hardware Mac/Windows eseguiti sui target dichiarati; eventuali target mancanti riportati esplicitamente come non validati, senza etichettare completata la loro compatibilità.
- [ ] Architettura, schema/migrazione, istruzioni build, limiti e avvisi asset aggiornati. Documento finale distingue fatti verificati, risultati e lavoro rinviato.

## Appendice A — Whitelist puntuale dei campioni

La seguente appendice è generata dai byte verificati il 22 ottobre 2026. Per tutti i record: autore/titolare dichiarato Versilian Studios LLC, licenza CC0 1.0 Universal, stessa revisione upstream fissata nella sezione 7, redistribuzione con il progetto consentita dalla fonte ufficiale; nessun WAV integrato in questa consegna e approvazione sonora ancora pendente. Il registro JSON/CSV ripete questi campi per ogni file e contiene anche misure, dimensioni e Git blob SHA-1.

### A.1 — Acoustic Bass Drum / MIDI 35

- **File originale:** `BDrumNew_hit_v5_rr1_Sum.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Bass%20Drum%201/BDrumNew_hit_v5_rr1_Sum.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Bass%20Drum%201/BDrumNew_hit_v5_rr1_Sum.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `642aaf3375eaff1307973f7de3ccbf74d18e71fb3cf9412e021b08fc3bf1981c`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 3.334 s; 588,092 byte; picco -9.635 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.2 — Acoustic Snare / MIDI 38

- **File originale:** `Snare2_HitSN_v6_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Snare%20Drum%2C%20Modern%201/Snare2_HitSN_v6_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Snare%20Drum%2C%20Modern%201/Snare2_HitSN_v6_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `ca0ee01347176a65f9d7e361203a61e736fa14acb2a743df355a4bded1ee40bd`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 1.364 s; 240,660 byte; picco -15.843 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.3 — Closed Hi-Hat / MIDI 42

- **File originale:** `HiHat_HitC_v3_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Hi-Hat%20Cymbal/HiHat_HitC_v3_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Hi-Hat%20Cymbal/HiHat_HitC_v3_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `b65ca06ea4f8b5209fc8d98912094be1212911f21e01bd949cc9960db049d37c`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 1.240 s; 218,792 byte; picco -15.546 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.4 — Pedal Hi-Hat / MIDI 44

- **File originale:** `HiHat_Close_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Hi-Hat%20Cymbal/HiHat_Close_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Hi-Hat%20Cymbal/HiHat_Close_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `4e4dbb4ddbeea653f95c2c229685fd5966ed9492fb8489d89d19dbd864f1a6b4`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 1.342 s; 236,820 byte; picco -19.973 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.5 — Open Hi-Hat / MIDI 46

- **File originale:** `HiHat_HitO_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Hi-Hat%20Cymbal/HiHat_HitO_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Hi-Hat%20Cymbal/HiHat_HitO_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `fa492ec14cc6924b43ad5fcf9e9a0f406d926998835bde2eb5847cd15ad25be7`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 5.819 s; 1,026,612 byte; picco -10.966 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.6 — Low Tom / MIDI 45

- **File originale:** `TomL_HitS_v3_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Tom%202/Stick/TomL_HitS_v3_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Tom%202/Stick/TomL_HitS_v3_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `bdc312f2f4c799cb43319a7aa0e664bf6194c6f8b14151051b375968df55eab4`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 2.883 s; 508,520 byte; picco -13.841 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.7 — High Tom / MIDI 50

- **File originale:** `TomH_HitS_v3_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Tom%201/Stick/TomH_HitS_v3_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Tom%201/Stick/TomH_HitS_v3_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `40bd5cf904ed20eb8e7bba6298e356b66cedd313b4f459c5a5884c054bf447a4`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 2.407 s; 424,648 byte; picco -15.917 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.8 — Crash Cymbal 1 / MIDI 49

- **File originale:** `cymbal_crash1_mf1.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Clash%20Cymbals%201/cymbal_crash1_mf1.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Clash%20Cymbals%201/cymbal_crash1_mf1.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `3b4a6ed42627eedca39f6480e9825dedeaab538197fc734f85be13ba655aa6aa`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 8.479 s; 1,495,666 byte; picco -5.218 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.9 — Tambourine / MIDI 54

- **File originale:** `Tamb1_Hit_v2_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Tambourine%201/Tamb1_Hit_v2_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Tambourine%201/Tamb1_Hit_v2_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `885ebf1d70f8e888f576682e4e4112b92b69649162f392c83b3379c91a09cf41`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 1.662 s; 293,256 byte; picco -16.406 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.10 — Cowbell / MIDI 56

- **File originale:** `Cowbell1_Normal_v3_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Cowbells/Cowbell1_Normal_v3_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Cowbells/Cowbell1_Normal_v3_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `e212cc27b3490d7927d41508bd7e90d6e09b0b8da169f0357341d3057c4910c4`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 0.940 s; 165,900 byte; picco -4.535 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.11 — Hi Bongo / MIDI 60

- **File originale:** `BongoH_Hit1_v2_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Bongos/BongoH_Hit1_v2_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Membranophones/Struck%20Membranophones/Bongos/BongoH_Hit1_v2_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `dd9b6239a151702966a995937f74ac022c9bb950c967277fbd16beaced0c9e1a`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 1.186 s; 209,356 byte; picco -20.566 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

### A.12 — Claves / MIDI 75

- **File originale:** `Claves1_Hit_v2_rr1_Mid.wav`.
- **Fonte ufficiale:** [pagina del file](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Claves/Claves1_Hit_v2_rr1_Mid.wav) · [WAV originale](https://raw.githubusercontent.com/sgossner/VCSL/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/Idiophones/Struck%20Idiophones/Claves/Claves1_Hit_v2_rr1_Mid.wav).
- **Autore/titolare dichiarato:** Versilian Studios LLC; esecutore individuale non accreditato per file nelle fonti ispezionate.
- **Licenza/versione:** [CC0 1.0 Universal](https://github.com/sgossner/VCSL/blob/c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e/LICENSE); revisione asset `c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e`.
- **Data accesso/verifica:** 2026-10-22. **Redistribuibile con OpenDAW:** sì, in base alla dichiarazione ufficiale e alla licenza; **incluso nell'app:** no.
- **SHA-256:** `bcabe27330761309eb4b4f6207e6e3b4be59815df86449ac05bfb9b81f19efc9`.
- **Dati osservati:** PCM16, 44100 Hz, 2 canali; 1.087 s; 191,812 byte; picco -24.232 dBFS; 0 campioni digitali a fondo scala.
- **Trattamenti:** nessuno, originale. **QA sonora:** ascolto e approvazione ancora da eseguire.

