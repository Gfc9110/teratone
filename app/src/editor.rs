use crate::project::{MidiClip, Note, Project, PPQ};
pub const SNAP: u64 = PPQ / 4;
pub const MAX_VISIBLE: usize = 3000;
pub const KEYBOARD_WIDTH: f32 = 76.0;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub start_tick: u64, pub pixels_per_beat: f32, pub scroll_y: f32, pub row_height: f32,
    pub width: f32, pub height: f32, pub octave_offset: i8,
}
impl Default for Viewport {
    fn default() -> Self { Self { start_tick: 0, pixels_per_beat: 64.0, scroll_y: 54.0*16.0, row_height: 16.0, width: 900.0, height: 480.0, octave_offset: 0 } }
}
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow { pub pitch: u8, pub y: f32, pub height: f32, pub black: bool, pub c: bool, pub in_scale: bool, pub label: String }
#[derive(Clone, Debug, PartialEq)]
pub struct NoteRect { pub id: u64, pub x: f32, pub y: f32, pub width: f32, pub height: f32, pub selected: bool, pub label: String }
#[derive(Clone, Debug)]
pub struct Gesture { pub original: Note, pub x: f32, pub y: f32, pub resize: bool, pub preview: Note }
pub fn label(pitch: u8) -> String { label_with_offset(pitch, 0) }
pub fn label_with_offset(pitch: u8, offset: i8) -> String {
    let name = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][pitch as usize % 12];
    format!("{name}{}", pitch as i32 / 12 - 1 + offset as i32)
}
pub fn beats_to_ticks(beat: f64) -> Result<u64, String> {
    if !beat.is_finite() || beat < 0.0 || beat > 432000.0 { return Err("Invalid musical position (0–24 hours at 300 BPM)".into()); }
    Ok((beat * PPQ as f64).round() as u64)
}
impl Viewport {
    pub fn checked(mut self, length: u64) -> Result<Self, String> {
        if !self.pixels_per_beat.is_finite() || !self.scroll_y.is_finite() || !self.row_height.is_finite() ||
            !self.width.is_finite() || !self.height.is_finite() || !(-1..=1).contains(&self.octave_offset) { return Err("Invalid piano viewport".into()); }
        self.pixels_per_beat = self.pixels_per_beat.clamp(24.0,256.0); self.row_height = self.row_height.clamp(12.0,32.0);
        self.width = self.width.clamp(1.0,4096.0); self.height = self.height.clamp(1.0,2160.0);
        self.scroll_y = self.scroll_y.clamp(0.0,(128.0*self.row_height-self.height).max(0.0));
        let max_start = (length as f64-self.width as f64/self.pixels_per_beat as f64*PPQ as f64).max(0.0) as u64;
        self.start_tick = self.start_tick.min(max_start); Ok(self)
    }
    pub fn x(&self, tick: u64) -> f32 { ((tick as f64 - self.start_tick as f64) / PPQ as f64 * self.pixels_per_beat as f64) as f32 }
    pub fn y(&self, pitch: u8) -> f32 { (127-pitch) as f32*self.row_height-self.scroll_y }
    pub fn pitch_at(&self, y: f32) -> Option<u8> {
        if !y.is_finite() || y<0.0 || y>=self.height { return None; }
        let row = ((y+self.scroll_y)/self.row_height).floor() as i32;
        (0..128).contains(&row).then_some((127-row) as u8)
    }
    pub fn keys(&self) -> Vec<KeyRow> {
        (0..=127u8).rev().filter_map(|pitch| {
            let y=self.y(pitch); if y+self.row_height<=0.0 || y>=self.height { return None; }
            Some(KeyRow {pitch,y,height:self.row_height,black:matches!(pitch%12,1|3|6|8|10),c:pitch%12==0,in_scale:false,label:label_with_offset(pitch,self.octave_offset)})
        }).collect()
    }
    pub fn fit(&mut self, clip: Option<&MidiClip>) {
        self.start_tick=0;
        let notes=clip.map(|c|c.notes.as_slice()).unwrap_or(&[]);
        let low=notes.iter().map(|n|n.pitch).min().unwrap_or(60); let high=notes.iter().map(|n|n.pitch).max().unwrap_or(72);
        self.row_height=(self.height/(high as f32-low as f32+3.0)).clamp(12.0,24.0);
        let center=(127.0-(high as f32+low as f32)/2.0+0.5)*self.row_height;
        self.scroll_y=(center-self.height/2.0).max(0.0);
        if let Some(c)=clip {
            let begin=c.notes.iter().map(|n|n.start).min().unwrap_or(0).saturating_sub(SNAP);
            let end=c.notes.iter().map(|n|n.start+n.duration).max().unwrap_or(c.length_ticks).max(begin+1);
            self.start_tick=begin;
            self.pixels_per_beat=(self.width/((end-begin) as f32/PPQ as f32+0.5)).clamp(24.0,256.0);
            *self=self.checked(c.length_ticks).expect("finite fitted viewport");
        }
    }
    pub fn zoom_y(&mut self, factor:f32) {
        let center=(self.scroll_y+self.height/2.0)/self.row_height;
        self.row_height=(self.row_height*factor).clamp(12.0,32.0); self.scroll_y=center*self.row_height-self.height/2.0;
    }
    /// Keep the musical position underneath the viewport center fixed while zooming.
    /// The caller clamps the result to the clip boundaries afterwards.
    pub fn zoom_x(&mut self, factor:f32) {
        self.zoom_x_at(factor,self.width/2.0);
    }
    /// Preserve the musical position under a pointer while changing scale.
    pub fn zoom_x_at(&mut self, factor:f32, x:f32) {
        if !factor.is_finite() || factor<=0.0 || !x.is_finite() {return;}
        let x=x.clamp(0.0,self.width) as f64;
        let center=self.start_tick as f64+x/self.pixels_per_beat as f64*PPQ as f64;
        self.pixels_per_beat=(self.pixels_per_beat*factor).clamp(24.0,256.0);
        self.start_tick=(center-x/self.pixels_per_beat as f64*PPQ as f64).max(0.0).round() as u64;
    }
    pub fn scene(&self, project: &Project, track: u64, clip: u64, selected: Option<u64>, preview: Option<&Note>) -> (Vec<NoteRect>, bool) {
        let selected=selected.into_iter().collect();
        self.scene_group(project,track,clip,&selected,preview.into_iter().cloned().collect::<Vec<_>>().as_slice())
    }
    pub fn scene_group(&self, project: &Project, track: u64, clip: u64, selected: &std::collections::HashSet<u64>, preview: &[Note]) -> (Vec<NoteRect>, bool) {
        let kit=project.track(track).and_then(|t|t.instrument.config.drums());
        let mut scene=Vec::new(); let Some(clip)=project.clip(track,clip) else {return(scene,false)};
        let preview:std::collections::HashMap<_,_>=preview.iter().map(|n|(n.id,n)).collect();
        for source in &clip.notes {
            let note=preview.get(&source.id).copied().unwrap_or(source);
            let x=self.x(note.start); let width=note.duration as f32/PPQ as f32*self.pixels_per_beat; let y=self.y(note.pitch);
            if x+width<=0.0 || x>=self.width || y+self.row_height<=0.0 || y>=self.height {continue;}
            if scene.len()==MAX_VISIBLE {return(scene,true);}
            scene.push(NoteRect{id:note.id,x,y:y+1.0,width:(width-1.0).max(2.0),height:self.row_height-2.0,
                selected:selected.contains(&note.id),label:kit.map_or_else(||label_with_offset(note.pitch,self.octave_offset),|k|k.note_label(note.pitch))});
        }
        (scene,false)
    }
    pub fn hit(&self,p:&Project,track:u64,clip:u64,x:f32,y:f32)->Option<(Note,bool)> {
        if !x.is_finite() || !y.is_finite() || x<0.0 || x>=self.width || y<0.0 || y>=self.height {return None;}
        let (scene,_)=self.scene(p,track,clip,None,None);
        let r=scene.iter().rev().find(|r|x>=r.x && x<r.x+r.width && y>=r.y && y<r.y+r.height)?;
        // A fixed 7 px handle covers the whole body of short notes, making
        // them impossible to move. Reserve at least half of every note for
        // moving while keeping the existing handle size for longer notes.
        let resize_width=7.0f32.min(r.width/2.0);
        Some((p.clip(track,clip)?.notes.iter().find(|n|n.id==r.id)?.clone(),x>=r.x+r.width-resize_width))
    }
    pub fn insertion(&self,length:u64,x:f32,y:f32)->Option<(u64,u8)> {
        if !x.is_finite() || x<0.0 || x>=self.width {return None;}
        let pitch=self.pitch_at(y)?; let tick=self.start_tick.checked_add((x as f64/self.pixels_per_beat as f64*PPQ as f64) as u64)?;
        let snapped=tick/SNAP*SNAP; (snapped<length).then_some((snapped,pitch))
    }
    pub fn drag(&self,gesture:&Gesture,x:f32,y:f32,length:u64)->Note {
        let mut n=gesture.original.clone(); if !x.is_finite() || !y.is_finite() {return n;}
        let delta=(((x-gesture.x)/self.pixels_per_beat*PPQ as f32)/SNAP as f32).round() as i64*SNAP as i64;
        if gesture.resize {n.duration=(n.duration as i64+delta).clamp(1,(length-n.start) as i64) as u64;}
        else {n.start=(n.start as i64+delta).clamp(0,(length-n.duration) as i64) as u64;
            n.pitch=(n.pitch as i32-((y-gesture.y)/self.row_height).round() as i32).clamp(0,127) as u8;}
        n
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn key_note_hit_and_insertion_use_one_transform() {
        let p=Project::demo();
        for row_height in [12.0,16.0,24.0,32.0] {for pitch in [0,60,127] {
            let mut v=Viewport{row_height,height:181.5,scroll_y:((127-pitch) as f32*row_height-40.25).max(0.0),..Default::default()};
            v=v.checked(p.length_ticks).unwrap(); let y=v.y(pitch); let mid=(y+row_height/2.0).clamp(0.1,v.height-0.1);
            assert_eq!(v.pitch_at(mid),Some(pitch));
            let key=v.keys().into_iter().find(|k|k.pitch==pitch).unwrap(); assert_eq!(key.y,y);
            let mut q=p.clone(); q.tracks[0].clips[0].notes[0].pitch=pitch;
            let rect=v.scene(&q,1,4,Some(10),None).0.into_iter().find(|r|r.id==10).unwrap();
            assert!((key.y+key.height/2.0-(rect.y+rect.height/2.0)).abs()<0.001);
            assert_eq!(v.insertion(p.length_ticks,21.0,mid).unwrap().1,pitch);
        }}
    }
    #[test] fn notation_only_changes_labels_and_invalid_values_are_rejected() {
        assert_eq!(label(60),"C4"); assert_eq!(label_with_offset(60,-1),"C3"); assert_eq!(label_with_offset(60,1),"C5");
        let v=Viewport::default(); assert!(Viewport{scroll_y:f32::NAN,..v}.checked(PPQ).is_err());
        assert!(beats_to_ticks(f64::INFINITY).is_err()); assert!(beats_to_ticks(-1.0).is_err());
        assert!(v.insertion(PPQ,-1.0,10.0).is_none()); assert!(v.pitch_at(-1.0).is_none());
    }
    #[test] fn hit_cull_snap_drag_and_centered_zoom() {
        let p=Project::demo(); let mut v=Viewport{scroll_y:54.0*16.0,..Default::default()};
        let (scene,capped)=v.scene(&p,1,4,None,None); assert!(!capped); assert_eq!(scene.len(),8);
        let r=&scene[0]; let (note,resize)=v.hit(&p,1,4,r.x+4.0,r.y+4.0).unwrap(); assert!(!resize);
        let g=Gesture{original:note.clone(),preview:note,x:4.0,y:4.0,resize:false};
        let n=v.drag(&g,68.0,-12.0,p.length_ticks); assert_eq!(n.start,PPQ); assert_eq!(n.pitch,61);
        let center=(v.scroll_y+v.height/2.0)/v.row_height;v.zoom_y(1.5);assert!(((v.scroll_y+v.height/2.0)/v.row_height-center).abs()<0.001);
        v.start_tick=12*PPQ;assert!(v.scene(&p,1,4,None,None).0.is_empty());
    }
    #[test] fn horizontal_zoom_preserves_center_until_clip_boundary() {
        let mut v=Viewport{start_tick:4*PPQ,width:400.0,pixels_per_beat:64.0,..Default::default()};
        let center=v.start_tick as f64+v.width as f64/(2.0*v.pixels_per_beat as f64)*PPQ as f64;
        v.zoom_x(1.25);
        let after=v.start_tick as f64+v.width as f64/(2.0*v.pixels_per_beat as f64)*PPQ as f64;
        assert!((center-after).abs()<=1.0);
        v.zoom_x(0.8);
        assert!((v.start_tick as i64-4*PPQ as i64).abs()<=1);
    }
    #[test] fn zoom_at_pointer_preserves_tick_under_pointer() {
        let mut v=Viewport{start_tick:4*PPQ,width:400.0,pixels_per_beat:64.0,..Default::default()};
        let x=73.0;let before=v.start_tick as f64+x as f64/v.pixels_per_beat as f64*PPQ as f64;
        v.zoom_x_at(1.5,x);let after=v.start_tick as f64+x as f64/v.pixels_per_beat as f64*PPQ as f64;
        assert!((before-after).abs()<=1.0);
    }
    #[test] fn short_notes_can_be_moved_as_well_as_resized() {
        let mut p=Project::demo();p.tracks[0].clips[0].notes[0].duration=SNAP;
        let v=Viewport{pixels_per_beat:24.0,scroll_y:54.0*16.0,..Default::default()};
        let rect=v.scene(&p,1,4,None,None).0.into_iter().find(|n|n.id==10).unwrap();
        assert!(rect.width<7.0);
        assert!(!v.hit(&p,1,4,rect.x+rect.width*0.25,rect.y+rect.height/2.0).unwrap().1);
        assert!(v.hit(&p,1,4,rect.x+rect.width*0.75,rect.y+rect.height/2.0).unwrap().1);
    }
}
