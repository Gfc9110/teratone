//! Middle-button panning through the real UI/controller adapter.
use slint::platform::software_renderer::{MinimalSoftwareWindow,RepaintBufferType};
use slint::platform::{Platform,PlatformError,WindowAdapter,WindowEvent,PointerEventButton};
use slint::ComponentHandle;
use opendaw_app::{controller::{self,Request,Response,ViewState,QUIT},mailbox::Mailbox};
use std::{cell::{Cell,RefCell},rc::Rc,sync::atomic::Ordering,time::{Duration,Instant}};
slint::include_modules!();
#[path="../src/ui_support.rs"] mod ui_support;
struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {fn create_window_adapter(&self)->Result<Rc<dyn WindowAdapter>,PlatformError>{Ok(self.0.clone())}}
fn settle(ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,last:&mut Option<ViewState>,initial_arrangement:bool){
    if ui.get_canvas_width()>0. && ui.get_canvas_height()>0. {m.borrow_mut().enqueue(Request::Viewport(ui.get_canvas_width(),ui.get_canvas_height())).unwrap();}
    let mut arr=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());
    if initial_arrangement {m.borrow_mut().enqueue(Request::ArrangementViewport(arr.0,arr.1,arr.2)).unwrap();}
    let deadline=Instant::now()+Duration::from_secs(5);let mut quiet=0;
    while Instant::now()<deadline {
        let next=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());
        if next!=arr {arr=next;m.borrow_mut().enqueue(Request::ArrangementViewport(arr.0,arr.1,arr.2)).unwrap();quiet=0;}
        m.borrow_mut().flush().unwrap();
        while let Ok(r)=c.receiver.try_recv(){match r {
            Response::View(v)=>{p.apply(ui,(*v).clone());*last=Some(*v);},
            Response::Ack{request_id,error,..}=>{assert!(error.is_none(),"{error:?}");m.borrow_mut().acknowledge(request_id);},
            Response::Fatal(e)=>panic!("{e}"),_=>{}
        }}
        if m.borrow().pending_count()==0 && last.is_some(){quiet+=1;if quiet==8{return;}}else{quiet=0;}
        std::thread::sleep(Duration::from_millis(5));
    }panic!("Controller did not settle");
}
fn pointer(w:&MinimalSoftwareWindow,x:f32,y:f32,kind:u8){let position=slint::LogicalPosition::new(x,y);w.dispatch_event(match kind {
    0=>WindowEvent::PointerPressed{position,button:PointerEventButton::Middle},
    1=>WindowEvent::PointerMoved{position},
    _=>WindowEvent::PointerReleased{position,button:PointerEventButton::Middle},
});}
fn wheel(w:&MinimalSoftwareWindow,x:f32,y:f32,dx:f32,dy:f32){w.dispatch_event(WindowEvent::PointerScrolled{position:slint::LogicalPosition::new(x,y),delta_x:dx,delta_y:dy});}
fn key(w:&MinimalSoftwareWindow,key:slint::platform::Key,down:bool){let text=key.into();w.dispatch_event(if down{WindowEvent::KeyPressed{text}}else{WindowEvent::KeyReleased{text}});}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let w=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);slint::platform::set_platform(Box::new(ProbePlatform(w.clone())))?;
    let ui=AppWindow::new()?;let c=controller::spawn(true);let m=Rc::new(RefCell::new(Mailbox::new(c.sender.clone(),c.flags.clone(),c.epoch.clone(),c.audition_epoch.clone())));
    ui_support::bind_editor(&ui,m.clone());let seeks=Rc::new(Cell::new(0));let seek_count=seeks.clone();ui.on_seek(move |_|seek_count.set(seek_count.get()+1));
    ui.show()?;w.set_size(slint::PhysicalSize::new(1280,820));
    let mut presenter=ui_support::Presenter::new(&ui);let mut last=None;
    macro_rules! pump {()=>{settle(&ui,&m,&c,&mut presenter,&mut last,true)}}
    macro_rules! pump_piano {()=>{settle(&ui,&m,&c,&mut presenter,&mut last,false)}}
    macro_rules! command {($request:expr)=>{{m.borrow_mut().enqueue($request).unwrap();pump!();}}}
    pump!();
    // A short project at the initial zoom must still be pannable when its
    // entire duration fits within the visible timeline.
    ui.set_navigation_linked(false);ui.set_project_beats(4.);ui.set_arrangement_zoom(48.);ui.set_arrangement_x(0.);
    let initial_x=ui.get_rows_x()+ui.get_header_size()+150.;let initial_y=ui.get_rows_y()+20.;
    pointer(&w,initial_x,initial_y,0);pointer(&w,initial_x-60.,initial_y,1);pointer(&w,initial_x-60.,initial_y,2);
    assert!(ui.get_arrangement_x() < -50.,"A short timeline must pan at the initial zoom");
    pump!();ui.set_workspace_split(0.55);ui.set_arrangement_zoom(96.);ui.set_arrangement_x(-180.);pump!();
    let row_y=ui.get_rows_y();let row_h=ui.get_row_size();let lane_x=ui.get_rows_x()+ui.get_header_size()+100.;
    let empty_y=row_y+2.*row_h+25.;assert!(empty_y<row_y+ui.get_rows_height()-5.,"Need empty timeline space");
    let revision=last.as_ref().unwrap().revision;let selected=last.as_ref().unwrap().selected_note;let before=ui.get_arrangement_x();
    pointer(&w,lane_x,empty_y,0);pointer(&w,lane_x-45.,empty_y,1);pump!();pointer(&w,lane_x-45.,empty_y,2);pump!();
    assert!(ui.get_arrangement_x()<before-35.,"Middle drag in empty timeline must pan horizontally");
    assert_eq!(last.as_ref().unwrap().revision,revision);assert_eq!(last.as_ref().unwrap().selected_note,selected);
    for _ in 0..10 {m.borrow_mut().enqueue(Request::DuplicateTrack(1)).unwrap();}pump!();command!(Request::SelectTrack(1));
    ui.set_arrangement_x(-200.);ui.set_arrangement_y(-100.);pump!();let start=(ui.get_arrangement_x(),ui.get_arrangement_y());
    let y=ui.get_rows_y()+20.;let x=ui.get_rows_x()+ui.get_header_size()+100.;let revision=last.as_ref().unwrap().revision;
    pointer(&w,x,y,0);pointer(&w,x-40.,y-15.,1);pump!();let first=(ui.get_arrangement_x(),ui.get_arrangement_y());
    pointer(&w,x-80.,y-30.,1);pump!();let second=(ui.get_arrangement_x(),ui.get_arrangement_y());pointer(&w,x-80.,y-30.,2);pump!();
    assert!(first.0<start.0-30. && first.1<start.1-10.,"Clip middle drag must pan both timeline axes: {start:?} -> {first:?}");
    assert!(second.0<first.0-25. && second.1<first.1-10.,"Pan must continue after presentation updates: {first:?} -> {second:?}");
    assert_eq!(last.as_ref().unwrap().revision,revision,"Timeline pan must not edit a clip or track");
    let after=ui.get_arrangement_x();pointer(&w,x-110.,y-50.,1);pump!();assert!((ui.get_arrangement_x()-after).abs()<0.01,"Release ends pan");
    // Continue a held middle drag when the starting clip leaves the viewport
    // and the arrangement presentation drops it from the visible model.
    command!(Request::AddClip{track:1,beat:32.});command!(Request::SelectTrack(1));
    ui.set_arrangement_x(-1200.);ui.set_arrangement_y(0.);pump!();
    let x=ui.get_rows_x()+ui.get_header_size()+300.;let y=ui.get_rows_y()+20.;
    pointer(&w,x,y,0);pointer(&w,x-350.,y,1);pump!();let after_exit=ui.get_arrangement_x();let state=ui.get_middle_pan_state();
    pointer(&w,x-410.,y,1);pump!();let continued=ui.get_arrangement_x();pointer(&w,x-410.,y,2);pump!();
    assert!(continued<after_exit-45.,"Middle pan must continue when the starting clip leaves view: {after_exit} -> {continued}, state {state}");
    assert_eq!(ui.get_middle_pan_state(),0,"Release must clear the recovered drag");
    ui.set_arrangement_x(-1200.);pump!();pointer(&w,x,y,0);pointer(&w,x-350.,y,1);pump!();pointer(&w,x-410.,y,1);pump!();
    w.dispatch_event(WindowEvent::PointerExited);pump!();assert_eq!(ui.get_middle_pan_state(),0,"Window exit must cancel a recovered drag");
    let after_cancel=ui.get_arrangement_x();pointer(&w,x-440.,y,1);pump!();assert!((ui.get_arrangement_x()-after_cancel).abs()<0.01);
    command!(Request::SelectClip{track:1,clip:4});command!(Request::FitNotes);command!(Request::Zoom(2.));
    let piano_y=last.as_ref().unwrap().viewport.scroll_y;command!(Request::ScrollPiano(2.,piano_y));
    let note=last.as_ref().unwrap().notes.iter().find(|n|n.x>=8. && n.x+n.width<ui.get_canvas_width()-8.).expect("Visible note").clone();
    let px=ui.get_grid_x()+note.x+note.width/2.;let py=ui.get_grid_y()+note.y+note.height/2.;
    let before=(last.as_ref().unwrap().viewport.start_tick,last.as_ref().unwrap().viewport.scroll_y);let revision=last.as_ref().unwrap().revision;
    let selected=last.as_ref().unwrap().selected_note;let notes=last.as_ref().unwrap().notes.len();
    pointer(&w,px,py,0);pointer(&w,px-45.,py-15.,1);pump!();pointer(&w,px-85.,py-30.,1);pump!();pointer(&w,px-85.,py-30.,2);pump!();
    let after=(last.as_ref().unwrap().viewport.start_tick,last.as_ref().unwrap().viewport.scroll_y);
    assert!(after.0>before.0 && after.1>before.1,"Middle drag on a note must pan both axes: {before:?} -> {after:?}");
    assert_eq!(last.as_ref().unwrap().revision,revision,"Panning must not edit notes");assert_eq!(last.as_ref().unwrap().selected_note,selected,"Panning must not select notes");
    assert!(notes>0);
    let scroll_y=last.as_ref().unwrap().viewport.scroll_y;command!(Request::ScrollPiano(2.,scroll_y));
    let x=ui.get_grid_x()+240.;let y=ui.get_grid_y()-10.;let before=last.as_ref().unwrap().viewport.start_tick;
    pointer(&w,x,y,0);pointer(&w,x-60.,y,1);pointer(&w,x-60.,y,2);pump!();
    assert!(last.as_ref().unwrap().viewport.start_tick>before,"Middle drag on the MIDI ruler must pan");
    assert_eq!(seeks.get(),0,"Middle drag must not seek");
    let before=last.as_ref().unwrap().viewport.scroll_y;let revision=last.as_ref().unwrap().revision;
    let x=ui.get_keyboard_x()+20.;let y=ui.get_keyboard_y()+40.;
    pointer(&w,x,y,0);pointer(&w,x,y-25.,1);pointer(&w,x,y-25.,2);pump!();
    assert!(last.as_ref().unwrap().viewport.scroll_y>before,"Middle drag on the keyboard must pan");
    assert_eq!(last.as_ref().unwrap().live_count,0,"Middle drag must not audition a key");assert_eq!(last.as_ref().unwrap().revision,revision);
    ui.set_velocity_visible(true);command!(Request::SetVelocityVisible(true));
    let scroll_y=last.as_ref().unwrap().viewport.scroll_y;command!(Request::ScrollPiano(2.,scroll_y));
    let bar=last.as_ref().unwrap().velocity_bars.iter().find(|b|b.x>20. && b.x<ui.get_canvas_width()-20.).expect("Visible velocity bar").clone();
    let before=last.as_ref().unwrap().viewport.start_tick;let revision=last.as_ref().unwrap().revision;let selected=last.as_ref().unwrap().selected_note;
    let x=ui.get_grid_x()+bar.x;let y=ui.get_grid_y()+ui.get_canvas_height()+30.;
    pointer(&w,x,y,0);pointer(&w,x-60.,y,1);pointer(&w,x-60.,y,2);pump!();
    assert!(last.as_ref().unwrap().viewport.start_tick>before,"Middle drag on a velocity bar must pan");
    assert_eq!(last.as_ref().unwrap().revision,revision,"Middle drag must not change velocity");assert_eq!(last.as_ref().unwrap().selected_note,selected);
    // Wheel navigation over both bar and lane background uses the same piano
    // viewport as the grid, without selecting notes or editing velocity.
    let bar=last.as_ref().unwrap().velocity_bars.iter().find(|b|b.x>100. && b.x<ui.get_canvas_width()-100.).expect("Visible velocity bar").clone();
    let y=ui.get_grid_y()+ui.get_canvas_height()+30.;let x=ui.get_grid_x()+bar.x+ui.get_piano_projection_x();
    let before_x=ui.get_piano_bar_x();wheel(&w,x,y,-6.5,0.);assert!(ui.get_piano_bar_x()<before_x-6.4,"Wheel over a velocity bar must scroll immediately");pump!();
    let bars=&last.as_ref().unwrap().velocity_bars;let empty=(20..(ui.get_canvas_width() as i32-20)).step_by(11).map(|x|x as f32)
        .find(|x|bars.iter().all(|bar|(bar.x - *x).abs()>10.)).expect("Empty lane position");
    let before=(ui.get_piano_bar_x(),ui.get_piano_bar_y());wheel(&w,ui.get_grid_x()+empty,y,-3.25,-7.5);
    assert!(ui.get_piano_bar_x()<before.0-3.2 && ui.get_piano_bar_y()<before.1-7.4,"Wheel over empty velocity lane must scroll both axes");pump!();
    assert_eq!(last.as_ref().unwrap().revision,revision,"Wheel navigation must not edit velocity");assert_eq!(last.as_ref().unwrap().selected_note,selected);
    assert_eq!(last.as_ref().unwrap().velocity_bars.iter().find(|b|b.id==bar.id).unwrap().value,bar.value);
    let zoom=last.as_ref().unwrap().viewport.pixels_per_beat;key(&w,slint::platform::Key::Control,true);wheel(&w,ui.get_grid_x()+empty,y,0.,-1.);key(&w,slint::platform::Key::Control,false);pump!();
    assert!(last.as_ref().unwrap().viewport.pixels_per_beat<zoom,"Control-wheel on velocity lane must zoom horizontally");
    let row=last.as_ref().unwrap().viewport.row_height;key(&w,slint::platform::Key::Control,true);key(&w,slint::platform::Key::Shift,true);wheel(&w,ui.get_grid_x()+empty,y,0.,-1.);key(&w,slint::platform::Key::Shift,false);key(&w,slint::platform::Key::Control,false);pump!();
    assert!(last.as_ref().unwrap().viewport.row_height<row,"Control-Shift-wheel on velocity lane must zoom vertically");
    w.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:2.});w.set_size(slint::PhysicalSize::new(2560,1640));pump!();
    let bar=last.as_ref().unwrap().velocity_bars.iter().find(|b|b.x>100. && b.x<ui.get_canvas_width()-100.).expect("Visible velocity bar at 200%");
    let x=ui.get_grid_x()+bar.x+ui.get_piano_projection_x();let y=ui.get_grid_y()+ui.get_canvas_height()+30.;let before=ui.get_piano_bar_x();
    wheel(&w,x,y,-0.75,0.);assert!((ui.get_piano_bar_x()-(before-0.75)).abs()<0.05,"Fractional wheel on velocity must work at 200%");pump!();
    assert_eq!(last.as_ref().unwrap().revision,revision,"Velocity wheel at 200% must not edit notes");assert_eq!(last.as_ref().unwrap().selected_note,selected);
    w.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.});w.set_size(slint::PhysicalSize::new(1280,820));pump!();
    ui.set_velocity_visible(false);command!(Request::SetVelocityVisible(false));
    m.borrow_mut().enqueue(Request::SetNavigationLinked(true)).unwrap();pump_piano!();
    let scroll_y=last.as_ref().unwrap().viewport.scroll_y;m.borrow_mut().enqueue(Request::ScrollPiano(2.,scroll_y)).unwrap();pump_piano!();
    let x=ui.get_grid_x()+240.;let y=ui.get_grid_y()+100.;let before=last.as_ref().unwrap().viewport.start_tick;
    pointer(&w,x,y,0);pointer(&w,x-45.,y,1);pointer(&w,x-45.,y,2);pump_piano!();
    assert!(last.as_ref().unwrap().viewport.start_tick>before,"Linked MIDI pan must move the viewport: {before} -> {}, arrangement {}, bar {}, state {}",last.as_ref().unwrap().viewport.start_tick,ui.get_arrangement_x(),ui.get_piano_bar_x(),ui.get_middle_pan_state());
    let absolute=ui.get_clip_start_beat()+last.as_ref().unwrap().viewport.start_tick as f32/960.;
    assert!((ui.get_arrangement_x()+absolute*ui.get_arrangement_zoom()).abs()<2.,"Linked timeline must follow MIDI pan");
    assert_eq!(ui.get_middle_pan_state(),0,"Linked pan must release normally");
    ui.set_arrangement_y(0.);pump!();let x=ui.get_rows_x()+ui.get_header_size()+300.;let y=ui.get_rows_y()+20.;
    let before=last.as_ref().unwrap().viewport.start_tick;pointer(&w,x,y,0);pointer(&w,x-45.,y,1);pointer(&w,x-45.,y,2);pump!();
    assert!(last.as_ref().unwrap().viewport.start_tick>before,"Linked timeline pan must move MIDI time");assert_eq!(ui.get_middle_pan_state(),0);
    let x=ui.get_grid_x()+200.;let y=ui.get_grid_y()+100.;pointer(&w,x,y,0);w.dispatch_event(WindowEvent::PointerExited);pointer(&w,x-60.,y-20.,1);pump!();
    let canceled=last.as_ref().unwrap().viewport;pointer(&w,x-90.,y-20.,1);pump!();assert_eq!(last.as_ref().unwrap().viewport,canceled,"Canceled drag must not remain active");
    c.flags.fetch_or(QUIT,Ordering::Release);c.worker.join().unwrap();println!("PASS: middle-button pan and velocity-lane wheel/zoom preserve musical data; release and cancel stop the drag");Ok(())
}
