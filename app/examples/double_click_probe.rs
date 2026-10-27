//! Exercise note double-clicking through the rendered UI and controller.
use slint::platform::software_renderer::{MinimalSoftwareWindow,RepaintBufferType};
use slint::platform::{Platform,PlatformError,WindowAdapter,WindowEvent,PointerEventButton};
use slint::ComponentHandle;
use opendaw_app::{controller::{self,Request,Response,ViewState,QUIT},mailbox::Mailbox};
use std::{cell::RefCell,rc::Rc,sync::atomic::Ordering,time::{Duration,Instant}};
slint::include_modules!();
#[path="../src/ui_support.rs"] mod ui_support;
struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {fn create_window_adapter(&self)->Result<Rc<dyn WindowAdapter>,PlatformError>{Ok(self.0.clone())}}
fn settle(ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,last:&mut Option<ViewState>){
    if ui.get_canvas_width()>0. && ui.get_canvas_height()>0. {m.borrow_mut().enqueue(Request::Viewport(ui.get_canvas_width(),ui.get_canvas_height())).unwrap();}
    let deadline=Instant::now()+Duration::from_secs(5);let mut quiet=0;
    while Instant::now()<deadline {
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
fn click(w:&MinimalSoftwareWindow,x:f32,y:f32){let position=slint::LogicalPosition::new(x,y);
    w.dispatch_event(WindowEvent::PointerPressed{position,button:PointerEventButton::Left});
    w.dispatch_event(WindowEvent::PointerReleased{position,button:PointerEventButton::Left});
}
fn pause_clicks(){std::thread::sleep(Duration::from_millis(650));}
fn note_point(ui:&AppWindow,v:&ViewState)->(f32,f32,u64){
    let n=v.notes.iter().find(|n| n.width>=12. && n.x>=0. && n.x+n.width<ui.get_canvas_width() && n.y>=0. && n.y+n.height<ui.get_canvas_height()).expect("visible note");
    (ui.get_grid_x()+n.x+n.width/2.,ui.get_grid_y()+n.y+n.height/2.,n.id)
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let w=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);slint::platform::set_platform(Box::new(ProbePlatform(w.clone())))?;
    let ui=AppWindow::new()?;let c=controller::spawn(true);let m=Rc::new(RefCell::new(Mailbox::new(c.sender.clone(),c.flags.clone(),c.epoch.clone(),c.audition_epoch.clone())));
    ui_support::bind_editor(&ui,m.clone());ui.show()?;w.set_size(slint::PhysicalSize::new(1280,820));
    let mut p=ui_support::Presenter::new(&ui);let mut last=None;
    macro_rules! pump {()=>{settle(&ui,&m,&c,&mut p,&mut last)}}
    macro_rules! command {($r:expr)=>{{m.borrow_mut().enqueue($r).unwrap();pump!();}}}
    pump!();command!(Request::FitNotes);let (x,y,id)=note_point(&ui,last.as_ref().unwrap());
    let before=last.as_ref().unwrap().notes.len();let revision=last.as_ref().unwrap().revision;
    click(&w,x,y);pump!();assert_eq!(last.as_ref().unwrap().notes.len(),before,"One click must not remove a note");
    assert_eq!(last.as_ref().unwrap().revision,revision,"One click only selects the note");pause_clicks();
    click(&w,x,y);pump!();click(&w,x,y);pump!();
    assert!(last.as_ref().unwrap().notes.iter().all(|n|n.id!=id),"Double click must remove melodic note");
    command!(Request::Undo);assert!(last.as_ref().unwrap().notes.iter().any(|n|n.id==id),"Undo must restore melodic note");
    // A double click on empty grid should preserve the first click's insertion.
    command!(Request::AddClip{track:1,beat:0.});let original=last.as_ref().unwrap().notes.len();
    let key=last.as_ref().unwrap().keys.iter().find(|k|k.y>=0. && k.y+k.height<ui.get_canvas_height()).unwrap();
    let (x,y)=(ui.get_grid_x()+45.,ui.get_grid_y()+key.y+key.height/2.);pause_clicks();
    click(&w,x,y);pump!();click(&w,x,y);pump!();
    assert_eq!(last.as_ref().unwrap().notes.len(),original+1,"Double click on empty grid must leave inserted note");
    // Convert the active track and test the drum-specific mapped pitch layout.
    command!(Request::UseDrumKit{track:1});command!(Request::SetMidiViewMode(2));command!(Request::AddClip{track:1,beat:0.});
    assert!(last.as_ref().unwrap().drum_kit.is_some());
    let key=last.as_ref().unwrap().keys.iter().find(|k|k.y>=0. && k.y+k.height<ui.get_canvas_height()).expect("visible mapped drum key");
    let (x,y)=(ui.get_grid_x()+45.,ui.get_grid_y()+key.y+key.height/2.);pause_clicks();click(&w,x,y);pump!();
    let (x,y,id)=note_point(&ui,last.as_ref().unwrap());let count=last.as_ref().unwrap().notes.len();pause_clicks();
    click(&w,x,y);pump!();click(&w,x,y);pump!();
    assert_eq!(last.as_ref().unwrap().notes.len(),count-1,"Double click must remove a drum note");
    assert!(last.as_ref().unwrap().notes.iter().all(|n|n.id!=id));
    command!(Request::Undo);assert!(last.as_ref().unwrap().notes.iter().any(|n|n.id==id),"Undo must restore drum note");
    c.flags.fetch_or(QUIT,Ordering::Release);c.worker.join().unwrap();
    println!("PASS: melodic and mapped drum note double-click deletion, undo, single click and empty-grid insertion");Ok(())
}
