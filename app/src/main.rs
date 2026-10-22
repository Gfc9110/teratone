use opendaw_app::controller::{self, Request, Response, PANIC, STOP, QUIT};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicU32, Ordering}, mpsc::{SyncSender, TrySendError}}, time::Duration};
slint::include_modules!();
fn send(sender: &SyncSender<Request>, flags: &Arc<AtomicU32>, weak: &slint::Weak<AppWindow>, request: Request) {
    if let Err(error) = sender.try_send(request) {
        flags.fetch_or(PANIC, Ordering::Release);
        if let Some(ui) = weak.upgrade() {
            ui.set_status(match error {
                TrySendError::Full(_) => "Controller queue full. Pending actions canceled to avoid stuck notes.".into(),
                TrySendError::Disconnected(_) => "Application controller stopped. Close and inspect the error.".into(),
            });
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut silent = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() { "--silent" => silent = true, _ => return Err(format!("Unknown option: {arg}. Use --silent for an explicit no-audio clock.").into()) }
    }
    let ui = AppWindow::new()?;
    let connection = controller::spawn(silent);
    macro_rules! bind {
        ($method:ident, ($($arg:ident),*) => $request:expr) => {{
            let sender = connection.sender.clone(); let flags = connection.flags.clone(); let weak = ui.as_weak();
            ui.$method(move |$($arg),*| send(&sender, &flags, &weak, $request));
        }};
    }
    bind!(on_play, () => Request::Play);
    bind!(on_pause, () => Request::Pause);
    bind!(on_toggle_loop, () => Request::ToggleLoop);
    bind!(on_select_track, (i) => Request::SelectTrack(i.parse().unwrap_or(0)));
    bind!(on_select_clip, (track, clip) => Request::SelectClip { track: track.parse().unwrap_or(0), clip: clip.parse().unwrap_or(0) });
    bind!(on_add_track, (track) => Request::AddTrack { after: track.parse().unwrap_or(0) });
    bind!(on_duplicate_track, (track) => Request::DuplicateTrack(track.parse().unwrap_or(0)));
    bind!(on_remove_track, (track) => Request::RemoveTrack(track.parse().unwrap_or(0)));
    bind!(on_move_track, (track, direction) => Request::MoveTrack { track: track.parse().unwrap_or(0), direction });
    bind!(on_rename_track, (track, name) => Request::RenameTrack { track: track.parse().unwrap_or(0), name: name.into() });
    bind!(on_color_track, (track, index) => Request::ColorTrack { track: track.parse().unwrap_or(0),
        color: opendaw_app::project::TRACK_COLORS.get(index as usize).unwrap_or(&"").to_string() });
    bind!(on_finish_mix, () => Request::FinishMix);
    bind!(on_tempo, (value) => Request::Tempo(value as f64));
    bind!(on_mix, (track, gain, pan) => Request::Mix { track: track.parse().unwrap_or(0), gain, pan });
    bind!(on_undo, () => Request::Undo);
    bind!(on_redo, () => Request::Redo);
    bind!(on_remove_note, () => Request::Delete);
    bind!(on_zoom, (factor) => Request::Zoom(factor));
    bind!(on_pan_view, (beats) => Request::Pan(beats));
    bind!(on_pitch_view, (notes) => Request::Pitch(notes));
    bind!(on_seek, (beat) => Request::Seek(beat as f64));
    bind!(on_down, (x, y, right) => Request::Down(x, y, right));
    bind!(on_moved, (x, y) => Request::Move(x, y));
    bind!(on_up, (x, y) => Request::Up(x, y));
    bind!(on_audition, (track, pitch, on) => Request::Audition { track: track.parse().unwrap_or(0), pitch: pitch.clamp(0, 127) as u8, on });
    bind!(on_save_project, (path) => Request::Save(PathBuf::from(path.as_str())));
    bind!(on_open_project, (path) => Request::Open(PathBuf::from(path.as_str())));
    bind!(on_export_audio, (path) => Request::Export(PathBuf::from(path.as_str())));
    bind!(on_import_sample, (track, path, root, one_shot) => Request::Import { track: track.parse().unwrap_or(0), path: PathBuf::from(path.as_str()), root_note: root.clamp(0, 127) as u8, one_shot });
    bind!(on_use_sine, (track) => Request::UseSine(track.parse().unwrap_or(0)));
    {
        let flags = connection.flags.clone(); ui.on_stop(move || { flags.fetch_or(STOP, Ordering::Release); });
    }
    {
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                if ui.get_export_busy() { ui.set_status("An export is running. Close after it finishes.".into()); }
                else { ui.set_confirm_close(true); }
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_exit_confirmed(move || {
            if weak.upgrade().is_some_and(|ui| !ui.get_export_busy()) { let _ = slint::quit_event_loop(); }
        });
    }
    let timer = slint::Timer::default();
    let weak = ui.as_weak(); let requests = connection.sender.clone(); let flags = connection.flags.clone();
    let receiver = connection.receiver;
    let mut last_size = (0, 0);
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(ui) = weak.upgrade() else { return; };
        let size = (ui.get_canvas_width() as i32, ui.get_canvas_height() as i32);
        if size != last_size && size.0 > 0 && size.1 > 0 {
            send(&requests, &flags, &weak, Request::Viewport(size.0 as f32, size.1 as f32)); last_size = size;
        }
        for _ in 0..16 {
            let Ok(response) = receiver.try_recv() else { break; };
            match response {
                Response::View(view) => {
                    ui.set_project_name(view.name.into()); ui.set_status(view.status.into()); ui.set_selected_label(view.selected_label.into());
                    let tracks: Vec<TrackVisual> = view.tracks.into_iter().map(|t| TrackVisual {
                        id: t.id.to_string().into(), name: t.name.into(), instrument: t.instrument.into(), selected: t.selected,
                        tint: slint::Color::from_argb_encoded(0xff000000 | u32::from_str_radix(&t.color[1..], 16).unwrap_or(0x62bfa6)),
                    }).collect();
                    ui.set_track_items(ModelRc::new(VecModel::from(tracks))); ui.set_track_index(view.track_index as i32);
                    ui.set_track_id(view.track_id.to_string().into()); ui.set_track_name(view.track_name.into());
                    ui.set_color_index(opendaw_app::project::TRACK_COLORS.iter().position(|c| *c == view.track_color).map_or(-1, |i| i as i32));
                    let clip_names: Vec<SharedString> = view.clips.iter().map(|c| c.name.clone().into()).collect();
                    let clip_ids: Vec<SharedString> = view.clips.iter().map(|c| c.id.to_string().into()).collect();
                    ui.set_clip_names(ModelRc::new(VecModel::from(clip_names))); ui.set_clip_ids(ModelRc::new(VecModel::from(clip_ids)));
                    ui.set_clip_index(view.clip_index.map_or(-1, |i| i as i32)); ui.set_clip_start_beat(view.clip_start as f32 / 960.0);
                    ui.set_bpm(view.bpm.round() as i32); ui.set_gain(view.gain); ui.set_pan(view.pan);
                    ui.set_dirty(view.dirty); ui.set_can_undo(view.can_undo); ui.set_can_redo(view.can_redo); ui.set_export_busy(view.export_busy);
                    ui.set_pixels_per_beat(view.viewport.pixels_per_beat); ui.set_scroll_beat(view.viewport.start_tick as f32 / 960.0);
                    ui.set_top_note(view.viewport.top_note as i32);
                    let notes: Vec<NoteVisual> = view.notes.into_iter().map(|n| NoteVisual {
                        x: n.x, y: n.y, w: n.width, h: n.height, selected: n.selected, label: n.label.into(),
                    }).collect();
                    ui.set_note_items(ModelRc::new(VecModel::from(notes)));
                }
                Response::Meter(t) => {
                    let beat = t.position as f64 / 48000.0 * ui.get_bpm() as f64 / 60.0;
                    ui.set_playhead_x((beat as f32 - ui.get_clip_start_beat() - ui.get_scroll_beat()) * ui.get_pixels_per_beat());
                    ui.set_transport_text(format!("{:03} : {:02}", beat as u64 / 4 + 1, beat as u64 % 4 + 1).into());
                    ui.set_playing(t.playing != 0); ui.set_looping(t.looping != 0);
                    ui.set_peak_left(t.peak_left.clamp(0.0, 1.0)); ui.set_peak_right(t.peak_right.clamp(0.0, 1.0));
                    if t.device_error != 0 { ui.set_status(format!("Audio device error {}. Stop, then retry Play.", t.device_error).into()); }
                }
                Response::Fatal(error) => ui.set_status(error.into()),
            }
        }
    });
    let result = ui.run();
    timer.stop(); connection.flags.fetch_or(QUIT, Ordering::Release);
    let _ = connection.worker.join(); result?; Ok(())
}
