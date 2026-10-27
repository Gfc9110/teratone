//! Read-only MIDI reference state and viewport projection.
//!
//! References store clip IDs only. Notes always come from the current project,
//! and their positions are converted through the clips' absolute timeline starts.
use crate::{editor::{Viewport, MAX_VISIBLE}, project::{Project, PPQ}};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReferenceState { pub visible: bool, pub pinned: bool }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceRow {
    pub track: u64, pub clip: u64, pub label: String,
    pub visible: bool, pub pinned: bool, pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GhostRect { pub x: f32, pub y: f32, pub width: f32, pub height: f32, pub color: String }

pub fn rows(project: &Project, active: Option<u64>, refs: &HashMap<u64, ReferenceState>) -> Vec<ReferenceRow> {
    project.tracks.iter().flat_map(|track| track.clips.iter().map(move |clip| {
        let state = refs.get(&clip.id).copied().unwrap_or_default();
        ReferenceRow { track: track.id, clip: clip.id, label: format!("{} › {}", track.name, clip.name),
            visible: state.visible, pinned: state.pinned, active: active == Some(clip.id) }
    })).collect()
}

/// Compute ghost geometry without ever copying or mutating source notes. Caller supplies
/// the pitch transform, which allows the same references in filtered MIDI views.
pub fn scene(project: &Project, active_track: u64, active_clip: Option<u64>, viewport: &Viewport,
             refs: &HashMap<u64, ReferenceState>, y_of: impl Fn(u8)->Option<f32>) -> (Vec<GhostRect>, bool) {
    let Some(active) = active_clip.and_then(|id| project.clip(active_track, id)) else { return (Vec::new(), false); };
    let mut result = Vec::new();
    for track in &project.tracks {
        for clip in &track.clips {
            if clip.id == active.id || !refs.get(&clip.id).is_some_and(|r| r.visible) { continue; }
            for note in &clip.notes {
                let Some(y) = y_of(note.pitch) else { continue; };
                let absolute = clip.start_tick as i128 + note.start as i128;
                let local = absolute - active.start_tick as i128;
                let x = ((local - viewport.start_tick as i128) as f64 / PPQ as f64 * viewport.pixels_per_beat as f64) as f32;
                let width = note.duration as f32 / PPQ as f32 * viewport.pixels_per_beat;
                if x + width <= 0.0 || x >= viewport.width || y + viewport.row_height <= 0.0 || y >= viewport.height { continue; }
                if result.len() == MAX_VISIBLE { return (result, true); }
                result.push(GhostRect { x, y: y+1.0, width: (width-1.0).max(2.0), height: viewport.row_height-2.0, color: track.color.clone() });
            }
        }
    }
    (result, false)
}

pub fn prune(project: &Project, refs: &mut HashMap<u64, ReferenceState>) {
    let valid: HashSet<_> = project.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)).collect();
    refs.retain(|id, _| valid.contains(id));
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn ghost_uses_absolute_clip_positions_and_does_not_mutate_source() {
        let mut p=Project::demo(); p.tracks[1].clips[0].start_tick=2*PPQ;
        let original=p.clone(); let mut refs=HashMap::new(); refs.insert(6,ReferenceState{visible:true,pinned:true});
        let v=Viewport{scroll_y:(127-48) as f32*16.0-20.0,width:700.0,height:120.0,..Default::default()};
        let (ghosts,capped)=scene(&p,1,Some(4),&v,&refs,|pitch|Some(v.y(pitch)));
        assert!(!capped);assert_eq!(ghosts.len(),2);assert!((ghosts[0].x-128.0).abs()<0.001);assert_eq!(p,original);
        let (none,_)=scene(&p,2,Some(6),&v,&refs,|pitch|Some(v.y(pitch)));assert!(none.is_empty());
    }
    #[test] fn references_are_pruned_after_source_deletion() {
        let mut p=Project::demo();let mut refs=HashMap::new();refs.insert(6,ReferenceState{visible:true,pinned:true});
        p.tracks.remove(1);prune(&p,&mut refs);assert!(refs.is_empty());
    }
}
