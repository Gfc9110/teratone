use crate::{bridge::{Engine, Telemetry}, editor::{Gesture, NoteRect, Viewport}, project::{Edit, History, Instrument, Note, Project, PPQ}, storage};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicU32, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
pub const PANIC: u32 = 1; pub const STOP: u32 = 2; pub const QUIT: u32 = 4;
#[derive(Debug)]
pub enum Request {
    Play, Pause, ToggleLoop, Seek(f64), SelectTrack(i32), Tempo(f64), Mix(f32, f32), Undo, Redo, Delete,
    Viewport(f32, f32), Zoom(f32), Pan(i32), Pitch(i32),
    Down(f32, f32, bool), Move(f32, f32), Up(f32, f32),
    Audition(u8, bool), Save(PathBuf), Open(PathBuf), Export(PathBuf), Import(PathBuf, u8, bool), UseSine,
}
#[derive(Clone)]
pub struct ViewState {
    pub name: String, pub status: String, pub tracks: Vec<String>, pub track_index: i32,
    pub bpm: f64, pub gain: f32, pub pan: f32, pub dirty: bool, pub can_undo: bool, pub can_redo: bool,
    pub notes: Vec<NoteRect>, pub viewport: Viewport, pub export_busy: bool, pub selected_label: String,
}
pub enum Response { View(Box<ViewState>), Meter(Telemetry), Fatal(String) }
pub struct Connection {
    pub sender: SyncSender<Request>, pub receiver: Receiver<Response>, pub flags: Arc<AtomicU32>, pub worker: JoinHandle<()>,
}
struct State {
    project: Project, history: History, engine: Engine, viewport: Viewport, track_index: usize,
    selected: Option<u64>, gesture: Option<Gesture>, insertion: Option<(u64, u8)>,
    dirty: bool, needs_compile: bool, looping: bool, silent: bool, status: String,
    export_job: Option<JoinHandle<Result<(), String>>>,
}
impl State {
    fn track_id(&self) -> u64 { self.project.tracks[self.track_index].id }
    fn quiesce(&mut self) -> Result<(), String> {
        self.engine.stop()?; self.engine.close_device()?;
        let _ = self.engine.render(1)?; // apply the priority stop while there is no device consumer
        self.gesture = None; self.insertion = None; Ok(())
    }
    fn edit(&mut self, edit: Edit) -> Result<(), String> {
        self.quiesce()?;
        self.history.execute(&mut self.project, edit)?; self.dirty = true; self.needs_compile = true;
        self.status = "Edited. Playback stopped; press Play to compile and resume.".into(); Ok(())
    }
    fn compile(&mut self) -> Result<(), String> {
        if self.needs_compile { self.engine.load(&self.project)?; self.needs_compile = false; }
        Ok(())
    }
    fn view(&self) -> ViewState {
        let t = &self.project.tracks[self.track_index];
        let preview = self.gesture.as_ref().map(|g| &g.preview);
        let (notes, capped) = self.viewport.scene(&self.project, t.id, self.selected, preview);
        let selected_label = self.selected.and_then(|id| t.notes.iter().find(|n| n.id == id))
            .map(|n| format!("{} · {:.2} beats · velocity {}", crate::editor::label(n.pitch), n.duration as f64 / PPQ as f64, n.velocity))
            .unwrap_or_else(|| "Click empty space to insert · drag to move · drag right edge to resize".into());
        ViewState { name: self.project.name.clone(), status: if capped { format!("{}  |  View capped at 3000 notes; zoom in.", self.status) } else { self.status.clone() },
            tracks: self.project.tracks.iter().map(|t| t.name.clone()).collect(), track_index: self.track_index as i32,
            bpm: self.project.bpm, gain: t.gain, pan: t.pan, dirty: self.dirty, can_undo: self.history.can_undo(), can_redo: self.history.can_redo(),
            notes, viewport: self.viewport, export_busy: self.export_job.is_some(), selected_label }
    }
    fn request(&mut self, r: Request, flags: &AtomicU32) -> Result<(), String> {
        let track_id = self.track_id();
        match r {
            Request::Play => {
                self.compile()?;
                if flags.load(Ordering::Acquire) & (STOP | QUIT) != 0 { return Ok(()); }
                self.engine.set_loop(self.looping, 0, self.project.frame_at(self.project.length_ticks, 48000))?;
                self.engine.start_device(self.silent)?; self.engine.play()?;
                self.status = if self.silent { "SILENT CLOCK — transport simulation; no audio output" } else { "Playing through native output" }.into();
            }
            Request::Pause => self.engine.pause()?,
            Request::ToggleLoop => {
                self.looping = !self.looping;
                if !self.needs_compile { self.engine.set_loop(self.looping, 0, self.project.frame_at(self.project.length_ticks, 48000))?; }
                self.status = if self.looping { "Loop: full project range" } else { "Loop disabled" }.into();
            }
            Request::Seek(beat) => {
                if !beat.is_finite() || beat < 0.0 { return Err("Invalid seek position".into()); }
                self.compile()?;
                let tick = ((beat * PPQ as f64) as u64).min(self.project.length_ticks);
                self.engine.seek(self.project.frame_at(tick, 48000))?;
                self.status = "Seek resets voices; notes starting before the position are not chased.".into();
            }
            Request::SelectTrack(i) => {
                if i < 0 || i as usize >= self.project.tracks.len() { return Err("Invalid track".into()); }
                self.engine.panic()?; self.track_index = i as usize; self.selected = None; self.gesture = None;
                let highest = self.project.tracks[self.track_index].notes.iter().map(|n| n.pitch).max().unwrap_or(72);
                self.viewport.top_note = highest.saturating_add(8).clamp(24, 127);
            }
            Request::Tempo(bpm) => self.edit(Edit::Tempo(bpm))?,
            Request::Mix(gain, pan) => {
                self.history.execute(&mut self.project, Edit::Mix { track: track_id, gain, pan })?;
                self.dirty = true;
                if !self.needs_compile { self.engine.mix(self.track_index as u32, gain, pan)?; }
                self.status = if self.needs_compile {
                    "Mixer edit stored for the next playback compile; changes share one undo step."
                } else { "Mixer change applied; consecutive changes share one undo step." }.into();
            }
            Request::Undo | Request::Redo => {
                self.quiesce()?;
                let changed = if matches!(r, Request::Undo) { self.history.undo(&mut self.project)? } else { self.history.redo(&mut self.project)? };
                if changed { self.dirty = true; self.needs_compile = true; self.status = "History applied; playback stopped.".into(); }
            }
            Request::Delete => { if let Some(id) = self.selected.take() { self.edit(Edit::Delete { track: track_id, id })?; } }
            Request::Viewport(w, h) => {
                if w.is_finite() && h.is_finite() { self.viewport.width = w.clamp(1.0, 4096.0); self.viewport.height = h.clamp(1.0, 2160.0); }
            }
            Request::Zoom(f) => { if f.is_finite() && f > 0.0 { self.viewport.pixels_per_beat = (self.viewport.pixels_per_beat * f).clamp(24.0, 256.0); } }
            Request::Pan(beats) => {
                self.viewport.start_tick = (self.viewport.start_tick as i64 + beats as i64 * PPQ as i64).clamp(0, self.project.length_ticks.saturating_sub(PPQ) as i64) as u64;
            }
            Request::Pitch(semitones) => self.viewport.top_note = (self.viewport.top_note as i32 + semitones).clamp(12, 127) as u8,
            Request::Down(x, y, right) => {
                self.gesture = None; self.insertion = None;
                if let Some((note, resize)) = self.viewport.hit(&self.project, track_id, x, y) {
                    self.selected = Some(note.id);
                    if right { self.edit(Edit::Delete { track: track_id, id: note.id })?; self.selected = None; }
                    else { self.gesture = Some(Gesture { original: note.clone(), preview: note, x, y, resize }); }
                } else if !right {
                    self.selected = None; self.insertion = self.viewport.insertion(self.project.length_ticks, x, y);
                }
            }
            Request::Move(x, y) => {
                if let Some(g) = &mut self.gesture { g.preview = self.viewport.drag(g, x, y, self.project.length_ticks); }
            }
            Request::Up(x, y) => {
                if let Some(g) = self.gesture.take() {
                    let note = self.viewport.drag(&g, x, y, self.project.length_ticks);
                    if note != g.original { self.edit(Edit::Replace { track: track_id, note })?; }
                } else if let Some((start, pitch)) = self.insertion.take() {
                    let id = self.project.allocate_id()?;
                    let note = Note { id, start, duration: (PPQ / 2).min(self.project.length_ticks - start).max(1), pitch, velocity: 100 };
                    self.edit(Edit::Insert { track: track_id, note })?; self.selected = Some(id);
                }
            }
            Request::Audition(pitch, on) => {
                if on { self.compile()?; self.engine.start_device(self.silent)?; }
                self.engine.live_note(self.track_index as u32, u64::from(pitch) + 1, pitch, on)?;
            }
            Request::Save(path) => {
                self.project = storage::save(&path, &self.project)?; self.dirty = false;
                self.status = format!("Saved {} (previous checkpoint retained)", path.display());
            }
            Request::Open(path) => {
                if self.dirty { return Err("Save current edits before opening another project.".into()); }
                let candidate = storage::load(&path)?; self.engine.load(&candidate)?;
                self.project = candidate; self.history = History::default(); self.needs_compile = false;
                self.track_index = 0; self.selected = None; self.gesture = None; self.insertion = None;
                self.status = format!("Opened {}", path.display());
            }
            Request::Export(path) => {
                if self.export_job.is_some() { return Err("An export is already running.".into()); }
                let project = self.project.clone(); self.status = format!("Exporting {} using an independent engine", path.display());
                self.export_job = Some(thread::spawn(move || storage::export_atomic(&path, &project)));
            }
            Request::Import(path, root_note, one_shot) => {
                self.quiesce()?;
                let full = path.canonicalize().map_err(|e| e.to_string())?;
                let instrument = Instrument::Sampler { sample_path: full.to_str().ok_or("Sample path must be UTF-8")?.into(), root_note, one_shot };
                let mut candidate = self.project.clone(); candidate.tracks[self.track_index].instrument = instrument.clone();
                self.engine.load(&candidate)?; // native importer validates the file before committing the document
                self.history.execute(&mut self.project, Edit::Instrument { track: track_id, instrument })?;
                self.dirty = true; self.needs_compile = false; self.status = "Short WAV sample loaded. Sample files are copied into assets/ on Save.".into();
            }
            Request::UseSine => self.edit(Edit::Instrument { track: track_id, instrument: Instrument::Sine })?,
        }
        Ok(())
    }
}
pub fn spawn(silent: bool) -> Connection {
    let (sender, requests) = mpsc::sync_channel(64);
    let (responses, receiver) = mpsc::sync_channel(8);
    let flags = Arc::new(AtomicU32::new(0)); let worker_flags = flags.clone();
    let worker = thread::spawn(move || {
        let engine = match Engine::new(48000, 256) { Ok(e) => e, Err(e) => { let _ = responses.send(Response::Fatal(e)); return; } };
        let mut state = State { project: Project::demo(), history: History::default(), engine, viewport: Viewport::default(),
            track_index: 0, selected: None, gesture: None, insertion: None, dirty: false, needs_compile: true,
            looping: false, silent, status: if silent { "SILENT CLOCK mode; no physical audio output" } else { "Ready. Play the example or edit a note." }.into(), export_job: None };
        let mut pending = Some(Box::new(state.view())); let mut last_meter = Instant::now();
        loop {
            let emergency = worker_flags.swap(0, Ordering::AcqRel);
            if emergency & QUIT != 0 { break; }
            if emergency & (PANIC | STOP) != 0 {
                // Bounded cancellation of UI backlog, so an old queued Play cannot override Stop.
                for _ in 0..64 { if requests.try_recv().is_err() { break; } }
                if let Err(e) = state.quiesce() { state.status = e; }
                else { state.status = "Stopped. Pending UI actions canceled.".into(); }
                pending = Some(Box::new(state.view()));
            }
            match requests.recv_timeout(Duration::from_millis(8)) {
                Ok(request) => { if let Err(e) = state.request(request, &worker_flags) { state.status = e; } pending = Some(Box::new(state.view())); }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if state.export_job.as_ref().is_some_and(|j| j.is_finished()) {
                let result = state.export_job.take().unwrap().join();
                state.status = match result { Ok(Ok(())) => "Export complete. Playback state was not reset.".into(), Ok(Err(e)) => e, Err(_) => "Export worker failed".into() };
                pending = Some(Box::new(state.view()));
            }
            if let Some(view) = pending.take() {
                match responses.try_send(Response::View(view)) {
                    Ok(()) => {}, Err(TrySendError::Full(Response::View(view))) => pending = Some(view),
                    Err(TrySendError::Disconnected(_)) => break, Err(_) => unreachable!(),
                }
            }
            if last_meter.elapsed() >= Duration::from_millis(33) {
                if let Ok(t) = state.engine.telemetry() { let _ = responses.try_send(Response::Meter(t)); }
                last_meter = Instant::now();
            }
        }
        let _ = state.quiesce();
        // No detached export: closing the app waits for this explicit job to finish.
        if let Some(job) = state.export_job.take() { let _ = job.join(); }
    });
    Connection { sender, receiver, flags, worker }
}
