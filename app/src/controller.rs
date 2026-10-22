use crate::{arrangement::{self, ArrangementViewport, TrackRow}, bridge::{Engine, Telemetry}, editor::{Gesture, NoteRect, Viewport}, project::{Edit, History, Impact, Instrument, Note, Project, PPQ, MAX_TRACKS}, storage};
use std::{collections::{HashMap, VecDeque}, path::PathBuf, sync::{Arc, atomic::{AtomicU32, AtomicU64, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
pub const PANIC: u32 = 1; pub const STOP: u32 = 2; pub const QUIT: u32 = 4;
#[derive(Debug)]
pub enum Request {
    Play, Pause, ToggleLoop, Seek(f64), SelectTrack(u64), SelectClip { track: u64, clip: u64 },
    AddTrack { after: u64 }, DuplicateTrack(u64), RemoveTrack(u64), MoveTrack { track: u64, direction: i32 },
    ReorderTrack { track: u64, index: i32 },
    SetMute { track: u64, value: bool }, SetSolo { track: u64, value: bool },
    ToggleMute(u64), ToggleSolo(u64),
    SetGain { track: u64, value: f32 }, SetPan { track: u64, value: f32 },
    ArrangementViewport(f32, f32, f32), SeekProject(f64),
    RenameTrack { track: u64, name: String }, ColorTrack { track: u64, color: String },
    Tempo(f64), Mix { track: u64, gain: f32, pan: f32 }, FinishMix, Undo, Redo, Delete,
    Viewport(f32, f32), Zoom(f32), Pan(i32), Pitch(i32),
    Down(f32, f32, bool), Move(f32, f32), Up(f32, f32),
    Audition { track: u64, pitch: u8, on: bool }, Save(PathBuf), Open(PathBuf), Export(PathBuf),
    Import { track: u64, path: PathBuf, root_note: u8, one_shot: bool }, UseSine(u64),
}
#[derive(Clone)]
pub struct ClipView { pub id: u64, pub name: String }
#[derive(Clone)]
pub struct ViewState {
    pub name: String, pub status: String, pub tracks: Vec<TrackRow>, pub track_id: u64, pub track_index: usize,
    pub clips: Vec<ClipView>, pub clip_index: Option<usize>, pub clip_start: u64, pub track_name: String, pub track_color: String,
    pub bpm: f64, pub gain: f32, pub pan: f32, pub dirty: bool, pub can_undo: bool, pub can_redo: bool,
    pub mute: bool, pub solo: bool, pub audible: bool, pub project_beats: f32, pub arrangement_limited: bool,
    pub instrument: String, pub sample_path: String, pub root_note: u8, pub one_shot: bool, pub is_sampler: bool,
    pub revision: u64, pub mix_label: String,
    pub notes: Vec<NoteRect>, pub viewport: Viewport, pub export_busy: bool, pub selected_label: String,
}
pub enum Response {
    View(Box<ViewState>), Meter(Telemetry), Fatal(String),
    Ack { request_id: u64, revision: u64, error: Option<String>, canceled: bool },
}
#[derive(Debug)]
pub struct Envelope { pub request_id: u64, pub epoch: u64, pub request: Request }
impl Request {
    pub fn canceled_by_stop(&self) -> bool { matches!(self, Self::Play | Self::Pause | Self::ToggleLoop | Self::Seek(_) | Self::SeekProject(_) |
        Self::Audition { .. } | Self::Down(..) | Self::Move(..) | Self::Up(..)) }
}
pub struct Connection {
    pub sender: SyncSender<Envelope>, pub receiver: Receiver<Response>, pub flags: Arc<AtomicU32>, pub epoch: Arc<AtomicU64>, pub worker: JoinHandle<()>,
}
struct State {
    project: Project, history: History, engine: Engine, viewport: Viewport, track_id: u64, clip_id: Option<u64>,
    selected: Option<u64>, gesture: Option<Gesture>, insertion: Option<(u64, u8)>,
    live: HashMap<(u64, u8), u64>, next_live: u64,
    dirty: bool, needs_compile: bool, looping: bool, silent: bool, status: String,
    export_job: Option<JoinHandle<Result<(), String>>>,
    arrangement: ArrangementViewport, revision: u64, mix_dirty: bool, pending_mix: bool,
    submitted_mix: u64, applied_mix: u64,
}
impl State {
    fn new(silent: bool) -> Result<Self, String> {
        Ok(Self { project: Project::demo(), history: History::default(), engine: Engine::new(48000, 256)?, viewport: Viewport { top_note: 73, ..Viewport::default() },
            track_id: 1, clip_id: Some(4), selected: None, gesture: None, insertion: None,
            live: HashMap::new(), next_live: 1, dirty: false, needs_compile: true, looping: false, silent,
            status: if silent { "SILENT CLOCK mode; no physical audio output" } else { "Ready. Select a track or add an oscillator." }.into(), export_job: None,
            arrangement: ArrangementViewport::default(), revision: 0, mix_dirty: false, pending_mix: false, submitted_mix: 0, applied_mix: 0 })
    }
    fn track_index(&self, id: u64) -> Result<usize, String> {
        self.project.tracks.iter().position(|t| t.id == id).ok_or_else(|| "Track no longer exists".into())
    }
    fn clip(&self) -> Option<&crate::project::MidiClip> { self.project.clip(self.track_id, self.clip_id?) }
    fn release_audition(&mut self) -> Result<(), String> {
        let active = std::mem::take(&mut self.live);
        for ((track, pitch), id) in active {
            let index = self.track_index(track)?;
            if let Err(error) = self.engine.live_note(index as u32, id, pitch, false) { self.engine.panic()?; return Err(error); }
        }
        Ok(())
    }
    fn repair_selection(&mut self, fallback: usize) {
        if self.project.track(self.track_id).is_none() {
            self.track_id = self.project.tracks[fallback.min(self.project.tracks.len() - 1)].id;
        }
        if self.clip().is_none() { self.clip_id = self.project.track(self.track_id).and_then(|t| t.clips.first()).map(|c| c.id); }
        if !self.selected.is_some_and(|id| self.clip().is_some_and(|c| c.notes.iter().any(|n| n.id == id))) { self.selected = None; }
    }
    fn quiesce(&mut self) -> Result<(), String> {
        self.engine.stop()?; self.engine.close_device()?;
        let _ = self.engine.render(1)?; // Only consumer after the device has joined.
        self.live.clear(); self.history.finish_gesture(); self.gesture = None; self.insertion = None; Ok(())
    }
    fn sync_mix(&mut self) -> Result<(), String> {
        self.mix_dirty = true;
        if self.needs_compile { self.pending_mix = false; return Ok(()); }
        match self.engine.try_mix(&self.project) {
            Ok(Some(serial)) => { self.submitted_mix = serial; self.pending_mix = false; }
            Ok(None) => self.pending_mix = true, // retain the authoritative document; retry its latest complete state
            Err(error) => { self.needs_compile = true; self.pending_mix = false; self.engine.panic()?;
                return Err(format!("Edit retained; audio needs recompilation: {error}")); }
        }
        Ok(())
    }
    fn poll_mix(&mut self) -> Result<bool, String> {
        let before = (self.pending_mix, self.applied_mix, self.mix_dirty);
        if self.pending_mix { self.sync_mix()?; }
        if !self.needs_compile {
            self.applied_mix = self.engine.mix_status()?.applied;
            if !self.pending_mix && self.applied_mix >= self.submitted_mix { self.mix_dirty = false; }
        }
        Ok(before != (self.pending_mix, self.applied_mix, self.mix_dirty))
    }
    fn edit(&mut self, edit: Edit) -> Result<(), String> {
        let fallback = self.track_index(self.track_id)?;
        let impact = edit.impact();
        if impact == Impact::Structural { self.quiesce()?; }
        self.history.execute(&mut self.project, edit)?;
        self.dirty = true; self.revision += 1; self.repair_selection(fallback);
        match impact {
            Impact::Structural => { self.needs_compile = true; self.status = "Edited. Playback stopped; press Play to resume.".into(); }
            Impact::Metadata => self.status = "Track properties updated. Playback continues.".into(),
            Impact::Realtime => { self.sync_mix()?; self.status = "Mixer updated; gain is preserved when muted.".into(); }
        }
        Ok(())
    }
    fn compile(&mut self) -> Result<(), String> {
        let closed_with_mix = self.mix_dirty && self.engine.telemetry()?.device_open == 0;
        if self.needs_compile || closed_with_mix {
            self.engine.load(&self.project)?; self.needs_compile = false; self.mix_dirty = false;
            self.pending_mix = false; self.submitted_mix = 0; self.applied_mix = 0;
        }
        Ok(())
    }
    fn view(&self) -> ViewState {
        let t = self.project.track(self.track_id).expect("selected track invariant");
        let preview = self.gesture.as_ref().map(|g| &g.preview);
        let (notes, capped) = self.viewport.scene(&self.project, t.id, self.clip_id.unwrap_or(0), self.selected, preview);
        let selected_label = self.selected.and_then(|id| self.clip()?.notes.iter().find(|n| n.id == id))
            .map(|n| format!("{} · {:.2} beats · velocity {}", crate::editor::label(n.pitch), n.duration as f64 / PPQ as f64, n.velocity))
            .unwrap_or_else(|| if self.clip().is_none() { "This track has no MIDI clips.".into() }
                else { "Click empty space to insert · drag to move · drag right edge to resize".into() });
        let (tracks, arrangement_limited) = arrangement::scene(&self.project, self.arrangement, self.track_id, self.clip_id);
        let (sample_path, root_note, one_shot, is_sampler) = match &t.instrument.config {
            Instrument::Sine => (String::new(), 60, false, false),
            Instrument::Sampler { sample_path, root_note, one_shot } => (sample_path.clone(), *root_note, *one_shot, true),
        };
        ViewState { name: self.project.name.clone(), status: if capped { format!("{} | View capped at 3000 notes; zoom in.", self.status) } else { self.status.clone() },
            tracks,
            mute: t.mute, solo: t.solo, audible: self.project.audible(t.id), project_beats: self.project.length_ticks as f32 / PPQ as f32, arrangement_limited,
            instrument: t.instrument.config.label().into(), sample_path, root_note, one_shot, is_sampler,
            revision: self.revision, mix_label: if self.needs_compile || (self.mix_dirty && self.engine.telemetry().is_ok_and(|t| t.device_open == 0)) { "Ready on Play" } else if self.pending_mix || self.mix_dirty { "Applying mixer…" } else { "Mixer synchronized" }.into(),
            track_id: t.id, track_index: self.track_index(t.id).unwrap(), track_name: t.name.clone(), track_color: t.color.clone(),
            clips: t.clips.iter().map(|c| ClipView { id: c.id, name: c.name.clone() }).collect(),
            clip_index: t.clips.iter().position(|c| Some(c.id) == self.clip_id), clip_start: self.clip().map_or(0, |c| c.start_tick),
            bpm: self.project.bpm, gain: t.gain, pan: t.pan, dirty: self.dirty, can_undo: self.history.can_undo(), can_redo: self.history.can_redo(),
            notes, viewport: self.viewport, export_busy: self.export_job.is_some(), selected_label }
    }
    fn select_track(&mut self, id: u64) -> Result<(), String> {
        self.track_index(id)?; self.release_audition()?; self.history.finish_gesture();
        self.track_id = id; self.clip_id = self.project.track(id).and_then(|t| t.clips.first()).map(|c| c.id);
        self.selected = None; self.gesture = None; self.insertion = None; self.viewport.start_tick = 0;
        let highest = self.clip().and_then(|c| c.notes.iter().map(|n| n.pitch).max()).unwrap_or(72);
        self.viewport.top_note = highest.saturating_add(1).clamp(24, 127); Ok(())
    }
    fn request(&mut self, r: Request, flags: &AtomicU32) -> Result<(), String> {
        let track_id = self.track_id;
        match r {
            Request::Play => {
                self.compile()?;
                if flags.load(Ordering::Acquire) & (STOP | QUIT) != 0 { return Ok(()); }
                self.engine.set_loop(self.looping, 0, self.project.frame_at(self.project.length_ticks, 48000))?;
                self.engine.start_device(self.silent)?; self.engine.play()?;
                self.status = if self.silent { "SILENT CLOCK — transport simulation; no audio output" } else { "Playing through native output" }.into();
            }
            Request::Pause => { self.engine.pause()?; self.live.clear(); self.gesture = None; self.insertion = None; }
            Request::ToggleLoop => {
                self.looping = !self.looping;
                if !self.needs_compile { self.engine.set_loop(self.looping, 0, self.project.frame_at(self.project.length_ticks, 48000))?; }
                self.status = if self.looping { "Loop: full project range" } else { "Loop disabled" }.into();
            }
            Request::Seek(beat) => {
                if !beat.is_finite() || beat < 0.0 { return Err("Invalid seek position".into()); }
                self.compile()?;
                let begin = self.clip().map_or(0, |c| c.start_tick);
                let relative = (beat * PPQ as f64).min(self.project.length_ticks as f64) as u64;
                let tick = begin.saturating_add(relative).min(self.project.length_ticks);
                self.engine.seek(self.project.frame_at(tick, 48000))?; self.live.clear();
                self.status = "Seek resets voices; earlier notes are not chased.".into();
            }
            Request::ArrangementViewport(start, scale, width) => self.arrangement = ArrangementViewport::checked(start, scale, width)?,
            Request::SeekProject(beat) => {
                if !beat.is_finite() || beat < 0.0 { return Err("Invalid seek position".into()); }
                self.compile()?; let tick = (beat * PPQ as f64).min(self.project.length_ticks as f64) as u64;
                self.engine.seek(self.project.frame_at(tick, 48000))?; self.live.clear();
            }
            Request::ToggleMute(track) | Request::ToggleSolo(track) => {
                let t = self.project.track(track).ok_or("Track no longer exists")?;
                let (mute, solo) = if matches!(r, Request::ToggleMute(_)) { (!t.mute, t.solo) } else { (t.mute, !t.solo) };
                self.edit(Edit::Audibility { track, mute, solo })?;
            }
            Request::SetMute { track, value } | Request::SetSolo { track, value } => {
                let t = self.project.track(track).ok_or("Track no longer exists")?;
                let (mute, solo) = if matches!(r, Request::SetMute { .. }) { (value, t.solo) } else { (t.mute, value) };
                if (mute, solo) != (t.mute, t.solo) { self.edit(Edit::Audibility { track, mute, solo })?; }
            }
            Request::SetGain { track, value } => {
                let t = self.project.track(track).ok_or("Track no longer exists")?;
                if t.gain != value { self.edit(Edit::Mix { track, gain: value, pan: t.pan })?; }
            }
            Request::SetPan { track, value } => {
                let t = self.project.track(track).ok_or("Track no longer exists")?;
                if t.pan != value { self.edit(Edit::Mix { track, gain: t.gain, pan: value })?; }
            }
            Request::ReorderTrack { track, index } => {
                let old = self.track_index(track)?;
                if index < 0 || index as usize >= self.project.tracks.len() { return Err("Invalid destination".into()); }
                if old != index as usize { self.edit(Edit::MoveTrack { track, index: index as usize })?; }
            }
            Request::SelectTrack(id) => self.select_track(id)?,
            Request::SelectClip { track, clip } => {
                if self.project.clip(track, clip).is_none() { return Err("Clip no longer exists".into()); }
                self.select_track(track)?; self.clip_id = Some(clip); self.viewport.start_tick = 0;
                let highest = self.clip().and_then(|c| c.notes.iter().map(|n| n.pitch).max()).unwrap_or(72);
                self.viewport.top_note = highest.saturating_add(1).clamp(24, 127);
            }
            Request::AddTrack { after } => {
                self.track_index(after)?; if self.project.tracks.len() >= MAX_TRACKS { return Err("This increment supports up to 16 tracks.".into()); }
                let new_id = self.project.next_id; self.edit(Edit::AddTrack { after })?; self.select_track(new_id)?;
            }
            Request::DuplicateTrack(id) => {
                self.track_index(id)?; if self.project.tracks.len() >= MAX_TRACKS { return Err("This increment supports up to 16 tracks.".into()); }
                let new_id = self.project.next_id; self.edit(Edit::DuplicateTrack { track: id })?; self.select_track(new_id)?;
            }
            Request::RemoveTrack(id) => {
                self.track_index(id)?; if self.project.tracks.len() <= 1 { return Err("Keep at least one track.".into()); }
                self.edit(Edit::RemoveTrack { track: id })?;
            }
            Request::MoveTrack { track, direction } => {
                let from = self.track_index(track)?;
                let index = (from as i64 + direction.signum() as i64).clamp(0, self.project.tracks.len() as i64 - 1) as usize;
                if index != from { self.edit(Edit::MoveTrack { track, index })?; }
            }
            Request::RenameTrack { track, name } => {
                let t = self.project.track(track).ok_or("Track no longer exists")?;
                if t.name != name { self.edit(Edit::TrackInfo { track, name, color: t.color.clone() })?; }
            }
            Request::ColorTrack { track, color } => {
                let t = self.project.track(track).ok_or("Track no longer exists")?;
                if t.color != color { self.edit(Edit::TrackInfo { track, name: t.name.clone(), color })?; }
            }
            Request::Tempo(bpm) => self.edit(Edit::Tempo(bpm))?,
            Request::Mix { track, gain, pan } => self.edit(Edit::Mix { track, gain, pan })?,
            Request::FinishMix => self.history.finish_gesture(),
            Request::Undo | Request::Redo => {
                let redo = matches!(r, Request::Redo);
                let impact = if redo { self.history.redo_impact() } else { self.history.undo_impact() };
                let fallback = self.track_index(self.track_id)?;
                if impact == Some(Impact::Structural) { self.quiesce()?; }
                let changed = if redo { self.history.redo(&mut self.project)? } else { self.history.undo(&mut self.project)? };
                if changed {
                    self.dirty = true; self.revision += 1; self.repair_selection(fallback); self.gesture = None; self.insertion = None;
                    if impact == Some(Impact::Structural) { self.needs_compile = true; self.status = "History applied. Press Play to resume.".into(); }
                    else { self.sync_mix()?; self.status = "History applied; playback continues.".into(); }
                }
            }
            Request::Delete => { if let (Some(id), Some(clip)) = (self.selected, self.clip_id) { self.edit(Edit::Delete { track: track_id, clip, id })?; self.selected = None; } }
            Request::Viewport(w, h) => {
                if w.is_finite() && h.is_finite() { self.viewport.width = w.clamp(1.0, 4096.0); self.viewport.height = h.clamp(1.0, 2160.0); }
            }
            Request::Zoom(f) => { if f.is_finite() && f > 0.0 { self.viewport.pixels_per_beat = (self.viewport.pixels_per_beat * f).clamp(24.0, 256.0); } }
            Request::Pan(beats) => {
                let length = self.clip().map_or(0, |c| c.length_ticks);
                self.viewport.start_tick = (self.viewport.start_tick as i64 + beats as i64 * PPQ as i64).clamp(0, length.saturating_sub(PPQ) as i64) as u64;
            }
            Request::Pitch(semitones) => self.viewport.top_note = (self.viewport.top_note as i32 + semitones).clamp(12, 127) as u8,
            Request::Down(x, y, right) => {
                let clip = self.clip_id.ok_or("Select a MIDI clip before editing")?;
                let length = self.clip().ok_or("Clip no longer exists")?.length_ticks;
                self.gesture = None; self.insertion = None;
                if let Some((note, resize)) = self.viewport.hit(&self.project, track_id, clip, x, y) {
                    self.selected = Some(note.id);
                    if right { self.edit(Edit::Delete { track: track_id, clip, id: note.id })?; self.selected = None; }
                    else { self.gesture = Some(Gesture { original: note.clone(), preview: note, x, y, resize }); }
                } else if !right {
                    self.selected = None; self.insertion = self.viewport.insertion(length, x, y);
                }
            }
            Request::Move(x, y) => {
                let length = self.clip().map_or(0, |c| c.length_ticks);
                if let Some(g) = &mut self.gesture { g.preview = self.viewport.drag(g, x, y, length); }
            }
            Request::Up(x, y) => {
                let Some(clip) = self.clip_id else { return Ok(()); };
                let length = self.clip().ok_or("Clip no longer exists")?.length_ticks;
                if let Some(g) = self.gesture.take() {
                    let note = self.viewport.drag(&g, x, y, length);
                    if note != g.original { self.edit(Edit::Replace { track: track_id, clip, note })?; }
                } else if let Some((start, pitch)) = self.insertion.take() {
                    let index = self.clip().unwrap().notes.len(); let id = self.project.allocate_id()?;
                    let note = Note { id, start, duration: (PPQ / 2).min(length - start).max(1), pitch, velocity: 100 };
                    self.edit(Edit::Insert { track: track_id, clip, index, note })?; self.selected = Some(id);
                }
            }
            Request::Audition { track, pitch, on } => {
                let index = self.track_index(track)?;
                if on {
                    self.compile()?; self.engine.start_device(self.silent)?;
                    if let Some(previous) = self.live.remove(&(track, pitch)) { self.engine.live_note(index as u32, previous, pitch, false)?; }
                    if self.live.len() >= 128 || self.next_live >= (1u64 << 63) - 1 { return Err("Live audition limit reached".into()); }
                    let token = self.next_live; self.next_live += 1;
                    self.engine.live_note(index as u32, token, pitch, true)?; self.live.insert((track, pitch), token);
                } else if let Some(token) = self.live.remove(&(track, pitch)) { self.engine.live_note(index as u32, token, pitch, false)?; }
            }
            Request::Save(path) => {
                self.project = storage::save(&path, &self.project)?; self.dirty = false; self.history.finish_gesture();
                self.status = format!("Saved {} (format v2)", path.display());
            }
            Request::Open(path) => {
                if self.dirty { return Err("Save current edits before opening another project.".into()); }
                let (candidate, source_version) = storage::load_versioned(&path)?; self.engine.load(&candidate)?;
                self.live.clear(); self.project = candidate; self.history = History::default(); self.needs_compile = false;
                self.track_id = self.project.tracks[0].id; self.clip_id = None; self.selected = None; self.gesture = None; self.insertion = None;
                self.revision += 1; self.mix_dirty = false; self.pending_mix = false; self.submitted_mix = 0; self.applied_mix = 0;
                self.repair_selection(0); self.viewport.start_tick = 0;
                let highest = self.clip().and_then(|c| c.notes.iter().map(|n| n.pitch).max()).unwrap_or(72);
                self.viewport.top_note = highest.saturating_add(1).clamp(24, 127);
                self.status = if source_version == 1 { "Opened v1 project. Save upgrades to v2 and retains recovery/pre-m3-v1.json.".into() }
                    else { format!("Opened {}", path.display()) };
            }
            Request::Export(path) => {
                if self.export_job.is_some() { return Err("An export is already running.".into()); }
                let project = self.project.clone(); self.status = format!("Exporting {} using an independent engine", path.display());
                self.export_job = Some(thread::spawn(move || storage::export_atomic(&path, &project)));
            }
            Request::Import { track, path, root_note, one_shot } => {
                let index = self.track_index(track)?; self.quiesce()?;
                let full = path.canonicalize().map_err(|e| e.to_string())?;
                let instrument = Instrument::Sampler { sample_path: full.to_str().ok_or("Sample path must be UTF-8")?.into(), root_note, one_shot };
                let mut candidate = self.project.clone(); candidate.tracks[index].instrument.config = instrument.clone();
                self.engine.load(&candidate)?;
                self.history.execute(&mut self.project, Edit::Instrument { track, instrument })?;
                self.dirty = true; self.revision += 1; self.needs_compile = false; self.mix_dirty = false; self.pending_mix = false;
                self.submitted_mix = 0; self.applied_mix = 0; self.status = "WAV loaded into this instance. Save copies samples into assets/.".into();
            }
            Request::UseSine(track) => { self.track_index(track)?; self.edit(Edit::Instrument { track, instrument: Instrument::Sine })?; }
        }
        Ok(())
    }
}
pub fn spawn(silent: bool) -> Connection {
    let (sender, requests) = mpsc::sync_channel::<Envelope>(64);
    let (responses, receiver) = mpsc::sync_channel(8);
    let flags = Arc::new(AtomicU32::new(0)); let worker_flags = flags.clone();
    let epoch = Arc::new(AtomicU64::new(0)); let worker_epoch = epoch.clone();
    let worker = thread::spawn(move || {
        let mut state = match State::new(silent) { Ok(s) => s, Err(e) => { let _ = responses.send(Response::Fatal(e)); return; } };
        let mut pending = Some(Box::new(state.view())); let mut last_meter = Instant::now();
        let mut acknowledgments = VecDeque::new(); let mut deferred = None;
        loop {
            let emergency = worker_flags.swap(0, Ordering::AcqRel);
            if emergency & QUIT != 0 { break; }
            if emergency & STOP != 0 {
                if let Err(e) = state.quiesce() { state.status = e; }
                else { state.status = "Stopped. Pending document edits are preserved.".into(); }
                pending = Some(Box::new(state.view()));
            } else if emergency & PANIC != 0 {
                if let Err(e) = state.engine.panic() { state.status = e; }
                state.live.clear(); state.gesture = None; state.insertion = None;
                state.status = "Panic: voices released. Pending document edits are preserved.".into();
                pending = Some(Box::new(state.view()));
            }
            while let Some(ack) = acknowledgments.pop_front() {
                match responses.try_send(ack) {
                    Ok(()) => {}, Err(TrySendError::Full(ack)) => { acknowledgments.push_front(ack); break; },
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
            if acknowledgments.len() < 64 {
                match deferred.take().map(Ok).unwrap_or_else(|| requests.recv_timeout(Duration::from_millis(8))) {
                    Ok(envelope) => {
                        // Honor a priority flag that arrived during recv_timeout before dispatching
                        // this request. A new Play after Stop must wait for quiescence, not be lost.
                        if worker_flags.load(Ordering::Acquire) != 0 { deferred = Some(envelope); continue; }
                        let canceled = envelope.epoch != worker_epoch.load(Ordering::Acquire) && envelope.request.canceled_by_stop();
                        let error = if canceled { None } else { state.request(envelope.request, &worker_flags).err() };
                        if let Some(error) = &error { state.status = error.clone(); }
                        acknowledgments.push_back(Response::Ack { request_id: envelope.request_id, revision: state.revision, error, canceled });
                        pending = Some(Box::new(state.view()));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            } else { thread::sleep(Duration::from_millis(8)); } // control thread only; bounded acknowledgment backlog
            match state.poll_mix() {
                Ok(true) => pending = Some(Box::new(state.view())),
                Err(error) => { state.status = error; pending = Some(Box::new(state.view())); },
                _ => {}
            }
            if state.export_job.as_ref().is_some_and(|j| j.is_finished()) {
                let result = state.export_job.take().unwrap().join();
                state.status = match result { Ok(Ok(())) => "Export complete. Playback state was not reset.".into(), Ok(Err(e)) => e, Err(_) => "Export worker failed".into() };
                pending = Some(Box::new(state.view()));
            }
            if acknowledgments.is_empty() {
                if let Some(view) = pending.take() {
                    match responses.try_send(Response::View(view)) {
                        Ok(()) => {}, Err(TrySendError::Full(Response::View(view))) => pending = Some(view),
                        Err(TrySendError::Disconnected(_)) => break, Err(_) => unreachable!(),
                    }
                }
            }
            if last_meter.elapsed() >= Duration::from_millis(33) {
                if let Ok(t) = state.engine.telemetry() { let _ = responses.try_send(Response::Meter(t)); }
                last_meter = Instant::now();
            }
        }
        let _ = state.quiesce();
        if let Some(job) = state.export_job.take() { let _ = job.join(); }
    });
    Connection { sender, receiver, flags, epoch, worker }
}

#[cfg(test)] mod tests {
    use super::*;
    fn request(state: &mut State, r: Request) { state.request(r, &AtomicU32::new(0)).unwrap(); state.project.validate().unwrap(); }
    #[test] fn identity_selection_survives_duplicate_reorder_delete_and_history() {
        let mut state = State::new(true).unwrap();
        request(&mut state, Request::DuplicateTrack(1)); let id = state.track_id; let instance = state.project.track(id).unwrap().instrument.id;
        request(&mut state, Request::MoveTrack { track: id, direction: 1 });
        assert_eq!(state.track_id, id); assert_eq!(state.project.track(id).unwrap().instrument.id, instance);
        request(&mut state, Request::RemoveTrack(id)); assert_ne!(state.track_id, id); assert!(!state.view().tracks.is_empty());
        request(&mut state, Request::Undo); assert!(state.project.track(id).is_some());
        request(&mut state, Request::Redo); assert!(state.project.track(id).is_none()); assert!(state.clip().is_some());
        assert!(state.request(Request::RenameTrack { track: id, name: "Stale callback".into() }, &AtomicU32::new(0)).is_err());
    }
    #[test] fn metadata_and_undo_preserve_running_engine() {
        let mut state = State::new(true).unwrap(); state.compile().unwrap(); state.engine.play().unwrap(); state.engine.render(256).unwrap();
        request(&mut state, Request::RenameTrack { track: 1, name: "Lead renamed".into() });
        request(&mut state, Request::ColorTrack { track: 1, color: "#ABCDEF".into() });
        assert!(!state.needs_compile); state.engine.render(256).unwrap(); let t = state.engine.telemetry().unwrap(); assert_eq!(t.position, 512); assert_eq!(t.playing, 1);
        request(&mut state, Request::Undo); assert!(!state.needs_compile); state.engine.render(256).unwrap(); assert_eq!(state.engine.telemetry().unwrap().position, 768);
    }
    #[test] fn delayed_mix_targets_original_track_and_history_repairs_selection() {
        let mut state = State::new(true).unwrap(); request(&mut state, Request::SelectTrack(2));
        request(&mut state, Request::Mix { track: 1, gain: 0.123, pan: -0.6 });
        assert_eq!(state.project.track(1).unwrap().gain, 0.123); assert_eq!(state.project.track(2).unwrap().gain, 0.55);
        request(&mut state, Request::AddTrack { after: 2 }); let added = state.track_id;
        request(&mut state, Request::Undo); assert!(state.project.track(added).is_none()); assert!(state.project.track(state.track_id).is_some());
        request(&mut state, Request::Redo); assert!(state.project.track(added).is_some());
    }
    #[test] fn live_mute_solo_and_axes_do_not_rebuild_reset_or_overwrite_faders() {
        let mut state = State::new(true).unwrap(); state.compile().unwrap(); state.engine.play().unwrap(); state.engine.render(256).unwrap();
        request(&mut state, Request::SetGain { track: 1, value: 0.321 });
        request(&mut state, Request::SetPan { track: 1, value: -0.7 });
        request(&mut state, Request::SetSolo { track: 1, value: true });
        request(&mut state, Request::SetSolo { track: 2, value: true });
        request(&mut state, Request::SetMute { track: 1, value: true });
        assert!(!state.project.audible(1)); assert!(state.project.audible(2)); assert!(!state.needs_compile);
        state.engine.render(256).unwrap(); state.poll_mix().unwrap();
        assert_eq!(state.engine.telemetry().unwrap().position, 512); assert!(!state.mix_dirty);
        request(&mut state, Request::Undo); assert!(state.project.audible(1));
        request(&mut state, Request::Redo); assert!(!state.project.audible(1));
        assert_eq!(state.project.track(1).unwrap().gain, 0.321); assert_eq!(state.project.track(1).unwrap().pan, -0.7);
    }
    #[test] fn full_native_mix_queue_recovers_latest_flags_after_stop_and_recompile() {
        let mut state = State::new(true).unwrap(); state.compile().unwrap();
        for i in 0..8 { request(&mut state, Request::SetGain { track: 1, value: 0.2 + i as f32 / 100.0 }); }
        request(&mut state, Request::SetMute { track: 1, value: true }); assert!(state.pending_mix);
        state.quiesce().unwrap(); state.poll_mix().unwrap(); assert!(!state.pending_mix);
        // The accepted snapshot can still await a consumer after Stop. Compilation must load the desired document.
        state.compile().unwrap(); assert!(!state.mix_dirty); assert_eq!(state.engine.mix_status().unwrap().submitted, 0);
        state.engine.play().unwrap(); let actual = state.engine.render(1024).unwrap();
        let mut p = state.project.clone(); p.tracks[0].clips.clear(); p.tracks[0].mute = false;
        let mut reference = Engine::new(48000, 256).unwrap(); reference.load(&p).unwrap(); reference.play().unwrap();
        assert_eq!(actual, reference.render(1024).unwrap());
    }
    #[test] fn worker_acknowledges_durable_edits_even_when_stop_invalidates_pending_transport() {
        let connection = spawn(true);
        for (i, request) in [Request::SetMute { track: 1, value: true }, Request::SetSolo { track: 2, value: true },
            Request::RenameTrack { track: 1, name: "After Stop".into() }, Request::Play].into_iter().enumerate() {
            connection.sender.send(Envelope { request_id: i as u64 + 1, epoch: 0, request }).unwrap();
        }
        connection.epoch.store(1, Ordering::Release); connection.flags.fetch_or(STOP, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(5); let mut acks = Vec::new(); let mut verified = false;
        while Instant::now() < deadline && (acks.len() < 4 || !verified) {
            match connection.receiver.recv_timeout(Duration::from_millis(100)).unwrap() {
                Response::Ack { request_id, error, .. } => { assert!(error.is_none()); acks.push(request_id); }
                Response::View(v) if v.revision >= 3 => {
                    assert_eq!(v.track_name, "After Stop"); assert!(v.tracks[0].mute); assert!(v.tracks[1].solo); verified = true;
                }
                _ => {}
            }
        }
        connection.flags.fetch_or(QUIT, Ordering::Release); connection.worker.join().unwrap();
        assert!(verified); assert_eq!(acks, vec![1, 2, 3, 4]);
    }

    #[test] fn new_play_after_priority_stop_is_not_lost() {
        let connection = spawn(true);
        for id in 1..=12 {
            let epoch = connection.epoch.fetch_add(1, Ordering::AcqRel) + 1;
            connection.flags.fetch_or(STOP, Ordering::Release);
            connection.sender.send(Envelope { request_id: id, epoch, request: Request::Play }).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3); let mut acknowledged = false; let mut playing = false;
            while Instant::now() < deadline && !playing {
                match connection.receiver.recv_timeout(Duration::from_millis(100)).unwrap() {
                    Response::Ack { request_id, canceled, error, .. } if request_id == id => {
                        assert!(!canceled); assert!(error.is_none()); acknowledged = true;
                    }
                    Response::Meter(t) if acknowledged && t.playing == 1 && t.device_open == 1 => playing = true,
                    _ => {}
                }
            }
            assert!(playing, "new Play after Stop must execute, request {id}");
        }
        connection.flags.fetch_or(QUIT, Ordering::Release); connection.worker.join().unwrap();
    }

}
