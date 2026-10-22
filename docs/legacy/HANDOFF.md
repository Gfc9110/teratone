# Open DAW — handoff M1 + M2

## Obiettivo e ambito

Realizzare **davvero** le prime due milestone di un motore audio DAW completamente custom in **C++20**, open source e indipendente dalla UI e dal game engine. La futura UI sarà Godot 4 tramite GDExtension, ma **Godot non è necessario né va integrato in M1/M2**.

- **M1 — sintetizzatore e rendering offline:** note MIDI, polifonia, ADSR, generazione di un file WAV, test deterministici.
- **M2 — Audio Graph e riproduzione realtime:** più nodi, mixer stereo, routing, backend WASAPI per Windows, controlli runtime, metriche e test.
- **Non implementare ora**: pianoroll grafico, timeline editor, VST/CLAP, import MP3/FLAC, sample-based instruments, progetti completi, recording, Android/iOS.
- **Non fare dipendere il core da JUCE, Tracktion Engine o Godot**. Sono ammesse solo API di sistema e librerie permissive se assolutamente necessarie, documentando e giustificando ogni dipendenza. Preferire zero dipendenze esterne nell'MVP.
- Tutti i nuovi file possono essere distribuiti sotto MIT; non copiare codice o asset proprietari Waveform.

## Vincoli architetturali

1. **Core headless:** libreria C++ statica o condivisa, indipendente dal dispositivo audio, con entry point `render(startFrame, numFrames, outputs)` (o equivalente). Nessuna dipendenza da UI o Windows nei file del core.
2. **Tempo e precisione:** `sampleRate` esplicito; contatore temporale assoluto di **64 bit in frame audio**. Gli eventi note-on/note-off/parametri hanno offset a livello di singolo sample all'interno del blocco. Il risultato non deve cambiare musicalmente quando si varia il block size.
3. **Audio formato interno:** float32, preferibilmente planar, stereo per l'MVP. Effettuare conversioni di formato e interleaving **solo nel backend di output**.
4. **Nodi audio:** API comune per sorgenti (synth ora, `SamplePlayer` in futuro), processori (gain/pan), mixer/bus, senza assumere che ogni sorgente sia un oscillatore. Supportare input/output e eventi MIDI separati dal dato audio.
5. **Audio graph:** topologia diretta aciclica (DAG) con nodi sorgente, processori e bus; scheduling topologico. Rejection esplicita di cicli e connessioni non valide. Creazione e ricostruzione del grafo avvengono fuori dall'audio thread.
6. **Realtime safety:** nessuna allocazione, lock bloccante, chiamata al filesystem, logging sincrono, COM inizializzazione o caricamento di asset durante `process()`. Tutti i buffer e le risorse sono predisposti sul control thread; comunicare tramite comandi a costo costante / SPSC queue o stati atomici. La proprietà e la durata dei nodi non devono provocare deallocazioni nell'audio thread. Parametri continui con smoothing leggero per evitare click.
7. **Moduli separati:** `core/`, `dsp/`, `sequencing/`, `graph/`, `io/wav/`, `platform/windows/`, `examples/`, `tests/`. Le esatte cartelle possono variare, ma mantenere questa separazione funzionale.
8. **Future-proof sampler:** `AudioNode` non assume generatori sintetici; in futuro è possibile caricare sample immutabili fuori dal callback, usarli in più voci contemporaneamente e controllarne pitch, loop, velocity, gain, pan e envelope. Prevedere una nozione di asset audio a livello di design senza implementare il decoder ora.

## M1 — specifiche concrete

- Sintetizzatore sottrattivo minimo con onda sine (obbligatoria), saw e square opzionali.
- Mappatura frequenza MIDI standard: `f = 440 * pow(2, (note - 69) / 12)`.
- 16 o 32 voci simultanee con note on/off, gestione note ripetute, voice stealing deterministico.
- Envelope ADSR per voce; envelope stabile anche con tempi quasi nulli e note-off durante attack/decay.
- Sequenza di prova con eventi a posizione assoluta in frame, almeno 2 note sovrapposte, accordo e note-off.
- Esportazione WAV PCM 16-bit stereo 44.1/48 kHz (oppure WAV float32 documentato). Applicare conversione e clamp corretti, niente overflow in header.
- `daw_demo --offline output.wav --seconds 10 --sample-rate 48000 --block-size 256` o un'interfaccia CLI comparabile.
- API adatta anche a un renderer offline non in tempo reale; nessun accesso dispositivo necessario.

### Acceptance tests M1

1. Build e test headless passano con CMake + CTest almeno su piattaforma di sviluppo; su Windows compilazione MSVC.
2. WAV valido: header, dimensioni, durata, canali, sample rate, contenuto non silenzioso; picchi finiti e nel range previsto.
3. Nota MIDI 69 produce circa 440 Hz verificando campioni o zero crossing; altre note seguono il temperamento equabile.
4. Note on/off iniziano/escono alla posizione sample prevista anche a cavallo dei confini tra blocchi.
5. Render con blocchi da 64, 128, 256 e 512 campioni produce output equivalente entro tolleranza numerica.
6. Polifonia, ADSR, note ripetute e voice stealing non producono NaN/Inf o voice leaks.
7. Due esecuzioni dello stesso caso offline producono risultati identici o equivalenti entro una tolleranza definita.

## M2 — specifiche concrete

- Riutilizzare **lo stesso** core di rendering di M1 (niente secondo motore separato per realtime).
- Graph demo con **almeno due synth** su due tracce, `GainPanNode` per traccia e `MixerNode`/master bus con output stereo.
- Supportare il mix additivo e impedire overs di test: gain staging configurabile, eventuale soft limiter finale come opzione, **non nascondere il clipping nei test**.
- Consentire modifica runtime almeno di gain, pan, note on/off e transport play/stop tramite thread di controllo; parametri smoothing senza click. Ricostruzione completa del grafo durante playback può essere rinviata se esplicitamente documentata e protetta.
- Backend Windows **WASAPI shared, event-driven** come primo backend reale; enumerazione/default device e recupero corretto del formato supportato. Gestire formati e sample rate reali, conversioni necessarie, shutdown e errori. Un backend audio futuro deve essere inseribile senza modificare i nodi DSP.
- In modalità shared WASAPI, adattarsi al periodo/buffer effettivamente negoziato con il dispositivo: **non assumere** che un requested 128/256 frames sia garantito. Separare granularità del DSP e buffer dell'API.
- Tracking `callbackDuration`, `bufferBudget`, underrun/glitch se rilevabile e sample rate effettivo. Log/statistiche fuori dall'audio thread.
- CLI esemplificativa: `daw_demo --realtime` con istruzioni per suonare/modificare il mix. Per macOS/Linux implementare almeno un backend offline/dummy; non fingere che WASAPI sia testato fuori da Windows.

### Acceptance tests M2

1. Unit test del grafo: topological sort, disconnessione, cicli, silenzio delle sorgenti inattive, mix di due synth, gain e pan.
2. Stress offline e simulazione callback per rendering a blocchi con variazione periodica dei comandi: nessuna allocazione/lock nel percorso audio (verifica con strumentazione o audit e test). Nessun crash, niente NaN/Inf.
3. Build Windows x64 e test MSVC. Test manuale audio WASAPI su dispositivo fisico: note, volume, pan, play/stop, riavvio del backend; riportare dispositivo/buffer/sample rate verificati.
4. Misurare p50/p95/max elaborazione DSP e budget a buffer 128/256/512, distinguendo simulazione dal reale callback driver. Non promettere underrun zero su tutti i computer.
5. Test offline M1 continuano a passare senza regressioni.

## Compatibilità progettuale con funzionalità future

Tre aggiunte differenti che devono poter entrare nel graph senza riscrivere le fondamenta:

### A. Audio track / clip player
Caricamento di un file audio e riproduzione lungo la timeline, seek, gain, mute, regioni. Caricamento/decodifica *fuori* dall'audio thread; per file lunghi preferire streaming con ring buffer e gestione underrun. Formati futuri WAV, FLAC, MP3.

### B. Drum sampler
`DrumSamplerNode` triggerato da MIDI; un sample per pad con gain, pan, envelope, pitch, choke groups e one-shot. Espandibile a 16 o 64 pad e velocity layers. Riferimento funzionale: Micro Drum Sampler di Waveform, senza riutilizzarne codice o risorse.

### C. Chromatic sampler
`SamplerNode` con root note e mappatura dei sample a note MIDI, trasposizione variabile e polifonia. Pitch base con rapporto `2^((note-rootNote+cents/100)/12)` moltiplicato per `sourceSampleRate / engineSampleRate` nell'incremento della posizione sorgente. Posizione lettura frazionaria, interpolazione; inizialmente lineare per prototipo, poi sinc/polyphase con antialiasing. La trasposizione tramite velocità di lettura **cambia anche la durata**; time-stretch/pitch-preserving è funzionalità separata e successiva. Più zone/split, round-robin, loop e velocity layers possono essere aggiunte dopo.

**Non implementare questi tre moduli in M1/M2**, ma dimostrare con design/test/placeholder delle interfacce che nessuno richiederà una seconda audio pipeline.

## Esecuzione e report richiesto

- Lavorare in modo incrementale con test automatici: prima M1 end-to-end, poi graph M2, poi WASAPI.
- Preferire test di più configurazioni/varianti in batch, non aggiustamenti manuali singoli.
- Consegnare un `README.md` con istruzioni riproducibili per build/test, CLI, architettura e limiti.
- Salvare `docs/ARCHITECTURE.md` con scelta delle API, regole realtime, modello threading, ownership dei buffer e strategia dei futuri sampler.
- Fornire report finale con elenco file modificati, comandi usati, output test, test manuali completati/non completati, stato di M1 e M2 (PASS/PARTIAL/BLOCKED), limiti noti e prossima milestone.
- Non dichiarare PASS per WASAPI finché una build Windows e un playback su hardware reale non sono stati verificati; se macchina non Windows, produrre codice e test cross-platform, contrassegnando la validazione Windows come pendente.

## Fonti per i punti specifici

- Manuale Waveform, famiglia Sampler: https://tracktion.github.io/waveform_manual/latest/samplers-and-instruments/
- Microsoft WASAPI overview: https://learn.microsoft.com/en-us/windows/win32/coreaudio/wasapi
- Microsoft event-driven shared rendering: https://learn.microsoft.com/en-us/windows/win32/coreaudio/rendersharedeventdriven
