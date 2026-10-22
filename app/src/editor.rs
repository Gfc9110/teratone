use crate::project::{Note, Project, PPQ};
pub const ROW_HEIGHT: f32 = 16.0;
pub const SNAP: u64 = PPQ / 4;
pub const MAX_VISIBLE: usize = 3000;
#[derive(Clone, Copy, Debug)]
pub struct Viewport { pub start_tick: u64, pub pixels_per_beat: f32, pub top_note: u8, pub width: f32, pub height: f32 }
impl Default for Viewport {
    fn default() -> Self { Self { start_tick: 0, pixels_per_beat: 64.0, top_note: 84, width: 900.0, height: 480.0 } }
}
#[derive(Clone, Debug)]
pub struct NoteRect { pub id: u64, pub x: f32, pub y: f32, pub width: f32, pub height: f32, pub selected: bool, pub label: String }
#[derive(Clone, Debug)]
pub struct Gesture { pub original: Note, pub x: f32, pub y: f32, pub resize: bool, pub preview: Note }
pub fn label(pitch: u8) -> String {
    let name = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][pitch as usize % 12];
    format!("{name}{}", pitch as i32 / 12 - 1)
}
impl Viewport {
    pub fn x(&self, tick: u64) -> f32 { ((tick as f64 - self.start_tick as f64) / PPQ as f64 * self.pixels_per_beat as f64) as f32 }
    pub fn scene(&self, project: &Project, track: u64, selected: Option<u64>, preview: Option<&Note>) -> (Vec<NoteRect>, bool) {
        let mut scene = Vec::new(); let Some(track) = project.track(track) else { return (scene, false); };
        for source in &track.notes {
            let note = preview.filter(|n| n.id == source.id).unwrap_or(source);
            let x = self.x(note.start); let width = note.duration as f32 / PPQ as f32 * self.pixels_per_beat;
            let y = (self.top_note as i32 - note.pitch as i32) as f32 * ROW_HEIGHT;
            if x + width < 0.0 || x >= self.width || y + ROW_HEIGHT <= 0.0 || y >= self.height { continue; }
            if scene.len() == MAX_VISIBLE { return (scene, true); }
            scene.push(NoteRect { id: note.id, x, y: y + 1.0, width: (width - 1.0).max(2.0), height: ROW_HEIGHT - 2.0,
                selected: selected == Some(note.id), label: label(note.pitch) });
        }
        (scene, false)
    }
    pub fn hit(&self, p: &Project, track: u64, x: f32, y: f32) -> Option<(Note, bool)> {
        if !x.is_finite() || !y.is_finite() { return None; }
        let (scene, _) = self.scene(p, track, None, None);
        let rect = scene.iter().rev().find(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)?;
        let note = p.track(track)?.notes.iter().find(|n| n.id == rect.id)?.clone();
        Some((note, x >= rect.x + rect.width - 7.0))
    }
    pub fn insertion(&self, length: u64, x: f32, y: f32) -> Option<(u64, u8)> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x >= self.width || y >= self.height { return None; }
        let tick = self.start_tick + (x as f64 / self.pixels_per_beat as f64 * PPQ as f64) as u64;
        let snapped = tick / SNAP * SNAP;
        let pitch = self.top_note as i32 - (y / ROW_HEIGHT) as i32;
        if snapped >= length || !(0..=127).contains(&pitch) { None } else { Some((snapped, pitch as u8)) }
    }
    pub fn drag(&self, gesture: &Gesture, x: f32, y: f32, length: u64) -> Note {
        let mut note = gesture.original.clone();
        if !x.is_finite() || !y.is_finite() { return note; }
        let delta = (((x - gesture.x) / self.pixels_per_beat * PPQ as f32) / SNAP as f32).round() as i64 * SNAP as i64;
        if gesture.resize {
            note.duration = (note.duration as i64 + delta).clamp(1, (length - note.start) as i64) as u64;
        } else {
            note.start = (note.start as i64 + delta).clamp(0, (length - note.duration) as i64) as u64;
            note.pitch = (note.pitch as i32 - ((y - gesture.y) / ROW_HEIGHT).round() as i32).clamp(0, 127) as u8;
        }
        note
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn hit_cull_snap_drag() {
        let p = Project::demo(); let v = Viewport::default(); let (scene, capped) = v.scene(&p, 1, None, None);
        assert!(!capped); assert_eq!(scene.len(), 8);
        let rect = &scene[0]; let (note, resize) = v.hit(&p, 1, rect.x + 4.0, rect.y + 4.0).unwrap(); assert!(!resize);
        let g = Gesture { original: note.clone(), preview: note, x: 4.0, y: 4.0, resize: false };
        let moved = v.drag(&g, 68.0, -12.0, p.length_ticks); assert_eq!(moved.start, PPQ); assert_eq!(moved.pitch, 61);
        assert_eq!(v.insertion(p.length_ticks, 21.0, 16.0).unwrap(), (SNAP, 83));
    }
    #[test] fn offscreen_notes_do_not_create_rectangles() {
        let p = Project::demo(); let v = Viewport { start_tick: 12 * PPQ, ..Viewport::default() };
        assert!(v.scene(&p, 1, None, None).0.is_empty());
    }
}
