//! Per-project UI state stored under the user's settings directory. The project
//! itself and its musical undo history remain untouched.
use std::{collections::HashSet, fs, io::Read, path::{Path,PathBuf}};
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
use crate::{editor::Viewport, project::{Project,PPQ}};

const VERSION:u32=1;
const LIMIT:u64=512*1024;
const MAX_REFERENCES:usize=4096;

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(default)]
pub struct StoredViewport {
    pub start_tick:u64, pub pixels_per_beat:f32, pub scroll_y:f32,
    pub row_height:f32, pub octave_offset:i8,
}
impl Default for StoredViewport {
    fn default()->Self {Self::from_viewport(Viewport::default())}
}
impl StoredViewport {
    pub fn from_viewport(v:Viewport)->Self {Self {start_tick:v.start_tick,pixels_per_beat:v.pixels_per_beat,scroll_y:v.scroll_y,row_height:v.row_height,octave_offset:v.octave_offset}}
    pub fn apply(&self,base:Viewport)->Viewport {Viewport{start_tick:self.start_tick,pixels_per_beat:self.pixels_per_beat,scroll_y:self.scroll_y,row_height:self.row_height,octave_offset:self.octave_offset,..base}}
    fn sanitize(&mut self,length:u64) {
        self.start_tick=self.start_tick.min(length);
        self.pixels_per_beat=if self.pixels_per_beat.is_finite(){self.pixels_per_beat.clamp(24.0,256.0)}else{64.0};
        self.row_height=if self.row_height.is_finite(){self.row_height.clamp(12.0,32.0)}else{16.0};
        self.scroll_y=if self.scroll_y.is_finite(){self.scroll_y.clamp(0.0,128.0*self.row_height)}else{0.0};
        self.octave_offset=self.octave_offset.clamp(-1,1);
    }
}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct StoredReference { pub clip_id:u64, pub visible:bool, pub pinned:bool }
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(default)]
pub struct ProjectViewState {
    pub version:u32,
    pub track_id:u64, pub clip_id:Option<u64>,
    pub arrangement_track:u64, pub arrangement_clip:Option<u64>,
    pub follow_selection:bool, pub references:Vec<StoredReference>,
    pub viewport:Option<StoredViewport>, pub midi_view_mode:i32,
    pub snap_index:i32, pub scale_mode:i32, pub scale_root:i32,
    pub arrangement_start_beat:f32, pub arrangement_scroll_y:f32, pub arrangement_zoom:f32, pub navigation_linked:bool,
}
impl Default for ProjectViewState {
    fn default()->Self {Self {version:VERSION,track_id:0,clip_id:None,arrangement_track:0,arrangement_clip:None,
        follow_selection:true,references:Vec::new(),viewport:None,midi_view_mode:0,snap_index:2,scale_mode:0,scale_root:0,arrangement_start_beat:0.0,arrangement_scroll_y:0.0,arrangement_zoom:48.0,navigation_linked:false}}
}
fn path_hash(path:&Path)->Result<String,String> {
    let path=path.canonicalize().map_err(|e|format!("Cannot resolve project location: {e}"))?;
    let mut hash=Sha256::new();
    #[cfg(unix)] {use std::os::unix::ffi::OsStrExt;hash.update(path.as_os_str().as_bytes());}
    #[cfg(windows)] {use std::os::windows::ffi::OsStrExt;for unit in path.as_os_str().encode_wide(){hash.update(unit.to_le_bytes());}}
    #[cfg(not(any(unix,windows)))] {hash.update(path.to_string_lossy().as_bytes());}
    Ok(format!("{:x}",hash.finalize()))
}
pub fn state_path(base:&Path,project_path:&Path)->Result<PathBuf,String> {Ok(base.join(format!("{}.json",path_hash(project_path)?)))}
impl ProjectViewState {
    /// Clamp scalar values and drop any identities absent from this project.
    pub fn sanitize(mut self,project:&Project)->Result<Self,String> {
        if self.version!=VERSION {return Err(format!("Unsupported project view version {}",self.version));}
        if project.track(self.track_id).is_none() {self.track_id=project.tracks.first().map_or(0,|t|t.id);self.clip_id=None;}
        if self.clip_id.is_some_and(|id|project.clip(self.track_id,id).is_none()) {self.clip_id=None;}
        if project.track(self.arrangement_track).is_none() {self.arrangement_track=self.track_id;self.arrangement_clip=self.clip_id;}
        if self.arrangement_clip.is_some_and(|id|project.clip(self.arrangement_track,id).is_none()) {self.arrangement_clip=None;}
        if self.follow_selection {self.arrangement_track=self.track_id;self.arrangement_clip=self.clip_id;}
        let ids:HashSet<_>=project.tracks.iter().flat_map(|t|t.clips.iter().map(|c|c.id)).collect();let mut seen=HashSet::new();
        self.references.retain(|r|ids.contains(&r.clip_id) && seen.insert(r.clip_id) && (r.visible || r.pinned));
        self.references.truncate(MAX_REFERENCES);
        if !(0..=2).contains(&self.midi_view_mode) || (self.midi_view_mode==2 && project.track(self.track_id).and_then(|t|t.instrument.config.drums()).is_none()) {self.midi_view_mode=0;}
        if !(0..=3).contains(&self.snap_index) {self.snap_index=2;}
        if !(0..=2).contains(&self.scale_mode) {self.scale_mode=0;}
        if !(0..=11).contains(&self.scale_root) {self.scale_root=0;}
        self.arrangement_start_beat=if self.arrangement_start_beat.is_finite(){self.arrangement_start_beat.clamp(0.0,project.length_ticks as f32 / PPQ as f32)}else{0.0};
        self.arrangement_scroll_y=if self.arrangement_scroll_y.is_finite(){self.arrangement_scroll_y.clamp(-10000.0,0.0)}else{0.0};
        self.arrangement_zoom=if self.arrangement_zoom.is_finite(){self.arrangement_zoom.clamp(12.0,192.0)}else{48.0};
        let length=self.clip_id.and_then(|id|project.clip(self.track_id,id)).map_or(project.length_ticks,|c|c.length_ticks);
        if let Some(v)=&mut self.viewport {v.sanitize(length);}
        Ok(self)
    }
    pub fn load(base:&Path,project_path:&Path,project:&Project)->Result<Option<Self>,String> {
        let path=state_path(base,project_path)?;
        let file=match fs::File::open(path) {Ok(f)=>f,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(format!("Cannot read project view: {e}"))};
        let mut data=Vec::new();file.take(LIMIT+1).read_to_end(&mut data).map_err(|e|e.to_string())?;
        if data.len() as u64>LIMIT {return Err("Project view settings are too large".into());}
        let state:Self=serde_json::from_slice(&data).map_err(|e|format!("Cannot read project view: {e}"))?;
        Ok(Some(state.sanitize(project)?))
    }
    pub fn save(&self,base:&Path,project_path:&Path)->Result<(),String> {
        if self.version!=VERSION || self.references.len()>MAX_REFERENCES || !self.arrangement_start_beat.is_finite() || !self.arrangement_scroll_y.is_finite() || !self.arrangement_zoom.is_finite()
            || self.viewport.as_ref().is_some_and(|v|!v.pixels_per_beat.is_finite() || !v.scroll_y.is_finite() || !v.row_height.is_finite()) {
            return Err("Invalid project view state".into());
        }
        let bytes=serde_json::to_vec(self).map_err(|e|e.to_string())?;
        if bytes.len() as u64>LIMIT {return Err("Project view settings are too large".into());}
        fs::create_dir_all(base).map_err(|e|e.to_string())?;
        crate::storage::atomic_write(&state_path(base,project_path)?,&bytes)
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn roundtrip_is_outside_project_and_discards_invalid_references() {
        let dir=std::env::temp_dir().join(format!("opendaw-project-view-{}",std::process::id()));
        let project_path=dir.join("song.opendaw");let base=dir.join("settings");fs::create_dir_all(&project_path).unwrap();
        let p=Project::demo();let original=p.clone();let state=ProjectViewState{track_id:1,clip_id:Some(4),arrangement_track:2,arrangement_clip:Some(6),follow_selection:false,
            references:vec![StoredReference{clip_id:6,visible:true,pinned:true},StoredReference{clip_id:999,visible:true,pinned:false}],viewport:Some(StoredViewport{start_tick:100,pixels_per_beat:100.0,scroll_y:77.5,row_height:18.0,octave_offset:0}),midi_view_mode:1,snap_index:3,scale_mode:1,scale_root:5,arrangement_start_beat:2.0,arrangement_scroll_y:-120.0,arrangement_zoom:96.0,navigation_linked:true,..Default::default()};
        state.save(&base,&project_path).unwrap();let loaded=ProjectViewState::load(&base,&project_path,&p).unwrap().unwrap();
        assert_eq!(loaded.references.len(),1);assert_eq!(loaded.references[0].clip_id,6);assert_eq!(loaded.viewport,state.viewport);assert_eq!(loaded.arrangement_scroll_y,-120.0);assert_eq!(loaded.arrangement_zoom,96.0);assert!(loaded.navigation_linked);assert_eq!(p,original);
        assert!(!project_path.join("view.json").exists());assert!(state_path(&base,&project_path).unwrap().starts_with(&base));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn rejects_unknown_versions_and_sanitizes_missing_identity() {
        let mut s=ProjectViewState{version:2,..Default::default()};assert!(s.clone().sanitize(&Project::demo()).is_err());s.version=1;
        s.track_id=999;s.clip_id=Some(999);s.references=vec![StoredReference{clip_id:999,visible:true,pinned:true}];s.snap_index=999;s.midi_view_mode=2;
        let s=s.sanitize(&Project::demo()).unwrap();assert_eq!(s.track_id,1);assert_eq!(s.clip_id,None);assert!(s.references.is_empty());assert_eq!(s.snap_index,2);assert_eq!(s.midi_view_mode,0);
    }
    #[test] fn follow_selection_keeps_arrangement_and_editor_synchronized() {
        let s=ProjectViewState{track_id:1,clip_id:Some(4),arrangement_track:2,arrangement_clip:Some(6),follow_selection:true,..Default::default()}.sanitize(&Project::demo()).unwrap();
        assert_eq!((s.arrangement_track,s.arrangement_clip),(1,Some(4)));
    }
    #[test] fn oversized_view_file_is_rejected_without_touching_project() {
        let dir=std::env::temp_dir().join(format!("opendaw-project-view-limit-{}",std::process::id()));
        let project_path=dir.join("song.opendaw");let base=dir.join("settings");fs::create_dir_all(&project_path).unwrap();fs::create_dir_all(&base).unwrap();
        fs::write(state_path(&base,&project_path).unwrap(),vec![b' ';LIMIT as usize+1]).unwrap();
        let p=Project::demo();let before=p.clone();assert!(ProjectViewState::load(&base,&project_path,&p).is_err());assert_eq!(p,before);
        fs::remove_dir_all(dir).unwrap();
    }
}
