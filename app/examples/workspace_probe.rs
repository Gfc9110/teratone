//! Layout regression checks against the software renderer and real controller.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent, PointerEventButton};
use slint::{ComponentHandle, SharedPixelBuffer};
use opendaw_app::{controller::{self, Request, Response, ViewState, QUIT}, mailbox::Mailbox};
use std::{cell::{Cell, RefCell}, io::Write, path::Path, rc::Rc, sync::atomic::Ordering, time::{Duration, Instant}};
slint::include_modules!();
#[path = "../src/ui_support.rs"] mod ui_support;
struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {fn create_window_adapter(&self)->Result<Rc<dyn WindowAdapter>,PlatformError>{Ok(self.0.clone())}}
fn settle(ui:&AppWindow, mailbox:&RefCell<Mailbox>, connection:&controller::Connection, presenter:&mut ui_support::Presenter, last:&mut Option<ViewState>) {
    let deadline=Instant::now()+Duration::from_secs(5); let mut quiet=0;
    if ui.get_canvas_width()>0. && ui.get_canvas_height()>0. {mailbox.borrow_mut().enqueue(Request::Viewport(ui.get_canvas_width(),ui.get_canvas_height())).unwrap();}
    mailbox.borrow_mut().enqueue(Request::ArrangementViewport(-ui.get_arrangement_x()/ui.get_arrangement_zoom(),ui.get_arrangement_zoom(),ui.get_lane_width())).unwrap();
    while Instant::now()<deadline {
        mailbox.borrow_mut().flush().unwrap();
        while let Ok(response)=connection.receiver.try_recv(){match response{
            Response::View(v)=>{presenter.apply(ui,(*v).clone());*last=Some(*v);},
            Response::Ack{request_id,error,..}=>{assert!(error.is_none(),"{error:?}");mailbox.borrow_mut().acknowledge(request_id);},
            Response::Fatal(e)=>panic!("{e}"),_=>{}}}
        if mailbox.borrow().pending_count()==0 && last.is_some(){quiet+=1;if quiet==8{return}} else {quiet=0};
        std::thread::sleep(Duration::from_millis(5));
    } panic!("Controller did not settle");
}
fn snapshot(window:&MinimalSoftwareWindow,name:&str,w:u32,h:u32){
    let mut pixels=SharedPixelBuffer::<slint::Rgb8Pixel>::new(w,h);window.request_redraw();
    assert!(window.draw_if_needed(|renderer|{renderer.render(pixels.make_mut_slice(),w as usize);}));
    let path=Path::new(env!("CARGO_MANIFEST_DIR")).join("../evidence/redesign").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();let mut f=std::fs::File::create(path).unwrap();
    write!(f,"P6\n{w} {h}\n255\n").unwrap();f.write_all(pixels.as_bytes()).unwrap();
}
fn click(window:&MinimalSoftwareWindow,x:f32,y:f32){
    let position=slint::LogicalPosition::new(x,y);
    window.dispatch_event(WindowEvent::PointerPressed{position,button:PointerEventButton::Left});
    window.dispatch_event(WindowEvent::PointerReleased{position,button:PointerEventButton::Left});
}
fn key(window:&MinimalSoftwareWindow,key:slint::SharedString){
    window.dispatch_event(WindowEvent::KeyPressed{text:key.clone()});window.dispatch_event(WindowEvent::KeyReleased{text:key});
}
fn wheel(window:&MinimalSoftwareWindow,x:f32,y:f32,dy:f32){
    window.dispatch_event(WindowEvent::PointerScrolled{position:slint::LogicalPosition::new(x,y),delta_x:0.,delta_y:dy});
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let window=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui=AppWindow::new()?;ui.set_silent_mode(true);let connection=controller::spawn(true);
    let mailbox=Rc::new(RefCell::new(Mailbox::new(connection.sender.clone(),connection.flags.clone(),connection.epoch.clone(),connection.audition_epoch.clone())));
    {let m=mailbox.clone();ui.on_rename_track(move|id,name|m.borrow_mut().enqueue(Request::RenameTrack{track:id.parse().unwrap(),name:name.into()}).unwrap());}
    {let m=mailbox.clone();ui.on_select_track(move|id|m.borrow_mut().enqueue(Request::SelectTrack(id.parse().unwrap())).unwrap());}
    ui_support::bind_editor(&ui,mailbox.clone());ui.show()?;window.set_size(slint::PhysicalSize::new(1280,820));
    let mut presenter=ui_support::Presenter::new(&ui);let mut last=None;
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let plays=Rc::new(Cell::new(0));let play_count=plays.clone();ui.on_play(move||play_count.set(play_count.get()+1));
    click(&window,ui.get_play_x(),ui.get_play_y());
    assert_eq!(plays.get(),1,"Icon transport must respond to pointer input");
    assert_eq!(ui.get_focused_action_hint().as_str(),"Start playback","Focused icon must expose its description in the status bar");
    key(&window,slint::platform::Key::Space.into());
    assert_eq!(plays.get(),2,"Focused icon transport must respond to keyboard input");
    assert!(ui.get_focused_tooltip_visible(),"Keyboard focus must expose the icon tooltip");
    let tempo_edits=Rc::new(Cell::new(0));let tempo_edits_out=tempo_edits.clone();
    ui.on_tempo(move |v|tempo_edits_out.set(v));
    ui.set_bpm(99);let (tempo_x,tempo_y,tempo_w)=(ui.get_tempo_control_x(),ui.get_tempo_control_y(),ui.get_tempo_control_width());
    assert!((tempo_w-104.).abs()<0.01,"Tempo control must have a fixed width");
    click(&window,tempo_x+90.,tempo_y+15.);
    assert_eq!(ui.get_bpm(),100);assert_eq!(tempo_edits.get(),100);
    assert!((ui.get_tempo_control_x()-tempo_x).abs()<0.01 && (ui.get_tempo_control_width()-tempo_w).abs()<0.01,"Changing to three digits must not resize or move the control");
    click(&window,tempo_x+14.,tempo_y+15.);assert_eq!(ui.get_bpm(),99);assert_eq!(tempo_edits.get(),99);
    click(&window,tempo_x+52.,tempo_y+15.);key(&window,slint::platform::Key::UpArrow.into());
    assert_eq!(ui.get_bpm(),100,"Keyboard arrows must adjust tempo");
    ui.set_bpm(30);click(&window,tempo_x+14.,tempo_y+15.);assert_eq!(ui.get_bpm(),30);
    ui.set_bpm(300);click(&window,tempo_x+90.,tempo_y+15.);assert_eq!(ui.get_bpm(),300);
    assert!((ui.get_tempo_control_width()-tempo_w).abs()<0.01,"Tempo bounds must not change the control width");
    ui.set_bpm(120);
    click(&window,299.,20.);click(&window,310.,60.);
    assert!(ui.get_about_visible(),"Help opens About OpenDAW");ui.set_about_visible(false);
    mailbox.borrow_mut().enqueue(Request::FollowSelection(false)).unwrap();
    mailbox.borrow_mut().enqueue(Request::SelectTrack(2)).unwrap();settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(ui.get_inspector_track_id().as_str(),"2");assert_eq!(ui.get_track_id().as_str(),"1");
    assert!(ui.get_sound_context_label().contains("Sine lead"));
    assert!(ui.get_clip_context_label().contains("Sine lead") && ui.get_clip_context_label().contains("Main"));
    mailbox.borrow_mut().enqueue(Request::FollowSelection(true)).unwrap();mailbox.borrow_mut().enqueue(Request::SelectTrack(1)).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    snapshot(&window,"phase4-combined-1280.ppm",1280,820);
    mailbox.borrow_mut().enqueue(Request::ToggleMute(1)).unwrap();
    mailbox.borrow_mut().enqueue(Request::ToggleSolo(2)).unwrap();settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(last.as_ref().unwrap().tracks[0].mute && last.as_ref().unwrap().tracks[1].solo,"Mute and solo must keep their independent states");
    snapshot(&window,"compact-studio-states-1280.ppm",1280,820);
    mailbox.borrow_mut().enqueue(Request::ToggleMute(1)).unwrap();
    mailbox.borrow_mut().enqueue(Request::ToggleSolo(2)).unwrap();settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    mailbox.borrow_mut().enqueue(Request::ToggleReference{track:2,clip:6}).unwrap();
    mailbox.borrow_mut().enqueue(Request::ZoomY(0.5)).unwrap();
    mailbox.borrow_mut().enqueue(Request::ScrollPiano(0.,624.)).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(!last.as_ref().unwrap().ghosts.is_empty(),"Visible reference notes must be rendered: viewport {:?}, references {:?}",last.as_ref().unwrap().viewport,last.as_ref().unwrap().reference_rows);
    snapshot(&window,"phase4-references-1280.ppm",1280,820);
    window.set_size(slint::PhysicalSize::new(1920,1080));settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    snapshot(&window,"phase4-combined-1920.ppm",1920,1080);
    window.set_size(slint::PhysicalSize::new(1024,768));mailbox.borrow_mut().enqueue(Request::HideReferences).unwrap();settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    ui.set_inspector_visible(true);ui.set_inspector_pinned(true);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_editor_toolbar_end_x()<=ui.get_workspace_right_x()+0.5,"Unified toolbar overflows pinned workspace: {} > {}",ui.get_editor_toolbar_end_x(),ui.get_workspace_right_x());
    let pinned_width=ui.get_lane_width();snapshot(&window,"phase4-inspector-pinned-1024.ppm",1024,768);
    click(&window,ui.get_inspector_close_x(),ui.get_inspector_close_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(!ui.get_inspector_visible(),"The pinned inspector close button must respond to a click");
    click(&window,ui.get_inspector_toggle_x(),ui.get_inspector_toggle_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_inspector_visible() && ui.get_inspector_pinned(),"The global toggle must reopen the pinned inspector");
    ui.set_inspector_custom_width(400.);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_inspector_pinned() && (ui.get_inspector_custom_width()-400.).abs()<0.01,"Automatic overlay must retain pin and requested width");
    assert!(ui.get_lane_width()>pinned_width+200.,"A wide inspector must overlay when the toolbar would otherwise overflow");
    assert!(ui.get_editor_toolbar_end_x()<=ui.get_workspace_right_x()+0.5,"Toolbar must fit after automatic overlay");
    snapshot(&window,"phase4-wide-inspector-overlay-1024.ppm",1024,768);
    ui.set_inspector_custom_width(240.);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!((ui.get_lane_width()-pinned_width).abs()<1.,"Reducing the inspector must restore the pinned layout");
    ui.set_inspector_visible(false);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_lane_width()>pinned_width+200.,"Closing inspector must free its reserved width");
    ui.set_inspector_visible(true);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    ui.set_inspector_pinned(false);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_lane_width()>pinned_width+200.,"Temporary inspector must not reserve workspace width");
    snapshot(&window,"phase4-inspector-overlay-1024.ppm",1024,768);
    click(&window,ui.get_inspector_close_x(),ui.get_inspector_close_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(!ui.get_inspector_visible(),"The temporary inspector close button must respond to a click");
    click(&window,ui.get_inspector_toggle_x(),ui.get_inspector_toggle_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_inspector_visible() && !ui.get_inspector_pinned(),"The global toggle must reopen the temporary inspector");
    window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:2.});window.set_size(slint::PhysicalSize::new(2000,1520));
    assert!((ui.get_tempo_control_width()-104.).abs()<0.01,"Tempo control must remain fixed at 200% scaling");
    ui.set_inspector_pinned(true);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    ui.set_sound_tab(true);ui.set_project_tab(false);ui.set_inspector_scroll(0.);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(ui.get_synth_wave(),0,"The sine shape and PWM sliders must start disabled");
    let revision=last.as_ref().unwrap().revision;let selected=last.as_ref().unwrap().selected_note;let piano_y=ui.get_piano_scroll_y();
    let sx=ui.get_synth_shape_x();let sy=ui.get_synth_shape_y();let dx=ui.get_synth_duty_x();let dy=ui.get_synth_duty_y();
    click(&window,sx,sy);click(&window,dx,dy);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(last.as_ref().unwrap().revision,revision,"Clicking disabled inspector sliders must not edit the piano below");
    assert_eq!(last.as_ref().unwrap().selected_note,selected,"Clicking a disabled inspector slider must not select a note below");
    wheel(&window,sx,sy,-100.);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_inspector_scroll()<0.,"Scrolling over a disabled inspector control must scroll the inspector");
    assert!((ui.get_piano_scroll_y()-piano_y).abs()<0.01,"Inspector scrolling must not scroll the piano underneath");
    ui.set_inspector_scroll(0.);
    let instance=last.as_ref().unwrap().instance_id;
    mailbox.borrow_mut().enqueue(Request::SynthParameter{track:1,instance,field:opendaw_app::synth::SynthField::Waveform,value:1.}).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);let revision=last.as_ref().unwrap().revision;
    click(&window,ui.get_synth_shape_x(),ui.get_synth_shape_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(last.as_ref().unwrap().revision>revision && last.as_ref().unwrap().synth.unwrap().shape<0.9,
        "Enabled inspector controls must still receive pointer input");
    mailbox.borrow_mut().enqueue(Request::SynthParameter{track:1,instance,field:opendaw_app::synth::SynthField::Waveform,value:0.}).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);ui.set_sound_tab(false);
    click(&window,ui.get_inspector_close_x(),ui.get_inspector_close_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(!ui.get_inspector_visible(),"At 1000x760 and 200%, the overlaid inspector close button must work");
    click(&window,ui.get_inspector_toggle_x(),ui.get_inspector_toggle_y());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_inspector_visible() && ui.get_inspector_pinned(),"At 200%, the global toggle must reopen the inspector");
    ui.set_inspector_visible(false);window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.});window.set_size(slint::PhysicalSize::new(1024,768));
    ui.set_workspace_split(-1.);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    snapshot(&window,"phase4-combined-1024.ppm",1024,768);
    ui.set_workspace_split(0.45);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let saved=ui.get_arrangement_height_value();ui.set_workspace_focus(1);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(ui.get_canvas_height(),0.);snapshot(&window,"phase4-focus-timeline-1024.ppm",1024,768);
    ui.set_workspace_focus(2);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);assert!(ui.get_canvas_height()>0.);
    snapshot(&window,"phase4-focus-piano-1024.ppm",1024,768);ui.set_workspace_focus(0);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!((ui.get_arrangement_height_value()-saved).abs()<1.,"Combined must restore the split");
    for (density,expected) in [(0,48.),(1,58.),(2,64.)]{
        ui.set_ui_density(density);ui.set_inspector_visible(true);ui.set_inspector_pinned(true);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!((ui.get_row_size()-expected).abs()<0.01);
        assert!(ui.get_editor_toolbar_end_x()<=ui.get_workspace_right_x()+0.5,"Toolbar overflows density {density}");
    }
    ui.set_inspector_visible(false);
    ui.set_ui_density(0);settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let x=ui.get_rows_x()+50.;let y=ui.get_rows_y()+14.;click(&window,x,y);click(&window,x,y);
    let added:slint::SharedString="X".into();window.dispatch_event(WindowEvent::KeyPressed{text:added.clone()});window.dispatch_event(WindowEvent::KeyReleased{text:added});
    let enter:slint::SharedString=slint::platform::Key::Return.into();window.dispatch_event(WindowEvent::KeyPressed{text:enter.clone()});window.dispatch_event(WindowEvent::KeyReleased{text:enter});
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(last.as_ref().unwrap().tracks[0].name.contains('X'),"Double-click header must allow direct renaming");
    let mut focused=false;
    for _ in 0..160 {key(&window,slint::platform::Key::Tab.into());if ui.get_focused_track_id().as_str()=="2" {focused=true;break;}}
    assert!(focused,"Track header must be reachable by Tab");
    key(&window,slint::platform::Key::Space.into());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(ui.get_inspector_track_id().as_str(),"2","Space selects the focused track");
    key(&window,slint::platform::Key::F2.into());key(&window,"Y".into());key(&window,slint::platform::Key::Return.into());
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(last.as_ref().unwrap().tracks[1].name.contains('Y'),"F2 renames the focused track");
    mailbox.borrow_mut().enqueue(Request::SelectTrack(1)).unwrap();settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let mut focused=false;
    for _ in 0..160 {key(&window,slint::platform::Key::Tab.into());if ui.get_focused_track_id().as_str()=="2" {focused=true;break;}}
    assert!(focused,"Track header must remain reachable by Tab after renaming");
    key(&window,slint::platform::Key::Return.into());settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(ui.get_inspector_track_id().as_str(),"2","Return selects the focused track");
    mailbox.borrow_mut().enqueue(Request::SelectTrack(1)).unwrap();mailbox.borrow_mut().enqueue(Request::UseDrumKit{track:1}).unwrap();
    mailbox.borrow_mut().enqueue(Request::FollowSelection(false)).unwrap();mailbox.borrow_mut().enqueue(Request::SelectTrack(2)).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_is_drums() && ui.get_sound_context_label().contains("Sine lead") && ui.get_inspector_track_id().as_str()=="2",
        "Locked drum Sound must identify the editor track rather than the selected track");
    println!("PASS: 1024x768 workspace, context labels, focus restore, density and keyboard track selection/rename");
    connection.flags.fetch_or(QUIT,Ordering::Release);connection.worker.join().unwrap();Ok(())
}
