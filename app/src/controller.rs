use crate::{arrangement::{self, ArrangementViewport, TrackRow}, bridge::{Engine, Telemetry}, editor::{self, Gesture, KeyRow, NoteRect, Viewport}, project::{Edit, History, Impact, Instrument, Project, PPQ, MAX_TRACKS}, storage};
use crate::drums::{DrumKit,DrumPad,PadField};
use crate::synth::{SynthParameters,SynthPreset,SynthField};
use std::{collections::{HashMap, VecDeque}, path::PathBuf, sync::{Arc, atomic::{AtomicU32, AtomicU64, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
pub const PANIC: u32 = 1; pub const STOP: u32 = 2; pub const QUIT: u32 = 4; pub const RELEASE: u32 = 8;
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
    AddClip { track: u64, beat: f64 }, DuplicateClip { track: u64, clip: u64 }, RemoveClip { track: u64, clip: u64 },
    MoveClip { track: u64, clip: u64, destination: u64, beat: f64 }, ResizeClip { track: u64, clip: u64, beats: f64 },
    RenameClip { track: u64, clip: u64, name: String }, Velocity { track: u64, clip: u64, note: u64, value: i32 },
    AddDrumTrack{after:u64},UseDrumKit{track:u64},SelectPad{track:u64,instance:u64,pad:u64},
    PadParameter{track:u64,instance:u64,pad:u64,field:PadField,value:f32},
    TogglePad{track:u64,instance:u64,pad:u64,field:PadField},
    RenamePad{track:u64,instance:u64,pad:u64,name:String},MapPad{track:u64,instance:u64,pad:u64,midi:i32},
    ImportPad{track:u64,instance:u64,pad:u64,path:PathBuf},ClearPad{track:u64,instance:u64,pad:u64},
    PadAuditionOn{token:u64,epoch:u64,track:u64,instance:u64,pad:u64,velocity:u8},
    SynthParameter {track:u64,instance:u64,field:SynthField,value:f32},
    FactoryPreset {track:u64,instance:u64,index:usize},
    SaveSynthPreset {track:u64,instance:u64,path:PathBuf,name:String}, LoadSynthPreset {track:u64,instance:u64,path:PathBuf},
    Tempo(f64), Mix { track: u64, gain: f32, pan: f32 }, FinishMix, Undo, Redo, Delete { track: u64, clip: u64, note: u64 },
    Viewport(f32, f32), Zoom(f32), ZoomY(f32), Pan(i32), Pitch(i32), ScrollPiano(f64, f32), FitNotes, Notation(i32),
    Down { track: u64, clip: u64, token: u64, viewport: Viewport, x: f32, y: f32, right: bool },
    Move { token: u64, x: f32, y: f32 }, Up { token: u64, x: f32, y: f32 }, CancelGesture(u64),
    AuditionOn { token: u64, epoch: u64, track: u64, instance: u64, pitch: u8, velocity: u8 }, AuditionOff(u64),
    Save(PathBuf), Open(PathBuf), Export(PathBuf),
    Import { track: u64, path: PathBuf, root_note: u8, one_shot: bool }, UseSine(u64),
}
#[derive(Clone)]
pub struct ClipView { pub id: u64, pub name: String }
#[derive(Clone)]
pub struct ViewState {
    pub name: String, pub status: String, pub tracks: Vec<TrackRow>, pub track_id: u64, pub track_index: usize,
    pub clips: Vec<ClipView>, pub clip_index: Option<usize>, pub clip_start: u64, pub clip_id: u64, pub clip_length: u64, pub clip_name: String, pub track_name: String, pub track_color: String,
    pub bpm: f64, pub gain: f32, pub pan: f32, pub dirty: bool, pub can_undo: bool, pub can_redo: bool,
    pub mute: bool, pub solo: bool, pub audible: bool, pub project_beats: f32, pub arrangement_limited: bool,
    pub drum_kit:Option<DrumKit>,pub selected_pad:u64,
    pub synth:Option<SynthParameters>, pub synth_preset:Option<SynthPreset>,
    pub instrument: String, pub sample_path: String, pub root_note: u8, pub one_shot: bool, pub is_sampler: bool,
    pub revision: u64, pub mix_label: String, pub instance_id: u64, pub selected_note: u64, pub velocity: i32, pub keys: Vec<KeyRow>, pub live_count: usize,
    pub notes: Vec<NoteRect>, pub viewport: Viewport, pub export_busy: bool, pub selected_label: String,
}
pub enum Response {
    View(Box<ViewState>), Meter(Telemetry), DrumMeter{track:u64,instance:u64,status:crate::bridge::DrumStatus}, Fatal(String),
    Ack { request_id: u64, revision: u64, error: Option<String>, canceled: bool },
}
#[derive(Debug)]
pub struct Envelope { pub request_id: u64, pub epoch: u64, pub request: Request }
impl Request {
    pub fn canceled_by_stop(&self) -> bool { matches!(self, Self::Play | Self::Pause | Self::ToggleLoop | Self::Seek(_) | Self::SeekProject(_) |
        Self::AuditionOn { .. } | Self::PadAuditionOn{..} | Self::AuditionOff(_) | Self::Down { .. } | Self::Move { .. } | Self::Up { .. }) }
}
pub struct Connection {
    pub sender: SyncSender<Envelope>, pub receiver: Receiver<Response>, pub flags: Arc<AtomicU32>, pub epoch: Arc<AtomicU64>, pub audition_epoch: Arc<AtomicU64>, pub worker: JoinHandle<()>,
}
#[derive(Clone, Copy)]
struct InputContext { track: u64, clip: u64, token: u64, viewport: Viewport }
struct LiveNote { track: u64, instance: u64, pitch: u8, native_id: u64 }
struct State {
    project: Project, history: History, engine: Engine,staging:storage::DrumStaging,selected_pad:Option<u64>, viewport: Viewport, track_id: u64, clip_id: Option<u64>,
    selected: Option<u64>, gesture: Option<Gesture>, insertion: Option<(u64, u8)>,
    live: HashMap<u64, LiveNote>, next_live: u64,
    input: Option<InputContext>, viewports: HashMap<u64, Viewport>, viewport_ready: bool,
    dirty: bool, needs_compile: bool, looping: bool, silent: bool, status: String,
    export_job: Option<JoinHandle<Result<(), String>>>,
    arrangement: ArrangementViewport, revision: u64, mix_dirty: bool, pending_mix: bool,
    submitted_mix: u64, applied_mix: u64,
}
impl State {
    fn new(silent: bool) -> Result<Self, String> {
        Ok(Self { project: Project::demo(), history: History::default(), engine: Engine::new(48000, 256)?,staging:storage::DrumStaging::default(),selected_pad:None, viewport: Viewport::default(),
            track_id: 1, clip_id: Some(4), selected: None, gesture: None, insertion: None,
            live: HashMap::new(), next_live: 1, input: None, viewports: HashMap::new(), viewport_ready: false, dirty: false, needs_compile: true, looping: false, silent,
            status: if silent { "SILENT CLOCK mode; no physical audio output" } else { "Ready. Select a track or add an oscillator." }.into(), export_job: None,
            arrangement: ArrangementViewport::default(), revision: 0, mix_dirty: false, pending_mix: false, submitted_mix: 0, applied_mix: 0 })
    }
    fn track_index(&self, id: u64) -> Result<usize, String> {
        self.project.tracks.iter().position(|t| t.id == id).ok_or_else(|| "Track no longer exists".into())
    }
    fn clip(&self) -> Option<&crate::project::MidiClip> { self.project.clip(self.track_id, self.clip_id?) }
    fn release_audition(&mut self) -> Result<(), String> {
        self.live.clear();self.engine.cancel_live()
    }
    fn release_live(&mut self, note: LiveNote) -> Result<(), String> {
        let index = self.track_index(note.track)?;
        if self.project.tracks[index].instrument.id != note.instance { self.engine.panic()?; return Err("Audition instance changed".into()); }
        if let Err(error) = self.engine.live_note(index as u32, note.native_id, note.pitch, false) { self.engine.panic()?; return Err(error); } Ok(())
    }
    fn cache_viewport(&mut self) {
        if let Some(id)=self.clip_id {
            if !self.viewports.contains_key(&id) && self.viewports.len()>=4096 { if let Some(old)=self.viewports.keys().next().copied() {self.viewports.remove(&old);} }
            self.viewports.insert(id,self.viewport);
        }
    }
    fn fit_notes(&mut self) {
        let mut v=self.viewport; v.fit(self.clip());
        if self.clip().is_none_or(|c|c.notes.is_empty()) {
            if let Some(kit)=self.project.track(self.track_id).and_then(|t|t.instrument.config.drums()){
                let low=kit.pads.iter().map(|p|p.midi_note).min().unwrap_or(35);let high=kit.pads.iter().map(|p|p.midi_note).max().unwrap_or(75);
                v.row_height=(v.height/(high as f32-low as f32+3.0)).clamp(12.0,24.0);v.scroll_y=((127.0-(high as f32+low as f32)/2.0+0.5)*v.row_height-v.height/2.0).max(0.0);
            }
        }self.viewport=v;
    }
    fn constrain_viewport(&mut self) {
        let length=self.clip().map_or(self.project.length_ticks,|c|c.length_ticks);
        self.viewport=self.viewport.checked(length).expect("validated viewport");
    }
    fn restore_viewport(&mut self) {
        let old=self.viewport;
        if let Some(v)=self.clip_id.and_then(|id|self.viewports.get(&id)).copied() {
            self.viewport=Viewport{width:old.width,height:old.height,octave_offset:old.octave_offset,..v};
        } else {self.viewport.start_tick=0;self.fit_notes();}
        self.constrain_viewport();
    }
    fn select_clip(&mut self, track:u64, clip:Option<u64>) -> Result<(),String> {
        self.track_index(track)?;
        if clip.is_some_and(|id|self.project.clip(track,id).is_none()) {return Err("Clip no longer exists".into());}
        if self.track_id==track && self.clip_id==clip {return Ok(());}
        self.release_audition()?; self.cache_viewport(); self.history.finish_gesture();
        self.track_id=track; self.clip_id=clip;self.selected_pad=self.project.track(track).and_then(|t|t.instrument.config.drums()).and_then(|k|k.pads.first()).map(|p|p.id); self.selected=None; self.gesture=None; self.insertion=None; self.input=None;
        self.restore_viewport();Ok(())
    }
    fn repair_selection(&mut self, fallback: usize) {
        if self.project.track(self.track_id).is_none() {
            self.track_id = self.project.tracks[fallback.min(self.project.tracks.len() - 1)].id;
        }
        if let Some(id)=self.clip_id {
            if let Some(track)=self.project.tracks.iter().find(|t|t.clips.iter().any(|c|c.id==id)) {self.track_id=track.id;}
        }
        let kit=self.project.track(self.track_id).and_then(|t|t.instrument.config.drums());
        if !self.selected_pad.is_some_and(|id|kit.is_some_and(|k|k.pad(id).is_some())){self.selected_pad=kit.and_then(|k|k.pads.first()).map(|p|p.id);}
        if self.clip().is_none() { self.clip_id = self.project.track(self.track_id).and_then(|t| t.clips.first()).map(|c| c.id); }
        if !self.selected.is_some_and(|id| self.clip().is_some_and(|c| c.notes.iter().any(|n| n.id == id))) { self.selected = None; }
    }
    fn quiesce(&mut self) -> Result<(), String> {
        self.engine.stop()?; self.engine.close_device()?;
        let _ = self.engine.render(1)?; // Only consumer after the device has joined.
        self.live.clear(); self.history.finish_gesture(); self.gesture = None; self.insertion = None; self.input=None; Ok(())
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
    fn drum_pad(&self,track:u64,instance:u64,pad:u64)->Result<&DrumPad,String>{
        let t=self.project.track(track).ok_or("Track no longer exists")?;if t.instrument.id!=instance{return Err("Stale drum instance".into());}
        t.instrument.config.drums().and_then(|k|k.pad(pad)).ok_or_else(||"Drum pad no longer exists".into())
    }
    fn drum_structure(&mut self,edit:Edit)->Result<(),String>{
        // Decode/validate a separate candidate BEFORE stopping or mutating the running project.
        let mut candidate=self.project.clone();History::default().execute(&mut candidate,edit.clone())?;
        let mut prepared=Engine::new(48000,256)?;prepared.load(&candidate)?;
        self.edit(edit)?;self.engine=prepared;self.needs_compile=false;self.mix_dirty=false;self.pending_mix=false;self.submitted_mix=0;self.applied_mix=0;
        self.status="Drum instrument updated. Playback stopped; press Play to resume.".into();Ok(())
    }
    fn synth_instrument(&self,track:u64,instance:u64)->Result<&Instrument,String>{
        let t=self.project.track(track).ok_or("Track no longer exists")?;
        if t.instrument.id!=instance || t.instrument.config.synth().is_none(){return Err("Synth instance changed or no longer exists".into());} Ok(&t.instrument.config)
    }
    fn apply_preset(&mut self,track:u64,instance:u64,preset:SynthPreset)->Result<(),String>{
        self.synth_instrument(track,instance)?;preset.validate()?;self.history.finish_gesture();
        self.edit(Edit::Synth{track,instance,instrument:Instrument::Synth{parameters:preset.parameters,preset:Some(preset)},field:None})?;
        self.status="Preset copied into this instance. ADSR applies to new notes.".into();Ok(())
    }
    fn edit(&mut self, edit: Edit) -> Result<(), String> {
        let fallback = self.track_index(self.track_id)?;
        let impact = edit.impact();
        if impact == Impact::Structural { self.cache_viewport(); self.quiesce()?; }
        let previous_clip=self.clip_id;
        self.history.execute(&mut self.project, edit)?;
        self.dirty = true; self.revision += 1; self.repair_selection(fallback);
        if previous_clip!=self.clip_id {self.restore_viewport();} self.constrain_viewport();
        match impact {
            Impact::Structural => { self.needs_compile = true; self.status = "Edited. Playback stopped; press Play to resume.".into(); }
            Impact::Metadata => self.status = "Track properties updated. Playback continues.".into(),
            Impact::Realtime => { self.sync_mix()?; self.status = "Track sound updated. ADSR applies to new notes; playback continues.".into(); }
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
            .map(|n| format!("{} · {:.2} beats · velocity {}", t.instrument.config.drums().map_or_else(||crate::editor::label_with_offset(n.pitch,self.viewport.octave_offset),|k|k.note_label(n.pitch)), n.duration as f64 / PPQ as f64, n.velocity))
            .unwrap_or_else(|| if self.clip().is_none() { "This track has no MIDI clips.".into() }
                else { "Click empty space to insert · drag to move · drag right edge to resize".into() });
        let (tracks, arrangement_limited) = arrangement::scene(&self.project, self.arrangement, self.track_id, self.clip_id);
        let (sample_path, root_note, one_shot, is_sampler) = match &t.instrument.config {
            Instrument::Sine | Instrument::Synth { .. } | Instrument::DrumKit{..} => (String::new(), 60, false, false),
            Instrument::Sampler { sample_path, root_note, one_shot } => (sample_path.clone(), *root_note, *one_shot, true),
        };
        let mut keys=self.viewport.keys();if let Some(kit)=t.instrument.config.drums(){for key in &mut keys{key.label=kit.note_label(key.pitch);key.black=false;key.c=false;}}
        ViewState { name: self.project.name.clone(), status: if capped { format!("{} | View capped at 3000 notes; zoom in.", self.status) } else { self.status.clone() },
            tracks,
            instance_id:t.instrument.id,selected_note:self.selected.unwrap_or(0),
            velocity:self.selected.and_then(|id|self.clip()?.notes.iter().find(|n|n.id==id)).map_or(100,|n|n.velocity as i32),
            keys,live_count:self.live.len(),drum_kit:t.instrument.config.drums().cloned(),selected_pad:self.selected_pad.unwrap_or(0),
            mute: t.mute, solo: t.solo, audible: self.project.audible(t.id), project_beats: self.project.length_ticks as f32 / PPQ as f32, arrangement_limited,
            synth:t.instrument.config.synth(),synth_preset:t.instrument.config.preset().cloned(),
            instrument: t.instrument.config.label().into(), sample_path, root_note, one_shot, is_sampler,
            revision: self.revision, mix_label: if self.needs_compile || (self.mix_dirty && self.engine.telemetry().is_ok_and(|t| t.device_open == 0)) { "Ready on Play" } else if self.pending_mix || self.mix_dirty { "Applying sound…" } else { "Sound synchronized" }.into(),
            track_id: t.id, track_index: self.track_index(t.id).unwrap(), track_name: t.name.clone(), track_color: t.color.clone(),
            clips: t.clips.iter().map(|c| ClipView { id: c.id, name: c.name.clone() }).collect(),
            clip_index: t.clips.iter().position(|c| Some(c.id) == self.clip_id), clip_start: self.clip().map_or(0, |c| c.start_tick),
            clip_id:self.clip_id.unwrap_or(0),clip_length:self.clip().map_or(0,|c|c.length_ticks),clip_name:self.clip().map_or(String::new(),|c|c.name.clone()),
            bpm: self.project.bpm, gain: t.gain, pan: t.pan, dirty: self.dirty, can_undo: self.history.can_undo(), can_redo: self.history.can_redo(),
            notes, viewport: self.viewport, export_busy: self.export_job.is_some(), selected_label }
    }
    fn select_track(&mut self, id: u64) -> Result<(), String> {
        self.track_index(id)?; if self.track_id==id {return Ok(());}
        let clip=self.project.track(id).and_then(|t|t.clips.first()).map(|c|c.id);
        self.select_clip(id,clip)
    }
    fn request(&mut self, r: Request, flags: &AtomicU32) -> Result<(), String> {
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
            Request::SelectClip { track, clip } => self.select_clip(track,Some(clip))?,
            Request::AddClip { track, beat } => {
                let id=self.project.next_id;let start=editor::beats_to_ticks(beat)?;
                self.edit(Edit::AddClip{track,start,length:4*PPQ})?;self.select_clip(track,Some(id))?;
            }
            Request::DuplicateClip { track,clip } => {
                let id=self.project.next_id;self.edit(Edit::DuplicateClip{track,clip})?;self.select_clip(track,Some(id))?;
            }
            Request::RemoveClip { track,clip } => self.edit(Edit::RemoveClip{track,clip})?,
            Request::RenameClip {track,clip,name} => {
                if self.project.clip(track,clip).ok_or("Clip no longer exists")?.name!=name {self.edit(Edit::ClipName{track,clip,name})?;}
            }
            Request::MoveClip { track,clip,destination,beat } => {
                let start=editor::beats_to_ticks(beat)?;
                let old=self.project.clip(track,clip).ok_or("Clip no longer exists")?;
                if destination!=track || start!=old.start_tick {
                    self.cache_viewport();self.edit(Edit::MoveClip{track,clip,destination,start})?;self.select_clip(destination,Some(clip))?;
                }
            }
            Request::ResizeClip {track,clip,beats} => {
                let length=editor::beats_to_ticks(beats)?;
                if self.project.clip(track,clip).ok_or("Clip no longer exists")?.length_ticks!=length {self.edit(Edit::ResizeClip{track,clip,length})?;}
            }
            Request::Velocity{track,clip,note,value} => {
                if !(1..=127).contains(&value) {return Err("Velocity must be 1–127".into());}
                self.edit(Edit::Velocity{track,clip,note,value:value as u8})?;
            }
            Request::AddTrack { after } => {
                self.track_index(after)?; if self.project.tracks.len() >= MAX_TRACKS { return Err("This increment supports up to 64 tracks.".into()); }
                let new_id = self.project.next_id; self.edit(Edit::AddTrack { after })?; self.select_track(new_id)?;
            }
            Request::DuplicateTrack(id) => {
                self.track_index(id)?; if self.project.tracks.len() >= MAX_TRACKS { return Err("This increment supports up to 64 tracks.".into()); }
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
            Request::AddDrumTrack{after}=>{
                self.track_index(after)?;let id=self.project.next_id;self.drum_structure(Edit::AddDrumTrack{after})?;self.select_track(id)?;self.fit_notes();self.constrain_viewport();
            }
            Request::UseDrumKit{track}=>{self.track_index(track)?;self.drum_structure(Edit::UseDrumKit{track})?;self.repair_selection(0);self.fit_notes();self.constrain_viewport();}
            Request::SelectPad{track,instance,pad}=>{
                let pitch=self.drum_pad(track,instance,pad)?.midi_note;self.select_track(track)?;self.selected_pad=Some(pad);self.history.finish_gesture();
                let y=self.viewport.y(pitch);if y<0. || y+self.viewport.row_height>self.viewport.height{self.viewport.scroll_y=(127.0-pitch as f32+0.5)*self.viewport.row_height-self.viewport.height/2.0;self.constrain_viewport();}
            }
            Request::PadParameter{track,instance,pad,field,value}=>{
                let mut replacement=self.drum_pad(track,instance,pad)?.clone();replacement.parameters.change(field,value)?;
                if &replacement!=self.drum_pad(track,instance,pad)?{self.edit(Edit::Pad{track,instance,pad:replacement,field})?;}
            }
            Request::TogglePad{track,instance,pad,field}=>{
                let p=self.drum_pad(track,instance,pad)?;let value=match field{PadField::Mute=>!p.parameters.mute,PadField::OneShot=>!p.parameters.one_shot,_=>return Err("Invalid pad toggle".into())};
                self.request(Request::PadParameter{track,instance,pad,field,value:if value{1.}else{0.}},flags)?;self.history.finish_gesture();
            }
            Request::RenamePad{track,instance,pad,name}=>{let mut replacement=self.drum_pad(track,instance,pad)?.clone();replacement.name=name;self.edit(Edit::Pad{track,instance,pad:replacement,field:PadField::Name})?;}
            Request::MapPad{track,instance,pad,midi}=>{
                if !(0..=127).contains(&midi){return Err("MIDI mapping must be 0..127".into());}
                let mut replacement=self.drum_pad(track,instance,pad)?.clone();replacement.midi_note=midi as u8;
                self.drum_structure(Edit::Pad{track,instance,pad:replacement,field:PadField::MidiNote})?;
            }
            Request::ImportPad{track,instance,pad,path}=>{
                let mut replacement=self.drum_pad(track,instance,pad)?.clone();let (sample,created)=self.staging.prepare_import(&path)?;replacement.sample=Some(sample.clone());
                if let Err(e)=self.drum_structure(Edit::Pad{track,instance,pad:replacement,field:PadField::Sample}){self.staging.discard_failed(&sample,created);return Err(e);}
                self.status="WAV replaced in this pad. Undo retains the previous sample; Save makes the project portable.".into();
            }
            Request::ClearPad{track,instance,pad}=>{let mut replacement=self.drum_pad(track,instance,pad)?.clone();replacement.sample=None;self.drum_structure(Edit::Pad{track,instance,pad:replacement,field:PadField::Sample})?;}
            Request::PadAuditionOn{token,epoch,track,instance,pad,velocity}=>{
                let pitch=self.drum_pad(track,instance,pad)?.midi_note;
                self.request(Request::AuditionOn{token,epoch,track,instance,pitch,velocity},flags)?;
            }
            Request::SynthParameter{track,instance,field,value}=>{
                let i=self.synth_instrument(track,instance)?;let mut parameters=i.synth().unwrap();let preset=i.preset().cloned();
                parameters.change(field,value)?;
                if Some(parameters)!=i.synth(){
                    self.edit(Edit::Synth{track,instance,instrument:Instrument::Synth{parameters,preset},field:Some(field)})?;
                }
            }
            Request::FactoryPreset{track,instance,index}=>self.apply_preset(track,instance,SynthPreset::factory(index)?)?,
            Request::LoadSynthPreset{track,instance,path}=>{
                self.synth_instrument(track,instance)?;let preset=storage::load_synth_preset(&path)?;self.apply_preset(track,instance,preset)?;
            }
            Request::SaveSynthPreset{track,instance,path,name}=>{
                let parameters=self.synth_instrument(track,instance)?.synth().unwrap();
                let preset=SynthPreset{version:1,name,source:"User".into(),parameters};
                storage::save_synth_preset(&path,&preset)?;self.apply_preset(track,instance,preset)?;
                self.status="Preset saved. File contains synth settings; track volume and notes are retained.".into();
            }
            Request::Tempo(bpm) => self.edit(Edit::Tempo(bpm))?,
            Request::Mix { track, gain, pan } => self.edit(Edit::Mix { track, gain, pan })?,
            Request::FinishMix => self.history.finish_gesture(),
            Request::Undo | Request::Redo => {
                let redo = matches!(r, Request::Redo);
                let impact = if redo { self.history.redo_impact() } else { self.history.undo_impact() };
                let fallback = self.track_index(self.track_id)?;
                let previous_clip=self.clip_id;
                if impact == Some(Impact::Structural) { self.cache_viewport(); self.quiesce()?; }
                let changed = if redo { self.history.redo(&mut self.project)? } else { self.history.undo(&mut self.project)? };
                if changed {
                    self.dirty = true; self.revision += 1; self.repair_selection(fallback);
                    if previous_clip!=self.clip_id {self.restore_viewport();}
                    self.constrain_viewport(); self.gesture = None; self.insertion = None;
                    if impact == Some(Impact::Structural) { self.needs_compile = true; self.status = "History applied. Press Play to resume.".into(); }
                    else { self.sync_mix()?; self.status = "History applied; playback continues.".into(); }
                }
            }
            Request::Delete{track,clip,note} => {self.edit(Edit::Delete{track,clip,id:note})?;self.selected=None;}
            Request::Viewport(w,h) => {
                if !w.is_finite() || !h.is_finite() {return Err("Invalid viewport size".into());}
                if self.viewport_ready && self.viewport.width==w.clamp(1.0,4096.0) && self.viewport.height==h.clamp(1.0,2160.0) {return Ok(());}
                let center=(self.viewport.scroll_y+self.viewport.height/2.0)/self.viewport.row_height;
                self.viewport.width=w.clamp(1.0,4096.0);self.viewport.height=h.clamp(1.0,2160.0);
                if !self.viewport_ready {self.fit_notes();self.viewport_ready=true;}
                else {self.viewport.scroll_y=center*self.viewport.row_height-self.viewport.height/2.0;}
                self.constrain_viewport();
            }
            Request::Zoom(f) | Request::ZoomY(f) => {
                if !f.is_finite() || f<=0.0 {return Err("Invalid zoom".into());}
                self.gesture=None;self.insertion=None;self.input=None;
                if matches!(r,Request::Zoom(_)) {self.viewport.pixels_per_beat=(self.viewport.pixels_per_beat*f).clamp(24.0,256.0);}
                else {self.viewport.zoom_y(f);}self.constrain_viewport();
            }
            Request::Pan(beats) => {
                self.viewport.start_tick=(self.viewport.start_tick as i64+beats as i64*PPQ as i64).max(0) as u64;self.constrain_viewport();
            }
            Request::Pitch(semitones) => {self.viewport.scroll_y-=semitones as f32*self.viewport.row_height;self.constrain_viewport();}
            Request::ScrollPiano(beat,y) => {
                let start=editor::beats_to_ticks(beat)?;
                if !y.is_finite() {return Err("Invalid vertical scroll".into());}
                self.viewport.start_tick=start;self.viewport.scroll_y=y;self.constrain_viewport();
            }
            Request::FitNotes => {self.gesture=None;self.insertion=None;self.input=None;self.fit_notes();}
            Request::Notation(offset) => {
                if !(-1..=1).contains(&offset) {return Err("Middle C must be C3, C4 or C5".into());}
                self.viewport.octave_offset=offset as i8;
            }
            Request::Down{track,clip,token,viewport,x,y,right} => {
                if token==0 || token>=(1u64<<63) {return Err("Invalid gesture identity".into());}
                let length=self.project.clip(track,clip).ok_or("Clip no longer exists")?.length_ticks;
                let viewport=viewport.checked(length)?;
                self.select_clip(track,Some(clip))?;self.viewport=viewport;
                self.gesture=None;self.insertion=None;self.input=Some(InputContext{track,clip,token,viewport});
                if let Some((note,resize))=viewport.hit(&self.project,track,clip,x,y) {
                    self.selected=Some(note.id);
                    if right {self.edit(Edit::Delete{track,clip,id:note.id})?;self.selected=None;self.input=None;}
                    else {self.gesture=Some(Gesture{original:note.clone(),preview:note,x,y,resize});}
                } else if !right {self.selected=None;self.insertion=viewport.insertion(length,x,y);}
            }
            Request::Move{token,x,y} => {
                if let Some(ctx)=self.input.filter(|c|c.token==token) {
                    let length=self.project.clip(ctx.track,ctx.clip).ok_or("Clip no longer exists")?.length_ticks;
                    if let Some(g)=&mut self.gesture {g.preview=ctx.viewport.drag(g,x,y,length);}
                }
            }
            Request::Up{token,x,y} => {
                let Some(ctx)=self.input.filter(|c|c.token==token) else {return Ok(());}; self.input=None;
                let length=self.project.clip(ctx.track,ctx.clip).ok_or("Clip no longer exists")?.length_ticks;
                if let Some(g)=self.gesture.take() {
                    let note=ctx.viewport.drag(&g,x,y,length);
                    if note!=g.original {self.edit(Edit::Replace{track:ctx.track,clip:ctx.clip,note})?;}
                } else if let Some((start,pitch))=self.insertion.take() {
                    if ctx.viewport.insertion(length,x,y).is_none() {return Ok(());}
                    let id=self.project.next_id;
                    self.edit(Edit::AddNote{track:ctx.track,clip:ctx.clip,start,duration:(PPQ/2).min(length-start).max(1),pitch,velocity:100})?;
                    self.selected=Some(id);
                }
            }
            Request::CancelGesture(token) => {
                if self.input.is_some_and(|c|c.token==token) {self.input=None;self.gesture=None;self.insertion=None;}
            }
            Request::AuditionOn{token,track,instance,pitch,velocity,..} => {
                let index=self.track_index(track)?;
                if token==0 || token>=(1u64<<63) || self.live.contains_key(&token) || self.live.len()>=128 || pitch>127 || !(1..=127).contains(&velocity) || self.project.tracks[index].instrument.id!=instance {
                    return Err("Invalid/stale audition target, token or MIDI value".into());
                }
                self.compile()?;self.engine.start_device(self.silent)?;
                if self.next_live>=(1u64<<63)-1 {return Err("Live identity space exhausted".into());}
                let native_id=self.next_live;self.next_live+=1;
                self.engine.live_note_velocity(index as u32,native_id,pitch,velocity,true)?;
                self.live.insert(token,LiveNote{track,instance,pitch,native_id});
            }
            Request::AuditionOff(token) => {if let Some(note)=self.live.remove(&token) {self.release_live(note)?;}}
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
                self.repair_selection(0); self.viewports.clear(); self.input=None; self.fit_notes();
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
    let audition_epoch=Arc::new(AtomicU64::new(0));let worker_audition_epoch=audition_epoch.clone();
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
            if emergency & RELEASE != 0 {
                if let Err(e)=state.release_audition() {state.status=e;}
                state.gesture=None;state.insertion=None;state.input=None;pending=Some(Box::new(state.view()));
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
                        let canceled = (envelope.epoch != worker_epoch.load(Ordering::Acquire) && envelope.request.canceled_by_stop()) ||
                            matches!(&envelope.request, Request::AuditionOn{epoch,..}|Request::PadAuditionOn{epoch,..} if *epoch!=worker_audition_epoch.load(Ordering::Acquire));
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
                if !state.needs_compile{
                    if let Some(t)=state.project.track(state.track_id).filter(|t|t.instrument.config.drums().is_some()){
                        if let Ok(status)=state.engine.drum_status(state.track_index(t.id).unwrap() as u32){let _=responses.try_send(Response::DrumMeter{track:t.id,instance:t.instrument.id,status});}
                    }
                }
                last_meter = Instant::now();
            }
        }
        let _ = state.quiesce();
        if let Some(job) = state.export_job.take() { let _ = job.join(); }
    });
    Connection { sender, receiver, flags, epoch, audition_epoch, worker }
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
                let response=match connection.receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(response)=>response,
                    Err(mpsc::RecvTimeoutError::Timeout)=>continue, // allow cold prepare within the existing 3s deadline
                    Err(error)=>panic!("Controller disconnected: {error}"),
                };
                match response {
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

    #[test] fn clip_crud_selection_and_viewport_memory_survive_history() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        request(&mut s,Request::ScrollPiano(2.0,900.25));let viewport=s.viewport;
        request(&mut s,Request::SelectTrack(2));request(&mut s,Request::SelectTrack(1));assert_eq!(s.viewport,viewport);
        request(&mut s,Request::DuplicateClip{track:1,clip:4});let copy=s.clip_id.unwrap();assert_ne!(copy,4);assert_eq!(s.project.length_ticks,32*PPQ);
        request(&mut s,Request::MoveClip{track:1,clip:copy,destination:2,beat:2.0});assert_eq!(s.track_id,2);assert_eq!(s.clip_id,Some(copy));
        request(&mut s,Request::Undo);assert_eq!(s.track_id,1);assert_eq!(s.clip_id,Some(copy));
        request(&mut s,Request::Undo);assert!(s.project.clip(1,copy).is_none());assert_eq!(s.project.length_ticks,16*PPQ);
        request(&mut s,Request::RemoveClip{track:1,clip:4});assert_eq!(s.clip_id,None);assert!(s.view().notes.is_empty());
        request(&mut s,Request::AddClip{track:1,beat:3.0});assert!(s.clip().unwrap().notes.is_empty());
        let before=s.project.clone();let revision=s.revision;request(&mut s,Request::Notation(-1));request(&mut s,Request::ZoomY(1.25));request(&mut s,Request::FitNotes);
        assert_eq!(s.project,before);assert_eq!(s.revision,revision);
    }
    #[test] fn token_audition_releases_original_target_and_independent_same_pitch() {
        let mut s=State::new(true).unwrap();let original=s.project.clone();
        for token in [1,2] {request(&mut s,Request::AuditionOn{token,epoch:0,track:1,instance:3,pitch:60,velocity:100});}
        assert_eq!(s.live.len(),2);assert_ne!(s.live[&1].native_id,s.live[&2].native_id);
        request(&mut s,Request::AuditionOff(1));assert_eq!(s.live.len(),1);
        request(&mut s,Request::SelectTrack(2));assert!(s.live.is_empty());request(&mut s,Request::AuditionOff(2));
        assert_eq!(s.project,original);assert!(!s.history.can_undo());
        assert!(s.request(Request::AuditionOn{token:3,epoch:0,track:2,instance:3,pitch:60,velocity:100},&AtomicU32::new(0)).is_err());
        s.quiesce().unwrap();
    }
    #[test] fn stale_note_gestures_cannot_commit_into_another_clip() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(500.0,230.0));let v=s.viewport;
        let before=s.project.note_count();request(&mut s,Request::Down{track:1,clip:4,token:50,viewport:v,x:400.0,y:20.0,right:false});
        request(&mut s,Request::SelectTrack(2));request(&mut s,Request::Up{token:50,x:400.0,y:20.0});assert_eq!(s.project.note_count(),before);
        assert!(s.request(Request::ResizeClip{track:1,clip:4,beats:1.0},&AtomicU32::new(0)).is_err());assert_eq!(s.project.note_count(),before);
    }
    #[test] fn focus_release_cancels_queued_audition_without_canceling_new_play() {
        let c=spawn(true);c.audition_epoch.store(1,Ordering::Release);c.flags.fetch_or(RELEASE,Ordering::Release);
        for (id,request) in [(1,Request::AuditionOn{token:1,epoch:0,track:1,instance:3,pitch:60,velocity:100}),(2,Request::Play)] {
            c.sender.send(Envelope{request_id:id,epoch:0,request}).unwrap();
        }
        let deadline=Instant::now()+Duration::from_secs(3);let mut canceled=false;let mut playing=false;
        while Instant::now()<deadline && !(canceled && playing) {
            match c.receiver.recv_timeout(Duration::from_millis(100)).unwrap() {
                Response::Ack{request_id:1,canceled:value,error,..}=>{assert!(error.is_none());canceled=value;}
                Response::Meter(t) if t.playing==1=>playing=true,
                Response::View(v)=>assert_eq!(v.live_count,0),_=>{}
            }
        }
        c.flags.fetch_or(QUIT,Ordering::Release);c.worker.join().unwrap();assert!(canceled && playing);
    }

    #[test] fn synth_edits_preserve_playback_and_reject_stale_instances_with_queue_recovery() {
        let mut state=State::new(true).unwrap();state.compile().unwrap();state.engine.play().unwrap();state.engine.render(256).unwrap();
        request(&mut state,Request::SelectTrack(2));
        request(&mut state,Request::FactoryPreset{track:1,instance:3,index:2});
        assert_eq!(state.project.track(2).unwrap().instrument.config,Instrument::Sine);assert!(!state.needs_compile);
        state.engine.render(256).unwrap();state.poll_mix().unwrap();let meter=state.engine.telemetry().unwrap();assert_eq!(meter.playing,1);assert_eq!(meter.position,512);assert!(!state.mix_dirty);
        let before=state.project.clone();assert!(state.request(Request::SynthParameter{track:1,instance:999,field:SynthField::Attack,value:0.9},&AtomicU32::new(0)).is_err());assert_eq!(state.project,before);
        request(&mut state,Request::Undo);assert_eq!(state.project.track(1).unwrap().instrument.config,Instrument::Sine);request(&mut state,Request::Redo);
        for i in 0..12 {request(&mut state,Request::SynthParameter{track:1,instance:3,field:SynthField::Shape,value:i as f32/12.0});}
        assert!(state.pending_mix);assert!(!state.needs_compile);state.quiesce().unwrap();state.poll_mix().unwrap();assert!(!state.pending_mix);
        state.compile().unwrap();assert_eq!(state.engine.mix_status().unwrap().submitted,0);assert_eq!(state.project.track(1).unwrap().instrument.config.synth().unwrap().shape,11.0/12.0);
    }
    #[test] fn preset_files_validate_before_changing_track_and_save_snapshot() {
        let mut state=State::new(true).unwrap();let path=std::env::temp_dir().join(format!("opendaw-m34-preset-{}.json",std::process::id()));
        request(&mut state,Request::FactoryPreset{track:1,instance:3,index:3});let expected=state.project.track(1).unwrap().instrument.config.synth();
        request(&mut state,Request::SaveSynthPreset{track:1,instance:3,path:path.clone(),name:"My Triangle".into()});
        request(&mut state,Request::FactoryPreset{track:1,instance:3,index:1});request(&mut state,Request::LoadSynthPreset{track:1,instance:3,path:path.clone()});
        assert_eq!(state.project.track(1).unwrap().instrument.config.synth(),expected);
        std::fs::write(&path,b"{broken}").unwrap();let before=state.project.clone();assert!(state.request(Request::LoadSynthPreset{track:1,instance:3,path:path.clone()},&AtomicU32::new(0)).is_err());assert_eq!(state.project,before);
        std::fs::remove_file(path).unwrap();
    }

    #[test] fn drum_parameters_are_live_undoable_and_stale_pad_edits_are_rejected(){
        let mut s=State::new(true).unwrap();request(&mut s,Request::AddDrumTrack{after:1});let track=s.track_id;let instance=s.project.track(track).unwrap().instrument.id;let pad=s.project.track(track).unwrap().instrument.config.drums().unwrap().pads[0].id;
        s.engine.play().unwrap();s.engine.render(256).unwrap();let before=s.engine.telemetry().unwrap().position;
        request(&mut s,Request::PadParameter{track,instance,pad,field:PadField::Gain,value:0.5});request(&mut s,Request::FinishMix);request(&mut s,Request::TogglePad{track,instance,pad,field:PadField::Mute});
        s.engine.render(256).unwrap();s.poll_mix().unwrap();assert_eq!(s.engine.telemetry().unwrap().position,before+256);assert!(!s.needs_compile);assert!(!s.mix_dirty);
        request(&mut s,Request::Undo);assert!(!s.drum_pad(track,instance,pad).unwrap().parameters.mute);request(&mut s,Request::Undo);assert_ne!(s.drum_pad(track,instance,pad).unwrap().parameters.gain,0.5);
        let saved=s.project.clone();assert!(s.request(Request::PadParameter{track,instance:999,pad,field:PadField::Gain,value:0.4},&AtomicU32::new(0)).is_err());assert_eq!(s.project,saved);
        for value in 0..12{request(&mut s,Request::PadParameter{track,instance,pad,field:PadField::Gain,value:value as f32/12.});}assert!(s.pending_mix);s.quiesce().unwrap();s.poll_mix().unwrap();s.compile().unwrap();assert!(!s.needs_compile);
    }
    #[test] fn invalid_pad_replacement_preserves_running_document_and_does_not_fill_staging(){
        let mut s=State::new(true).unwrap();request(&mut s,Request::AddDrumTrack{after:1});let track=s.track_id;let instance=s.project.track(track).unwrap().instrument.id;let pads=&s.project.track(track).unwrap().instrument.config.drums().unwrap().pads;let pad=pads[0].id;let occupied=pads[1].midi_note;
        s.engine.play().unwrap();s.engine.render(256).unwrap();let before=s.project.clone();
        let file=std::env::temp_dir().join(format!("opendaw-m35-bad-{}.wav",std::process::id()));std::fs::write(&file,b"not a WAV").unwrap();
        for _ in 0..8{assert!(s.request(Request::ImportPad{track,instance,pad,path:file.clone()},&AtomicU32::new(0)).is_err());assert_eq!(s.staging.byte_count(),0);}
        assert!(s.request(Request::MapPad{track,instance,pad,midi:occupied as i32},&AtomicU32::new(0)).is_err());assert_eq!(s.project,before);assert_eq!(s.engine.telemetry().unwrap().playing,1);assert!(!s.needs_compile);std::fs::remove_file(file).unwrap();
    }
    #[test] fn pad_preview_uses_identity_velocity_and_focus_cancellation_keeps_score_running(){
        let mut s=State::new(true).unwrap();request(&mut s,Request::AddDrumTrack{after:1});let track=s.track_id;let instance=s.project.track(track).unwrap().instrument.id;let pad=s.project.track(track).unwrap().instrument.config.drums().unwrap().pads[0].id;
        s.engine.play().unwrap();s.engine.render(256).unwrap();request(&mut s,Request::PadAuditionOn{token:1,epoch:0,track,instance,pad,velocity:37});std::thread::sleep(Duration::from_millis(25));request(&mut s,Request::AuditionOff(1));
        assert!(s.live.is_empty());assert_eq!(s.project.note_count(),10);s.release_audition().unwrap();std::thread::sleep(Duration::from_millis(20));assert_eq!(s.engine.drum_status(s.track_index(track).unwrap() as u32).unwrap().active_voices,0);assert_eq!(s.engine.telemetry().unwrap().playing,1);s.quiesce().unwrap();
    }

}
