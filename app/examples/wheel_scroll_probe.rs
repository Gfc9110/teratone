//! High-resolution wheel interaction through the real controller and UI adapter.
use opendaw_app::{controller::{self,Request,Response,ViewState,QUIT},mailbox::Mailbox};
use slint::{platform::{Platform,PlatformError,WindowAdapter,WindowEvent,PointerEventButton,Key},platform::software_renderer::{MinimalSoftwareWindow,RepaintBufferType},ComponentHandle,SharedPixelBuffer,Model};
use std::{cell::RefCell,rc::Rc,sync::atomic::Ordering,time::{Duration,Instant}};
slint::include_modules!();
#[path="../src/ui_support.rs"] mod ui_support;
struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl Platform for TestPlatform {fn create_window_adapter(&self)->Result<Rc<dyn WindowAdapter>,PlatformError>{Ok(self.0.clone())}}
struct Timer {size:(i32,i32),arr:(f32,f32,f32),scroll:f32,session:u64,revision:u64,piano:Option<bool>,velocity:Option<bool>,last:Option<ViewState>}
impl Default for Timer {fn default()->Self{Self{size:(0,0),arr:(0.,0.,0.),scroll:0.,session:0,revision:0,piano:None,velocity:None,last:None}}}
fn tick(ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,t:&mut Timer){
    let size=(ui.get_canvas_width() as i32,ui.get_canvas_height() as i32);
    if size!=t.size && size.0>0 && size.1>0 {m.borrow_mut().enqueue(Request::ViewportForSession(t.session,size.0 as f32,size.1 as f32)).unwrap();t.size=size;}
    let arr=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());
    if arr!=t.arr && arr.2>0. {m.borrow_mut().enqueue(Request::ArrangementViewportForSession(t.session,arr.0,arr.1,arr.2)).unwrap();t.arr=arr;}
    let y=ui.get_arrangement_y();if y!=t.scroll {m.borrow_mut().enqueue(Request::ArrangementScrollForSession(t.session,y)).unwrap();t.scroll=y;}
    let piano=ui.get_workspace_focus()!=1;let velocity=ui.get_velocity_visible() && piano;
    if t.piano!=Some(piano){m.borrow_mut().enqueue(Request::SetPianoVisible(piano)).unwrap();t.piano=Some(piano);}
    if t.velocity!=Some(velocity){m.borrow_mut().enqueue(Request::SetVelocityVisible(velocity)).unwrap();t.velocity=Some(velocity);}
    m.borrow_mut().flush().unwrap();
    while let Ok(response)=c.receiver.try_recv(){match response{
        Response::View(v)=>{if v.revision>=t.revision {
            let switched=t.session!=v.project_session;if switched {t.session=v.project_session;t.size=(0,0);t.arr=(f32::NAN,f32::NAN,f32::NAN);t.scroll=f32::NAN;}
            let session=v.project_session;let snapshot=v.processed_request_id;
            let linked=v.navigation_linked;let (piano,arrangement,residual)={let mb=m.borrow();(mb.piano_scroll_unseen(session,v.track_id,v.clip_id,snapshot),mb.arrangement_view_unseen(session,snapshot),mb.piano_scroll_residual(session,v.track_id,v.clip_id,v.piano_navigation_request_id))};
            t.last=Some((*v).clone());p.apply_with_navigation(ui,*v,piano,arrangement,residual);if linked && !arrangement && !switched {t.arr=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());}m.borrow_mut().retire_navigation_views(session,snapshot);
        }},
        Response::Ack{request_id,revision,error,..}=>{m.borrow_mut().acknowledge(request_id);t.revision=t.revision.max(revision);assert!(error.is_none(),"{error:?}");},Response::Fatal(e)=>panic!("{e}"),_=>{}}}
}
fn settle(ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,t:&mut Timer){let end=Instant::now()+Duration::from_secs(6);let mut quiet=0;loop{tick(ui,m,c,p,t);if m.borrow().pending_count()==0 && t.last.is_some(){quiet+=1;if quiet>=12 {break}} else {quiet=0;}assert!(Instant::now()<end,"view did not settle");std::thread::sleep(Duration::from_millis(4));}}
fn key(window:&MinimalSoftwareWindow,key:Key,down:bool){let text=key.into();window.dispatch_event(if down{WindowEvent::KeyPressed{text}}else{WindowEvent::KeyReleased{text}});}
fn wheel(window:&MinimalSoftwareWindow,position:slint::LogicalPosition,dx:f32){window.dispatch_event(WindowEvent::PointerScrolled{position,delta_x:dx,delta_y:0.});}
fn c5(ui:&AppWindow)->NoteVisual {let notes=ui.get_note_items();(0..notes.row_count()).filter_map(|i|notes.row_data(i)).find(|n|n.label=="C5").expect("C5 must remain visible")}
fn note_x(ui:&AppWindow)->f32 {c5(ui).x+ui.get_piano_projection_x()}
fn ghost_x(ui:&AppWindow)->f32 {let m=ui.get_ghost_items();m.row_data(0).expect("reference must be visible").x+ui.get_piano_projection_x()}
fn velocity_x(ui:&AppWindow)->f32 {let m=ui.get_velocity_items();(0..m.row_count()).filter_map(|i|m.row_data(i)).find(|b|b.id=="13").expect("velocity for C5 must be visible").x+ui.get_piano_projection_x()}
fn piano_burst(w:&MinimalSoftwareWindow,ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,t:&mut Timer,shift:bool){
    let origin=ui.get_piano_bar_x();let initial_note=note_x(ui);let initial_ghost=ghost_x(ui);let initial_velocity=velocity_x(ui);
    let pos=slint::LogicalPosition::new(ui.get_grid_x()+100.,ui.get_grid_y()+100.);if shift {key(w,Key::Shift,true);}
    wheel(w,pos,-0.75);
    for (actual,initial,label) in [(note_x(ui),initial_note,"note"),(ghost_x(ui),initial_ghost,"ghost"),(velocity_x(ui),initial_velocity,"velocity")] {assert!((actual-(initial-0.75)).abs()<0.1,"{label} must move immediately");}
    assert!((ui.get_piano_display_beat()+ui.get_piano_bar_x()/ui.get_pixels_per_beat()).abs()<0.0001,"ruler follows local scrollbar");
    tick(ui,m,c,p,t);std::thread::sleep(Duration::from_millis(30)); // queue an old view
    wheel(w,pos,-0.75);let before=ui.get_piano_bar_x();tick(ui,m,c,p,t);let mut last=ui.get_piano_bar_x();assert!(last<=before+0.05,"old view must not reverse horizontal scroll");
    // Hit a projected note while newer scroll input is still in flight.
    let n=c5(ui);let note=slint::LogicalPosition::new(ui.get_grid_x()+n.x+ui.get_piano_projection_x()+4.,ui.get_grid_y()+n.y+ui.get_piano_projection_y()+n.h/2.);
    if shift {key(w,Key::Shift,false);}w.dispatch_event(WindowEvent::PointerPressed{position:note,button:PointerEventButton::Left});w.dispatch_event(WindowEvent::PointerReleased{position:note,button:PointerEventButton::Left});if shift {key(w,Key::Shift,true);}
    settle(ui,m,c,p,t);assert_eq!(ui.get_selected_note_id().as_str(),"13","click during pending scroll must hit the projected note at {note:?}, note ({}, {}) and projection ({}, {})",n.x,n.y,ui.get_piano_projection_x(),ui.get_piano_projection_y());last=ui.get_piano_bar_x();
    for _ in 0..78 {wheel(w,pos,-0.75);tick(ui,m,c,p,t);let now=ui.get_piano_bar_x();assert!(now<=last+0.05,"scroll reversed: {last} -> {now}");last=now;std::thread::sleep(Duration::from_millis(2));}
    if shift {key(w,Key::Shift,false);}settle(ui,m,c,p,t);let final_x=ui.get_piano_bar_x();
    assert!((final_x-(origin-60.)).abs()<0.3,"fractional wheel distance lost: {origin} -> {final_x}");
    assert!(ui.get_piano_projection_x().abs()<0.1,"projection must converge after the last view");
    for (actual,initial,label) in [(note_x(ui),initial_note,"note"),(ghost_x(ui),initial_ghost,"ghost"),(velocity_x(ui),initial_velocity,"velocity")] {assert!((actual-(initial+final_x-origin)).abs()<0.2,"{label} must converge to scrollbar");}
    let view=t.last.as_ref().unwrap();assert!((view.viewport.start_tick as f64/960.0+final_x as f64/view.viewport.pixels_per_beat as f64).abs()<0.001,"controller and view must converge");
    assert_eq!(ui.get_selected_note_id().as_str(),"13","click during pending scroll must hit the projected note");
}
fn timeline_burst(w:&MinimalSoftwareWindow,ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,t:&mut Timer,shift:bool,linked:bool){
    let origin=ui.get_arrangement_x();let pos=slint::LogicalPosition::new(ui.get_rows_x()+ui.get_header_size()+100.,ui.get_rows_y()+20.);if shift {key(w,Key::Shift,true);}
    wheel(w,pos,-0.75);tick(ui,m,c,p,t);std::thread::sleep(Duration::from_millis(30));wheel(w,pos,-0.75);let before=ui.get_arrangement_x();tick(ui,m,c,p,t);let mut last=ui.get_arrangement_x();assert!(last<=before+0.05,"old view must not reverse timeline");
    for _ in 0..78 {wheel(w,pos,-0.75);tick(ui,m,c,p,t);let now=ui.get_arrangement_x();assert!(now<=last+0.05,"timeline reversed: {last} -> {now}");last=now;std::thread::sleep(Duration::from_millis(2));}
    if shift {key(w,Key::Shift,false);}settle(ui,m,c,p,t);let final_x=ui.get_arrangement_x();assert!((final_x-(origin-60.)).abs()<0.3,"timeline distance lost: {origin} -> {final_x}");
    if linked {let start=-final_x/ui.get_arrangement_zoom();let piano=-ui.get_piano_bar_x()/ui.get_pixels_per_beat()+ui.get_clip_start_beat();assert!((start-piano).abs()<0.02,"linked views must converge: {start} vs {piano}");}
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let w=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);slint::platform::set_platform(Box::new(TestPlatform(w.clone())))?;let ui=AppWindow::new()?;ui.show()?;w.set_size(slint::PhysicalSize::new(1280,820));let mut pixels=SharedPixelBuffer::<slint::Rgb8Pixel>::new(1280,820);w.draw_if_needed(|r|{r.render(pixels.make_mut_slice(),1280);});
    let c=controller::spawn(true);let m=Rc::new(RefCell::new(Mailbox::new(c.sender.clone(),c.flags.clone(),c.epoch.clone(),c.audition_epoch.clone())));ui_support::bind_editor(&ui,m.clone());let mut p=ui_support::Presenter::new(&ui);let mut t=Timer::default();settle(&ui,&m,&c,&mut p,&mut t);
    ui.set_velocity_visible(true);m.borrow_mut().enqueue(Request::Zoom(2.0)).unwrap();m.borrow_mut().enqueue(Request::ToggleReference{track:2,clip:6}).unwrap();settle(&ui,&m,&c,&mut p,&mut t);let row=t.last.as_ref().unwrap().viewport.row_height;m.borrow_mut().enqueue(Request::ZoomY(12.0/row)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);
    piano_burst(&w,&ui,&m,&c,&mut p,&mut t,false);
    let n=c5(&ui);let position=slint::LogicalPosition::new(ui.get_grid_x()+n.x+ui.get_piano_projection_x()+4.,ui.get_grid_y()+n.y+ui.get_piano_projection_y()+n.h/2.);
    w.dispatch_event(WindowEvent::PointerPressed{position,button:PointerEventButton::Left});w.dispatch_event(WindowEvent::PointerReleased{position,button:PointerEventButton::Left});settle(&ui,&m,&c,&mut p,&mut t);assert_eq!(ui.get_selected_note_id().as_str(),"13","hit testing must match the projected note");
    ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);w.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:2.});w.set_size(slint::PhysicalSize::new(2560,1640));settle(&ui,&m,&c,&mut p,&mut t);piano_burst(&w,&ui,&m,&c,&mut p,&mut t,true);
    w.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.});w.set_size(slint::PhysicalSize::new(1280,820));settle(&ui,&m,&c,&mut p,&mut t);
    ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);
    let pan_pos=slint::LogicalPosition::new(ui.get_grid_x()+100.,ui.get_grid_y()+100.);wheel(&w,pan_pos,-0.75);tick(&ui,&m,&c,&mut p,&mut t);std::thread::sleep(Duration::from_millis(30));
    w.dispatch_event(WindowEvent::PointerPressed{position:pan_pos,button:PointerEventButton::Middle});let moved=slint::LogicalPosition::new(pan_pos.x-15.,pan_pos.y+8.);
    w.dispatch_event(WindowEvent::PointerMoved{position:moved});w.dispatch_event(WindowEvent::PointerReleased{position:moved,button:PointerEventButton::Middle});let desired=ui.get_piano_bar_x();tick(&ui,&m,&c,&mut p,&mut t);assert!((ui.get_piano_bar_x()-desired).abs()<0.1,"older scroll view must not undo middle-button pan");settle(&ui,&m,&c,&mut p,&mut t);assert!((ui.get_piano_bar_x()+15.75).abs()<0.3,"middle pan after wheel must converge");
    ui.set_arrangement_zoom(128.);ui.set_arrangement_x(0.);settle(&ui,&m,&c,&mut p,&mut t);timeline_burst(&w,&ui,&m,&c,&mut p,&mut t,false,false);
    ui.set_arrangement_x(0.);settle(&ui,&m,&c,&mut p,&mut t);timeline_burst(&w,&ui,&m,&c,&mut p,&mut t,true,false);
    ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);m.borrow_mut().enqueue(Request::SetNavigationLinked(true)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);timeline_burst(&w,&ui,&m,&c,&mut p,&mut t,false,true);
    m.borrow_mut().enqueue(Request::SetNavigationLinked(false)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);
    let initial=ui.get_piano_bar_x();let tiny=slint::LogicalPosition::new(ui.get_grid_x()+100.,ui.get_grid_y()+100.);
    for _ in 0..32 {wheel(&w,tiny,-0.01);settle(&ui,&m,&c,&mut p,&mut t);}
    let final_x=ui.get_piano_bar_x();assert!((final_x-(initial-0.32)).abs()<0.04,"sub-tick impulses lost: {initial} -> {final_x}");
    m.borrow_mut().enqueue(Request::FitNotes).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    assert!(ui.get_piano_projection_x().abs()<0.01,"fit must discard fractional scroll");
    m.borrow_mut().enqueue(Request::RestoreView).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    assert!(ui.get_piano_projection_x().abs()<0.01,"restore must discard fractional scroll");
    ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);
    wheel(&w,tiny,-0.01);m.borrow_mut().enqueue(Request::Zoom(1.1)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    assert!(ui.get_piano_projection_x().abs()<0.01,"zoom queued after a scroll must discard its remainder");
    ui.invoke_scroll_piano(0.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);
    m.borrow_mut().enqueue(Request::SetNavigationLinked(true)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    let initial=ui.get_piano_bar_x();for _ in 0..32 {wheel(&w,tiny,-0.01);settle(&ui,&m,&c,&mut p,&mut t);}
    assert!((ui.get_piano_bar_x()-(initial-0.32)).abs()<0.05,"linked scrolling must retain sub-tick impulses");
    assert!((-ui.get_piano_bar_x()/ui.get_pixels_per_beat()+ui.get_clip_start_beat()+ui.get_arrangement_x()/ui.get_arrangement_zoom()).abs()<0.002,"linked fractional scroll must keep both views aligned");
    m.borrow_mut().enqueue(Request::SetNavigationLinked(false)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    wheel(&w,tiny,-0.01);tick(&ui,&m,&c,&mut p,&mut t);
    m.borrow_mut().enqueue(Request::SelectTrack(2)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    assert_eq!(ui.get_track_id().as_str(),"2");assert!(ui.get_piano_projection_x().abs()<0.01,"switching clips must discard the old remainder");
    m.borrow_mut().enqueue(Request::SelectTrack(1)).unwrap();settle(&ui,&m,&c,&mut p,&mut t);
    assert_eq!(ui.get_track_id().as_str(),"1");assert!(ui.get_piano_projection_x().abs()<0.01,"returning to a clip must not restore a stale remainder");
    ui.invoke_scroll_piano(1000.0,600.0);settle(&ui,&m,&c,&mut p,&mut t);
    let edge=ui.get_piano_bar_x();for _ in 0..4 {wheel(&w,tiny,-0.01);settle(&ui,&m,&c,&mut p,&mut t);}assert!(ui.get_piano_bar_x()>=edge-0.15,"scroll cannot retain a remainder beyond the clip boundary");
    wheel(&w,tiny,0.5);settle(&ui,&m,&c,&mut p,&mut t);assert!(ui.get_piano_bar_x()>edge+0.3,"reversing at the edge must move immediately");
    c.flags.fetch_or(QUIT,Ordering::Release);c.worker.join().unwrap();println!("PASS: fractional horizontal wheel, Shift-normalized wheel, timeline and piano, linked views, 100% and 200%, notes/ghost/velocity/hit testing");Ok(())
}
