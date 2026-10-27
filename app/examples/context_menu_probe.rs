//! Exercise contextual actions on objects other than the selected track.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent, PointerEventButton};
use slint::ComponentHandle;
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
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui=AppWindow::new()?;
    let connection=controller::spawn(true);
    let mailbox=Rc::new(RefCell::new(Mailbox::new(connection.sender.clone(),connection.flags.clone(),connection.epoch.clone(),connection.audition_epoch.clone())));
    { let m=mailbox.clone(); ui.on_toggle_mute(move |id| m.borrow_mut().enqueue(Request::ToggleMute(id.parse().unwrap())).unwrap()); }
    ui_support::bind_editor(&ui,mailbox.clone());
    ui.show()?; window.set_size(slint::PhysicalSize::new(1280,820));
    let mut presenter=ui_support::Presenter::new(&ui); let mut last=None;
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let x=ui.get_rows_x(); let y=ui.get_rows_y()+ui.get_row_size();
    click(&window,x+40.0,y+15.0,PointerEventButton::Right);
    click(&window,x+80.0,y+28.0+4.0+2.5*27.6,PointerEventButton::Left);
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(!last.as_ref().unwrap().tracks[0].mute && last.as_ref().unwrap().tracks[1].mute, "Track menu must mute the clicked, non-selected track");
    let cx=ui.get_rows_x()+ui.get_header_size()+100.0; let cy=ui.get_rows_y()+ui.get_row_size()+18.0;
    click(&window,cx,cy,PointerEventButton::Right);
    click(&window,cx+40.0,cy+44.0,PointerEventButton::Left);
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(last.as_ref().unwrap().track_id,2,"Clip menu must target clicked track");
    assert_eq!(last.as_ref().unwrap().clips.len(),2,"Clip menu must duplicate clicked clip");
    // The first track has only the original 16-beat clip. Create at beat 18
    // while the second track remains selected from the preceding operation.
    let empty_x=ui.get_rows_x()+ui.get_header_size()+18.0*ui.get_arrangement_zoom()+ui.get_arrangement_x()+3.0;
    let empty_y=ui.get_rows_y()+18.0;
    assert!(empty_x>ui.get_rows_x()+ui.get_header_size() && empty_x<1280.0);
    click(&window,empty_x,empty_y,PointerEventButton::Right);
    click(&window,empty_x+40.0,empty_y+18.0,PointerEventButton::Left);
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(last.as_ref().unwrap().track_id,1,"Empty timeline menu must target clicked track");
    assert_eq!(last.as_ref().unwrap().clip_start,18*960,"New clip must use the clicked beat");
    mailbox.borrow_mut().enqueue(Request::SelectClip{track:1,clip:4}).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let note=&last.as_ref().unwrap().notes[0];
    let note_id=note.id; let nx=ui.get_grid_x()+note.x+note.width/2.0; let ny=ui.get_grid_y()+note.y+note.height/2.0;
    click(&window,nx,ny,PointerEventButton::Right);
    click(&window,nx+40.0,ny+18.0,PointerEventButton::Left);
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(last.as_ref().unwrap().selected_note,note_id,"Right click must select the note beneath it");
    assert!(ui.get_inspector_visible() && ui.get_inspector_detail()==2,"Note inspector must open from MIDI menu");
    println!("PASS: contextual track, clip, timeline and MIDI actions target the clicked objects");
    connection.flags.fetch_or(QUIT,Ordering::Release); connection.worker.join().unwrap(); Ok(())
}
