//! Presentation data for the velocity lane, independently culled on the time axis.
use crate::{editor::{Viewport, MAX_VISIBLE}, project::MidiClip};

#[derive(Clone, Debug, PartialEq)]
pub struct VelocityBar { pub id: u64, pub x: f32, pub value: u8, pub selected: bool }

pub fn scene(viewport: &Viewport, clip: Option<&MidiClip>, selected: Option<u64>) -> Vec<VelocityBar> {
    let Some(clip) = clip else { return Vec::new(); };
    clip.notes.iter().filter_map(|note| {
        let x = viewport.x(note.start);
        // Allow a narrow hit target at the left edge even when the note starts just outside.
        if x < -4.0 || x >= viewport.width { return None; }
        Some(VelocityBar { id: note.id, x, value: note.velocity, selected: selected == Some(note.id) })
    }).take(MAX_VISIBLE).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, PPQ};
    #[test]
    fn lane_is_independent_of_vertical_scroll_and_tracks_selection() {
        let project = Project::demo();
        let mut viewport = Viewport::default();
        let clip = project.clip(1, 4);
        let first = scene(&viewport, clip, Some(10));
        assert!(first.iter().any(|bar| bar.id == 10 && bar.selected && bar.value == 100));
        viewport.scroll_y = 0.0;
        assert_eq!(first, scene(&viewport, clip, Some(10)));
        viewport.start_tick = 12 * PPQ;
        assert!(scene(&viewport, clip, None).is_empty());
    }
}
