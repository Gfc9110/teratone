use opendaw_app::{controller::{self, Request, Response, QUIT}, mailbox::Mailbox};
use slint::ComponentHandle;
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::atomic::Ordering, time::Duration};
slint::include_modules!();
mod ui_support;
fn send(mailbox: &RefCell<Mailbox>, weak: &slint::Weak<AppWindow>, request: Request) {
    let background = matches!(request, Request::Viewport(..) | Request::ArrangementViewport(..) | Request::Move{..} | Request::ScrollPiano(..));
    let result = mailbox.borrow_mut().enqueue(request);
    if let Some(ui) = weak.upgrade() {
        if let Err(error) = result { ui.set_action_error(error.into()); }
        else if !background { ui.set_action_error("".into()); }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut silent = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() { "--silent" => silent = true, _ => return Err(format!("Unknown option: {arg}. Use --silent for an explicit no-audio clock.").into()) }
    }
    let ui = AppWindow::new()?;
    let connection = controller::spawn(silent);
    let mailbox = Rc::new(RefCell::new(Mailbox::new(connection.sender.clone(), connection.flags.clone(), connection.epoch.clone(),connection.audition_epoch.clone())));
    macro_rules! bind {
        ($method:ident, ($($arg:ident),*) => $request:expr) => {{
            let mailbox = mailbox.clone(); let weak = ui.as_weak();
            ui.$method(move |$($arg),*| send(&mailbox, &weak, $request));
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
    bind!(on_reorder_track, (track, index) => Request::ReorderTrack { track: track.parse().unwrap_or(0), index });
    bind!(on_rename_track, (track, name) => Request::RenameTrack { track: track.parse().unwrap_or(0), name: name.into() });
    bind!(on_color_track, (track, index) => Request::ColorTrack { track: track.parse().unwrap_or(0),
        color: opendaw_app::project::TRACK_COLORS.get(index as usize).unwrap_or(&"").to_string() });
    bind!(on_toggle_mute, (track) => Request::ToggleMute(track.parse().unwrap_or(0)));
    bind!(on_toggle_solo, (track) => Request::ToggleSolo(track.parse().unwrap_or(0)));
    bind!(on_set_gain, (track, value) => Request::SetGain { track: track.parse().unwrap_or(0), value });
    bind!(on_set_pan, (track, value) => Request::SetPan { track: track.parse().unwrap_or(0), value });
    bind!(on_finish_mix, () => Request::FinishMix);
    bind!(on_tempo, (value) => Request::Tempo(value as f64));
    bind!(on_undo, () => Request::Undo);
    bind!(on_redo, () => Request::Redo);
    bind!(on_zoom, (factor) => Request::Zoom(factor));
    bind!(on_pan_view, (beats) => Request::Pan(beats));
    bind!(on_pitch_view, (notes) => Request::Pitch(notes));
    bind!(on_seek, (beat) => Request::Seek(beat as f64));
    bind!(on_seek_project, (beat) => Request::SeekProject(beat as f64));
    bind!(on_save_project, (path) => Request::Save(PathBuf::from(path.as_str())));
    bind!(on_open_project, (path) => Request::Open(PathBuf::from(path.as_str())));
    bind!(on_export_audio, (path) => Request::Export(PathBuf::from(path.as_str())));
    bind!(on_import_sample, (track, path, root, one_shot) => Request::Import { track: track.parse().unwrap_or(0), path: PathBuf::from(path.as_str()), root_note: root.clamp(0, 127) as u8, one_shot });
    bind!(on_use_sine, (track) => Request::UseSine(track.parse().unwrap_or(0)));
    ui_support::bind_editor(&ui,mailbox.clone());
    { let mailbox = mailbox.clone(); ui.on_stop(move || mailbox.borrow_mut().stop()); }
    {
        let weak = ui.as_weak(); let mailbox = mailbox.clone();
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                if ui.get_export_busy() { ui.set_status("An export is running. Close after it finishes.".into()); }
                else {ui.invoke_cancel_interactions(); mailbox.borrow_mut().stop(); ui.set_confirm_close(true); }
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
    let weak = ui.as_weak(); let receiver = connection.receiver;
    let mut presenter = ui_support::Presenter::new(&ui);
    let mut last_size = (0, 0); let mut last_arrangement = (0.0, 0.0, 0.0); let mut latest_revision = 0;
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(ui) = weak.upgrade() else { return; };
        ui.invoke_tick_drag(); ui.invoke_clamp_scroll();
        let size = (ui.get_canvas_width() as i32, ui.get_canvas_height() as i32);
        if size != last_size && size.0 > 0 && size.1 > 0 {
            send(&mailbox, &weak, Request::Viewport(size.0 as f32, size.1 as f32)); last_size = size;
        }
        let arrangement = (-ui.get_arrangement_x() / ui.get_arrangement_zoom(), ui.get_arrangement_zoom(), ui.get_lane_width());
        if arrangement != last_arrangement && arrangement.2 > 0.0 {
            send(&mailbox, &weak, Request::ArrangementViewport(arrangement.0, arrangement.1, arrangement.2)); last_arrangement = arrangement;
        }
        if let Err(error) = mailbox.borrow_mut().flush() { ui.set_action_error(error.into()); }
        for _ in 0..64 {
            let Ok(response) = receiver.try_recv() else { break; };
            match response {
                Response::View(view) => { if view.revision >= latest_revision { presenter.apply(&ui, *view); } }
                Response::Ack { request_id, revision, error, canceled: _ } => {
                    mailbox.borrow_mut().acknowledge(request_id); latest_revision = latest_revision.max(revision);
                    if let Some(error) = error { ui.set_action_error(error.into()); }
                }
                Response::Meter(t) => {
                    let beat = t.position as f64 / 48000.0 * ui.get_bpm() as f64 / 60.0;
                    ui.set_project_position(beat as f32);
                    ui.set_transport_text(format!("{:03} : {:02}", beat as u64 / 4 + 1, beat as u64 % 4 + 1).into());
                    ui.set_playing(t.playing != 0); ui.set_looping(t.looping != 0);
                    ui.set_peak_left(t.peak_left.clamp(0.0, 1.0)); ui.set_peak_right(t.peak_right.clamp(0.0, 1.0));
                    if t.device_error != 0 { ui.set_action_error(format!("Audio device error {}. Stop, then retry Play.", t.device_error).into()); }
                }
                Response::DrumMeter{track,instance,status}=>presenter.drum_meter(&ui,track,instance,status),
                Response::Fatal(error) => ui.set_action_error(error.into()),
            }
        }
        presenter.tick_drums(&ui);
        ui.set_pending_actions(mailbox.borrow().pending_count() as i32);
    });
    let result = ui.run();
    timer.stop(); connection.flags.fetch_or(QUIT, Ordering::Release);
    let _ = connection.worker.join(); result?; Ok(())
}
