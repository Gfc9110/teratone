//! Pitch-row layouts used by the piano roll. These are view-only: the source
//! MIDI notes and instrument mappings are never filtered or rewritten.
use std::collections::{HashMap, HashSet};
use crate::{drums::DrumKit, editor::{self, Gesture, KeyRow, NoteRect, Viewport, MAX_VISIBLE}, project::{MidiClip, Note, Project, PPQ}};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode { #[default] Piano, Used, Drum }
impl ViewMode {
    pub fn from_index(value: i32) -> Option<Self> { match value { 0=>Some(Self::Piano), 1=>Some(Self::Used), 2=>Some(Self::Drum), _=>None } }
    pub fn index(self) -> i32 { match self { Self::Piano=>0, Self::Used=>1, Self::Drum=>2 } }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scale { pub mode: u8, pub root: u8 }
impl Scale {
    pub fn checked(mode:i32, root:i32) -> Option<Self> { (0..=2).contains(&mode).then_some(())?; (0..=11).contains(&root).then_some(())?; Some(Self{mode:mode as u8,root:root as u8}) }
    pub fn contains(self, pitch:u8) -> bool {
        let relative=(pitch+12-self.root)%12;
        match self.mode { 1=>matches!(relative,0|2|4|5|7|9|11), 2=>matches!(relative,0|2|3|5|7|8|10), _=>false }
    }
}

/// Pitches in visual order, highest row first. A snapshot is retained during
/// each drag so changing note pitches cannot move the hit targets mid-gesture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PitchLayout { pub mode: ViewMode, pitches: Vec<u8>, rows: [i16;128] }
impl PitchLayout {
    pub fn for_view(mode:ViewMode, clip:Option<&MidiClip>, kit:Option<&DrumKit>) -> Self {
        let mode=if mode==ViewMode::Drum && kit.is_none() {ViewMode::Piano} else {mode};
        let mut present=[false;128];
        match mode {
            ViewMode::Piano => present.fill(true),
            ViewMode::Used | ViewMode::Drum => {
                if let Some(clip)=clip {let mut count=0;for n in &clip.notes {if !present[n.pitch as usize] {present[n.pitch as usize]=true;count+=1;}if count==128 {break;}}}
                if mode==ViewMode::Drum {if let Some(kit)=kit {for pad in &kit.pads {present[pad.midi_note as usize]=true;}}}
                // An empty clip remains editable without inventing notes or pads.
                if !present.iter().any(|p|*p) {present.fill(true);}
            }
        }
        let pitches:Vec<_>=(0..=127u8).rev().filter(|p|present[*p as usize]).collect();
        let mut rows=[-1;128];for (row,pitch) in pitches.iter().enumerate(){rows[*pitch as usize]=row as i16;}
        Self{mode,pitches,rows}
    }
    pub fn row_count(&self) -> usize {self.pitches.len()}
    /// In compact views, include the pitches of visible read-only references
    /// without copying or changing their source notes.
    pub fn with_extra_pitches(mut self, pitches:impl IntoIterator<Item=u8>) -> Self {
        if self.mode==ViewMode::Piano {return self;}
        let mut present=[false;128];for &pitch in &self.pitches {present[pitch as usize]=true;}
        let mut count=self.pitches.len();for pitch in pitches {
            if !present[pitch as usize] {present[pitch as usize]=true;count+=1;}
            if count==128 {break;}
        }
        self.pitches=(0..=127u8).rev().filter(|p|present[*p as usize]).collect();
        self.rows=[-1;128];for (row,pitch) in self.pitches.iter().enumerate(){self.rows[*pitch as usize]=row as i16;}
        self
    }
    pub fn clamp_scroll(&self, viewport:&mut Viewport) {
        viewport.scroll_y=viewport.scroll_y.clamp(0.0,(self.row_count() as f32*viewport.row_height-viewport.height).max(0.0));
    }
    /// Fit pitch rows without assuming a chromatic layout.
    pub fn fit_vertical(&self,viewport:&mut Viewport,clip:Option<&MidiClip>) {
        let mut rows=clip.into_iter().flat_map(|c|c.notes.iter()).filter_map(|n|self.index(n.pitch));
        let Some(first)=rows.next() else {viewport.scroll_y=0.0;self.clamp_scroll(viewport);return;};
        let (low,high)=rows.fold((first,first),|(lo,hi),row|(lo.min(row),hi.max(row)));
        viewport.row_height=(viewport.height/(high as f32-low as f32+3.0)).clamp(12.0,24.0);
        viewport.scroll_y=((low as f32+high as f32+1.0)*viewport.row_height/2.0-viewport.height/2.0).max(0.0);
        self.clamp_scroll(viewport);
    }
    fn index(&self,pitch:u8) -> Option<usize> {let row=self.rows[pitch as usize];(row>=0).then_some(row as usize)}
    pub fn y(&self,viewport:&Viewport,pitch:u8)->Option<f32> {Some(self.index(pitch)? as f32*viewport.row_height-viewport.scroll_y)}
    pub fn pitch_at(&self,viewport:&Viewport,y:f32)->Option<u8> {
        if !y.is_finite() || y<0.0 || y>=viewport.height {return None;}
        self.pitches.get(((y+viewport.scroll_y)/viewport.row_height).floor() as usize).copied()
    }
    pub fn keys(&self,v:&Viewport,kit:Option<&DrumKit>,scale:Scale)->Vec<KeyRow> {
        self.pitches.iter().enumerate().filter_map(|(index,&pitch)| {
            let y=index as f32*v.row_height-v.scroll_y;
            if y+v.row_height<=0.0 || y>=v.height {return None;}
            let drum=kit.is_some();
            Some(KeyRow{pitch,y,height:v.row_height,black:!drum && matches!(pitch%12,1|3|6|8|10),c:!drum && pitch%12==0,
                in_scale:!drum && scale.contains(pitch),label:kit.map_or_else(||editor::label_with_offset(pitch,v.octave_offset),|k|k.note_label(pitch))})
        }).collect()
    }
    pub fn scene(&self,v:&Viewport,p:&Project,track:u64,clip:u64,selected:&HashSet<u64>,preview:&[Note])->(Vec<NoteRect>,bool) {
        let kit=p.track(track).and_then(|t|t.instrument.config.drums());
        let Some(clip)=p.clip(track,clip) else {return(Vec::new(),false)};
        let preview:HashMap<_,_>=preview.iter().map(|n|(n.id,n)).collect(); let mut scene=Vec::new();
        for source in &clip.notes {
            let n=preview.get(&source.id).copied().unwrap_or(source); let Some(y)=self.y(v,n.pitch) else {continue;};
            let x=v.x(n.start);let width=n.duration as f32/PPQ as f32*v.pixels_per_beat;
            if x+width<=0.0 || x>=v.width || y+v.row_height<=0.0 || y>=v.height {continue;}
            if scene.len()==MAX_VISIBLE {return(scene,true);}
            scene.push(NoteRect{id:n.id,x,y:y+1.0,width:(width-1.0).max(2.0),height:v.row_height-2.0,selected:selected.contains(&n.id),
                label:kit.map_or_else(||editor::label_with_offset(n.pitch,v.octave_offset),|k|k.note_label(n.pitch))});
        } (scene,false)
    }
    pub fn hit(&self,v:&Viewport,p:&Project,track:u64,clip:u64,x:f32,y:f32)->Option<(Note,bool)> {
        if !x.is_finite() || !y.is_finite() || x<0.0 || x>=v.width || y<0.0 || y>=v.height {return None;}
        let (scene,_)=self.scene(v,p,track,clip,&HashSet::new(),&[]);
        let rect=scene.iter().rev().find(|r|x>=r.x && x<r.x+r.width && y>=r.y && y<r.y+r.height)?;
        let resize_width=7.0f32.min(rect.width/2.0);
        Some((p.clip(track,clip)?.notes.iter().find(|n|n.id==rect.id)?.clone(),x>=rect.x+rect.width-resize_width))
    }
    pub fn insertion(&self,v:&Viewport,length:u64,x:f32,y:f32)->Option<(u64,u8)> {
        self.insertion_snap(v,length,x,y,editor::SNAP)
    }
    pub fn insertion_snap(&self,v:&Viewport,length:u64,x:f32,y:f32,snap:u64)->Option<(u64,u8)> {
        if !x.is_finite() || x<0.0 || x>=v.width {return None;}
        let pitch=self.pitch_at(v,y)?;let tick=v.start_tick.checked_add((x as f64/v.pixels_per_beat as f64*PPQ as f64) as u64)?;
        let snap=snap.max(1);let snapped=tick/snap*snap;(snapped<length).then_some((snapped,pitch))
    }
    pub fn drag(&self,v:&Viewport,g:&Gesture,x:f32,y:f32,length:u64)->Note {
        self.drag_snap(v,g,x,y,length,editor::SNAP)
    }
    pub fn drag_snap(&self,v:&Viewport,g:&Gesture,x:f32,y:f32,length:u64,snap:u64)->Note {
        let mut n=g.original.clone();if !x.is_finite() || !y.is_finite(){return n;}
        let snap=snap.clamp(1,i64::MAX as u64) as i64;
        let delta=((((x-g.x)/v.pixels_per_beat*PPQ as f32)/snap as f32).round() as i64).saturating_mul(snap);
        if g.resize {n.duration=(n.duration as i64).saturating_add(delta).clamp(1,(length-n.start) as i64) as u64;}
        else {n.start=(n.start as i64).saturating_add(delta).clamp(0,(length-n.duration) as i64) as u64;}
        if !g.resize {
            if let Some(index)=self.index(g.original.pitch) {
                let row=(index as i32+((y-g.y)/v.row_height).round() as i32).clamp(0,self.pitches.len() as i32-1);
                n.pitch=self.pitches[row as usize];
            }
        } n
    }
    pub fn drag_group(&self,v:&Viewport,g:&Gesture,notes:&[Note],x:f32,y:f32,length:u64)->Vec<Note> {
        self.drag_group_snap(v,g,notes,x,y,length,editor::SNAP)
    }
    pub fn drag_group_snap(&self,v:&Viewport,g:&Gesture,notes:&[Note],x:f32,y:f32,length:u64,snap:u64)->Vec<Note> {
        let anchor=self.drag_snap(v,g,x,y,length,snap);
        let min_tick=notes.iter().map(|n|-(n.start as i64)).max().unwrap_or(0);
        let max_tick=notes.iter().map(|n|(length-n.start-n.duration) as i64).min().unwrap_or(0);
        let dx=(anchor.start as i64-g.original.start as i64).clamp(min_tick,max_tick);
        // Preserve musical intervals, including in a compressed row layout.
        // A vertical step that would move a group member outside the visible
        // mapping is refused until the user chooses a suitable row or Piano.
        let min_pitch=notes.iter().map(|n|-(n.pitch as i32)).max().unwrap_or(0);
        let max_pitch=notes.iter().map(|n|127-n.pitch as i32).min().unwrap_or(0);
        let mut dy=(anchor.pitch as i32-g.original.pitch as i32).clamp(min_pitch,max_pitch);
        if notes.iter().any(|n|self.index((n.pitch as i32+dy) as u8).is_none()) {dy=0;}
        notes.iter().cloned().map(|mut n| {n.start=(n.start as i64+dx) as u64;n.pitch=(n.pitch as i32+dy) as u8;n}).collect()
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn used_rows_are_non_destructive_and_render_hit_insert_share_coordinates() {
        let p=Project::demo();let clip=p.clip(1,4).unwrap();let before=clip.clone();let layout=PitchLayout::for_view(ViewMode::Used,Some(clip),None);
        assert!(layout.row_count()<128);let mut v=Viewport{scroll_y:0.0,..Default::default()};layout.clamp_scroll(&mut v);
        let scene=layout.scene(&v,&p,1,4,&HashSet::new(),&[]).0;assert_eq!(scene.len(),clip.notes.len());
        for rect in scene {let n=clip.notes.iter().find(|n|n.id==rect.id).unwrap();assert_eq!(layout.pitch_at(&v,rect.y+rect.height/2.0),Some(n.pitch));assert_eq!(layout.hit(&v,&p,1,4,rect.x+1.0,rect.y+rect.height/2.0).unwrap().0.id,n.id);assert_eq!(layout.insertion(&v,clip.length_ticks,rect.x+1.0,rect.y+rect.height/2.0).unwrap().1,n.pitch);}
        assert_eq!(p.clip(1,4).unwrap(),&before);
    }
    #[test] fn mapped_drum_rows_and_scale_use_real_pitches() {
        let mut id=0;let kit=DrumKit::factory(||{id+=1;Ok(id)}).unwrap();let layout=PitchLayout::for_view(ViewMode::Drum,None,Some(&kit));
        assert_eq!(layout.row_count(),kit.pads.len());let v=Viewport{scroll_y:0.0,..Default::default()};
        for pad in &kit.pads {let y=layout.y(&v,pad.midi_note).unwrap();assert_eq!(layout.pitch_at(&v,y+1.0),Some(pad.midi_note));assert!(layout.keys(&v,Some(&kit),Scale::default()).iter().any(|k|k.pitch==pad.midi_note && k.label==kit.note_label(pad.midi_note)));}
        let major=Scale::checked(1,0).unwrap();assert!(major.contains(60) && major.contains(64) && !major.contains(61));
        let minor=Scale::checked(2,9).unwrap();assert!(minor.contains(69) && minor.contains(72) && !minor.contains(73));
        assert!(Scale::checked(3,0).is_none());
    }
    #[test] fn piano_mode_on_drum_track_keeps_existing_pad_labels() {
        let mut id=0;let kit=DrumKit::factory(||{id+=1;Ok(id)}).unwrap();
        let layout=PitchLayout::for_view(ViewMode::Piano,None,Some(&kit));
        let pitch=kit.pads[0].midi_note;let v=Viewport{scroll_y:(127-pitch) as f32*16.0,height:128.0,..Default::default()};
        let key=layout.keys(&v,Some(&kit),Scale::default()).into_iter().find(|k|k.pitch==pitch).unwrap();
        assert_eq!(key.label,kit.note_label(pitch));assert!(!key.black && !key.c);
        let mut project=Project::demo();project.tracks[0].instrument.config=crate::project::Instrument::DrumKit{kit:kit.clone()};
        project.tracks[0].clips[0].notes[0].pitch=pitch;
        let note=layout.scene(&v,&project,1,4,&HashSet::new(),&[]).0.into_iter().find(|n|n.id==10).unwrap();
        assert_eq!(note.label,kit.note_label(pitch));
    }
    #[test] fn snap_is_independent_from_pitch_layout_and_drag_uses_visible_rows() {
        let p=Project::demo();let clip=p.clip(1,4).unwrap();let layout=PitchLayout::for_view(ViewMode::Used,Some(clip),None);
        let v=Viewport{scroll_y:0.0,pixels_per_beat:64.0,..Default::default()};let n=clip.notes.iter().max_by_key(|n|n.pitch).unwrap().clone();
        let y=layout.y(&v,n.pitch).unwrap()+5.0;
        assert_eq!(layout.insertion_snap(&v,clip.length_ticks,10.0,y,PPQ/8).unwrap().0,PPQ/8);
        assert_eq!(layout.insertion_snap(&v,clip.length_ticks,10.0,y,PPQ/4).unwrap().0,0);
        let g=Gesture{original:n.clone(),preview:n,x:0.0,y,resize:false};
        let moved=layout.drag_snap(&v,&g,10.0,y+v.row_height,clip.length_ticks,PPQ/8);
        assert_eq!(moved.start,g.original.start+PPQ/8);
        assert_eq!(layout.index(moved.pitch).unwrap(),layout.index(g.original.pitch).unwrap()+1);
    }
    #[test] fn grouped_notes_keep_intervals_in_compressed_view() {
        let p=Project::demo();let clip=p.clip(1,4).unwrap();let layout=PitchLayout::for_view(ViewMode::Used,Some(clip),None);
        let v=Viewport{scroll_y:0.0,..Default::default()};let mut notes=clip.notes.clone();notes.sort_by_key(|n|std::cmp::Reverse(n.pitch));notes.dedup_by_key(|n|n.pitch);notes.truncate(2);
        let n=notes[0].clone();let g=Gesture{original:n.clone(),preview:n,x:0.0,y:0.0,resize:false};
        let moved=layout.drag_group_snap(&v,&g,&notes,64.0,v.row_height,clip.length_ticks,PPQ/4);
        assert_eq!(notes[0].pitch as i32-notes[1].pitch as i32,moved[0].pitch as i32-moved[1].pitch as i32);
    }
    #[test] fn reference_rows_include_ghost_pitch_without_changing_active_clip() {
        let p=Project::demo();let clip=p.clip(1,4).unwrap();let before=clip.clone();
        let layout=PitchLayout::for_view(ViewMode::Used,Some(clip),None).with_extra_pitches([43,48]);
        let v=Viewport{scroll_y:0.0,..Default::default()};
        assert_eq!(layout.pitch_at(&v,layout.y(&v,43).unwrap()+1.0),Some(43));
        assert_eq!(layout.pitch_at(&v,layout.y(&v,48).unwrap()+1.0),Some(48));
        assert_eq!(p.clip(1,4).unwrap(),&before);
    }
}
