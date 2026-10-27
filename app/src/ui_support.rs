//! Presentation only; shared with the software interaction probe.
use crate::{AppWindow, ClipVisual, MiniVisual, NoteVisual, TrackVisual, KeyVisual,PadVisual,RecentProjectVisual,VelocityVisual,ReferenceVisual,GhostVisual};
use opendaw_app::{arrangement::TrackRow, controller::ViewState};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{rc::Rc,cell::{Cell,RefCell},time::{Duration,Instant},collections::HashMap};
use opendaw_app::{drums::{DrumPad,PadField},bridge::DrumStatus,synth::{SynthParameters,SynthPreset,SynthField},controller::Request,editor::{KeyRow,Viewport},mailbox::Mailbox};

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
    id: u64, name: String, color: String, gain: f32, pan: f32, sample: String, root: u8, one_shot: bool, synth:Option<SynthParameters>, preset:Option<SynthPreset>,
}
pub struct Presenter {
    project_session: Option<u64>, project_path: Option<std::path::PathBuf>, recent: Vec<RecentProjectVisual>,
    pads:Rc<VecModel<PadVisual>>,pad_rows:Vec<(u64,String,u8,bool)>,pad_mutes:Rc<VecModel<bool>>,pulse_model:Rc<VecModel<bool>>,
    pad_cache:Option<DrumPad>,drum_target:Option<(u64,u64)>,last_triggers:[u64;16],meter_generation:u64,pulses:[Option<Instant>;16],
    model: Rc<VecModel<TrackVisual>>, rows: Vec<TrackRow>, inspector: Option<Inspector>, clip_info: Option<(u64,String)>, keys:Vec<KeyRow>, key_model:Rc<VecModel<KeyVisual>>, velocity_bars:Vec<opendaw_app::velocity_lane::VelocityBar>, velocity_model:Rc<VecModel<VelocityVisual>>,
    notes:Vec<opendaw_app::editor::NoteRect>, rendered_notes:Vec<opendaw_app::editor::NoteRect>, note_model:Rc<VecModel<NoteVisual>>,
    references:Vec<opendaw_app::midi_references::ReferenceRow>, reference_model:Rc<VecModel<ReferenceVisual>>,
    ghosts:Vec<opendaw_app::midi_references::GhostRect>, ghost_model:Rc<VecModel<GhostVisual>>,
    arrangement_inspector:Option<opendaw_app::controller::TrackInspectorView> }
impl Presenter {
    pub fn new(ui: &AppWindow) -> Self {
        ui.set_track_limit(opendaw_app::project::MAX_TRACKS as i32);
        let model = Rc::new(VecModel::default()); ui.set_track_items(model.clone().into());
        let key_model=Rc::new(VecModel::default());ui.set_key_items(key_model.clone().into());
        let velocity_model=Rc::new(VecModel::default());ui.set_velocity_items(velocity_model.clone().into());
        let note_model=Rc::new(VecModel::default());ui.set_note_items(note_model.clone().into());
        let reference_model=Rc::new(VecModel::default());ui.set_reference_items(reference_model.clone().into());
        let ghost_model=Rc::new(VecModel::default());ui.set_ghost_items(ghost_model.clone().into());
        let pads=Rc::new(VecModel::default());ui.set_drum_pads(pads.clone().into());
        let pulse_model=Rc::new(VecModel::from(vec![false;16]));ui.set_pad_pulses(pulse_model.clone().into());
        let pad_mutes=Rc::new(VecModel::from(vec![false;16]));ui.set_pad_mutes(pad_mutes.clone().into());
        Self { project_session:None,project_path:None,recent:Vec::new(),pads,pad_rows:Vec::new(),pad_mutes,pulse_model,pad_cache:None,drum_target:None,last_triggers:[0;16],meter_generation:0,pulses:[None;16],model, rows: Vec::new(), inspector: None, clip_info:None,keys:Vec::new(),key_model,velocity_bars:Vec::new(),velocity_model,notes:Vec::new(),rendered_notes:Vec::new(),note_model,references:Vec::new(),reference_model,ghosts:Vec::new(),ghost_model,arrangement_inspector:None }
    }
    pub fn drum_meter(&mut self,ui:&AppWindow,track:u64,instance:u64,status:DrumStatus){
        if self.drum_target!=Some((track,instance)){return;}
        if self.meter_generation!=status.generation{self.meter_generation=status.generation;self.last_triggers=[0;16];}
        for i in 0..status.pad_count.min(16) as usize{if self.last_triggers[i]!=status.triggers[i]{self.pulses[i]=Some(Instant::now()+Duration::from_millis(120));self.last_triggers[i]=status.triggers[i];}}
        self.tick_drums(ui);
    }
    pub fn tick_drums(&mut self,_ui:&AppWindow){
        let now=Instant::now();for i in 0..16{let on=self.pulses[i].is_some_and(|end|end>now);if self.pulse_model.row_data(i)!=Some(on){self.pulse_model.set_row_data(i,on);}}
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
        let switched = self
            .project_session
            .is_some_and(|session| session != view.project_session);
        if switched {
            ui.invoke_cancel_interactions();
            self.inspector = None;
            self.arrangement_inspector = None;
            self.clip_info = None;
            self.pad_cache = None;
            self.drum_target = None;
            if view.project_view_loaded {ui.set_arrangement_zoom(view.linked_scale);}
            ui.set_arrangement_x(-view.linked_start*ui.get_arrangement_zoom());
            ui.set_arrangement_y(view.arrangement_scroll_y);
            ui.set_project_position(0.0);
            ui.set_playing(false);
            ui.set_looping(false);
            ui.set_transport_text("001 : 01".into());
            ui.set_home_visible(false);
            ui.set_confirm_switch(false);
            ui.set_action_error("".into());
        }
        if self.project_session.is_none() || switched {
            ui.set_new_parent(view.default_folder.to_string_lossy().into_owned().into());
        }
        if self.project_path != view.project_path {
            ui.set_project_path(
                view.project_path
                    .as_ref()
                    .map_or(String::new(), |p| p.to_string_lossy().into_owned())
                    .into(),
            );
            if let Some(path) = &view.project_path {
                ui.set_save_as_visible(false);
                ui.set_export_path(
                    path.join("mixdown.wav")
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                );
                ui.set_new_parent(view.default_folder.to_string_lossy().into_owned().into());
            }
            self.project_path = view.project_path;
        }
        self.project_session = Some(view.project_session);
        ui.set_ui_project_session(view.project_session.to_string().into());
        ui.set_has_project(view.project_session > 0);
        let recent: Vec<_> = view
            .recent_projects
            .into_iter()
            .map(|p| RecentProjectVisual {
                available: p.path.join("project.json").is_file(),
                name: p.name.into(),
                path: p.path.to_string_lossy().into_owned().into(),
            })
            .collect();
        if self.recent != recent {
            ui.set_recent_projects(ModelRc::new(VecModel::from(recent.clone())));
            self.recent = recent;
        }
        ui.set_project_name(view.name.into()); ui.set_status(view.status.into()); ui.set_selected_label(view.selected_label.into());
        self.tracks(view.tracks); ui.set_track_index(view.track_index as i32);
        let info=&view.inspector_track;let prior_info=self.arrangement_inspector.as_ref();
        let changed_info=prior_info.is_none_or(|p|p.id!=info.id);
        ui.set_inspector_track_id(info.id.to_string().into());ui.set_arrangement_track_id(info.id.to_string().into());ui.set_inspector_track_index(info.index as i32);
        ui.set_inspector_track_name(info.name.clone().into());
        if changed_info || prior_info.is_some_and(|p|p.name!=info.name) {ui.set_inspector_track_name_draft(info.name.clone().into());}
        if changed_info || prior_info.is_some_and(|p|p.color!=info.color) {ui.set_inspector_color_index(opendaw_app::project::TRACK_COLORS.iter().position(|c|*c==info.color).map_or(-1,|i|i as i32));}
        if changed_info || prior_info.is_some_and(|p|p.gain!=info.gain) {ui.set_inspector_gain(info.gain);}
        if changed_info || prior_info.is_some_and(|p|p.pan!=info.pan) {ui.set_inspector_pan(info.pan);}
        if changed_info || prior_info.is_some_and(|p|p.sample_path!=info.sample_path || p.sample_root!=info.sample_root || p.sample_one_shot!=info.sample_one_shot) {
            ui.set_inspector_sample_path(info.sample_path.clone().into());ui.set_inspector_sample_root(info.sample_root as i32);ui.set_inspector_sample_one_shot(info.sample_one_shot);
        }
        ui.set_inspector_muted(info.mute);ui.set_inspector_soloed(info.solo);ui.set_inspector_audible(info.audible);
        ui.set_inspector_instrument(info.instrument.clone().into());ui.set_inspector_is_synth(info.is_synth);ui.set_inspector_is_drums(info.is_drums);
        self.arrangement_inspector=Some(info.clone());
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
        ui.set_is_drums(view.drum_kit.is_some());
        let target=view.drum_kit.as_ref().map(|_|(view.track_id,view.instance_id));
        let rows:Vec<_>=view.drum_kit.as_ref().map_or(Vec::new(),|k|k.pads.iter().map(|p|(p.id,p.name.clone(),p.midi_note,p.sample.is_some())).collect());
        if self.drum_target!=target || !self.pad_rows.iter().map(|p|p.0).eq(rows.iter().map(|p|p.0)){
            self.drum_target=target;self.last_triggers=[0;16];self.meter_generation=0;self.pulses=[None;16];
        }
        if self.pad_rows!=rows{
            ui.invoke_release_audition();
            self.pads.set_vec(rows.iter().map(|p|PadVisual{id:p.0.to_string().into(),name:p.1.clone().into(),midi:p.2.into(),filled:p.3,muted:false}).collect::<Vec<_>>());self.pad_rows=rows;
        }
        if let Some(kit)=&view.drum_kit{
            ui.set_drum_kit_name(kit.name.clone().into());for (i,p) in kit.pads.iter().enumerate(){if self.pad_mutes.row_data(i)!=Some(p.parameters.mute){self.pad_mutes.set_row_data(i,p.parameters.mute);}}
            if let Some(p)=kit.pad(view.selected_pad){
                let old=self.pad_cache.as_ref();let selected=changed || old.is_none_or(|old|old.id!=p.id);
                ui.set_pad_id(p.id.to_string().into());
                if selected || old.is_some_and(|o|o.name!=p.name){ui.set_pad_name(p.name.clone().into());}
                if selected || old.is_some_and(|o|o.midi_note!=p.midi_note){ui.set_pad_midi(p.midi_note as i32);}
                if selected || old.is_some_and(|o|o.parameters.gain!=p.parameters.gain){ui.set_pad_gain(p.parameters.gain);}
                if selected || old.is_some_and(|o|o.parameters.pan!=p.parameters.pan){ui.set_pad_pan(p.parameters.pan);}
                if selected || old.is_some_and(|o|o.parameters.tune_semitones!=p.parameters.tune_semitones){ui.set_pad_tune(p.parameters.tune_semitones.round() as i32);}
                if selected || old.is_some_and(|o|o.parameters.choke_group!=p.parameters.choke_group){ui.set_pad_choke(p.parameters.choke_group as i32);}
                ui.set_pad_muted(p.parameters.mute);ui.set_pad_one_shot(p.parameters.one_shot);ui.set_pad_filled(p.sample.is_some());
                ui.set_pad_source(p.sample.as_ref().map_or_else(String::new,|s|s.display()).into());
                if selected {ui.set_pad_import_path("".into());} // Keep the user draft across ACKs; never expose internal cache paths.
                self.pad_cache=Some(p.clone());
            }
        }else{self.pad_cache=None;ui.set_pad_id("0".into());}
        ui.set_is_synth(view.synth.is_some());
        let sound=view.synth.unwrap_or_default();let old_sound=prior.and_then(|p|p.synth).unwrap_or_default();
        let source_changed=changed || prior.is_some_and(|p|p.synth.is_some()!=view.synth.is_some());
        if source_changed || old_sound.waveform!=sound.waveform {ui.set_synth_wave(sound.waveform.index() as i32);}
        if source_changed || old_sound.shape!=sound.shape {ui.set_synth_shape(sound.shape);}
        if source_changed || old_sound.pulse_width!=sound.pulse_width {ui.set_synth_duty(sound.pulse_width);}
        if source_changed || old_sound.attack!=sound.attack {ui.set_synth_attack((sound.attack*1000.0).round() as i32);}
        if source_changed || old_sound.decay!=sound.decay {ui.set_synth_decay((sound.decay*1000.0).round() as i32);}
        if source_changed || old_sound.sustain!=sound.sustain {ui.set_synth_sustain((sound.sustain*100.0).round() as i32);}
        if source_changed || old_sound.release!=sound.release {ui.set_synth_release((sound.release*1000.0).round() as i32);}
        let label=view.synth_preset.as_ref().map(|p|format!("{}{}",p.name,if Some(p.parameters)!=view.synth{" · modified"}else{""}))
            .unwrap_or_else(||if view.synth==Some(SynthParameters::default()){"Legacy Sine".into()}else{"Custom sound".into()});
        ui.set_preset_label(label.into());
        if changed || prior.is_some_and(|p|p.preset!=view.synth_preset) {ui.set_preset_name(view.synth_preset.as_ref().map_or("My sound",|p|p.name.as_str()).into());}
        self.inspector = Some(Inspector { id: view.track_id, name: view.track_name.clone(), color: view.track_color.clone(), gain: view.gain, pan: view.pan,
            sample: view.sample_path.clone(), root: view.root_note, one_shot: view.one_shot,synth:view.synth,preset:view.synth_preset.clone() });
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
            ui.invoke_release_keyboard();
            self.key_model.set_vec(view.keys.iter().map(|k|KeyVisual{pitch:k.pitch as i32,y:k.y,h:k.height,black:k.black,c:k.c,in_scale:k.in_scale,label:k.label.clone().into()}).collect::<Vec<_>>());
            self.keys=view.keys;
        }
        if self.velocity_bars != view.velocity_bars {
            self.velocity_model.set_vec(view.velocity_bars.iter().map(|b| VelocityVisual {
                id: b.id.to_string().into(), x: b.x, value: b.value as i32, selected: b.selected,
            }).collect::<Vec<_>>());
            self.velocity_bars = view.velocity_bars;
        }
        if self.notes!=view.notes {
            let rendered:Vec<_>=opendaw_app::note_presentation::unique_notes(&view.notes).into_iter().cloned().collect();
            let visual=|n:&opendaw_app::editor::NoteRect|NoteVisual{x:n.x,y:n.y,w:n.width,h:n.height,selected:n.selected,label:n.label.clone().into()};
            let changed:Vec<_>=if self.rendered_notes.len()==rendered.len() {self.rendered_notes.iter().zip(&rendered).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some(i)).collect()} else {Vec::new()};
            if !changed.is_empty() && changed.len()<=rendered.len()/4+1 {for i in changed {self.note_model.set_row_data(i,visual(&rendered[i]));}}
            else if self.rendered_notes!=rendered {self.note_model.set_vec(rendered.iter().map(visual).collect::<Vec<_>>());}
            self.rendered_notes=rendered;
            self.notes=view.notes;
        }
        ui.set_follow_selection(view.follow_selection);
        ui.set_reference_count(view.reference_rows.iter().filter(|r|r.visible && !r.active).count() as i32);
        if self.references!=view.reference_rows {
            self.reference_model.set_vec(view.reference_rows.iter().map(|r|ReferenceVisual{track_id:r.track.to_string().into(),clip_id:r.clip.to_string().into(),label:r.label.clone().into(),visible:r.visible,pinned:r.pinned,active:r.active}).collect::<Vec<_>>());
            self.references=view.reference_rows;
        }
        if self.ghosts!=view.ghosts {
            self.ghost_model.set_vec(view.ghosts.iter().map(|g|GhostVisual{x:g.x,y:g.y,w:g.width,h:g.height,tint:slint::Color::from_argb_encoded(0xff000000 | u32::from_str_radix(g.color.trim_start_matches('#'),16).unwrap_or(0x62bfa6))}).collect::<Vec<_>>());
            self.ghosts=view.ghosts;
        }
        ui.set_midi_view_mode(view.midi_view_mode);ui.set_scale_mode(view.scale_mode);ui.set_scale_root(view.scale_root);
        ui.set_piano_row_count(view.piano_row_count as i32);ui.set_snap_index(view.snap_index);
        ui.set_navigation_linked(view.navigation_linked);
        if view.navigation_linked {
            if (ui.get_arrangement_zoom()-view.linked_scale).abs()>0.001 {ui.set_arrangement_zoom(view.linked_scale);}
            let x=-view.linked_start*view.linked_scale;
            if (ui.get_arrangement_x()-x).abs()>0.001 {ui.set_arrangement_x(x);}
        }
        ui.invoke_clamp_scroll();if selection_changed && !switched {ui.invoke_reveal_clip();}self.tick_drums(ui);
    }
}

/// The native app and software probe share exactly the same clip/piano input adapter.
pub fn bind_editor(ui:&AppWindow, mailbox:Rc<RefCell<Mailbox>>) {
    fn queue(m:&RefCell<Mailbox>,weak:&slint::Weak<AppWindow>,request:Request) {
        let background=matches!(request,Request::Move{..}|Request::ScrollPiano(..)|Request::ScrollPianoForSession(..)|Request::CancelGesture(_)|Request::AuditionOff(_));
        let result=m.borrow_mut().enqueue(request);
        if let Some(ui)=weak.upgrade(){if let Err(error)=result {ui.set_action_error(error.into());} else if !background {ui.set_action_error("".into());}}
    }
    macro_rules! bind {($method:ident,($($arg:ident),*)=>$request:expr)=>{{
        let m=mailbox.clone();let weak=ui.as_weak();ui.$method(move|$($arg),*|queue(&m,&weak,$request));
    }};}
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_synth_parameter(move|track,instance,field,value|{
            match SynthField::from_index(field){Ok(field)=>queue(&m,&weak,Request::SynthParameter{track:track.parse().unwrap_or(0),instance:instance.parse().unwrap_or(0),field,value}),
                Err(e)=>if let Some(ui)=weak.upgrade(){ui.set_action_error(e.into());}}
        });
    }
    bind!(on_factory_preset,(track,instance,index)=>Request::FactoryPreset{track:track.parse().unwrap_or(0),instance:instance.parse().unwrap_or(0),index:index as usize});
    bind!(on_save_synth_preset,(track,instance,path,name)=>Request::SaveSynthPreset{track:track.parse().unwrap_or(0),instance:instance.parse().unwrap_or(0),path:std::path::PathBuf::from(path.as_str()),name:name.into()});
    bind!(on_load_synth_preset,(track,instance,path)=>Request::LoadSynthPreset{track:track.parse().unwrap_or(0),instance:instance.parse().unwrap_or(0),path:std::path::PathBuf::from(path.as_str())});
    bind!(on_add_drums,(track)=>Request::AddDrumTrack{after:track.parse().unwrap_or(0)});
    bind!(on_use_drums,(track)=>Request::UseDrumKit{track:track.parse().unwrap_or(0)});
    fn pad_request(m:&RefCell<Mailbox>,weak:&slint::Weak<AppWindow>,make:impl FnOnce(u64,u64,u64)->Result<Request,String>){
        let Some(ui)=weak.upgrade() else{return;};match make(ui.get_track_id().parse().unwrap_or(0),ui.get_instance_id().parse().unwrap_or(0),ui.get_pad_id().parse().unwrap_or(0)){
            Ok(r)=>queue(m,weak,r),Err(e)=>ui.set_action_error(e.into())
        }
    }
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_pad_parameter(move|field,value|pad_request(&m,&weak,|track,instance,pad|Ok(Request::PadParameter{track,instance,pad,field:PadField::from_index(field)?,value})));}
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_toggle_pad(move|field|pad_request(&m,&weak,|track,instance,pad|Ok(Request::TogglePad{track,instance,pad,field:PadField::from_index(field)?})));}
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_rename_pad(move|name|pad_request(&m,&weak,|track,instance,pad|Ok(Request::RenamePad{track,instance,pad,name:name.into()})));}
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_map_pad(move|midi|pad_request(&m,&weak,|track,instance,pad|Ok(Request::MapPad{track,instance,pad,midi})));}
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_import_pad(move|path|pad_request(&m,&weak,|track,instance,pad|Ok(Request::ImportPad{track,instance,pad,path:std::path::PathBuf::from(path.as_str())})));}
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_clear_pad(move||pad_request(&m,&weak,|track,instance,pad|Ok(Request::ClearPad{track,instance,pad})));}
    {let m=mailbox.clone();let weak=ui.as_weak();ui.on_select_pad(move|id|{if let Some(ui)=weak.upgrade(){queue(&m,&weak,Request::SelectPad{track:ui.get_track_id().parse().unwrap_or(0),instance:ui.get_instance_id().parse().unwrap_or(0),pad:id.parse().unwrap_or(0)});}});}
    bind!(on_new_clip,(track,beat)=>Request::AddClip{track:track.parse().unwrap_or(0),beat:beat as f64});
    bind!(on_duplicate_clip,(track,clip)=>Request::DuplicateClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_remove_clip,(track,clip)=>Request::RemoveClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_rename_clip,(track,clip,name)=>Request::RenameClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0),name:name.into()});
    bind!(on_move_clip,(track,clip,destination,beat)=>Request::MoveClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0),destination:destination.parse().unwrap_or(0),beat:beat as f64});
    bind!(on_resize_clip,(track,clip,beats)=>Request::ResizeClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0),beats:beats as f64});
    bind!(on_zoom_y,(factor)=>Request::ZoomY(factor));bind!(on_fit_notes,()=>Request::FitNotes);bind!(on_restore_view,()=>Request::RestoreView);bind!(on_notation,(offset)=>Request::Notation(offset));
    bind!(on_zoom_at,(factor,x)=>Request::ZoomAt(factor,x));
    bind!(on_set_midi_view_mode,(index)=>Request::SetMidiViewMode(index));
    bind!(on_set_scale,(mode,root)=>Request::SetScale(mode,root));bind!(on_set_snap,(index)=>Request::SetSnap(index));
    bind!(on_set_navigation_linked,(linked)=>Request::SetNavigationLinked(linked));
    bind!(on_toggle_reference,(track,clip)=>Request::ToggleReference{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_toggle_reference_pin,(track,clip)=>Request::ToggleReferencePin{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_hide_references,()=>Request::HideReferences);bind!(on_follow_selection_changed,(follow)=>Request::FollowSelection(follow));
    bind!(on_activate_editor_clip,(track,clip)=>Request::ActivateEditorClip{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    bind!(on_delete_note_at,(track,clip,_x,_y)=>Request::DeleteContext{track:track.parse().unwrap_or(0),clip:clip.parse().unwrap_or(0)});
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_scroll_piano(move|beat,y|{
            if let Some(ui)=weak.upgrade() {
                let session=ui.get_ui_project_session().parse().unwrap_or(0);
                queue(&m,&weak,Request::ScrollPianoForSession(session,beat as f64,y));
            }
        });
    }
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_select_velocity_note(move|note|{
            if let Some(ui)=weak.upgrade(){queue(&m,&weak,Request::SelectVelocityNote{track:ui.get_track_id().parse().unwrap_or(0),clip:ui.get_clip_id().parse().unwrap_or(0),note:note.parse().unwrap_or(0)});}
        });
    }
    {
        let m=mailbox.clone();let weak=ui.as_weak();ui.on_set_velocity_note(move|note,value|{
            if let Some(ui)=weak.upgrade(){queue(&m,&weak,Request::Velocity{track:ui.get_track_id().parse().unwrap_or(0),clip:ui.get_clip_id().parse().unwrap_or(0),note:note.parse().unwrap_or(0),value});}
        });
    }

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
        ui.on_down(move|x,y,right,extend|{
            let Some(ui)=weak.upgrade() else {return;}; if ui.get_clip_id()=="0" {ui.set_action_error("Create a MIDI clip first.".into());return;}
            let token=match m.borrow_mut().token(){Ok(t)=>t,Err(e)=>{ui.set_action_error(e.into());return;}};
            let viewport=Viewport{start_tick:(ui.get_scroll_beat() as f64*960.0).round() as u64,pixels_per_beat:ui.get_pixels_per_beat(),
                scroll_y:ui.get_piano_scroll_y(),row_height:ui.get_piano_row_height(),width:ui.get_canvas_width(),height:ui.get_canvas_height(),octave_offset:(ui.get_notation_index()-1) as i8};
            queue(&m,&weak,Request::Down{track:ui.get_track_id().parse().unwrap_or(0),clip:ui.get_clip_id().parse().unwrap_or(0),token,viewport,x,y,right,extend});gesture.set(Some(token));
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
    let pad_held=Rc::new(RefCell::new(HashMap::new()));
    {let m=mailbox.clone();let weak=ui.as_weak();let held=pad_held.clone();ui.on_drum_pad_gate(move|id,on|{
        let pad=id.parse().unwrap_or(0);if let Some(token)=held.borrow_mut().remove(&pad){queue(&m,&weak,Request::AuditionOff(token));}
        if on{let Some(ui)=weak.upgrade()else{return;};let result=m.borrow_mut().pad_on(ui.get_track_id().parse().unwrap_or(0),ui.get_instance_id().parse().unwrap_or(0),pad,ui.get_pad_preview_velocity().clamp(1,127) as u8);
            match result{Ok(token)=>{held.borrow_mut().insert(pad,token);},Err(e)=>ui.set_action_error(e.into())}
        }
    });}
    {let m=mailbox.clone();let held=held.clone();ui.on_release_keyboard(move||{if held.take().is_some(){m.borrow_mut().release_audition();}});}
    {let m=mailbox;ui.on_release_audition(move||{held.set(None);pad_held.borrow_mut().clear();m.borrow_mut().release_audition();});}
}
