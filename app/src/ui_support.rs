//! Presentation only; shared with the software interaction probe.
use crate::{AppWindow, ClipVisual, MiniVisual, NoteVisual, TrackVisual, KeyVisual};
use opendaw_app::{arrangement::TrackRow, controller::ViewState};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{rc::Rc,cell::{Cell,RefCell}};
use opendaw_app::{controller::Request,editor::{KeyRow,Viewport},mailbox::Mailbox};

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
pub struct Presenter { model: Rc<VecModel<TrackVisual>>, rows: Vec<TrackRow>, inspector: Option<Inspector>, clip_info: Option<(u64,String)>, keys:Vec<KeyRow>, key_model:Rc<VecModel<KeyVisual>> }
impl Presenter {
    pub fn new(ui: &AppWindow) -> Self {
        let model = Rc::new(VecModel::default()); ui.set_track_items(model.clone().into());
        let key_model=Rc::new(VecModel::default());ui.set_key_items(key_model.clone().into());
        Self { model, rows: Vec::new(), inspector: None, clip_info:None,keys:Vec::new(),key_model }
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
        let selection_changed=changed || self.clip_info.as_ref().is_none_or(|c|c.0!=view.clip_id);
        if selection_changed {ui.invoke_cancel_interactions();}
        if self.clip_info.as_ref().is_none_or(|c|c.0!=view.clip_id || c.1!=view.clip_name) {ui.set_clip_name_draft(view.clip_name.clone().into());}
        self.clip_info=Some((view.clip_id,view.clip_name));
        ui.set_clip_id(view.clip_id.to_string().into());ui.set_clip_length_beats(view.clip_length as f32/960.0);
        ui.set_instance_id(view.instance_id.to_string().into());ui.set_selected_note_id(view.selected_note.to_string().into());ui.set_note_velocity(view.velocity);
        ui.set_clip_index(view.clip_index.map_or(-1, |i| i as i32)); ui.set_clip_start_beat(view.clip_start as f32 / 960.0);
        ui.set_bpm(view.bpm.round() as i32);
        ui.set_muted(view.mute); ui.set_soloed(view.solo); ui.set_audible(view.audible);
        ui.set_instrument(view.instrument.into());
        ui.set_project_beats(view.project_beats); ui.set_arrangement_limited(view.arrangement_limited); ui.set_mix_label(view.mix_label.into());
        ui.set_dirty(view.dirty); ui.set_can_undo(view.can_undo); ui.set_can_redo(view.can_redo); ui.set_export_busy(view.export_busy);
        ui.set_pixels_per_beat(view.viewport.pixels_per_beat); ui.set_scroll_beat(view.viewport.start_tick as f32 / 960.0);
        ui.set_piano_scroll_y(view.viewport.scroll_y);ui.set_piano_row_height(view.viewport.row_height);ui.set_notation_index(view.viewport.octave_offset as i32+1);
        ui.set_piano_bar_y(-view.viewport.scroll_y);ui.set_piano_bar_x(-(view.viewport.start_tick as f32)/960.0*view.viewport.pixels_per_beat);
        if self.keys!=view.keys {
            ui.invoke_release_audition();
            self.key_model.set_vec(view.keys.iter().map(|k|KeyVisual{pitch:k.pitch as i32,y:k.y,h:k.height,black:k.black,c:k.c,label:k.label.clone().into()}).collect::<Vec<_>>());
            self.keys=view.keys;
        }
        ui.set_note_items(ModelRc::new(VecModel::from(view.notes.into_iter().map(|n| NoteVisual {
            x: n.x, y: n.y, w: n.width, h: n.height, selected: n.selected, label: n.label.into(),
        }).collect::<Vec<_>>())));
        ui.invoke_clamp_scroll();if selection_changed {ui.invoke_reveal_clip();}
    }
}

/// The native app and software probe share exactly the same clip/piano input adapter.
pub fn bind_editor(ui:&AppWindow, mailbox:Rc<RefCell<Mailbox>>) {
    fn queue(m:&RefCell<Mailbox>,weak:&slint::Weak<AppWindow>,request:Request) {
        let background=matches!(request,Request::Move{..}|Request::ScrollPiano(..)|Request::CancelGesture(_)|Request::AuditionOff(_));
        let result=m.borrow_mut().enqueue(request);
        if let Some(ui)=weak.upgrade(){if let Err(error)=result {ui.set_action_error(error.into());} else if !background {ui.set_action_error("".into());}}
    }
    macro_rules! bind {($method:ident,($($arg:ident),*)=>$request:expr)=>{{
        let m=mailbox.clone();let weak=ui.as_weak();ui.$method(move|$($arg),*|queue(&m,&weak,$request));
    }};}
    bind!(on_new_clip,(track,beat)=>Request::AddClip{track:track.parse().unwrap_or(0),beat:beat as f64});
    bind!(on_duplicate_clip,(track,clip)=>Request::DuplicateClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_remove_clip,(track,clip)=>Request::RemoveClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_rename_clip,(track,clip,name)=>Request::RenameClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0),name:name.into()});
    bind!(on_move_clip,(track,clip,destination,beat)=>Request::MoveClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0),destination:destination.parse().unwrap_or(0),beat:beat as f64});
    bind!(on_resize_clip,(track,clip,beats)=>Request::ResizeClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0),beats:beats as f64});
    bind!(on_zoom_y,(factor)=>Request::ZoomY(factor));bind!(on_fit_notes,()=>Request::FitNotes);bind!(on_notation,(offset)=>Request::Notation(offset));
    bind!(on_scroll_piano,(beat,y)=>Request::ScrollPiano(beat as f64,y));
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_velocity(move|value|{
            if let Some(ui)=weak.upgrade(){queue(&m,&weak,Request::Velocity{track:ui.get_track_id().parse().unwrap_or(0),clip:ui.get_clip_id().parse().unwrap_or(0),note:ui.get_selected_note_id().parse().unwrap_or(0),value});}
        });
    }
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_remove_note(move||{
            if let Some(ui)=weak.upgrade(){queue(&m,&weak,Request::Delete{track:ui.get_track_id().parse().unwrap_or(0),clip:ui.get_clip_id().parse().unwrap_or(0),note:ui.get_selected_note_id().parse().unwrap_or(0)});}
        });
    }
    let gesture=Rc::new(Cell::new(None));
    {
        let m=mailbox.clone();let weak=ui.as_weak();let gesture=gesture.clone();
        ui.on_down(move|x,y,right|{
            let Some(ui)=weak.upgrade() else {return;}; if ui.get_clip_id()=="0" {ui.set_action_error("Create a MIDI clip first.".into());return;}
            let token=match m.borrow_mut().token(){Ok(t)=>t,Err(e)=>{ui.set_action_error(e.into());return;}};
            let viewport=Viewport{start_tick:(ui.get_scroll_beat() as f64*960.0).round() as u64,pixels_per_beat:ui.get_pixels_per_beat(),
                scroll_y:ui.get_piano_scroll_y(),row_height:ui.get_piano_row_height(),width:ui.get_canvas_width(),height:ui.get_canvas_height(),octave_offset:(ui.get_notation_index()-1) as i8};
            queue(&m,&weak,Request::Down{track:ui.get_track_id().parse().unwrap_or(0),clip:ui.get_clip_id().parse().unwrap_or(0),token,viewport,x,y,right});gesture.set(Some(token));
        });
    }
    {
        let m=mailbox.clone();let weak=ui.as_weak();let gesture=gesture.clone();ui.on_moved(move|x,y|{if let Some(token)=gesture.get(){queue(&m,&weak,Request::Move{token,x,y});}});
    }
    {
        let m=mailbox.clone();let weak=ui.as_weak();let gesture=gesture.clone();ui.on_up(move|x,y|{if let Some(token)=gesture.take(){queue(&m,&weak,Request::Up{token,x,y});}});
    }
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_cancel_note(move||{if let Some(token)=gesture.take(){queue(&m,&weak,Request::CancelGesture(token));}});
    }
    let held=Rc::new(Cell::new(None));
    {
        let m=mailbox.clone();let weak=ui.as_weak();let held=held.clone();
        ui.on_key_gate(move|pitch,on|{
            if let Some(token)=held.take(){queue(&m,&weak,Request::AuditionOff(token));}
            if on {
                let Some(ui)=weak.upgrade() else {return;};
                let result=m.borrow_mut().audition_on(ui.get_track_id().parse().unwrap_or(0),ui.get_instance_id().parse().unwrap_or(0),pitch.clamp(0,127) as u8,100);
                match result {Ok(token)=>held.set(Some(token)),Err(e)=>ui.set_action_error(e.into())}
            }
        });
    }
    {let m=mailbox;ui.on_release_audition(move||{held.set(None);m.borrow_mut().release_audition();});}
}
