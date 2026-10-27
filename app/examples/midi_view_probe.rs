//! Software UI probe for non-destructive MIDI view modes.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent, PointerEventButton};
use slint::{ComponentHandle,Model};
use opendaw_app::{controller::{self, Request, Response, ViewState, QUIT}, mailbox::Mailbox};
use std::{cell::RefCell, rc::Rc, sync::atomic::Ordering, time::{Duration, Instant}};
slint::include_modules!();
#[path = "../src/ui_support.rs"] mod ui_support;
struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> { Ok(self.0.clone()) }
}
fn click(window: &MinimalSoftwareWindow, x: f32, y: f32, button: PointerEventButton) {
    let position = slint::LogicalPosition::new(x, y);
    window.dispatch_event(WindowEvent::PointerPressed { position, button });
    window.dispatch_event(WindowEvent::PointerReleased { position, button });
}
fn settle(ui: &AppWindow, mailbox: &RefCell<Mailbox>, connection: &controller::Connection, presenter: &mut ui_support::Presenter, last: &mut Option<ViewState>) {
    mailbox.borrow_mut().enqueue(Request::Viewport(ui.get_canvas_width(), ui.get_canvas_height())).unwrap();
    mailbox.borrow_mut().enqueue(Request::ArrangementViewport(-ui.get_arrangement_x()/ui.get_arrangement_zoom(), ui.get_arrangement_zoom(), ui.get_lane_width())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4); let mut quiet = 0;
    let mut last_arrangement=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());
    while Instant::now() < deadline {
        let arrangement=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());
        if arrangement!=last_arrangement {mailbox.borrow_mut().enqueue(Request::ArrangementViewport(arrangement.0,arrangement.1,arrangement.2)).unwrap();last_arrangement=arrangement;quiet=0;}
        mailbox.borrow_mut().flush().unwrap();
        while let Ok(response) = connection.receiver.try_recv() {
            match response {
                Response::View(v) => { presenter.apply(ui, (*v).clone()); *last = Some(*v); }
                Response::Ack { request_id, error, .. } => { assert!(error.is_none(), "{error:?}"); mailbox.borrow_mut().acknowledge(request_id); }
                Response::DrumMeter{track,instance,status}=>presenter.drum_meter(ui,track,instance,status),
                Response::Fatal(e) => panic!("{e}"), _ => {},
            }
        }
        presenter.tick_drums(ui);
        if mailbox.borrow().pending_count() == 0 && last.is_some() { quiet += 1; if quiet == 8 { return; } } else { quiet = 0; }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Controller did not settle");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use opendaw_app::project::PPQ;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui=AppWindow::new()?;
    let connection=controller::spawn(true);
    let mailbox=Rc::new(RefCell::new(Mailbox::new(connection.sender.clone(),connection.flags.clone(),connection.epoch.clone(),connection.audition_epoch.clone())));
    { let m=mailbox.clone();ui.on_undo(move || m.borrow_mut().enqueue(Request::Undo).unwrap()); }
    { let m=mailbox.clone();ui.on_select_track(move |id| m.borrow_mut().enqueue(Request::SelectTrack(id.parse().unwrap())).unwrap()); }
    ui_support::bind_editor(&ui,mailbox.clone());
    ui.show()?;window.set_size(slint::PhysicalSize::new(1280,820));
    let mut presenter=ui_support::Presenter::new(&ui);let mut last=None;
    macro_rules! pump {()=>{settle(&ui,&mailbox,&connection,&mut presenter,&mut last)}}
    pump!();let before=last.as_ref().unwrap().revision;let old_x=last.as_ref().unwrap().viewport.start_tick;
    let old_scale=last.as_ref().unwrap().viewport.pixels_per_beat;
    ui.invoke_set_midi_view_mode(1);pump!();
    let v=last.as_ref().unwrap();assert_eq!(v.midi_view_mode,1);assert_eq!(v.piano_row_count,5);
    assert_eq!(v.revision,before,"Changing view must not edit the project");
    assert_eq!(v.viewport.start_tick,old_x);assert_eq!(v.viewport.pixels_per_beat,old_scale);
    assert_eq!(ui.get_piano_row_count(),5);assert_eq!(v.notes.len(),8);
    ui.invoke_toggle_reference("2".into(),"6".into());pump!();
    let v=last.as_ref().unwrap();assert_eq!(v.piano_row_count,7,"Used view must include visible ghost pitches");
    assert!(!v.ghosts.is_empty(),"Reference notes must remain visible in Used view");
    assert_eq!(v.revision,before,"Reference visibility must not alter the project");
    ui.invoke_toggle_reference_pin("2".into(),"6".into());pump!();
    ui.invoke_follow_selection_changed(false);pump!();
    ui.invoke_select_track("2".into());pump!();
    let v=last.as_ref().unwrap();assert!(!v.follow_selection);assert_eq!(v.track_id,1);assert_eq!(v.clip_id,4);
    assert_eq!(v.inspector_track.id,2,"Locked editor must allow selecting another arrangement track");
    // A click on a ghost may insert into the active clip, but must never
    // alter the source reference clip, even while the arrangement selects it.
    let g=&v.ghosts[0];let x=ui.get_grid_x()+g.x+g.width.min(8.0)/2.0;let y=ui.get_grid_y()+g.y+g.height/2.0;
    click(&window,x,y,PointerEventButton::Left);pump!();
    assert_eq!(last.as_ref().unwrap().track_id,1);assert_eq!(last.as_ref().unwrap().notes.len(),9);
    let path=std::env::temp_dir().join(format!("opendaw-midi-view-probe-{}",std::process::id()));std::fs::create_dir_all(&path)?;
    mailbox.borrow_mut().enqueue(Request::Save(path.clone())).unwrap();pump!();
    let saved=opendaw_app::storage::load(&path)?;assert_eq!(saved.clip(2,6),opendaw_app::project::Project::demo().clip(2,6),"Ghost source must be unchanged");
    ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().notes.len(),8);
    ui.invoke_activate_editor_clip("2".into(),"6".into());pump!();
    assert_eq!(last.as_ref().unwrap().track_id,2);assert_eq!(last.as_ref().unwrap().notes.len(),2);
    ui.invoke_activate_editor_clip("1".into(),"4".into());pump!();
    let v=last.as_ref().unwrap();assert!(!v.ghosts.is_empty());assert!(v.reference_rows.iter().any(|r|r.clip==6 && r.pinned && r.visible),"Pinned reference survives clip changes");
    ui.invoke_follow_selection_changed(true);pump!();
    ui.invoke_toggle_reference_pin("2".into(),"6".into());pump!();
    ui.invoke_toggle_reference("2".into(),"6".into());pump!();
    assert_eq!(last.as_ref().unwrap().piano_row_count,5);
    std::fs::remove_dir_all(path)?;
    let before_scale=last.as_ref().unwrap().revision;ui.invoke_set_scale(1,0);pump!();
    let v=last.as_ref().unwrap();assert!(v.keys.iter().any(|k|k.pitch==60 && k.in_scale));
    assert_eq!(v.revision,before_scale,"Highlighting a scale must not edit notes");
    ui.invoke_set_snap(3);pump!();assert_eq!(last.as_ref().unwrap().snap_index,3);
    // Insert into a visible row on an empty beat, using the real grid pointer adapter.
    let v=last.as_ref().unwrap();let key=v.keys.iter().find(|k|k.pitch==72).unwrap();
    let expected_tick=PPQ+PPQ/8;let x=v.viewport.x(expected_tick+10);let y=key.y+key.height/2.0;
    assert!(x>0.0 && x<v.viewport.width && y>0.0 && y<v.viewport.height);
    click(&window,ui.get_grid_x()+x,ui.get_grid_y()+y,PointerEventButton::Left);pump!();
    let v=last.as_ref().unwrap();assert_eq!(v.notes.len(),9);let inserted=v.notes.iter().find(|n|n.id==v.selected_note).unwrap();
    assert!((inserted.x-v.viewport.x(expected_tick)).abs()<1.0,"Snap must determine note start");
    ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().notes.len(),8);
    ui.invoke_set_midi_view_mode(0);pump!();assert_eq!(last.as_ref().unwrap().piano_row_count,128);
    assert_eq!(last.as_ref().unwrap().notes.len(),8,"Returning to Piano must preserve the score");
    // Drum rows come from the real kit; a click inserts its actual MIDI pitch.
    mailbox.borrow_mut().enqueue(Request::AddDrumTrack{after:1}).unwrap();pump!();
    assert!(last.as_ref().unwrap().drum_kit.is_some());ui.invoke_set_midi_view_mode(2);pump!();
    let v=last.as_ref().unwrap();assert_eq!(v.midi_view_mode,2);let kit=v.drum_kit.as_ref().unwrap();
    assert_eq!(v.piano_row_count,kit.pads.len());
    let key=v.keys.iter().find(|key|kit.pads.iter().any(|pad|pad.midi_note==key.pitch)).unwrap();
    assert_eq!(key.label,kit.note_label(key.pitch));let pitch=key.pitch;let y=key.y+key.height/2.0;
    click(&window,ui.get_grid_x()+16.0,ui.get_grid_y()+y,PointerEventButton::Left);pump!();
    let v=last.as_ref().unwrap();let n=v.notes.iter().find(|n|n.id==v.selected_note).unwrap();
    assert_eq!(n.label,v.drum_kit.as_ref().unwrap().note_label(pitch));
    ui.invoke_undo();pump!();assert!(last.as_ref().unwrap().notes.is_empty());
    mailbox.borrow_mut().enqueue(Request::SelectTrack(1)).unwrap();pump!();
    assert_eq!(last.as_ref().unwrap().midi_view_mode,0,"Drum view cannot leak into an unmapped instrument");
    // Exact coincident notes are drawn once but remain independently editable.
    let mut duplicate_project=opendaw_app::project::Project::demo();
    let mut duplicate=duplicate_project.tracks[0].clips[0].notes[0].clone();duplicate.id=duplicate_project.next_id;duplicate_project.next_id+=1;
    let duplicate_id=duplicate.id;duplicate_project.tracks[0].clips[0].notes.insert(1,duplicate);
    let path=std::env::temp_dir().join(format!("opendaw-overlap-probe-{}",std::process::id()));std::fs::create_dir_all(&path)?;
    opendaw_app::storage::save(&path,&duplicate_project)?;
    mailbox.borrow_mut().enqueue(Request::SwitchProject{target:controller::ProjectTarget::Existing(path.clone()),unsaved:controller::UnsavedChanges::Discard}).unwrap();pump!();
    assert_eq!(last.as_ref().unwrap().notes.len(),9);assert_eq!(ui.get_note_items().row_count(),8,"Identical rectangles are drawn once");
    let n=last.as_ref().unwrap().notes.iter().find(|n|n.id==duplicate_id).unwrap();
    click(&window,ui.get_grid_x()+n.x+4.0,ui.get_grid_y()+n.y+n.height/2.0,PointerEventButton::Left);pump!();
    assert_eq!(last.as_ref().unwrap().selected_note,duplicate_id,"Hit testing must still choose the topmost original note");
    assert_eq!(ui.get_note_items().row_count(),9,"Selected overlapping note remains visibly distinct");
    ui.invoke_remove_note();pump!();assert_eq!(last.as_ref().unwrap().notes.len(),8);
    assert!(last.as_ref().unwrap().notes.iter().any(|n|n.id==10),"Deleting the top note must preserve the original");
    std::fs::remove_dir_all(path)?;
    println!("PASS: Follow/Lock, pinned ghosts and source safety; Used/Piano, scale, snap, pointer insertion, Undo, real Drum mapping and coincident-note editing");
    connection.flags.fetch_or(QUIT,Ordering::Release);connection.worker.join().unwrap();Ok(())
}
