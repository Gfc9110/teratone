use crate::{arrangement::{self, ArrangementViewport, TrackRow}, bridge::{Engine, Telemetry}, editor::{self, Gesture, KeyRow, NoteRect, Viewport}, project::{Edit, History, Impact, Instrument, Project, PPQ, MAX_TRACKS}, storage};
use crate::drums::{DrumKit,DrumPad,PadField};
use crate::synth::{SynthParameters,SynthPreset,SynthField};
use crate::project_library::{self, ProjectLibrary, RecentProject};
use std::{collections::{HashMap, HashSet, VecDeque}, path::PathBuf, sync::{Arc, atomic::{AtomicU32, AtomicU64, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
pub const PANIC: u32 = 1; pub const STOP: u32 = 2; pub const QUIT: u32 = 4; pub const RELEASE: u32 = 8;
#[derive(Debug)]
pub enum ProjectTarget { New { name: String, parent: PathBuf }, Existing(PathBuf) }
#[derive(Debug, Clone, Copy)]
pub enum UnsavedChanges { Reject, Save, Discard }
#[derive(Debug)]
pub enum Request {
    Play, Pause, ToggleLoop, Seek(f64), SelectTrack(u64), SelectClip { track: u64, clip: u64 },
    ActivateEditorClip { track: u64, clip: u64 }, FollowSelection(bool),
    ToggleReference { track: u64, clip: u64 }, ToggleReferencePin { track: u64, clip: u64 }, HideReferences,
    DeleteContext { track:u64, clip:u64 },
    SetNavigationLinked(bool),
    SetVelocityVisible(bool), SetPianoVisible(bool),
    AddTrack { after: u64 }, DuplicateTrack(u64), RemoveTrack(u64), MoveTrack { track: u64, direction: i32 },
    ReorderTrack { track: u64, index: i32 },
    SetMute { track: u64, value: bool }, SetSolo { track: u64, value: bool },
    ToggleMute(u64), ToggleSolo(u64),
    SetGain { track: u64, value: f32 }, SetPan { track: u64, value: f32 },
    ArrangementViewport(f32, f32, f32), ArrangementScroll(f32),
    ArrangementViewportForSession(u64,f32,f32,f32), ArrangementScrollForSession(u64,f32), SeekProject(f64),
    RenameTrack { track: u64, name: String }, ColorTrack { track: u64, color: String },
    AddClip { track: u64, beat: f64 }, DuplicateClip { track: u64, clip: u64 }, RemoveClip { track: u64, clip: u64 },
    MoveClip { track: u64, clip: u64, destination: u64, beat: f64 }, ResizeClip { track: u64, clip: u64, beats: f64 },
    RenameClip { track: u64, clip: u64, name: String }, Velocity { track: u64, clip: u64, note: u64, value: i32 }, SelectVelocityNote { track: u64, clip: u64, note: u64 },
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
    Viewport(f32, f32), ViewportForSession(u64,f32,f32), Zoom(f32), ZoomAt(f32,f32), ZoomY(f32), Pan(i32), Pitch(i32), ScrollPiano(f64, f32), ScrollPianoForSession(u64,f64,f32), FitNotes, RestoreView, Notation(i32),
    SetMidiViewMode(i32), SetScale(i32,i32), SetSnap(i32),
    Down { track: u64, clip: u64, token: u64, viewport: Viewport, x: f32, y: f32, right: bool, extend: bool },
    Move { token: u64, x: f32, y: f32 }, Up { token: u64, x: f32, y: f32 }, CancelGesture(u64),
    AuditionOn { token: u64, epoch: u64, track: u64, instance: u64, pitch: u8, velocity: u8 }, AuditionOff(u64),
    Save(PathBuf), SaveAs(PathBuf), Open(PathBuf), Export(PathBuf),
    SwitchProject { target: ProjectTarget, unsaved: UnsavedChanges },
    Import { track: u64, path: PathBuf, root_note: u8, one_shot: bool }, UseSine(u64),
}
#[derive(Clone)]
pub struct ClipView { pub id: u64, pub name: String }
#[derive(Clone, PartialEq)]
pub struct TrackInspectorView {
    pub id:u64, pub index:usize, pub name:String, pub color:String, pub gain:f32, pub pan:f32,
    pub mute:bool, pub solo:bool, pub audible:bool, pub instrument:String, pub is_synth:bool, pub is_drums:bool,
    pub sample_path:String, pub sample_root:u8, pub sample_one_shot:bool,
}
#[derive(Clone)]
pub struct ViewState {
    pub project_path: Option<PathBuf>, pub project_session: u64, pub recent_projects: Vec<RecentProject>, pub default_folder: PathBuf,
    pub name: String, pub status: String, pub tracks: Vec<TrackRow>, pub track_id: u64, pub track_index: usize,
    pub clips: Vec<ClipView>, pub clip_index: Option<usize>, pub clip_start: u64, pub clip_id: u64, pub clip_length: u64, pub clip_name: String, pub track_name: String, pub track_color: String,
    pub bpm: f64, pub gain: f32, pub pan: f32, pub dirty: bool, pub can_undo: bool, pub can_redo: bool,
    pub mute: bool, pub solo: bool, pub audible: bool, pub project_beats: f32, pub arrangement_limited: bool,
    pub drum_kit:Option<DrumKit>,pub selected_pad:u64,
    pub synth:Option<SynthParameters>, pub synth_preset:Option<SynthPreset>,
    pub instrument: String, pub sample_path: String, pub root_note: u8, pub one_shot: bool, pub is_sampler: bool,
    pub revision: u64, pub mix_label: String, pub instance_id: u64, pub selected_note: u64, pub velocity: i32, pub keys: Vec<KeyRow>, pub live_count: usize,
    pub notes: Vec<NoteRect>, pub velocity_bars: Vec<crate::velocity_lane::VelocityBar>, pub viewport: Viewport, pub export_busy: bool, pub selected_label: String,
    pub reference_rows: Vec<crate::midi_references::ReferenceRow>, pub ghosts: Vec<crate::midi_references::GhostRect>, pub follow_selection: bool,
    pub midi_view_mode: i32, pub scale_mode: i32, pub scale_root: i32, pub piano_row_count: usize, pub snap_index: i32,
    pub inspector_track: TrackInspectorView,
    pub navigation_linked:bool, pub linked_start:f32, pub linked_scale:f32, pub arrangement_scroll_y:f32, pub project_view_loaded:bool,
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
#[derive(Clone)]
struct InputContext { track: u64, clip: u64, token: u64, viewport: Viewport, layout:crate::midi_view::PitchLayout, snap:u64 }
struct LiveNote { track: u64, instance: u64, pitch: u8, native_id: u64 }
struct State {
    project_path: Option<PathBuf>, project_session: u64, library: ProjectLibrary,
    project: Project, history: History, engine: Engine,staging:storage::DrumStaging,selected_pad:Option<u64>, viewport: Viewport, track_id: u64, clip_id: Option<u64>,
    selected: Option<u64>, selected_notes: HashSet<u64>, gesture: Option<Gesture>, group_original: Vec<crate::project::Note>, group_preview: Vec<crate::project::Note>, insertion: Option<(u64, u8)>,
    context_note:Option<(u64,u64,u64)>,
    live: HashMap<u64, LiveNote>, next_live: u64,
    input: Option<InputContext>, viewports: HashMap<(u64,crate::midi_view::ViewMode), Viewport>, fit_restore: HashMap<(u64,crate::midi_view::ViewMode), Viewport>, viewport_ready: bool,
    dirty: bool, needs_compile: bool, looping: bool, silent: bool, status: String,
    export_job: Option<JoinHandle<Result<(), String>>>,
    arrangement: ArrangementViewport, revision: u64, mix_dirty: bool, pending_mix: bool,
    arrangement_scroll_y:f32,
    arrangement_track: u64, arrangement_clip: Option<u64>, follow_selection: bool,
    references: HashMap<u64, crate::midi_references::ReferenceState>,
    navigation_linked:bool,
    view_state_write_allowed:bool,
    velocity_visible:bool,
    piano_visible:bool,
    project_view_loaded:bool,
    midi_view_mode: crate::midi_view::ViewMode, scale: crate::midi_view::Scale, snap_index:i32,
    submitted_mix: u64, applied_mix: u64,
}
impl State {
    fn new(silent: bool) -> Result<Self, String> {
        Ok(Self { project_path: None, project_session: 0, library: ProjectLibrary::default(), project: Project::demo(), history: History::default(), engine: Engine::new(48000, 256)?,staging:storage::DrumStaging::default(),selected_pad:None, viewport: Viewport::default(),
            track_id: 1, clip_id: Some(4), selected: None, selected_notes: HashSet::new(), gesture: None, group_original: Vec::new(), group_preview: Vec::new(), insertion: None, context_note:None,
            live: HashMap::new(), next_live: 1, input: None, viewports: HashMap::new(), fit_restore: HashMap::new(), viewport_ready: false, dirty: false, needs_compile: true, looping: false, silent,
            status: if silent { "SILENT CLOCK mode; no physical audio output" } else { "Ready. Select a track or add an oscillator." }.into(), export_job: None,
            arrangement: ArrangementViewport::default(), arrangement_scroll_y:0.0,arrangement_track: 1, arrangement_clip: Some(4), follow_selection: true, references: HashMap::new(), navigation_linked:false,view_state_write_allowed:true,velocity_visible:false,piano_visible:true,project_view_loaded:false,
            midi_view_mode:crate::midi_view::ViewMode::Piano,scale:crate::midi_view::Scale::default(),snap_index:2,
            revision: 0, mix_dirty: false, pending_mix: false, submitted_mix: 0, applied_mix: 0 })
    }
    fn track_index(&self, id: u64) -> Result<usize, String> {
        self.project.tracks.iter().position(|t| t.id == id).ok_or_else(|| "Track no longer exists".into())
    }
    fn remember_project(&mut self) {
        if let Some(path) = &self.project_path {
            if let Err(error) = self.library.remember(path, &self.project.name) {
                self.status.push_str(&format!(" · Project is safe, but recent projects could not be saved: {error}"));
            }
        }
    }
    fn save_project_view(&self) -> Result<(),String> {
        if !self.view_state_write_allowed {return Ok(());}
        let (Some(path),Some(base))=(self.project_path.as_ref(),self.library.view_state_dir()) else {return Ok(());};
        let state=crate::project_view_state::ProjectViewState {
            track_id:self.track_id,clip_id:self.clip_id,arrangement_track:self.arrangement_track,arrangement_clip:self.arrangement_clip,follow_selection:self.follow_selection,
            references:self.references.iter().map(|(&clip_id,r)|crate::project_view_state::StoredReference{clip_id,visible:r.visible,pinned:r.pinned}).collect(),
            viewport:Some(crate::project_view_state::StoredViewport::from_viewport(self.viewport)),midi_view_mode:self.midi_view_mode.index(),snap_index:self.snap_index,
            scale_mode:self.scale.mode as i32,scale_root:self.scale.root as i32,arrangement_start_beat:self.arrangement.start,arrangement_scroll_y:self.arrangement_scroll_y,
            arrangement_zoom:self.arrangement.pixels_per_beat,navigation_linked:self.navigation_linked,
            ..Default::default()
        };
        state.save(&base,path)
    }
    fn restore_project_view(&mut self,path:&std::path::Path) -> Result<(),String> {
        let Some(base)=self.library.view_state_dir() else {return Ok(());};
        let Some(saved)=crate::project_view_state::ProjectViewState::load(&base,path,&self.project)? else {return Ok(());};
        self.project_view_loaded=true;
        self.track_id=saved.track_id;self.clip_id=saved.clip_id;self.arrangement_track=saved.arrangement_track;self.arrangement_clip=saved.arrangement_clip;
        self.follow_selection=saved.follow_selection;self.references=saved.references.into_iter().map(|r|(r.clip_id,crate::midi_references::ReferenceState{visible:r.visible,pinned:r.pinned})).collect();
        self.midi_view_mode=crate::midi_view::ViewMode::from_index(saved.midi_view_mode).unwrap_or_default();
        self.snap_index=saved.snap_index;self.scale=crate::midi_view::Scale::checked(saved.scale_mode,saved.scale_root).unwrap_or_default();
        self.arrangement.start=saved.arrangement_start_beat;
        self.arrangement_scroll_y=saved.arrangement_scroll_y;
        self.arrangement.pixels_per_beat=saved.arrangement_zoom;self.navigation_linked=saved.navigation_linked;
        if let Some(v)=saved.viewport {self.viewport=v.apply(self.viewport);self.viewport_ready=true;}
        self.repair_selection(0);self.constrain_viewport();if self.navigation_linked {self.sync_arrangement_from_piano();}Ok(())
    }
    fn save_project(&mut self, path: PathBuf) -> Result<(), String> {
        if path.as_os_str().is_empty() { return Err("Choose a project folder first".into()); }
        let project = storage::save(&path, &self.project)?;
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let changed_path=self.project_path.as_ref().is_some_and(|old|old!=&path);
        let old_view_error=if changed_path {self.save_project_view().err()} else {None};
        self.project = project; self.project_path = Some(path);
        self.dirty = false; self.history.finish_gesture(); self.revision += 1;
        self.status = format!("Saved {} (format v2)", self.project_path.as_ref().unwrap().display());
        if !self.view_state_write_allowed && !changed_path {self.status.push_str(" · Existing view preferences remain preserved and unreadable.");}
        if changed_path {
            // Do not overwrite a newer or unreadable sidecar even at a reused path.
            self.view_state_write_allowed=self.library.view_state_dir().and_then(|base|self.project_path.as_ref().map(|p|crate::project_view_state::ProjectViewState::load(&base,p,&self.project))).is_none_or(|result|result.is_ok());
            if self.view_state_write_allowed {if let Err(e)=self.save_project_view(){self.status.push_str(&format!(" · View preferences not saved: {e}"));}}
            else {self.status.push_str(" · Existing view preferences are unreadable and were preserved.");}
        }
        if let Some(e)=old_view_error {self.status.push_str(&format!(" · Previous view preferences not saved: {e}"));}
        self.remember_project(); Ok(())
    }
    fn switch_project(&mut self, target: ProjectTarget, unsaved: UnsavedChanges) -> Result<(), String> {
        if self.export_job.is_some() { return Err("Wait for the export to finish before switching projects".into()); }
        if self.dirty && matches!(unsaved, UnsavedChanges::Reject) {
            return Err("Save current edits or choose Discard before switching projects.".into());
        }
        // Validate and prepare before saving or replacing any current session state.
        let (mut candidate, path, version, create) = match target {
            ProjectTarget::New { name, parent } => {
                let path = project_library::new_project_path(&parent, &name)?;
                if path.exists() { return Err("A project with this name already exists in that folder. Choose another name or folder.".into()); }
                let project = Project::empty(name.trim().into()); project.validate()?;
                (project, path, 2, true)
            }
            ProjectTarget::Existing(path) => {
                let path = path.canonicalize().map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
                let (project, version) = storage::load_versioned(&path)?; (project, path, version, false)
            }
        };
        let mut engine = Engine::new(48000, 256)?; engine.load(&candidate)?;
        if self.dirty && matches!(unsaved, UnsavedChanges::Save) {
            self.save_project(self.project_path.clone().ok_or("Save the current project to a folder before continuing")?)?;
            if !create && self.project_path.as_ref() == Some(&path) {
                candidate = self.project.clone(); engine.load(&candidate)?;
            }
        }
        let candidate = if create {
            std::fs::create_dir_all(path.parent().ok_or("Invalid project folder")?).map_err(|e| e.to_string())?;
            // create_dir fails if another process claimed the name; never overwrite it.
            std::fs::create_dir(&path).map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
            storage::save(&path, &candidate)?
        } else { candidate };
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let previous_view_error=self.save_project_view().err();
        self.quiesce()?; self.engine = engine; self.project = candidate;
        self.project_path = Some(path.clone());
        self.project_session += 1; self.history = History::default(); self.staging = storage::DrumStaging::default();
        self.needs_compile = false; self.dirty = false; self.looping = false;
        self.track_id = self.project.tracks[0].id; self.clip_id = None; self.selected = None; self.selected_notes.clear(); self.selected_pad = None;
        self.arrangement_track = self.track_id; self.arrangement_clip = None; self.follow_selection = true; self.references.clear();
        self.midi_view_mode=crate::midi_view::ViewMode::Piano;self.scale=crate::midi_view::Scale::default();self.snap_index=2;self.navigation_linked=false;
        self.arrangement.start=0.0;self.arrangement_scroll_y=0.0;self.view_state_write_allowed=true;self.project_view_loaded=false;
        self.revision += 1; self.mix_dirty = false; self.pending_mix = false; self.submitted_mix = 0; self.applied_mix = 0;
        self.repair_selection(0); self.arrangement_clip=self.clip_id; self.viewports.clear(); self.fit_restore.clear(); self.fit_notes(); self.constrain_viewport();
        self.status = if create { format!("Created {}", path.display()) }
            else if version == 1 { "Opened v1 project. Save upgrades to v2 and retains recovery/pre-m3-v1.json.".into() }
            else { format!("Opened {}", path.display()) };
        if !create {
            if let Err(error)=self.restore_project_view(&path) {
                self.view_state_write_allowed=false;
                self.status.push_str(&format!(" · View preferences could not be read and were preserved: {error}"));
            }
        } else if let Some(base)=self.library.view_state_dir() {
            if crate::project_view_state::ProjectViewState::load(&base,&path,&self.project).is_err() {
                self.view_state_write_allowed=false;self.status.push_str(" · Existing view preferences are unreadable and were preserved.");
            }
        }
        if let Some(error)=previous_view_error {self.status.push_str(&format!(" · Previous view preferences not saved: {error}"));}
        self.remember_project(); Ok(())
    }
    fn clip(&self) -> Option<&crate::project::MidiClip> { self.project.clip(self.track_id, self.clip_id?) }
    fn pitch_layout(&self) -> crate::midi_view::PitchLayout {
        let kit=self.project.track(self.track_id).and_then(|t|t.instrument.config.drums());
        let extra=self.project.tracks.iter().flat_map(|t|t.clips.iter()).filter(|c|Some(c.id)!=self.clip_id && self.references.get(&c.id).is_some_and(|r|r.visible)).flat_map(|c|c.notes.iter().map(|n|n.pitch));
        crate::midi_view::PitchLayout::for_view(self.midi_view_mode,self.clip(),kit).with_extra_pitches(extra)
    }
    fn snap_ticks(&self)->u64 {match self.snap_index {0=>1,1=>PPQ,2=>PPQ/4,3=>PPQ/8,_=>PPQ/4}}
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
            let key=(id,self.midi_view_mode);
            if !self.viewports.contains_key(&key) && self.viewports.len()>=4096 { if let Some(old)=self.viewports.keys().next().copied() {self.viewports.remove(&old);} }
            self.viewports.insert(key,self.viewport);
        }
    }
    fn prune_viewport_caches(&mut self) {
        let ids:HashSet<_>=self.project.tracks.iter().flat_map(|t|t.clips.iter().map(|c|c.id)).collect();
        self.viewports.retain(|(id,_),_|ids.contains(id));self.fit_restore.retain(|(id,_),_|ids.contains(id));
    }
    #[cfg(test)]
    fn group_drag(viewport:Viewport, gesture:&Gesture, notes:&[crate::project::Note], x:f32, y:f32, length:u64) -> Vec<crate::project::Note> {
        let anchor=viewport.drag(gesture,x,y,length);
        let min_tick=notes.iter().map(|n|-(n.start as i64)).max().unwrap_or(0);
        let max_tick=notes.iter().map(|n|(length-n.start-n.duration) as i64).min().unwrap_or(0);
        let dx=(anchor.start as i64-gesture.original.start as i64).clamp(min_tick,max_tick);
        let min_pitch=notes.iter().map(|n|-(n.pitch as i32)).max().unwrap_or(0);
        let max_pitch=notes.iter().map(|n|127-n.pitch as i32).min().unwrap_or(0);
        let dy=(anchor.pitch as i32-gesture.original.pitch as i32).clamp(min_pitch,max_pitch);
        notes.iter().cloned().map(|mut n|{n.start=(n.start as i64+dx) as u64;n.pitch=(n.pitch as i32+dy) as u8;n}).collect()
    }
    fn fit_notes(&mut self) {
        let mut v=self.viewport; v.fit(self.clip());
        if self.clip().is_none_or(|c|c.notes.is_empty()) {
            if let Some(kit)=self.project.track(self.track_id).and_then(|t|t.instrument.config.drums()){
                let low=kit.pads.iter().map(|p|p.midi_note).min().unwrap_or(35);let high=kit.pads.iter().map(|p|p.midi_note).max().unwrap_or(75);
                v.row_height=(v.height/(high as f32-low as f32+3.0)).clamp(12.0,24.0);v.scroll_y=((127.0-(high as f32+low as f32)/2.0+0.5)*v.row_height-v.height/2.0).max(0.0);
            }
        }
        let layout=self.pitch_layout();
        if layout.mode!=crate::midi_view::ViewMode::Piano {
            layout.fit_vertical(&mut v,self.clip());
        }
        self.viewport=v;
    }
    fn constrain_viewport(&mut self) {
        let length=self.clip().map_or(self.project.length_ticks,|c|c.length_ticks);
        self.viewport=self.viewport.checked(length).expect("validated viewport");
        self.pitch_layout().clamp_scroll(&mut self.viewport);
    }
    fn sync_arrangement_from_piano(&mut self) {
        if !self.navigation_linked {return;}
        let Some(begin)=self.clip().map(|c|c.start_tick) else {return;};
        self.viewport.pixels_per_beat=self.viewport.pixels_per_beat.clamp(24.0,192.0);
        self.constrain_viewport();
        self.arrangement.pixels_per_beat=self.viewport.pixels_per_beat;
        self.arrangement.start=(begin+self.viewport.start_tick) as f32/PPQ as f32;
    }
    fn sync_piano_from_arrangement(&mut self) {
        if !self.navigation_linked {return;}
        let Some(begin)=self.clip().map(|c|c.start_tick) else {return;};
        self.viewport.pixels_per_beat=self.arrangement.pixels_per_beat.clamp(24.0,192.0);
        let absolute=(self.arrangement.start as f64*PPQ as f64).round().max(0.0) as u64;
        self.viewport.start_tick=absolute.saturating_sub(begin);
        self.constrain_viewport();self.sync_arrangement_from_piano();
    }
    fn restore_viewport(&mut self) {
        let old=self.viewport;
        if let Some(v)=self.clip_id.and_then(|id|self.viewports.get(&(id,self.midi_view_mode))).copied() {
            self.viewport=Viewport{width:old.width,height:old.height,octave_offset:old.octave_offset,..v};
        } else {self.viewport.start_tick=0;self.fit_notes();}
        self.constrain_viewport();
    }
    fn select_clip(&mut self, track:u64, clip:Option<u64>) -> Result<(),String> {
        self.track_index(track)?;
        if clip.is_some_and(|id|self.project.clip(track,id).is_none()) {return Err("Clip no longer exists".into());}
        if self.track_id==track && self.clip_id==clip {return Ok(());}
        self.release_audition()?; self.cache_viewport(); self.history.finish_gesture();
        self.references.retain(|_, reference| reference.pinned);
        self.track_id=track; self.clip_id=clip;self.selected_pad=self.project.track(track).and_then(|t|t.instrument.config.drums()).and_then(|k|k.pads.first()).map(|p|p.id); self.selected=None; self.selected_notes.clear(); self.gesture=None; self.group_original.clear(); self.group_preview.clear(); self.insertion=None; self.input=None;self.context_note=None;
        if self.midi_view_mode==crate::midi_view::ViewMode::Drum && self.project.track(track).and_then(|t|t.instrument.config.drums()).is_none() {self.midi_view_mode=crate::midi_view::ViewMode::Piano;}
        self.restore_viewport();Ok(())
    }
    fn select_arrangement_clip(&mut self, track:u64, clip:Option<u64>) -> Result<(),String> {
        self.track_index(track)?;
        if clip.is_some_and(|id|self.project.clip(track,id).is_none()) {return Err("Clip no longer exists".into());}
        self.arrangement_track=track; self.arrangement_clip=clip;
        if self.follow_selection { self.select_clip(track,clip)?; }
        Ok(())
    }
    fn repair_selection(&mut self, fallback: usize) {
        let previous_clip=self.clip_id;
        if self.project.track(self.track_id).is_none() {
            self.track_id = self.project.tracks[fallback.min(self.project.tracks.len() - 1)].id;
        }
        if let Some(id)=self.clip_id {
            if let Some(track)=self.project.tracks.iter().find(|t|t.clips.iter().any(|c|c.id==id)) {self.track_id=track.id;}
        }
        let kit=self.project.track(self.track_id).and_then(|t|t.instrument.config.drums());
        if !self.selected_pad.is_some_and(|id|kit.is_some_and(|k|k.pad(id).is_some())){self.selected_pad=kit.and_then(|k|k.pads.first()).map(|p|p.id);}
        if self.clip().is_none() { self.clip_id = self.project.track(self.track_id).and_then(|t| t.clips.first()).map(|c| c.id); }
        // A structural edit or history operation can implicitly change the editor
        // target. Give it the same reference lifetime as an explicit selection.
        if self.clip_id!=previous_clip {self.references.retain(|_,reference|reference.pinned);}
        if !self.selected.is_some_and(|id| self.clip().is_some_and(|c| c.notes.iter().any(|n| n.id == id))) { self.selected = None; }
        let valid:HashSet<_>=self.clip().map(|c|c.notes.iter().map(|n|n.id).collect()).unwrap_or_default();
        self.selected_notes.retain(|id|valid.contains(id));
        crate::midi_references::prune(&self.project, &mut self.references);
        if let Some(id)=self.arrangement_clip {
            if let Some(track)=self.project.tracks.iter().find(|t|t.clips.iter().any(|c|c.id==id)) {self.arrangement_track=track.id;}
        }
        if self.project.track(self.arrangement_track).is_none() {self.arrangement_track=self.track_id;self.arrangement_clip=self.clip_id;}
        if self.arrangement_clip.is_some_and(|id|self.project.clip(self.arrangement_track,id).is_none()) {
            self.arrangement_clip=self.project.track(self.arrangement_track).and_then(|t|t.clips.first()).map(|c|c.id);
        }
        if self.follow_selection { self.arrangement_track=self.track_id; self.arrangement_clip=self.clip_id; }
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
        let removes_clip=matches!(&edit,Edit::RemoveClip{..}|Edit::RemoveTrack{..});
        self.history.execute(&mut self.project, edit)?;
        if removes_clip {self.prune_viewport_caches();}
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
        let layout=self.pitch_layout();
        let preview = if self.gesture.is_none() {Vec::new()} else if self.group_preview.is_empty() {self.gesture.as_ref().map(|g|vec![g.preview.clone()]).unwrap_or_default()} else {self.group_preview.clone()};
        let (notes, capped) = if self.piano_visible {layout.scene(&self.viewport,&self.project, t.id, self.clip_id.unwrap_or(0), &self.selected_notes, &preview)} else {(Vec::new(),false)};
        let selected_label = self.selected.and_then(|id| self.clip()?.notes.iter().find(|n| n.id == id))
            .map(|n| format!("{} · {:.2} beats · velocity {}", t.instrument.config.drums().map_or_else(||crate::editor::label_with_offset(n.pitch,self.viewport.octave_offset),|k|k.note_label(n.pitch)), n.duration as f64 / PPQ as f64, n.velocity))
            .unwrap_or_else(|| if self.clip().is_none() { "This track has no MIDI clips.".into() }
                else { "Click empty space to insert · drag to move · drag right edge to resize".into() });
        let selected_label=if self.selected_notes.len()>1 {format!("{} notes selected · velocity: focused note",self.selected_notes.len())} else {selected_label};
        let (tracks, arrangement_limited) = arrangement::scene(&self.project, self.arrangement, self.arrangement_track, self.arrangement_clip);
        let it=self.project.track(self.arrangement_track).expect("arrangement track invariant");
        let (isample,iroot,ioneshot)=match &it.instrument.config {Instrument::Sampler{sample_path,root_note,one_shot}=>(sample_path.clone(),*root_note,*one_shot),_=>(String::new(),60,false)};
        let inspector_track=TrackInspectorView{id:it.id,index:self.track_index(it.id).unwrap(),name:it.name.clone(),color:it.color.clone(),gain:it.gain,pan:it.pan,mute:it.mute,solo:it.solo,audible:self.project.audible(it.id),instrument:it.instrument.config.label().into(),is_synth:it.instrument.config.synth().is_some(),is_drums:it.instrument.config.drums().is_some(),sample_path:isample,sample_root:iroot,sample_one_shot:ioneshot};
        let (ghosts, _) = if self.piano_visible {crate::midi_references::scene(&self.project,self.track_id,self.clip_id,&self.viewport,&self.references,|pitch|layout.y(&self.viewport,pitch))} else {(Vec::new(),false)};
        let reference_rows=crate::midi_references::rows(&self.project,self.clip_id,&self.references);
        let (sample_path, root_note, one_shot, is_sampler) = match &t.instrument.config {
            Instrument::Sine | Instrument::Synth { .. } | Instrument::DrumKit{..} => (String::new(), 60, false, false),
            Instrument::Sampler { sample_path, root_note, one_shot } => (sample_path.clone(), *root_note, *one_shot, true),
        };
        let keys=if self.piano_visible {layout.keys(&self.viewport,t.instrument.config.drums(),self.scale)} else {Vec::new()};
        ViewState { name: self.project.name.clone(), status: if capped { format!("{} | View capped at 3000 notes; zoom in.", self.status) } else { self.status.clone() },
            project_path: self.project_path.clone(), project_session: self.project_session,
            recent_projects: self.library.recent().to_vec(), default_folder: self.library.default_folder(),
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
            notes, velocity_bars: if self.piano_visible && self.velocity_visible {crate::velocity_lane::scene(&self.viewport, self.clip(), self.selected).into_iter().map(|mut b|{b.selected=self.selected_notes.contains(&b.id);b}).collect()} else {Vec::new()}, viewport: self.viewport, export_busy: self.export_job.is_some(), selected_label,
            reference_rows, ghosts, follow_selection:self.follow_selection,
            midi_view_mode:self.midi_view_mode.index(),scale_mode:self.scale.mode as i32,scale_root:self.scale.root as i32,piano_row_count:layout.row_count(),snap_index:self.snap_index, inspector_track,
            navigation_linked:self.navigation_linked,linked_start:self.arrangement.start,linked_scale:self.arrangement.pixels_per_beat,arrangement_scroll_y:self.arrangement_scroll_y,project_view_loaded:self.project_view_loaded }
    }
    fn select_track(&mut self, id: u64) -> Result<(), String> {
        self.track_index(id)?; if self.arrangement_track==id {return Ok(());}
        let clip=self.project.track(id).and_then(|t|t.clips.first()).map(|c|c.id);
        self.select_arrangement_clip(id,clip)
    }
    fn request(&mut self, r: Request, flags: &AtomicU32) -> Result<(), String> {
        // UI geometry can be queued before a project switch, then arrive after
        // its replacement is loaded. Drop such transient updates, never edits.
        let r=match r {
            Request::ArrangementViewportForSession(session,start,scale,width) => {
                if session!=self.project_session {return Ok(());} Request::ArrangementViewport(start,scale,width)
            }
            Request::ArrangementScrollForSession(session,y) => {
                if session!=self.project_session {return Ok(());} Request::ArrangementScroll(y)
            }
            Request::ViewportForSession(session,width,height) => {
                if session!=self.project_session {return Ok(());} Request::Viewport(width,height)
            }
            Request::ScrollPianoForSession(session,beat,y) => {
                if session!=self.project_session {return Ok(());} Request::ScrollPiano(beat,y)
            }
            other=>other,
        };
        let sync_piano=matches!(&r,Request::Zoom(_) | Request::ZoomAt(..) | Request::Pan(_) | Request::ScrollPiano(..) | Request::Viewport(..) | Request::FitNotes | Request::RestoreView | Request::SelectClip{..} | Request::SelectTrack(_) | Request::ActivateEditorClip{..} | Request::FollowSelection(_));
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
            Request::ArrangementViewport(start, scale, width) => {self.arrangement = ArrangementViewport::checked(start, scale, width)?;self.sync_piano_from_arrangement();},
            Request::ArrangementScroll(y) => {if !y.is_finite(){return Err("Invalid arrangement scroll".into());}self.arrangement_scroll_y=y.clamp(-10000.0,0.0);},
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
            Request::SelectClip { track, clip } => self.select_arrangement_clip(track,Some(clip))?,
            Request::ActivateEditorClip { track, clip } => {
                self.select_clip(track,Some(clip))?;
                self.arrangement_track=track;self.arrangement_clip=Some(clip);
            }
            Request::FollowSelection(follow) => {
                if follow && !self.follow_selection { self.select_clip(self.arrangement_track,self.arrangement_clip)?; }
                self.follow_selection=follow;
            }
            Request::SetNavigationLinked(linked) => {self.navigation_linked=linked;if linked {self.sync_arrangement_from_piano();}}
            Request::SetVelocityVisible(visible) => {self.velocity_visible=visible;}
            Request::SetPianoVisible(visible) => {self.piano_visible=visible;}
            Request::ToggleReference {track,clip} | Request::ToggleReferencePin {track,clip} => {
                if self.project.clip(track,clip).is_none() {return Err("Reference clip no longer exists".into());}
                let reference=self.references.entry(clip).or_default();
                if matches!(r,Request::ToggleReference{..}) {reference.visible=!reference.visible;}
                else {reference.pinned=!reference.pinned;if reference.pinned {reference.visible=true;}}
                self.gesture=None;self.group_original.clear();self.group_preview.clear();self.insertion=None;self.input=None;self.constrain_viewport();
            }
            Request::HideReferences => { for reference in self.references.values_mut() {reference.visible=false;} self.gesture=None;self.input=None;self.insertion=None;self.constrain_viewport(); }
            Request::DeleteContext{track,clip} => {
                if let Some((t,c,note))=self.context_note.take() {
                    if (t,c)==(track,clip) && self.track_id==track && self.clip_id==Some(clip) && self.project.clip(track,clip).is_some_and(|c|c.notes.iter().any(|n|n.id==note)) {
                        self.edit(Edit::Delete{track,clip,id:note})?;
                    }
                }
            }
            Request::AddClip { track, beat } => {
                let id=self.project.next_id;let start=editor::beats_to_ticks(beat)?;
                self.edit(Edit::AddClip{track,start,length:4*PPQ})?;self.select_arrangement_clip(track,Some(id))?;
            }
            Request::DuplicateClip { track,clip } => {
                let id=self.project.next_id;self.edit(Edit::DuplicateClip{track,clip})?;self.select_arrangement_clip(track,Some(id))?;
            }
            Request::RemoveClip { track,clip } => self.edit(Edit::RemoveClip{track,clip})?,
            Request::RenameClip {track,clip,name} => {
                if self.project.clip(track,clip).ok_or("Clip no longer exists")?.name!=name {self.edit(Edit::ClipName{track,clip,name})?;}
            }
            Request::MoveClip { track,clip,destination,beat } => {
                let start=editor::beats_to_ticks(beat)?;
                let old=self.project.clip(track,clip).ok_or("Clip no longer exists")?;
                if destination!=track || start!=old.start_tick {
                    self.cache_viewport();self.edit(Edit::MoveClip{track,clip,destination,start})?;self.select_arrangement_clip(destination,Some(clip))?;
                }
            }
            Request::ResizeClip {track,clip,beats} => {
                let length=editor::beats_to_ticks(beats)?;
                if self.project.clip(track,clip).ok_or("Clip no longer exists")?.length_ticks!=length {self.edit(Edit::ResizeClip{track,clip,length})?;}
            }
            Request::SelectVelocityNote{track,clip,note} => {
                // The lane is bound to the active clip. Reject stale gestures after a selection change.
                if self.track_id != track || self.clip_id != Some(clip) {return Err("Clip selection changed".into());}
                if !self.project.clip(track,clip).is_some_and(|c| c.notes.iter().any(|n| n.id == note)) {return Err("Note no longer exists".into());}
                self.selected = Some(note);self.selected_notes.clear();self.selected_notes.insert(note);
            }
            Request::Velocity{track,clip,note,value} => {
                if !(1..=127).contains(&value) {return Err("Velocity must be 1–127".into());}
                let existing = self.project.clip(track,clip).and_then(|c|c.notes.iter().find(|n|n.id==note)).ok_or("Note no longer exists")?;
                if existing.velocity != value as u8 {self.edit(Edit::Velocity{track,clip,note,value:value as u8})?;}
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
                    if impact==Some(Impact::Structural) {self.prune_viewport_caches();}
                    self.dirty = true; self.revision += 1; self.repair_selection(fallback);
                    if previous_clip!=self.clip_id {self.restore_viewport();}
                    self.constrain_viewport(); self.gesture = None; self.insertion = None;
                    if impact == Some(Impact::Structural) { self.needs_compile = true; self.status = "History applied. Press Play to resume.".into(); }
                    else { self.sync_mix()?; self.status = "History applied; playback continues.".into(); }
                }
            }
            Request::Delete{track,clip,note} => {
                if self.track_id==track && self.clip_id==Some(clip) && self.selected_notes.contains(&note) && self.selected_notes.len()>1 {
                    self.edit(Edit::RemoveMany{track,clip,ids:self.selected_notes.iter().copied().collect()})?;
                } else {self.edit(Edit::Delete{track,clip,id:note})?;}
                self.selected=None;self.selected_notes.clear();
            }
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
                if matches!(r,Request::Zoom(_)) {self.viewport.zoom_x(f);}
                else {self.viewport.zoom_y(f);}self.constrain_viewport();
            }
            Request::ZoomAt(f,x) => {
                if !f.is_finite() || f<=0.0 || !x.is_finite() || x<0.0 || x>self.viewport.width {return Err("Invalid zoom anchor".into());}
                self.gesture=None;self.insertion=None;self.input=None;
                self.viewport.zoom_x_at(f,x);self.constrain_viewport();
            }
            Request::SetMidiViewMode(index) => {
                let mode=crate::midi_view::ViewMode::from_index(index).ok_or("Invalid MIDI view")?;
                if mode==crate::midi_view::ViewMode::Drum && self.project.track(self.track_id).and_then(|t|t.instrument.config.drums()).is_none() {return Err("Drum view requires a mapped drum kit".into());}
                if self.midi_view_mode!=mode {
                    self.cache_viewport();self.midi_view_mode=mode;self.gesture=None;self.insertion=None;self.input=None;
                    let start=self.viewport.start_tick;let scale=self.viewport.pixels_per_beat;
                    if let Some(saved)=self.clip_id.and_then(|id|self.viewports.get(&(id,mode))).copied() {
                        self.viewport.row_height=saved.row_height;self.viewport.scroll_y=saved.scroll_y;
                    } else {self.fit_notes();}
                    self.viewport.start_tick=start;self.viewport.pixels_per_beat=scale;self.constrain_viewport();
                }
            }
            Request::SetScale(mode,root) => {self.scale=crate::midi_view::Scale::checked(mode,root).ok_or("Invalid scale")?;}
            Request::SetSnap(index) => {if !(0..=3).contains(&index) {return Err("Invalid snap value".into());}self.snap_index=index;}
            Request::Pan(beats) => {
                self.viewport.start_tick=(self.viewport.start_tick as i64+beats as i64*PPQ as i64).max(0) as u64;self.constrain_viewport();
            }
            Request::Pitch(semitones) => {self.viewport.scroll_y-=semitones as f32*self.viewport.row_height;self.constrain_viewport();}
            Request::ScrollPiano(beat,y) => {
                let start=editor::beats_to_ticks(beat)?;
                if !y.is_finite() {return Err("Invalid vertical scroll".into());}
                self.viewport.start_tick=start;self.viewport.scroll_y=y;self.constrain_viewport();
            }
            Request::FitNotes => {
                self.gesture=None;self.insertion=None;self.input=None;
                if let Some(id)=self.clip_id {
                    let key=(id,self.midi_view_mode);
                    if !self.fit_restore.contains_key(&key) && self.fit_restore.len()>=4096 {
                        if let Some(old)=self.fit_restore.keys().next().copied() {self.fit_restore.remove(&old);}
                    }
                    self.fit_restore.entry(key).or_insert(self.viewport);
                }
                self.fit_notes(); self.constrain_viewport();
            }
            Request::RestoreView => {
                self.gesture=None;self.insertion=None;self.input=None;
                if let Some(saved)=self.clip_id.and_then(|id|self.fit_restore.remove(&(id,self.midi_view_mode))) {
                    self.viewport=Viewport{width:self.viewport.width,height:self.viewport.height,octave_offset:self.viewport.octave_offset,..saved};
                    self.constrain_viewport();
                }
            }
            Request::Notation(offset) => {
                if !(-1..=1).contains(&offset) {return Err("Middle C must be C3, C4 or C5".into());}
                self.viewport.octave_offset=offset as i8;
            }
            Request::Down{track,clip,token,viewport,x,y,right,extend} => {
                if token==0 || token>=(1u64<<63) {return Err("Invalid gesture identity".into());}
                let length=self.project.clip(track,clip).ok_or("Clip no longer exists")?.length_ticks;
                let mut viewport=viewport.checked(length)?;
                self.select_clip(track,Some(clip))?;self.viewport=viewport;
                self.context_note=None;
                let layout=self.pitch_layout();layout.clamp_scroll(&mut viewport);self.viewport=viewport;
                let snap=self.snap_ticks();
                self.gesture=None;self.group_original.clear();self.group_preview.clear();self.insertion=None;self.input=Some(InputContext{track,clip,token,viewport,layout:layout.clone(),snap});
                if let Some((note,resize))=layout.hit(&viewport,&self.project,track,clip,x,y) {
                    if right {self.context_note=Some((track,clip,note.id));}
                    if extend && !right && self.selected_notes.remove(&note.id) {
                        self.selected=self.selected_notes.iter().next().copied();self.input=None;return Ok(());
                    }
                    if !extend && !self.selected_notes.contains(&note.id) {self.selected_notes.clear();}
                    self.selected_notes.insert(note.id);
                    self.selected=Some(note.id);
                    if right {self.input=None;}
                    else {
                        if !resize && self.selected_notes.len()>1 {self.group_original=self.clip().unwrap().notes.iter().filter(|n|self.selected_notes.contains(&n.id)).cloned().collect();}
                        self.gesture=Some(Gesture{original:note.clone(),preview:note,x,y,resize});
                    }
                } else {
                    if !extend {self.selected=None;self.selected_notes.clear();}
                    if right || extend {self.input=None;} else {self.insertion=layout.insertion_snap(&viewport,length,x,y,snap);}
                }
            }
            Request::Move{token,x,y} => {
                if let Some(ctx)=self.input.as_ref().filter(|c|c.token==token).cloned() {
                    let length=self.project.clip(ctx.track,ctx.clip).ok_or("Clip no longer exists")?.length_ticks;
                    if let Some(g)=&mut self.gesture {
                        g.preview=ctx.layout.drag_snap(&ctx.viewport,g,x,y,length,ctx.snap);
                        if !self.group_original.is_empty() {self.group_preview=ctx.layout.drag_group_snap(&ctx.viewport,g,&self.group_original,x,y,length,ctx.snap);}
                    }
                }
            }
            Request::Up{token,x,y} => {
                let Some(ctx)=self.input.as_ref().filter(|c|c.token==token).cloned() else {return Ok(());}; self.input=None;
                let length=self.project.clip(ctx.track,ctx.clip).ok_or("Clip no longer exists")?.length_ticks;
                if let Some(g)=self.gesture.take() {
                    if !self.group_original.is_empty() {
                        let notes=ctx.layout.drag_group_snap(&ctx.viewport,&g,&self.group_original,x,y,length,ctx.snap);
                        if notes!=self.group_original {self.edit(Edit::ReplaceMany{track:ctx.track,clip:ctx.clip,notes})?;}
                    } else {
                        let note=ctx.layout.drag_snap(&ctx.viewport,&g,x,y,length,ctx.snap);
                        if note!=g.original {self.edit(Edit::Replace{track:ctx.track,clip:ctx.clip,note})?;}
                    }
                    self.group_original.clear();self.group_preview.clear();
                } else if let Some((start,pitch))=self.insertion.take() {
                    if ctx.layout.insertion_snap(&ctx.viewport,length,x,y,ctx.snap).is_none() {return Ok(());}
                    let id=self.project.next_id;
                    self.edit(Edit::AddNote{track:ctx.track,clip:ctx.clip,start,duration:(PPQ/2).min(length-start).max(1),pitch,velocity:100})?;
                    self.selected=Some(id);self.selected_notes.clear();self.selected_notes.insert(id);
                }
            }
            Request::CancelGesture(token) => {
                if self.input.as_ref().is_some_and(|c|c.token==token) {self.input=None;self.gesture=None;self.group_original.clear();self.group_preview.clear();self.insertion=None;}
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
                self.save_project(path)?;
            }
            Request::SaveAs(path) => {
                if path.exists() { return Err("Choose a new project folder; Save as never replaces an existing project.".into()); }
                std::fs::create_dir_all(path.parent().ok_or("Invalid project folder")?).map_err(|e| e.to_string())?;
                std::fs::create_dir(&path).map_err(|e| e.to_string())?;
                self.save_project(path)?;
            }
            Request::Open(path) => self.switch_project(ProjectTarget::Existing(path), UnsavedChanges::Reject)?,
            Request::SwitchProject { target, unsaved } => self.switch_project(target, unsaved)?,
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
            Request::ArrangementViewportForSession(..) | Request::ArrangementScrollForSession(..) | Request::ViewportForSession(..) | Request::ScrollPianoForSession(..) => unreachable!("normalized above"),
        }
        if sync_piano {self.sync_arrangement_from_piano();}
        Ok(())
    }
}
pub fn spawn(silent: bool) -> Connection {
    spawn_with_library(silent, ProjectLibrary::default())
}
pub fn spawn_with_library(silent: bool, library: ProjectLibrary) -> Connection {
    let (sender, requests) = mpsc::sync_channel::<Envelope>(64);
    let (responses, receiver) = mpsc::sync_channel(8);
    let flags = Arc::new(AtomicU32::new(0)); let worker_flags = flags.clone();
    let epoch = Arc::new(AtomicU64::new(0)); let worker_epoch = epoch.clone();
    let audition_epoch=Arc::new(AtomicU64::new(0));let worker_audition_epoch=audition_epoch.clone();
    let worker = thread::spawn(move || {
        let mut state = match State::new(silent) { Ok(s) => s, Err(e) => { let _ = responses.send(Response::Fatal(e)); return; } };
        state.library = library;
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
        let _ = state.save_project_view();
        if let Some(job) = state.export_job.take() { let _ = job.join(); }
    });
    Connection { sender, receiver, flags, epoch, audition_epoch, worker }
}

#[cfg(test)] mod tests {
    use super::*;
    fn request(state: &mut State, r: Request) { state.request(r, &AtomicU32::new(0)).unwrap(); state.project.validate().unwrap(); }
    fn new_project(name: &str, parent: &std::path::Path, unsaved: UnsavedChanges) -> Request {
        Request::SwitchProject { target: ProjectTarget::New { name: name.into(), parent: parent.into() }, unsaved }
    }
    #[test] fn project_creation_switching_and_save_as_preserve_checkpoints() {
        let root=std::env::temp_dir().join(format!("opendaw-project-workflow-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap(); let mut s=State::new(true).unwrap();
        s.library=ProjectLibrary::load(root.join("settings/projects.json")).unwrap();
        request(&mut s,new_project("My first song",&root,UnsavedChanges::Reject));
        let first=s.project_path.clone().unwrap(); assert!(first.is_absolute()); assert_eq!(s.project.note_count(),0);
        assert_eq!(s.project.tracks.len(),1); assert_eq!(s.project.name,"My first song"); assert!(!s.dirty);
        assert_eq!(storage::load(&first).unwrap(),s.project); assert!(!s.history.can_undo());
        let saved=std::fs::read(first.join("project.json")).unwrap();
        assert!(s.request(new_project("My first song",&root,UnsavedChanges::Discard),&AtomicU32::new(0)).is_err());
        assert_eq!(std::fs::read(first.join("project.json")).unwrap(),saved);
        request(&mut s,Request::RenameTrack{track:1,name:"Keep this edit".into()});
        assert!(s.request(new_project("Second song",&root,UnsavedChanges::Reject),&AtomicU32::new(0)).is_err());
        assert!(s.dirty); assert!(!root.join("Second song.opendaw").exists());
        request(&mut s,new_project("Second song",&root,UnsavedChanges::Save));
        assert_eq!(storage::load(&first).unwrap().tracks[0].name,"Keep this edit");
        assert_eq!(s.project.name,"Second song"); assert!(!s.dirty); assert!(!s.history.can_undo());
        assert_eq!(s.library.recent().len(),2);
        request(&mut s,Request::RenameTrack{track:1,name:"Discard me".into()});
        request(&mut s,Request::SwitchProject{target:ProjectTarget::Existing(first.clone()),unsaved:UnsavedChanges::Discard});
        assert_eq!(s.project.tracks[0].name,"Keep this edit"); assert_eq!(s.library.recent()[0].path,first);
        let copy=root.join("Portable copy.opendaw"); request(&mut s,Request::SaveAs(copy.clone()));
        assert_eq!(s.project_path,Some(copy.canonicalize().unwrap())); assert_eq!(storage::load(&copy).unwrap(),s.project);
        assert!(s.request(Request::SaveAs(first.clone()),&AtomicU32::new(0)).is_err());
        assert_eq!(ProjectLibrary::load(root.join("settings/projects.json")).unwrap().recent()[0].path,s.project_path.clone().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn failed_switches_preserve_dirty_project_history_and_recent_list() {
        let root=std::env::temp_dir().join(format!("opendaw-project-failure-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap(); let mut s=State::new(true).unwrap();
        request(&mut s,new_project("Current",&root,UnsavedChanges::Reject));
        request(&mut s,Request::RenameTrack{track:1,name:"Unsaved".into()});
        let current=s.project.clone(); let recent=s.library.recent().to_vec(); let session=s.project_session;
        let invalid=root.join("Invalid.opendaw"); std::fs::create_dir(&invalid).unwrap();
        std::fs::write(invalid.join("project.json"),b"invalid").unwrap();
        for path in [invalid,root.join("Missing.opendaw")] {
            assert!(s.request(Request::SwitchProject{target:ProjectTarget::Existing(path),unsaved:UnsavedChanges::Discard},&AtomicU32::new(0)).is_err());
            assert_eq!(s.project,current); assert!(s.dirty); assert!(s.history.can_undo());
            assert_eq!(s.project_session,session); assert_eq!(s.library.recent(),recent);
        }
        // A Save-and-continue error must not discard or clear the session.
        let blocker=root.join("not-a-directory"); std::fs::write(&blocker,b"blocked").unwrap();
        s.project_path=Some(blocker.join("Current.opendaw"));
        assert!(s.request(new_project("Next",&root,UnsavedChanges::Save),&AtomicU32::new(0)).is_err());
        assert_eq!(s.project,current); assert!(s.dirty); assert!(!root.join("Next.opendaw").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn save_and_reopen_the_same_project_keeps_the_latest_edits() {
        let root=std::env::temp_dir().join(format!("opendaw-project-reopen-{}",std::process::id()));
        let mut s=State::new(true).unwrap();request(&mut s,new_project("Song",&root,UnsavedChanges::Reject));
        request(&mut s,Request::RenameTrack{track:1,name:"Latest edit".into()});
        let path=s.project_path.clone().unwrap();
        request(&mut s,Request::SwitchProject{target:ProjectTarget::Existing(path),unsaved:UnsavedChanges::Save});
        assert_eq!(s.project.tracks[0].name,"Latest edit"); assert!(!s.dirty);
        std::fs::remove_dir_all(root).unwrap();
    }
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
    #[test] fn velocity_lane_selection_and_edit_are_undoable_and_reject_stale_selection() {
        let mut state = State::new(true).unwrap();
        request(&mut state, Request::SelectVelocityNote {track: 1, clip: 4, note: 10});
        assert_eq!(state.selected, Some(10));
        request(&mut state, Request::Velocity {track: 1, clip: 4, note: 10, value: 37});
        assert_eq!(state.project.clip(1, 4).unwrap().notes[0].velocity, 37);
        request(&mut state, Request::Undo);
        assert_eq!(state.project.clip(1, 4).unwrap().notes[0].velocity, 100);
        request(&mut state, Request::Redo);
        assert_eq!(state.project.clip(1, 4).unwrap().notes[0].velocity, 37);
        assert!(state.request(Request::SelectVelocityNote {track: 2, clip: 4, note: 10}, &AtomicU32::new(0)).is_err());
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
    #[test] fn fit_restore_is_per_clip_and_does_not_edit_the_project() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        request(&mut s,Request::Zoom(1.5));request(&mut s,Request::ScrollPiano(2.0,900.25));
        let original=s.viewport;let project=s.project.clone();let revision=s.revision;
        request(&mut s,Request::FitNotes);assert_ne!(s.viewport,original);
        request(&mut s,Request::FitNotes); // a second Fit must not overwrite the original
        request(&mut s,Request::SelectTrack(2));request(&mut s,Request::SelectTrack(1));
        request(&mut s,Request::RestoreView);assert_eq!(s.viewport,original);
        assert_eq!(s.project,project);assert_eq!(s.revision,revision);
        request(&mut s,Request::RestoreView);assert_eq!(s.viewport,original);
    }
    #[test] fn editor_lock_distinguishes_timeline_selection_and_preserves_pinned_references() {
        let mut s=State::new(true).unwrap();let original=s.project.clone();
        request(&mut s,Request::ToggleReference{track:2,clip:6});
        request(&mut s,Request::ToggleReferencePin{track:2,clip:6});
        request(&mut s,Request::FollowSelection(false));
        request(&mut s,Request::SelectClip{track:2,clip:6});
        assert_eq!((s.track_id,s.clip_id),(1,Some(4)));
        assert_eq!((s.arrangement_track,s.arrangement_clip),(2,Some(6)));
        let view=s.view();assert!(view.tracks.iter().find(|t|t.id==2).unwrap().selected);assert!(!view.tracks.iter().find(|t|t.id==1).unwrap().selected);
        assert_eq!(s.project,original);assert!(!s.dirty);
        request(&mut s,Request::ActivateEditorClip{track:2,clip:6});assert_eq!((s.track_id,s.clip_id),(2,Some(6)));
        request(&mut s,Request::SelectClip{track:1,clip:4});assert_eq!((s.track_id,s.clip_id),(2,Some(6)));
        request(&mut s,Request::FollowSelection(true));assert_eq!((s.track_id,s.clip_id),(1,Some(4)));
        assert!(s.references[&6].pinned && s.references[&6].visible);
        request(&mut s,Request::HideReferences);assert!(s.references[&6].pinned && !s.references[&6].visible);
    }
    #[test] fn linked_navigation_uses_project_time_and_can_be_disabled() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(300.0,215.0));
        request(&mut s,Request::SetNavigationLinked(true));
        request(&mut s,Request::ScrollPiano(3.0,700.0));
        assert!((s.arrangement.start-3.0).abs()<0.001);
        request(&mut s,Request::ArrangementViewport(4.0,80.0,500.0));
        assert_eq!(s.viewport.start_tick,4*PPQ);assert_eq!(s.viewport.pixels_per_beat,80.0);
        request(&mut s,Request::SetNavigationLinked(false));let viewport=s.viewport;
        request(&mut s,Request::ArrangementViewport(7.0,60.0,500.0));assert_eq!(s.viewport,viewport);
    }
    #[test] fn stale_project_session_cannot_apply_view_updates() {
        let mut s=State::new(true).unwrap();
        request(&mut s,Request::ViewportForSession(0,300.0,200.0));
        request(&mut s,Request::ArrangementViewportForSession(0,2.0,90.0,800.0));
        request(&mut s,Request::ArrangementScrollForSession(0,-60.0));
        request(&mut s,Request::ScrollPianoForSession(0,1.0,700.0));
        assert_eq!(s.viewport.width,300.0);assert_eq!(s.arrangement.pixels_per_beat,90.0);assert_eq!(s.arrangement_scroll_y,-60.0);
        let before=s.viewport;let arrangement=s.arrangement;let scroll=s.arrangement_scroll_y;s.project_session=1;
        request(&mut s,Request::ViewportForSession(0,1000.0,1000.0));
        request(&mut s,Request::ArrangementViewportForSession(0,20.0,192.0,1000.0));
        request(&mut s,Request::ArrangementScrollForSession(0,-900.0));
        request(&mut s,Request::ScrollPianoForSession(0,10.0,0.0));
        assert_eq!(s.viewport,before);assert_eq!(s.arrangement.start,arrangement.start);assert_eq!(s.arrangement.pixels_per_beat,arrangement.pixels_per_beat);assert_eq!(s.arrangement_scroll_y,scroll);
        request(&mut s,Request::ArrangementScrollForSession(1,-120.0));assert_eq!(s.arrangement_scroll_y,-120.0);
    }
    #[test] fn hidden_piano_skips_scene_and_restores_view_and_notes() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        request(&mut s,Request::SetVelocityVisible(true));request(&mut s,Request::ToggleReference{track:2,clip:6});
        let before=s.viewport;let scene=s.view();assert!(!scene.notes.is_empty() && !scene.velocity_bars.is_empty());
        request(&mut s,Request::SetPianoVisible(false));let hidden=s.view();assert!(hidden.notes.is_empty() && hidden.keys.is_empty() && hidden.ghosts.is_empty() && hidden.velocity_bars.is_empty());
        request(&mut s,Request::SetPianoVisible(true));let shown=s.view();assert_eq!(s.viewport,before);assert_eq!(shown.notes.len(),scene.notes.len());assert_eq!(shown.velocity_bars.len(),scene.velocity_bars.len());
    }
    #[test] fn linked_navigation_handles_nonzero_clip_offset_and_unequal_widths() {
        let mut s=State::new(true).unwrap();s.project.length_ticks=64*PPQ;s.project.tracks[0].clips[0].start_tick=12*PPQ;
        request(&mut s,Request::Viewport(300.0,215.0));request(&mut s,Request::ArrangementViewport(0.0,80.0,900.0));
        request(&mut s,Request::SetNavigationLinked(true));assert!((s.arrangement.start-12.0).abs()<0.001);
        request(&mut s,Request::ArrangementViewport(16.0,80.0,900.0));assert_eq!(s.viewport.start_tick,4*PPQ);assert!((s.arrangement.start-16.0).abs()<0.001);
        request(&mut s,Request::ArrangementViewport(8.0,80.0,900.0));assert_eq!(s.viewport.start_tick,0);assert!((s.arrangement.start-12.0).abs()<0.001);
        request(&mut s,Request::ArrangementViewport(60.0,80.0,900.0));assert!(s.viewport.start_tick<16*PPQ);assert!((s.arrangement.start-(12.0+s.viewport.start_tick as f32/PPQ as f32)).abs()<0.001);
    }
    #[test] fn project_view_sidecar_roundtrips_save_as_and_preserves_unreadable_files() {
        let root=std::env::temp_dir().join(format!("opendaw-view-controller-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();let first=root.join("first.opendaw");std::fs::create_dir_all(&first).unwrap();storage::save(&first,&Project::demo()).unwrap();
        let second=root.join("second.opendaw");std::fs::create_dir_all(&second).unwrap();storage::save(&second,&Project::demo()).unwrap();
        let settings=root.join("settings/projects.json");let mut s=State::new(true).unwrap();s.library=ProjectLibrary::load(settings.clone()).unwrap();
        request(&mut s,Request::Open(first.clone()));request(&mut s,Request::FollowSelection(false));request(&mut s,Request::SelectTrack(2));
        request(&mut s,Request::ToggleReferencePin{track:2,clip:6});request(&mut s,Request::SetSnap(3));let before=s.project.clone();
        request(&mut s,Request::Open(second.clone()));request(&mut s,Request::Open(first.clone()));
        assert!(!s.follow_selection);assert_eq!(s.track_id,1);assert_eq!(s.arrangement_track,2);assert!(s.references[&6].pinned);assert_eq!(s.snap_index,3);assert_eq!(s.project,before);
        let copy=root.join("copy.opendaw");request(&mut s,Request::SaveAs(copy.clone()));request(&mut s,Request::Open(second));request(&mut s,Request::Open(copy.clone()));
        assert!(!s.follow_selection);assert_eq!(s.arrangement_track,2);assert!(s.references[&6].pinned);
        let base=s.library.view_state_dir().unwrap();let sidecar=crate::project_view_state::state_path(&base,&copy).unwrap();let unknown=br#"{"version":999}"#;
        std::fs::write(&sidecar,unknown).unwrap();let mut t=State::new(true).unwrap();t.library=ProjectLibrary::load(settings).unwrap();request(&mut t,Request::Open(copy));
        assert!(!t.view_state_write_allowed);assert!(t.status.contains("preserved"));t.save_project_view().unwrap();assert_eq!(std::fs::read(sidecar).unwrap(),unknown);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn changing_midi_view_preserves_horizontal_position_and_independent_vertical_history() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(300.0,215.0));
        request(&mut s,Request::Zoom(1.5));request(&mut s,Request::ScrollPiano(2.0,900.0));let piano=s.viewport;
        request(&mut s,Request::SetMidiViewMode(1));assert_eq!(s.viewport.start_tick,piano.start_tick);assert_eq!(s.viewport.pixels_per_beat,piano.pixels_per_beat);
        request(&mut s,Request::FitNotes);let fitted=s.viewport;request(&mut s,Request::SetMidiViewMode(0));
        assert_eq!(s.viewport.start_tick,fitted.start_tick);assert_eq!(s.viewport.pixels_per_beat,fitted.pixels_per_beat);
        assert!((s.viewport.scroll_y-piano.scroll_y).abs()<0.01);assert_eq!(s.viewport.row_height,piano.row_height);
        assert!(s.request(Request::SetMidiViewMode(2),&AtomicU32::new(0)).is_err());
    }
    #[test] fn inspector_and_arrangement_actions_target_selected_track_while_editor_is_locked() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::FollowSelection(false));request(&mut s,Request::SelectTrack(2));
        assert_eq!(s.view().inspector_track.id,2);assert_eq!(s.track_id,1);
        request(&mut s,Request::ToggleMute(2));assert!(s.project.track(2).unwrap().mute);assert!(!s.project.track(1).unwrap().mute);
        request(&mut s,Request::ToggleSolo(2));assert!(s.project.track(2).unwrap().solo);assert!(!s.project.track(1).unwrap().solo);
        request(&mut s,Request::DuplicateTrack(2));let duplicated=s.arrangement_track;assert_ne!(duplicated,2);assert_eq!(s.track_id,1);
        request(&mut s,Request::RemoveTrack(duplicated));assert_eq!(s.track_id,1);assert!(s.project.track(duplicated).is_none());
        request(&mut s,Request::AddTrack{after:2});let added=s.arrangement_track;assert_eq!(s.track_id,1);assert_eq!(s.track_index(added).unwrap(),s.track_index(2).unwrap()+1);
    }
    #[test] fn deleting_reference_prunes_it_and_invalid_requests_are_atomic() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::ToggleReference{track:2,clip:6});
        request(&mut s,Request::ToggleReferencePin{track:2,clip:6});
        let before=s.references.clone();
        assert!(s.request(Request::ToggleReference{track:1,clip:6},&AtomicU32::new(0)).is_err());assert_eq!(s.references,before);
        request(&mut s,Request::RemoveTrack(2));assert!(s.references.is_empty());
        request(&mut s,Request::Undo);assert!(s.references.is_empty());assert!(s.project.clip(2,6).is_some());
    }
    #[test] fn implicit_clip_fallback_clears_unpinned_references_after_delete_and_undo() {
        let mut deleted=State::new(true).unwrap();
        let fallback=deleted.project.next_id;request(&mut deleted,Request::AddClip{track:1,beat:0.0});
        let pinned=deleted.project.next_id;request(&mut deleted,Request::AddClip{track:2,beat:0.0});
        request(&mut deleted,Request::SelectClip{track:1,clip:4});
        request(&mut deleted,Request::ToggleReference{track:2,clip:6});
        request(&mut deleted,Request::ToggleReferencePin{track:2,clip:pinned});
        request(&mut deleted,Request::RemoveClip{track:1,clip:4});
        assert_eq!(deleted.clip_id,Some(fallback));assert!(!deleted.references.contains_key(&6));
        assert!(deleted.references[&pinned].pinned && deleted.references[&pinned].visible);
        request(&mut deleted,Request::Undo);
        assert!(!deleted.references.contains_key(&6));assert!(deleted.references[&pinned].pinned);

        let mut undone=State::new(true).unwrap();
        let pinned=undone.project.next_id;request(&mut undone,Request::AddClip{track:2,beat:0.0});
        let active=undone.project.next_id;request(&mut undone,Request::AddClip{track:1,beat:0.0});
        request(&mut undone,Request::ToggleReference{track:2,clip:6});
        request(&mut undone,Request::ToggleReferencePin{track:2,clip:pinned});
        assert_eq!(undone.clip_id,Some(active));request(&mut undone,Request::Undo);
        assert_eq!(undone.clip_id,Some(4));assert!(!undone.references.contains_key(&6));
        assert!(undone.references[&pinned].pinned && undone.references[&pinned].visible);
    }
    #[test] fn visible_ghost_is_read_only_even_when_clicked() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        let row=s.viewport.row_height;request(&mut s,Request::ScrollPiano(0.0,(127.0-48.0)*row-50.0));
        request(&mut s,Request::ToggleReference{track:2,clip:6});
        let source=s.project.clip(2,6).unwrap().clone();let ghost=s.view().ghosts[0].clone();let v=s.viewport;
        request(&mut s,Request::Down{track:1,clip:4,token:11,viewport:v,x:ghost.x+2.0,y:ghost.y+3.0,right:false,extend:false});
        request(&mut s,Request::Up{token:11,x:ghost.x+2.0,y:ghost.y+3.0});
        assert_eq!(s.project.clip(2,6).unwrap(),&source);
        assert_eq!(s.project.clip(1,4).unwrap().notes.len(),9);
    }
    #[test] fn context_delete_uses_original_active_note_identity_and_never_ghosts() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        let n=s.view().notes[0].clone();let v=s.viewport;
        request(&mut s,Request::Down{track:1,clip:4,token:41,viewport:v,x:n.x+2.0,y:n.y+3.0,right:true,extend:false});
        request(&mut s,Request::DeleteContext{track:1,clip:4});assert!(s.project.clip(1,4).unwrap().notes.iter().all(|n2|n2.id!=n.id));
        request(&mut s,Request::Undo);assert!(s.project.clip(1,4).unwrap().notes.iter().any(|n2|n2.id==n.id));
        let row=s.viewport.row_height;request(&mut s,Request::ScrollPiano(0.0,(127.0-48.0)*row-50.0));request(&mut s,Request::ToggleReference{track:2,clip:6});
        let ghost=s.view().ghosts[0].clone();let original=s.project.clone();let v=s.viewport;
        request(&mut s,Request::Down{track:1,clip:4,token:42,viewport:v,x:ghost.x+2.0,y:ghost.y+3.0,right:true,extend:false});
        request(&mut s,Request::DeleteContext{track:1,clip:4});assert_eq!(s.project,original);
    }
    #[test] fn right_click_selects_a_note_without_deleting_it() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        let rect=s.view().notes[0].clone();let before=s.project.clone();let revision=s.revision;let viewport=s.viewport;
        request(&mut s,Request::Down{track:1,clip:4,token:1,viewport,x:rect.x+rect.width/2.0,y:rect.y+rect.height/2.0,right:true,extend:false});
        assert_eq!(s.selected,Some(rect.id));assert!(s.input.is_none());assert_eq!(s.project,before);assert_eq!(s.revision,revision);
    }
    #[test] fn multi_selection_moves_and_deletes_atomically_with_undo() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        request(&mut s,Request::SetVelocityVisible(true));
        let v=s.viewport;let visible=s.view().notes;let a=visible[0].clone();let b=visible[1].clone();
        let hit=|r:&crate::editor::NoteRect| (r.x+r.width*0.25,r.y+r.height/2.0);
        let (ax,ay)=hit(&a);let (bx,by)=hit(&b);
        request(&mut s,Request::Down{track:1,clip:4,token:1,viewport:v,x:ax,y:ay,right:false,extend:false});
        request(&mut s,Request::Up{token:1,x:ax,y:ay});
        request(&mut s,Request::Down{track:1,clip:4,token:2,viewport:v,x:bx,y:by,right:false,extend:true});
        request(&mut s,Request::Up{token:2,x:bx,y:by});
        assert_eq!(s.selected_notes.len(),2);assert_eq!(s.view().velocity_bars.iter().filter(|b|b.selected).count(),2);
        let original=s.project.clone();
        request(&mut s,Request::Down{track:1,clip:4,token:3,viewport:v,x:ax,y:ay,right:false,extend:false});
        request(&mut s,Request::Move{token:3,x:ax+v.pixels_per_beat,y:ay-v.row_height});
        assert_eq!(s.group_preview.len(),2);
        request(&mut s,Request::Up{token:3,x:ax+v.pixels_per_beat,y:ay-v.row_height});
        for id in [a.id,b.id] {
            let old=original.clip(1,4).unwrap().notes.iter().find(|n|n.id==id).unwrap();
            let new=s.clip().unwrap().notes.iter().find(|n|n.id==id).unwrap();
            assert_eq!(new.start,old.start+PPQ);assert_eq!(new.pitch,old.pitch+1);
        }
        request(&mut s,Request::Undo);assert_eq!(s.project,original);
        request(&mut s,Request::Redo);let moved=s.project.clone();
        request(&mut s,Request::Delete{track:1,clip:4,note:a.id});assert_eq!(s.project.note_count(),moved.note_count()-2);
        request(&mut s,Request::Undo);assert_eq!(s.project,moved);
        request(&mut s,Request::Redo);assert_eq!(s.project.note_count(),moved.note_count()-2);
    }
    #[test] fn group_drag_keeps_relative_positions_at_clip_and_pitch_boundaries() {
        let v=Viewport::default();let notes=vec![
            crate::project::Note{id:1,start:PPQ,duration:PPQ,pitch:126,velocity:100},
            crate::project::Note{id:2,start:2*PPQ,duration:PPQ,pitch:127,velocity:80},
        ];
        let g=Gesture{original:notes[0].clone(),preview:notes[0].clone(),x:0.0,y:0.0,resize:false};
        let moved=State::group_drag(v,&g,&notes,10.0*v.pixels_per_beat,-4.0*v.row_height,4*PPQ);
        assert_eq!(moved[0].start,2*PPQ);assert_eq!(moved[1].start,3*PPQ);
        assert_eq!(moved[0].pitch,126);assert_eq!(moved[1].pitch,127);
        let moved=State::group_drag(v,&g,&notes,-10.0*v.pixels_per_beat,0.0,4*PPQ);
        assert_eq!(moved[0].start,0);assert_eq!(moved[1].start,PPQ);
    }
    #[test] fn changing_clip_cancels_a_pending_group_drag() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        let v=s.viewport;let visible=s.view().notes;let a=&visible[0];let b=&visible[1];
        let (ax,ay)=(a.x+a.width*0.25,a.y+a.height/2.0);let (bx,by)=(b.x+b.width*0.25,b.y+b.height/2.0);
        request(&mut s,Request::Down{track:1,clip:4,token:1,viewport:v,x:ax,y:ay,right:false,extend:false});request(&mut s,Request::Up{token:1,x:ax,y:ay});
        request(&mut s,Request::Down{track:1,clip:4,token:2,viewport:v,x:bx,y:by,right:false,extend:true});request(&mut s,Request::Up{token:2,x:bx,y:by});
        request(&mut s,Request::Down{track:1,clip:4,token:3,viewport:v,x:ax,y:ay,right:false,extend:false});
        request(&mut s,Request::Move{token:3,x:ax+v.pixels_per_beat,y:ay});let original=s.project.clone();
        request(&mut s,Request::SelectTrack(2));request(&mut s,Request::Up{token:3,x:ax+v.pixels_per_beat,y:ay});
        assert_eq!(s.project,original);assert!(s.selected_notes.is_empty());assert!(s.group_preview.is_empty());
    }
    #[test] fn deleted_clip_clears_selection_and_viewport_caches() {
        let mut s=State::new(true).unwrap();request(&mut s,Request::Viewport(550.0,215.0));
        request(&mut s,Request::FitNotes);s.viewports.insert((4,s.midi_view_mode),s.viewport);
        s.selected=Some(10);s.selected_notes.insert(10);
        request(&mut s,Request::RemoveClip{track:1,clip:4});
        assert!(!s.fit_restore.keys().any(|(id,_)|*id==4));assert!(!s.viewports.keys().any(|(id,_)|*id==4));assert!(s.selected_notes.is_empty());
        request(&mut s,Request::Undo);assert!(s.project.clip(1,4).is_some());assert!(!s.fit_restore.keys().any(|(id,_)|*id==4));
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
        let before=s.project.note_count();request(&mut s,Request::Down{track:1,clip:4,token:50,viewport:v,x:400.0,y:20.0,right:false,extend:false});
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
