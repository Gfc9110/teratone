//! Project-view roundtrip through the production controller and Presenter with
//! the same ordering as the desktop timer, using an isolated settings directory.
use opendaw_app::{controller::{self,Request,Response,QUIT}, mailbox::Mailbox, project::{Project,History,Edit,PPQ},project_library::ProjectLibrary,storage};
use slint::{platform::{Platform,PlatformError,WindowAdapter},platform::software_renderer::{MinimalSoftwareWindow,RepaintBufferType},ComponentHandle,SharedPixelBuffer};
use std::{cell::RefCell,rc::Rc,sync::atomic::Ordering,time::{Duration,Instant}};
slint::include_modules!();
#[path="../src/ui_support.rs"] mod ui_support;
struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl Platform for TestPlatform {fn create_window_adapter(&self)->Result<Rc<dyn WindowAdapter>,PlatformError>{Ok(self.0.clone())}}
struct TimerState { size:(i32,i32),arr:(f32,f32,f32),scroll:f32,piano:Option<bool>,velocity:Option<bool>,revision:u64,session:u64 }
impl Default for TimerState {fn default()->Self{Self{size:(0,0),arr:(0.0,0.0,0.0),scroll:0.0,piano:None,velocity:None,revision:0,session:0}}}
fn render(window:&MinimalSoftwareWindow) {
    window.request_redraw();let mut pixels=SharedPixelBuffer::<slint::Rgb8Pixel>::new(1280,820);
    window.draw_if_needed(|r|{r.render(pixels.make_mut_slice(),1280);});
}
fn settle(window:&MinimalSoftwareWindow,ui:&AppWindow,m:&RefCell<Mailbox>,c:&controller::Connection,p:&mut ui_support::Presenter,t:&mut TimerState) {
    let deadline=Instant::now()+Duration::from_secs(6);let mut quiet=0;
    loop {
        ui.invoke_tick_drag();ui.invoke_clamp_scroll();
        let size=(ui.get_canvas_width() as i32,ui.get_canvas_height() as i32);
        if size!=t.size && size.0>0 && size.1>0 {m.borrow_mut().enqueue(Request::ViewportForSession(t.session,size.0 as f32,size.1 as f32)).unwrap();t.size=size;}
        let arr=(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width());
        if arr!=t.arr && arr.2>0.0 {m.borrow_mut().enqueue(Request::ArrangementViewportForSession(t.session,arr.0,arr.1,arr.2)).unwrap();t.arr=arr;}
        let scroll=ui.get_arrangement_y();if scroll!=t.scroll {m.borrow_mut().enqueue(Request::ArrangementScrollForSession(t.session,scroll)).unwrap();t.scroll=scroll;}
        let piano=ui.get_workspace_focus()!=1;let velocity=ui.get_velocity_visible() && piano;
        if t.piano!=Some(piano) {m.borrow_mut().enqueue(Request::SetPianoVisible(piano)).unwrap();t.piano=Some(piano);}
        if t.velocity!=Some(velocity) {m.borrow_mut().enqueue(Request::SetVelocityVisible(velocity)).unwrap();t.velocity=Some(velocity);}
        m.borrow_mut().flush().unwrap();
        while let Ok(response)=c.receiver.try_recv() {match response {
            Response::View(v)=>{if v.revision>=t.revision {
                if t.session!=v.project_session {t.session=v.project_session;t.size=(0,0);t.arr=(f32::NAN,f32::NAN,f32::NAN);t.scroll=f32::NAN;}
                p.apply(ui,*v);
            }},
            Response::Ack{request_id,revision,error,..}=>{m.borrow_mut().acknowledge(request_id);t.revision=t.revision.max(revision);assert!(error.is_none(),"{error:?}");},
            Response::DrumMeter{track,instance,status}=>p.drum_meter(ui,track,instance,status),
            Response::Fatal(e)=>panic!("{e}"),_=>{}
        }}
        render(window);
        if m.borrow().pending_count()==0 {quiet+=1;if quiet>=15{return;}} else {quiet=0;}
        assert!(Instant::now()<deadline,"Timed out");std::thread::sleep(Duration::from_millis(4));
    }
}
fn main()->Result<(),Box<dyn std::error::Error>> {
    let window=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);slint::platform::set_platform(Box::new(TestPlatform(window.clone())))?;
    let ui=AppWindow::new()?;ui.set_home_visible(false);ui.set_workspace_split(0.5);ui.show()?;window.set_size(slint::PhysicalSize::new(1280,820));
    let root=std::env::temp_dir().join(format!("opendaw-view-ui-probe-{}",std::process::id()));std::fs::create_dir_all(&root)?;
    let a=root.join("a.opendaw");let b=root.join("b.opendaw");std::fs::create_dir_all(&a)?;std::fs::create_dir_all(&b)?;
    let mut project=Project::demo();project.length_ticks=128*PPQ;let mut history=History::default();
    for _ in 0..16 {history.execute(&mut project,Edit::AddTrack{after:1})?;}storage::save(&a,&project)?;storage::save(&b,&project)?;
    let c=controller::spawn_with_library(true,ProjectLibrary::load(root.join("settings/projects.json"))?);
    let m=Rc::new(RefCell::new(Mailbox::new(c.sender.clone(),c.flags.clone(),c.epoch.clone(),c.audition_epoch.clone())));
    ui_support::bind_editor(&ui,m.clone());let mut p=ui_support::Presenter::new(&ui);let mut t=TimerState::default();
    macro_rules! pump {()=>{settle(&window,&ui,&m,&c,&mut p,&mut t)}}
    macro_rules! request {($r:expr)=>{{m.borrow_mut().enqueue($r).unwrap();pump!();}}}
    pump!();request!(Request::Open(a.clone()));
    ui.set_arrangement_zoom(90.0);ui.set_arrangement_x(-360.0);ui.set_arrangement_y(-140.0);pump!();
    let saved_a=(ui.get_arrangement_zoom(),ui.get_arrangement_x(),ui.get_arrangement_y());assert!(saved_a.1< -100.0 && saved_a.2< -50.0,"A layout must actually scroll: {saved_a:?}");
    request!(Request::FollowSelection(false));request!(Request::SelectTrack(2));request!(Request::ToggleReferencePin{track:2,clip:6});
    request!(Request::Open(b.clone()));ui.set_arrangement_zoom(70.0);ui.set_arrangement_x(-700.0);ui.set_arrangement_y(-200.0);pump!();
    let saved_b=(ui.get_arrangement_zoom(),ui.get_arrangement_x(),ui.get_arrangement_y());assert!(saved_b.1< -100.0 && saved_b.2< -50.0,"B layout must actually scroll: {saved_b:?}");
    request!(Request::Open(a.clone()));let restored_a=(ui.get_arrangement_zoom(),ui.get_arrangement_x(),ui.get_arrangement_y());
    assert!((restored_a.0-saved_a.0).abs()<0.1 && (restored_a.1-saved_a.1).abs()<1.0 && (restored_a.2-saved_a.2).abs()<1.0,"A view was not restored: expected {saved_a:?}, got {restored_a:?}");
    assert!(!ui.get_follow_selection());assert_eq!(ui.get_track_id().as_str(),"1");assert_eq!(ui.get_arrangement_track_id().as_str(),"2");assert_eq!(ui.get_reference_count(),1);
    // A user can scroll and request another project between timer ticks. The
    // previously observed A viewport must not be applied to newly opened B.
    m.borrow_mut().enqueue(Request::Open(b.clone())).unwrap();ui.set_arrangement_zoom(90.0);ui.set_arrangement_x(-900.0);ui.set_arrangement_y(-260.0);pump!();
    let restored_b=(ui.get_arrangement_zoom(),ui.get_arrangement_x(),ui.get_arrangement_y());
    assert!((restored_b.0-saved_b.0).abs()<0.1 && (restored_b.1-saved_b.1).abs()<1.0 && (restored_b.2-saved_b.2).abs()<1.0,"Stale A viewport overwrote B: expected {saved_b:?}, got {restored_b:?}");
    c.flags.fetch_or(QUIT,Ordering::Release);c.worker.join().unwrap();std::fs::remove_dir_all(root)?;println!("PASS: project views restore zoom, scroll, references and editor lock, including pending UI updates at project switch");Ok(())
}
