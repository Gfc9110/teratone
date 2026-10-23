//! Bounded, render-neutral arrangement previews. Clip data stays in Project.
use crate::project::{Project, PPQ};

#[derive(Clone, Copy, Debug)]
pub struct ArrangementViewport { pub start: f32, pub pixels_per_beat: f32, pub width: f32 }
impl Default for ArrangementViewport {
    fn default() -> Self { Self { start: 0.0, pixels_per_beat: 48.0, width: 900.0 } }
}
impl ArrangementViewport {
    pub fn checked(start: f32, pixels_per_beat: f32, width: f32) -> Result<Self, String> {
        if !start.is_finite() || !pixels_per_beat.is_finite() || !width.is_finite() { return Err("Invalid arrangement viewport".into()); }
        Ok(Self { start: start.max(0.0), pixels_per_beat: pixels_per_beat.clamp(12.0, 192.0), width: width.clamp(1.0, 4096.0) })
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct MiniNote { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPreview { pub id: u64, pub name: String, pub start: f32, pub length: f32, pub selected: bool, pub notes: Vec<MiniNote> }
#[derive(Clone, Debug, PartialEq)]
pub struct TrackRow {
    pub id: u64, pub name: String, pub color: String, pub instrument: String,
    pub selected: bool, pub mute: bool, pub solo: bool, pub audible: bool, pub clips: Vec<ClipPreview>,
}
pub fn scene(project: &Project, viewport: ArrangementViewport, selected_track: u64, selected_clip: Option<u64>) -> (Vec<TrackRow>, bool) {
    let right = viewport.start + viewport.width / viewport.pixels_per_beat;
    let any_solo = project.tracks.iter().any(|t| t.solo);
    let mut count = 0; let mut limited = false;
    let rows = project.tracks.iter().map(|t| {
        let mut clips:Vec<ClipPreview> = t.clips.iter().filter_map(|c| {
            let start = c.start_tick as f32 / PPQ as f32; let length = c.length_ticks as f32 / PPQ as f32;
            if start + length <= viewport.start || start >= right { return None; }
            if count == 256 { limited = true; return None; } count += 1;
            let mut bins: [Option<(u8, u8)>; 32] = [None; 32];
            let top = c.notes.iter().map(|n| n.pitch).max().unwrap_or(72);
            let bottom = c.notes.iter().map(|n| n.pitch).min().unwrap_or(60);
            let span = (top - bottom).max(12) as f32;
            for n in &c.notes {
                let first = (n.start as f64 / c.length_ticks as f64 * 32.0).floor() as usize;
                let last = (((n.start + n.duration) as f64 / c.length_ticks as f64 * 32.0).ceil() as usize).min(32);
                for bin in bins.iter_mut().take(last).skip(first.min(31)) {
                    *bin = Some(bin.map_or((n.pitch, n.pitch), |(low, high)| (low.min(n.pitch), high.max(n.pitch))));
                }
            }
            let notes = bins.iter().enumerate().filter_map(|(i, bin)| bin.map(|(low, high)| MiniNote {
                x: i as f32 / 32.0, width: 1.0 / 32.0, y: (top - high) as f32 / (span + 2.0), height: (high - low + 1) as f32 / (span + 2.0),
            })).collect();
            Some(ClipPreview { id: c.id, name: c.name.clone(), start, length, selected: selected_track == t.id && selected_clip == Some(c.id), notes })
        }).collect();
        clips.sort_by_key(|c|c.selected); // the selected overlapping clip stays reachable on top
        TrackRow { id: t.id, name: t.name.clone(), color: t.color.clone(), instrument: t.instrument.config.label().into(), selected: selected_track == t.id,
            mute: t.mute, solo: t.solo, audible: !t.mute && (!any_solo || t.solo), clips }
    }).collect();
    (rows, limited)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn culling_never_removes_headers_and_previews_are_bounded() {
        let p = Project::demo(); let (rows, limited) = scene(&p, ArrangementViewport::default(), 1, Some(4));
        assert!(!limited); assert_eq!(rows.len(), 2); assert!(rows[0].clips[0].selected);
        assert!(rows.iter().flat_map(|r| &r.clips).all(|c| c.notes.len() <= 32));
        let (rows, _) = scene(&p, ArrangementViewport { start: 17.0, ..Default::default() }, 1, Some(4));
        assert_eq!(rows.len(), 2); assert!(rows.iter().all(|r| r.clips.is_empty()));
    }
    #[test] fn preview_and_model_agree_on_audibility() {
        let mut p = Project::demo(); p.tracks[0].solo = true; p.tracks[0].mute = true;
        let (rows, _) = scene(&p, Default::default(), 1, Some(4));
        for row in rows { assert_eq!(row.audible, p.audible(row.id)); }
        assert!(ArrangementViewport::checked(f32::NAN, 0.0, 50.0).is_err());
    }
}
