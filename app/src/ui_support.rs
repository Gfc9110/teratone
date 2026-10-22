//! Presentation only; shared with the software interaction probe.
use crate::{AppWindow, ClipVisual, MiniVisual, NoteVisual, TrackVisual};
use opendaw_app::{arrangement::TrackRow, controller::ViewState};
use slint::{Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

pub fn visual(t: &TrackRow) -> TrackVisual {
    TrackVisual {
        id: t.id.to_string().into(), name: t.name.clone().into(), instrument: t.instrument.clone().into(),
        selected: t.selected, mute: t.mute, solo: t.solo, audible: t.audible,
        tint: slint::Color::from_argb_encoded(0xff000000 | u32::from_str_radix(t.color.trim_start_matches('#'), 16).unwrap_or(0x62bfa6)),
        clips: ModelRc::new(VecModel::from(t.clips.iter().map(|c| ClipVisual {
            id: c.id.to_string().into(), name: c.name.clone().into(), start: c.start, length: c.length, selected: c.selected,
            notes: ModelRc::new(VecModel::from(c.notes.iter().map(|n| MiniVisual { x: n.x, y: n.y, w: n.width, h: n.height }).collect::<Vec<_>>())),
        }).collect::<Vec<_>>())),
    }
}

struct Inspector {
    id: u64, name: String, color: String, gain: f32, pan: f32, sample: String, root: u8, one_shot: bool,
}
pub struct Presenter { model: Rc<VecModel<TrackVisual>>, rows: Vec<TrackRow>, inspector: Option<Inspector> }
impl Presenter {
    pub fn new(ui: &AppWindow) -> Self {
        let model = Rc::new(VecModel::default()); ui.set_track_items(model.clone().into());
        Self { model, rows: Vec::new(), inspector: None }
    }
    pub fn tracks(&mut self, rows: Vec<TrackRow>) {
        if self.rows.len() == rows.len() && self.rows.iter().zip(&rows).all(|(a,b)| a.id == b.id) {
            for (index, row) in rows.iter().enumerate() {
                if *row != self.rows[index] { self.model.set_row_data(index, visual(row)); }
            }
        } else { self.model.set_vec(rows.iter().map(visual).collect::<Vec<_>>()); }
        self.rows = rows;
    }
    pub fn apply(&mut self, ui: &AppWindow, view: ViewState) {
        ui.set_project_name(view.name.into()); ui.set_status(view.status.into()); ui.set_selected_label(view.selected_label.into());
        self.tracks(view.tracks); ui.set_track_index(view.track_index as i32);
        ui.set_track_id(view.track_id.to_string().into()); ui.set_track_name(view.track_name.clone().into());
        let prior = self.inspector.as_ref(); let changed = prior.map_or(true, |p| p.id != view.track_id);
        // Keep unfinished text edits while unrelated telemetry / viewport views arrive.
        if changed || prior.is_some_and(|p| p.name != view.track_name) { ui.set_track_name_draft(view.track_name.clone().into()); }
        if changed || prior.is_some_and(|p| p.color != view.track_color) {
            ui.set_color_index(opendaw_app::project::TRACK_COLORS.iter().position(|c| *c == view.track_color).map_or(-1, |i| i as i32));
        }
        if changed || prior.is_some_and(|p| p.gain != view.gain) { ui.set_gain(view.gain); }
        if changed || prior.is_some_and(|p| p.pan != view.pan) { ui.set_pan(view.pan); }
        if changed || prior.is_some_and(|p| p.sample != view.sample_path || p.root != view.root_note || p.one_shot != view.one_shot) {
            ui.set_sample_path(view.sample_path.clone().into()); ui.set_sample_root(view.root_note as i32); ui.set_sample_one_shot(view.one_shot);
        }
        self.inspector = Some(Inspector { id: view.track_id, name: view.track_name.clone(), color: view.track_color.clone(), gain: view.gain, pan: view.pan,
            sample: view.sample_path.clone(), root: view.root_note, one_shot: view.one_shot });
        let names: Vec<SharedString> = view.clips.iter().map(|c| c.name.clone().into()).collect();
        let ids: Vec<SharedString> = view.clips.iter().map(|c| c.id.to_string().into()).collect();
        ui.set_clip_names(ModelRc::new(VecModel::from(names))); ui.set_clip_ids(ModelRc::new(VecModel::from(ids)));
        ui.set_clip_index(view.clip_index.map_or(-1, |i| i as i32)); ui.set_clip_start_beat(view.clip_start as f32 / 960.0);
        ui.set_bpm(view.bpm.round() as i32);
        ui.set_muted(view.mute); ui.set_soloed(view.solo); ui.set_audible(view.audible);
        ui.set_instrument(view.instrument.into());
        ui.set_project_beats(view.project_beats); ui.set_arrangement_limited(view.arrangement_limited); ui.set_mix_label(view.mix_label.into());
        ui.set_dirty(view.dirty); ui.set_can_undo(view.can_undo); ui.set_can_redo(view.can_redo); ui.set_export_busy(view.export_busy);
        ui.set_pixels_per_beat(view.viewport.pixels_per_beat); ui.set_scroll_beat(view.viewport.start_tick as f32 / 960.0);
        ui.set_top_note(view.viewport.top_note as i32);
        ui.set_note_items(ModelRc::new(VecModel::from(view.notes.into_iter().map(|n| NoteVisual {
            x: n.x, y: n.y, w: n.width, h: n.height, selected: n.selected, label: n.label.into(),
        }).collect::<Vec<_>>())));
        ui.invoke_clamp_scroll();
    }
}
