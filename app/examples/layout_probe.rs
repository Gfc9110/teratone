//! Software-rendered UI interaction test connected to the real controller, with no physical audio.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent, PointerEventButton};
use slint::{ComponentHandle, SharedPixelBuffer};
use opendaw_app::{controller::{self, Request, Response, ViewState, QUIT}, mailbox::Mailbox};
use std::{cell::{Cell, RefCell}, io::Write, path::Path, rc::Rc, sync::atomic::Ordering, time::{Duration, Instant}};
slint::include_modules!();
#[path = "../src/ui_support.rs"] mod ui_support;
struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> { Ok(self.0.clone()) }
}
fn snapshot(window: &MinimalSoftwareWindow, path: &str, width: u32, height: u32) -> SharedPixelBuffer<slint::Rgb8Pixel> {
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| { renderer.render(pixels.make_mut_slice(), width as usize); }));
    let mut file = std::fs::File::create(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
    write!(file, "P6\n{width} {height}\n255\n").unwrap(); file.write_all(pixels.as_bytes()).unwrap(); pixels
}
fn check_alignment(pixels: &SharedPixelBuffer<slint::Rgb8Pixel>, ui: &AppWindow, scale: f32) {
    let x_header=((ui.get_rows_x()+60.0)*scale) as usize;
    let x_lane=((ui.get_rows_x()+ui.get_header_size()+80.0)*scale) as usize;
    for index in 0..16 {
        let y=ui.get_rows_y()+ui.get_arrangement_y()+(index+1) as f32*ui.get_row_size()-1.0;
        if y <= ui.get_rows_y()+1.0 || y >= ui.get_rows_y()+ui.get_rows_height()-1.0 { continue; }
        let expected=(y*scale).floor() as i32;
        let positions = |x: usize| -> Vec<i32> { (-2..=2).filter_map(|offset| {
            let py=expected+offset;
            if py<0 || py>=pixels.height() as i32 { return None; }
            let p=pixels.as_slice()[py as usize*pixels.width() as usize+x];
            (p.r==0x32 && p.g==0x42 && p.b==0x54).then_some(py)
        }).collect() };
        let a=positions(x_header); let b=positions(x_lane);
        assert!(!a.is_empty() && !b.is_empty(), "Visible row divider must be present in header and lane");
        assert!(a.iter().any(|a| b.iter().any(|b| (a-b).abs() as f32<=scale)), "Header/lane alignment within one logical pixel");
    }
}
fn pointer(window: &MinimalSoftwareWindow, x: f32, y: f32, kind: u8) {
    let position = slint::LogicalPosition::new(x, y);
    window.dispatch_event(match kind {
        0 => WindowEvent::PointerPressed { position, button: PointerEventButton::Left },
        1 => WindowEvent::PointerMoved { position },
        _ => WindowEvent::PointerReleased { position, button: PointerEventButton::Left },
    });
}
fn click(window: &MinimalSoftwareWindow, x: f32, y: f32) { pointer(window,x,y,0); pointer(window,x,y,2); }
fn wheel(window: &MinimalSoftwareWindow, x: f32, y: f32, dx: f32, dy: f32) {
    window.dispatch_event(WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(x,y), delta_x:dx, delta_y:dy });
}
fn settle(ui: &AppWindow, mailbox: &RefCell<Mailbox>, connection: &controller::Connection, presenter: &mut ui_support::Presenter, last: &mut Option<ViewState>) {
    mailbox.borrow_mut().enqueue(Request::Viewport(ui.get_canvas_width(), ui.get_canvas_height())).unwrap();
    mailbox.borrow_mut().enqueue(Request::ArrangementViewport(-ui.get_arrangement_x()/ui.get_arrangement_zoom(), ui.get_arrangement_zoom(), ui.get_lane_width())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4); let mut quiet = 0;
    while Instant::now() < deadline {
        mailbox.borrow_mut().flush().unwrap();
        while let Ok(response) = connection.receiver.try_recv() {
            match response {
                Response::View(v) => { presenter.apply(ui, (*v).clone()); *last = Some(*v); }
                Response::Ack { request_id, error, .. } => { assert!(error.is_none(), "{error:?}"); mailbox.borrow_mut().acknowledge(request_id); }
                Response::Fatal(e) => panic!("{e}"), _ => {},
            }
        }
        if mailbox.borrow().pending_count() == 0 && last.is_some() { quiet += 1; if quiet == 8 { return; } } else { quiet = 0; }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Controller did not settle");
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui = AppWindow::new()?; let connection = controller::spawn(true);
    let mailbox = Rc::new(RefCell::new(Mailbox::new(connection.sender.clone(), connection.flags.clone(), connection.epoch.clone())));
    macro_rules! bind { ($method:ident, ($($arg:ident),*) => $request:expr) => {{
        let m = mailbox.clone(); ui.$method(move |$($arg),*| m.borrow_mut().enqueue($request).unwrap());
    }}; }
    bind!(on_play, () => Request::Play);
    bind!(on_undo, () => Request::Undo);
    bind!(on_redo, () => Request::Redo);
    bind!(on_set_gain, (track, value) => Request::SetGain { track:track.parse().unwrap(),value });
    bind!(on_finish_mix, () => Request::FinishMix);
    bind!(on_select_track, (id) => Request::SelectTrack(id.parse().unwrap()));
    bind!(on_select_clip, (track, clip) => Request::SelectClip { track:track.parse().unwrap(),clip:clip.parse().unwrap() });
    bind!(on_toggle_mute, (track) => Request::ToggleMute(track.parse().unwrap()));
    bind!(on_toggle_solo, (track) => Request::ToggleSolo(track.parse().unwrap()));
    bind!(on_add_track, (track) => Request::AddTrack { after:track.parse().unwrap() });
    bind!(on_duplicate_track, (track) => Request::DuplicateTrack(track.parse().unwrap()));
    let reorders = Rc::new(Cell::new(0)); let counter = reorders.clone(); let m = mailbox.clone();
    ui.on_reorder_track(move |track,index| { counter.set(counter.get()+1); m.borrow_mut().enqueue(Request::ReorderTrack { track:track.parse().unwrap(),index }).unwrap(); });
    let m = mailbox.clone(); ui.on_stop(move || m.borrow_mut().stop());
    ui.show()?; let mut presenter = ui_support::Presenter::new(&ui); let mut last = None;
    for (width,height,scale) in [(1280,820,1.0), (1000,760,1.0), (1000,760,2.0)] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor:scale });
        let physical_width = (width as f32*scale) as u32; let physical_height = (height as f32*scale) as u32;
        window.set_size(slint::PhysicalSize::new(physical_width,physical_height));
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(ui.get_canvas_height() >= 224.0, "Grid must show all demo notes: {}",ui.get_canvas_height());
        let pixels=snapshot(&window,&format!("../evidence/m32-arrangement-{width}-{}x.ppm",scale as u32),physical_width,physical_height);
        check_alignment(&pixels,&ui,scale);
        click(&window,ui.get_play_x(),ui.get_play_y()); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        ui.invoke_stop(); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        let x=ui.get_rows_x(); let y=ui.get_rows_y(); let row=ui.get_row_size();
        click(&window,ui.get_inspector_gain_x(),ui.get_inspector_gain_y());
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!((last.as_ref().unwrap().gain-0.7).abs()>0.05, "Slider pointer edit reaches controller");
        mailbox.borrow_mut().enqueue(Request::SetGain{track:1,value:0.7}).unwrap(); mailbox.borrow_mut().enqueue(Request::FinishMix).unwrap();
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        click(&window,x+149.0,y+52.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(last.as_ref().unwrap().tracks[0].mute, "Header M must mute the original ID");
        click(&window,ui.get_undo_x(),ui.get_undo_y()); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(!last.as_ref().unwrap().tracks[0].mute);
        click(&window,x+149.0,y+52.0); click(&window,x+149.0,y+52.0);
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(!last.as_ref().unwrap().tracks[0].mute, "Two fast toggles must both be retained");
        click(&window,x+185.0,y+52.0); click(&window,x+185.0,y+row+52.0);
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(last.as_ref().unwrap().tracks.iter().all(|r| r.solo && r.audible), "Header solo is cumulative");
        ui.set_track_name_draft("Unconfirmed typing".into()); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert_eq!(ui.get_track_name_draft().as_str(),"Unconfirmed typing", "Viewport updates preserve unfinished typing");
        click(&window,x+ui.get_header_size()+100.0,y+row+30.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert_eq!(ui.get_track_name_draft().as_str(),"Bass", "Selection resets the name field");
        assert_eq!(last.as_ref().unwrap().track_id,2); assert_eq!(last.as_ref().unwrap().clip_index,Some(0));
        assert!((ui.get_inspector_gain_value()-0.55).abs()<0.0001, "Inspector slider must follow selection after interaction");
        click(&window,ui.get_inspector_mute_x(),ui.get_inspector_mute_y()); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(last.as_ref().unwrap().tracks[1].mute && !last.as_ref().unwrap().tracks[1].audible, "Inspector edits selected ID and mute wins");
        for request in [Request::SetMute{track:2,value:false},Request::SetSolo{track:1,value:false},Request::SetSolo{track:2,value:false},Request::SelectTrack(1)] { mailbox.borrow_mut().enqueue(request).unwrap(); }
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        let before=ui.get_lane_width(); click(&window,ui.get_inspector_toggle_x(),ui.get_inspector_toggle_y());
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last); assert!(ui.get_lane_width() > before+200.0);
        click(&window,ui.get_inspector_toggle_x(),ui.get_inspector_toggle_y()); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        println!("PASS: real-controller header mute/solo, clip selection, inspector, undo, hide/show at {width}×{height}, scale {scale}");
    }
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor:1.0 }); window.set_size(slint::PhysicalSize::new(1280,820));
    for _ in 0..10 { mailbox.borrow_mut().enqueue(Request::DuplicateTrack(1)).unwrap(); }
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let x=ui.get_rows_x(); let y=ui.get_rows_y();
    wheel(&window,x+80.0,y+55.0,0.0,-160.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_arrangement_y() < -50.0, "Headers and lanes must share vertical scrolling");
    let scroll=ui.get_arrangement_y(); mailbox.borrow_mut().enqueue(Request::SetMute{track:1,value:true}).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last); assert_eq!(ui.get_arrangement_y(),scroll, "Model update must preserve scroll");
    wheel(&window,x+ui.get_header_size()+80.0,y+45.0,-120.0,0.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_arrangement_x() < 0.0, "Timeline must scroll independently of fixed headers");
    let pixels=snapshot(&window,"../evidence/m32-scrolled-12-tracks.ppm",1280,820); check_alignment(&pixels,&ui,1.0);
    ui.set_arrangement_y(0.0); ui.set_arrangement_x(0.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let first=last.as_ref().unwrap().tracks[0].id;
    pointer(&window,x+14.0,y+20.0,0); pointer(&window,x+14.0,y+ui.get_rows_height()-5.0,1);
    for _ in 0..12 { ui.invoke_tick_drag(); }
    pointer(&window,x+14.0,y+ui.get_rows_height()-5.0,2); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert_eq!(reorders.get(),1,"One drag must create one document command");
    assert!(last.as_ref().unwrap().tracks.iter().position(|r| r.id==first).unwrap()>=2, "Drag must auto-scroll and move original ID");
    ui.invoke_undo(); settle(&ui,&mailbox,&connection,&mut presenter,&mut last); assert_eq!(last.as_ref().unwrap().tracks[0].id,first);
    let before=last.as_ref().unwrap().tracks.len(); click(&window,ui.get_add_track_x(),ui.get_add_track_y());
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last); assert_eq!(last.as_ref().unwrap().tracks.len(),before+1);
    click(&window,ui.get_duplicate_track_x(),ui.get_duplicate_track_y());
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last); assert_eq!(last.as_ref().unwrap().tracks.len(),before+2);
    mailbox.borrow_mut().enqueue(Request::SelectTrack(2)).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    ui.invoke_reveal_selected(); assert!(ui.get_arrangement_y() < -500.0);
    snapshot(&window,"../evidence/m32-selected-track-revealed.ppm",1280,820);
    println!("PASS: common vertical scroll survives model edits; independent horizontal scroll; drag reorder with edge auto-scroll and one-step undo; Add / Duplicate / Show selected");
    click(&window,22.0,20.0); click(&window,80.0,60.0);
    assert!(ui.get_about_visible(), "Help opens About OpenDAW");
    snapshot(&window,"../evidence/m32-about.ppm",1280,820);
    let count=mailbox.borrow().pending_count(); click(&window,ui.get_play_x(),ui.get_play_y()); assert_eq!(mailbox.borrow().pending_count(),count,"About blocks transport");
    connection.flags.fetch_or(QUIT,Ordering::Release); connection.worker.join().unwrap();
    Ok(())
}
