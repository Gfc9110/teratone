# Compact workspace: audit e verifica

## Audit iniziale

- L'app desktop usa Rust e Slint (`app/ui/main.slint`); `app/src/main.rs` avvia l'interfaccia e `app/src/ui_support.rs` collega presentazione e comandi al controller.
- `app/src/controller.rs` mantiene selezione, viste e operazioni; `app/src/editor.rs` e `app/src/arrangement.rs` calcolano rispettivamente geometria e interazioni di piano roll e arrangement.
- `app/src/project.rs` contiene dati musicali e cronologia Undo/Redo. `app/src/storage.rs` salva e migra i progetti; le preferenze dei progetti recenti sono separate. Il motore audio C++ è isolato dall'interfaccia.
- Trasporto, loop, tracce e clip, mute/solo, volume/pan, strumenti, campioni, editing note, salvataggio e cronologia erano già collegati. Il mixer e la riproduzione hanno stato reale; non esistono controlli funzionanti per metronomo o routing avanzato.
- Le modifiche strutturali, comprese note e clip, fermano la riproduzione: il controller rende quiescente il motore prima di ricompilare la partitura. Le modifiche in tempo reale al mix e alcune proprietà non la fermano. Il redesign non cambia questa politica.
- Baseline prima delle modifiche: 69 test Rust superati; la prova software dell'interfaccia ha superato gli scenari funzionali eseguiti ma ha fallito la soglia prestazionale a 64 tracce/100.000 note (p95 risposta-disegno 94 ms, p95 frame software 85 ms, su questa macchina durante altre compilazioni). Immagine iniziale: `evidence/redesign/baseline-1280.png`.

## Compatibilità

Il layout è stato concepito come stato dell'interfaccia, separato dai dati musicali e dallo storico. Il formato dei progetti e il motore audio non richiedono migrazioni per questo intervento.

## Interazioni principali

| Input | Comportamento |
| --- | --- |
| Clic nel vuoto del piano roll | Inserisce una nota usando lo snap scelto; il default è 1/16. |
| Clic su una nota | Seleziona la nota; trascinarne il corpo la sposta, trascinarne il bordo destro ne cambia la durata. |
| Doppio clic su una nota | Elimina la nota nel piano roll, anche nelle tracce drums; l'operazione si può annullare. |
| Shift o Ctrl/Command + clic su una nota | Aggiunge o rimuove la nota dalla selezione; il trascinamento del corpo sposta il gruppo. Cancellazione e spostamento si annullano in un solo passo. |
| Clic destro nel piano roll | Su una nota apre il menu con eliminazione riferita alla nota cliccata; nel vuoto mostra i comandi della vista. |
| Clic destro su traccia, clip o timeline vuota | Apre le azioni relative all'elemento e alla posizione cliccati. |
| Doppio clic nella timeline vuota | Crea un clip nella traccia corrispondente. |
| Doppio clic sul nome traccia | Apre la rinomina diretta. L’intestazione può anche essere selezionata con Tab e Invio/Spazio, e rinominata con F2. |
| Trascinamento di un clip | Sposta il clip, anche fra tracce; il bordo destro cambia la durata. |
| Tasto centrale + trascinamento | Sposta la vista nella timeline o nel piano roll, senza modificare clip o note. |
| Rotella o gesto di scorrimento | Scorre verticalmente; la rotella orizzontale del mouse o il gesto orizzontale del trackpad seguono l'asse temporale, anche sulla corsia velocity. Ctrl/Command + rotella ingrandisce intorno al puntatore; aggiungere Shift per lo zoom verticale MIDI. |
| Fit notes / Restore | Adatta la vista alle note e ripristina la vista precedente del clip. |
| Trascinamento dello splitter | Ridimensiona i due editor; doppio clic ripristina il dimensionamento adattivo. |
| Escape | Annulla il gesto corrente quando l'editor ha il focus. |
| Delete / Backspace | Elimina la selezione MIDI quando il piano roll ha il focus. |
| Modificatore di piattaforma + Z / Shift + Z | Undo / Redo quando timeline o piano roll hanno il focus. |

I pulsanti zoom agiscono sul centro della vista. Il comando **Link timeline** coordina il tempo assoluto di timeline e piano roll; è disattivato per impostazione iniziale. **Refs** apre l'elenco dei clip: si possono mostrare, fissare e nascondere i riferimenti o scegliere **Follow selection / Editor locked**. Le ghost notes usano i dati del clip d'origine e non sono oggetti modificabili. In modalità Used sono incluse anche le righe necessarie ai riferimenti visibili. La vista Drum usa i nomi dei pad realmente presenti nello strumento; le scale maggiore e minore evidenziano le righe senza modificare le note.

## Preferenze e compatibilità

Le preferenze globali di layout sono salvate in `OpenDAW/ui.json` nella cartella delle impostazioni dell'utente. Inspector chiuso, densità Compact e navigazione indipendente sono i valori iniziali. Le preferenze globali illeggibili o di versione futura non vengono sovrascritte. Il layout, la densità e l'inspector fissato o temporaneo non entrano nella cronologia musicale.

Clip attivo, selezione arrangement, riferimenti, blocco dell'editor, vista MIDI e navigazione vengono salvati separatamente per progetto nella cartella delle impostazioni, identificando il percorso canonico tramite hash. Gli ID mancanti vengono rimossi durante il ripristino; dati di vista non leggibili o di una versione futura vengono preservati. Il formato musicale e il backend audio non vengono cambiati. La cronologia resta musicale: l'apertura dei pannelli e la navigazione non creano operazioni Undo.

## Limiti verificati

La velocity di una selezione multipla modifica la nota focalizzata. Con editor bloccato, Track segue la traccia selezionata nell’arrangement; Sound, Clip e Note mostrano esplicitamente il bersaglio dell’editor. Nelle viste a righe compresse, un trascinamento verticale di gruppo che porterebbe note fuori dalla mappatura visibile è limitato per preservare gli intervalli; la vista Piano consente tutte le altezze. Le modifiche strutturali a note e clip continuano a fermare la riproduzione. I controlli non implementati dal motore, come metronomo e routing avanzato, non vengono simulati.

## Verifiche eseguite

- Suite Rust con UI: 103 test unitari e 9 di integrazione superati; variante senza UI: 59 unitari e 9 di integrazione superati.
- Suite nativa: 8 test superati, inclusa la verifica delle allocazioni e dell'heap nel callback audio; controlli dell'ABI condivisa ed esportazione WAV superati.
- Probe software collegati al controller: trasporto, selezione e riordino tracce, clip, note, multiselezione, velocity e Undo/Redo, strumenti e campioni, inspector, splitter, Focus, menu, accesso da tastiera, viste MIDI, ghost notes, pin/lock, pan con tasto centrale, scroll frazionario e ripristino dopo il cambio progetto. I controlli non sostituiscono una prova con hardware audio o finestre di sistema reali.
- Il progetto `examples/First-session.opendaw` è stato validato ed esportato in WAV.
- La prova software iniziale a 1280×820, 64 tracce e 100.000 note era oltre le soglie del probe. La misura finale in versione release, con note coincidenti raggruppate solo nella presentazione, ha misurato p95 risposta/disegno 13,45 ms e frame 2,55 ms a 1280×820; 13,29 ms / 2,29 ms a 1000×760; 15,68 ms / 4,82 ms con scala 200%. Tutte e tre erano sotto i limiti del probe. La fixture contiene molte note coincidenti; queste misure non garantiscono 60 Hz per ogni progetto, macchina o renderer.
- La verifica è stata eseguita su macOS; la prova con dispositivo audio fisico, il selettore file nativo e i controlli su Windows/Linux restano da eseguire sui rispettivi ambienti.
