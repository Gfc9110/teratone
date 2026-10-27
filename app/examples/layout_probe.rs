//! Software-rendered UI interaction test connected to the real controller, with no physical audio.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent, PointerEventButton};
use slint::{ComponentHandle, SharedPixelBuffer, Model};
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
    for index in 0..ui.get_track_items().row_count() {
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
fn clip_piano_scenarios(window:&MinimalSoftwareWindow,ui:&AppWindow,mailbox:&RefCell<Mailbox>,connection:&controller::Connection,presenter:&mut ui_support::Presenter,last:&mut Option<ViewState>) {
    macro_rules! pump {()=>{settle(ui,mailbox,connection,presenter,last)}}
    macro_rules! command {($r:expr)=>{{mailbox.borrow_mut().enqueue($r).unwrap();pump!();}}}
    window.dispatch_event(WindowEvent::WindowActiveChanged(true));
    // Duplicate a full-length legacy clip: extension, notes and IDs are one undoable transaction.
    click(window,ui.get_duplicate_clip_x(),ui.get_duplicate_clip_y());pump!();
    let copy=last.as_ref().unwrap().clip_id;assert_ne!(copy,4);assert_eq!(last.as_ref().unwrap().clip_start,16*960);
    assert_eq!(last.as_ref().unwrap().project_beats,32.0);assert_eq!(last.as_ref().unwrap().notes.len(),8);
    let ids:Vec<_>=last.as_ref().unwrap().notes.iter().map(|n|n.id).collect();assert!(ids.iter().all(|id|!(*id>=10 && *id<=17)));
    assert!(ui.get_arrangement_x()<0.0,"Newly selected clip is revealed");
    click(window,ui.get_remove_clip_x(),ui.get_remove_clip_y());pump!();assert_eq!(last.as_ref().unwrap().clips.len(),1);
    ui.invoke_undo();pump!();command!(Request::SelectClip{track:1,clip:copy});
    assert_eq!(last.as_ref().unwrap().notes.iter().map(|n|n.id).collect::<Vec<_>>(),ids);
    ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().project_beats,16.0);
    ui.invoke_redo();pump!();command!(Request::SelectClip{track:1,clip:copy});
    assert_eq!(last.as_ref().unwrap().notes.iter().map(|n|n.id).collect::<Vec<_>>(),ids);
    ui.invoke_undo();pump!();
    // Pointer move across tracks and right-edge resize, committed once on release.
    click(window,ui.get_new_clip_x(),ui.get_new_clip_y());pump!();let new=last.as_ref().unwrap().clip_id;
    assert_eq!(last.as_ref().unwrap().clip_length,4*960);assert_eq!(last.as_ref().unwrap().clip_start,16*960);
    let x=ui.get_rows_x()+ui.get_header_size()+16.0*ui.get_arrangement_zoom()+ui.get_arrangement_x()+30.0;
    let y=ui.get_rows_y()+25.0;
    pointer(window,x,y,0);pointer(window,x+ui.get_arrangement_zoom(),y+ui.get_row_size(),1);
    snapshot(window,"../evidence/m36-clip-drag-preview.ppm",1280,820);
    pointer(window,x+ui.get_arrangement_zoom(),y+ui.get_row_size(),2);pump!();
    assert_eq!(last.as_ref().unwrap().track_id,2);assert_eq!(last.as_ref().unwrap().clip_id,new);assert_eq!(last.as_ref().unwrap().clip_start,17*960);
    ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().track_id,1);assert_eq!(last.as_ref().unwrap().clip_start,16*960);
    let edge=ui.get_rows_x()+ui.get_header_size()+20.0*ui.get_arrangement_zoom()+ui.get_arrangement_x()-5.0;
    pointer(window,edge,y,0);pointer(window,edge+2.0*ui.get_arrangement_zoom(),y,1);pointer(window,edge+2.0*ui.get_arrangement_zoom(),y,2);pump!();
    assert_eq!(last.as_ref().unwrap().clip_length,6*960);ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().clip_length,4*960);
    ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().clips.len(),1);
    // Empty tracks can be edited again by double-clicking a lane.
    click(window,ui.get_remove_clip_x(),ui.get_remove_clip_y());pump!();assert_eq!(last.as_ref().unwrap().clip_id,0);
    ui.set_arrangement_x(0.0);pump!();let x=ui.get_rows_x()+ui.get_header_size()+3.0*ui.get_arrangement_zoom();
    click(window,x,y);click(window,x,y);pump!();let clip=last.as_ref().unwrap().clip_id;
    assert_ne!(clip,0);assert_eq!(last.as_ref().unwrap().clip_start,3*960);assert!(last.as_ref().unwrap().notes.is_empty());
    let revision=last.as_ref().unwrap().revision;
    let key=last.as_ref().unwrap().keys.iter().find(|k|k.pitch==60).unwrap();let key_y=ui.get_keyboard_y()+key.y+key.height/2.0;let key_x=ui.get_keyboard_x()+45.0;
    pointer(window,key_x,key_y,0);pump!();assert_eq!(last.as_ref().unwrap().live_count,1,"Held key must stay sounding through ACK and view updates");
    pointer(window,key_x,key_y,2);pump!();assert_eq!(last.as_ref().unwrap().live_count,0);assert_eq!(last.as_ref().unwrap().revision,revision);
    pointer(window,key_x,key_y,0);pump!();window.dispatch_event(WindowEvent::PointerExited);pump!();assert_eq!(last.as_ref().unwrap().live_count,0,"Window exit releases audition");
    pointer(window,key_x,key_y,0);pump!();window.dispatch_event(WindowEvent::WindowActiveChanged(false));pump!();assert_eq!(last.as_ref().unwrap().live_count,0,"Window focus loss releases audition");
    window.dispatch_event(WindowEvent::WindowActiveChanged(true));pointer(window,key_x,key_y,2);pump!();
    pointer(window,key_x,key_y,0);pump!();command!(Request::SelectTrack(2));assert_eq!(last.as_ref().unwrap().live_count,0);pointer(window,key_x,key_y,2);pump!();command!(Request::SelectClip{track:1,clip});
    // A keyboard click must never insert a note; the grid beside it inserts exactly C4.
    assert!(last.as_ref().unwrap().notes.is_empty());let key=last.as_ref().unwrap().keys.iter().find(|k|k.pitch==60).unwrap();
    click(window,ui.get_grid_x()+ui.get_pixels_per_beat()*0.5+2.0,ui.get_grid_y()+key.y+key.height/2.0);pump!();
    assert_eq!(last.as_ref().unwrap().notes.len(),1);assert_eq!(last.as_ref().unwrap().notes[0].label,"C4");
    assert_ne!(last.as_ref().unwrap().selected_note,0);ui.invoke_velocity(37);pump!();assert_eq!(last.as_ref().unwrap().velocity,37);
    ui.invoke_rename_clip("1".into(),clip.to_string().into(),"Verse".into());pump!();assert_eq!(last.as_ref().unwrap().clip_name,"Verse");
    let revision=last.as_ref().unwrap().revision;
    for (offset,label) in [(-1,"C3"),(1,"C5"),(0,"C4")] {
        ui.invoke_notation(offset);pump!();assert_eq!(last.as_ref().unwrap().notes[0].label,label);
        assert_eq!(last.as_ref().unwrap().keys.iter().find(|k|k.pitch==60).unwrap().label,label);assert_eq!(last.as_ref().unwrap().revision,revision);
    }
    for (width,height,scale,rh) in [(1280,820,1.0,12.0),(1000,760,1.0,20.0),(1000,760,2.0,32.0)] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:scale});let pw=(width as f32*scale) as u32;let ph=(height as f32*scale) as u32;window.set_size(slint::PhysicalSize::new(pw,ph));pump!();
        command!(Request::ZoomY(rh/last.as_ref().unwrap().viewport.row_height));command!(Request::ScrollPiano(0.0,(127.0-64.0)*rh+0.375));
        let v=last.as_ref().unwrap();let key=v.keys.iter().find(|k|k.pitch==60).unwrap();let note=&v.notes[0];
        assert!((key.y+key.height/2.0-note.y-note.height/2.0).abs()<1.0,"Key and note centers align at every zoom/scale");
        snapshot(window,&format!("../evidence/m36-piano-{width}-{}x-row{}.ppm",scale as u32,rh as u32),pw,ph);
        let old_y=ui.get_keyboard_y();let old_scroll=ui.get_piano_scroll_y();wheel(window,ui.get_keyboard_x()+35.0,old_y+50.0,0.0,-21.5);pump!();assert!(ui.get_piano_scroll_y()>old_scroll);
    }
    // Save and reopen through the real persistence layer: notation did not change MIDI 60.
    let path=std::env::temp_dir().join(format!("opendaw-m36-ui-{}",std::process::id()));command!(Request::Save(path.clone()));
    let saved=opendaw_app::storage::load(&path).unwrap();let c=saved.clip(1,clip).unwrap();assert_eq!(c.notes.len(),1);assert_eq!(c.notes[0].pitch,60);assert_eq!(c.notes[0].velocity,37);assert_eq!(c.start_tick,3*960);assert_eq!(c.name,"Verse");std::fs::remove_dir_all(path).unwrap();
    window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.0});window.set_size(slint::PhysicalSize::new(1280,820));command!(Request::FitNotes);
    println!("PASS M3.3: pointer clip CRUD/transfer/resize, one-step undo/redo, extension, empty-lane creation, stable IDs and portable save");
    println!("PASS M3.3: fixed keyboard, C3/C4/C5 notation, MIDI 60 insertion, velocity, fractional scroll, three vertical zooms, 100/200% alignment");
    println!("PASS M3.3: keyboard audition never records; hold/release, window exit, focus loss and track change release the original token");
}

fn synth_scenarios(window:&MinimalSoftwareWindow,ui:&AppWindow,mailbox:&RefCell<Mailbox>,connection:&controller::Connection,presenter:&mut ui_support::Presenter,last:&mut Option<ViewState>){
    use opendaw_app::synth::{Waveform,SynthField};
    macro_rules! pump {()=>{settle(ui,mailbox,connection,presenter,last)}}
    macro_rules! command {($r:expr)=>{{mailbox.borrow_mut().enqueue($r).unwrap();pump!();}}}
    command!(Request::SelectTrack(1));let second=last.as_ref().unwrap().tracks.iter().find(|t|t.id==2).unwrap().clone();
    for (width,height,scale) in [(1280,820,1.0),(1000,760,1.0),(1000,760,2.0)]{
        window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:scale});let pw=(width as f32*scale) as u32;let ph=(height as f32*scale) as u32;window.set_size(slint::PhysicalSize::new(pw,ph));pump!();
        click(window,ui.get_synth_sound_tab_x(),ui.get_synth_sound_tab_y());pump!();ui.set_factory_index(2);
        click(window,ui.get_synth_preset_apply_x(),ui.get_synth_preset_apply_y());pump!();assert_eq!(last.as_ref().unwrap().synth.unwrap().waveform,Waveform::Square);
        click(window,ui.get_synth_shape_x(),ui.get_synth_shape_y());pump!();assert!(last.as_ref().unwrap().synth.unwrap().shape<0.9);assert!(ui.get_preset_label().contains("modified"));
        ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().synth.unwrap().shape,1.0);
        // Pointer drag is one history step; changing PWM never changes the other track.
        let x=ui.get_synth_duty_x();let y=ui.get_synth_duty_y();pointer(window,x,y,0);pointer(window,x+45.0,y,1);pointer(window,x+45.0,y,2);pump!();
        assert!(last.as_ref().unwrap().synth.unwrap().pulse_width>0.6);assert_eq!(last.as_ref().unwrap().tracks.iter().find(|t|t.id==2).unwrap(),&second);
        ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().synth.unwrap().pulse_width,0.25);
        // The actual combo keyboard interaction reaches the controller via the same adapter.
        click(window,ui.get_synth_waveform_x(),ui.get_synth_waveform_y());
        for key in [slint::platform::Key::DownArrow,slint::platform::Key::Return]{
            let text:slint::SharedString=key.into();window.dispatch_event(WindowEvent::KeyPressed{text:text.clone()});window.dispatch_event(WindowEvent::KeyReleased{text});
        }pump!();assert_eq!(last.as_ref().unwrap().synth.unwrap().waveform,Waveform::Triangle);
        let attack=last.as_ref().unwrap().synth.unwrap().attack;click(window,ui.get_synth_attack_x()+10.0,ui.get_synth_attack_y());pump!();assert!(last.as_ref().unwrap().synth.unwrap().attack>attack);
        snapshot(window,&format!("../evidence/m36-synth-{width}-{}x.ppm",scale as u32),pw,ph);
        let path=std::env::temp_dir().join(format!("opendaw-m36-ui-preset-{}-{}.json",std::process::id(),scale));
        ui.set_preset_path(path.to_str().unwrap().into());ui.set_preset_name("UI Triangle".into());
        wheel(window,ui.get_synth_save_preset_x(),height as f32-130.0,0.0,-500.0);pump!();
        let y=ui.get_synth_save_preset_y();assert!(y>130.0 && y<height as f32-55.0,"Preset controls reachable by scroll: {y}");
        click(window,ui.get_synth_save_preset_x(),y);pump!();assert!(path.exists());
        let saved=opendaw_app::storage::load_synth_preset(&path).unwrap();let instance=last.as_ref().unwrap().instance_id;
        command!(Request::SynthParameter{track:1,instance,field:SynthField::Attack,value:0.7});
        click(window,ui.get_synth_load_preset_x(),ui.get_synth_load_preset_y());pump!();assert_eq!(last.as_ref().unwrap().synth,Some(saved.parameters));
        snapshot(window,&format!("../evidence/m36-synth-presets-{width}-{}x.ppm",scale as u32),pw,ph);std::fs::remove_file(path).unwrap();
        ui.set_sound_tab(false);ui.set_inspector_scroll(0.0);pump!();
    }
    let instance=last.as_ref().unwrap().instance_id;command!(Request::SynthParameter{track:1,instance,field:SynthField::Waveform,value:0.0});
    ui.set_sound_tab(true);ui.set_inspector_scroll(0.0);pump!();let revision=last.as_ref().unwrap().revision;
    click(window,ui.get_synth_shape_x(),ui.get_synth_shape_y());click(window,ui.get_synth_duty_x(),ui.get_synth_duty_y());pump!();
    assert_eq!(last.as_ref().unwrap().revision,revision,"Inactive sine Shape/PWM controls do not edit the document");ui.invoke_undo();pump!();
    ui.set_sound_tab(false);ui.set_inspector_scroll(0.0);pump!();
    command!(Request::DuplicateTrack(1));let copy=last.as_ref().unwrap().track_id;let instance=last.as_ref().unwrap().instance_id;
    command!(Request::FactoryPreset{track:copy,instance,index:1});command!(Request::SelectTrack(1));assert_eq!(last.as_ref().unwrap().synth.unwrap().waveform,Waveform::Triangle);
    command!(Request::RemoveTrack(copy));command!(Request::SelectTrack(1));
    ui.set_sound_tab(false);ui.set_inspector_scroll(0.0);window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.0});window.set_size(slint::PhysicalSize::new(1280,820));pump!();
    println!("PASS M3.4: pointer preset/shape/PWM/ADSR, waveform combo, modified indicator, one-step undo, independent duplicate, preset save/load at 1280/1000 logical pixels and 100/200% scaling");
}

fn drum_scenarios(window:&MinimalSoftwareWindow,ui:&AppWindow,mailbox:&RefCell<Mailbox>,connection:&controller::Connection,presenter:&mut ui_support::Presenter,last:&mut Option<ViewState>){
    use opendaw_app::drums::FACTORY_ASSETS;
    macro_rules! pump {()=>{settle(ui,mailbox,connection,presenter,last)}}
    macro_rules! command {($r:expr)=>{{mailbox.borrow_mut().enqueue($r).unwrap();pump!();}}}
    click(window,ui.get_drums_add_x(),ui.get_drums_add_y());pump!();let track=last.as_ref().unwrap().track_id;let instance=last.as_ref().unwrap().instance_id;
    assert_eq!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads.len(),16);
    let source=std::env::temp_dir().join(format!("opendaw-m36-ui-import-{}.wav",std::process::id()));std::fs::write(&source,FACTORY_ASSETS[1].bytes).unwrap();
    for (width,height,scale) in [(1280,820,1.0),(1000,760,1.0),(1000,760,2.0)]{
        window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:scale});let pw=(width as f32*scale) as u32;let ph=(height as f32*scale) as u32;window.set_size(slint::PhysicalSize::new(pw,ph));
        ui.set_sound_tab(true);ui.set_inspector_scroll(0.0);pump!();
        let pad=last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].id;
        let x=ui.get_drums_grid_x()-ui.get_drums_grid_width()*0.375;let y=ui.get_drums_grid_y()-81.0;
        let revision=last.as_ref().unwrap().revision;ui.set_pad_preview_velocity(37);pointer(window,x,y,0);pump!();
        assert_eq!(ui.get_pad_pulses().row_data(0),Some(true),"Native trigger meter flashes the pad");
        assert_eq!(last.as_ref().unwrap().selected_pad,pad);assert_eq!(last.as_ref().unwrap().live_count,1,"Pad hold survives selection/viewport update");
        pointer(window,x,y,2);pump!();assert_eq!(last.as_ref().unwrap().live_count,0);assert_eq!(last.as_ref().unwrap().revision,revision);assert!(last.as_ref().unwrap().notes.is_empty());
        pointer(window,x,y,0);pump!();window.dispatch_event(WindowEvent::WindowActiveChanged(false));pump!();assert_eq!(last.as_ref().unwrap().live_count,0);
        window.dispatch_event(WindowEvent::WindowActiveChanged(true));pointer(window,x,y,2);pump!();
        let cx=ui.get_drums_grid_x()+ui.get_drums_grid_width()*0.375;let cy=ui.get_drums_grid_y()+27.0;
        pointer(window,cx,cy,0);pump!();assert_eq!(last.as_ref().unwrap().live_count,1,"Preview survives vertical scroll to a distant pad");
        assert!(last.as_ref().unwrap().keys.iter().any(|k|k.pitch==75));pointer(window,cx,cy,2);pump!();click(window,x,y);pump!();
        snapshot(window,&format!("../evidence/m36-drums-pads-{width}-{}x.ppm",scale as u32),pw,ph);
        wheel(window,x,height as f32-140.0,0.0,-280.0);pump!();
        assert!(ui.get_drums_mute_y()>140.0 && ui.get_drums_gain_y()<height as f32-50.0,"Pad controls reachable by scroll");
        click(window,ui.get_drums_mute_x(),ui.get_drums_mute_y());pump!();assert!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].parameters.mute);
        ui.invoke_undo();pump!();assert!(!last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].parameters.mute);
        click(window,ui.get_drums_mode_x(),ui.get_drums_mode_y());pump!();assert!(!last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].parameters.one_shot);ui.invoke_undo();pump!();
        let old=last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].parameters.gain;
        let x=ui.get_drums_gain_x();let y=ui.get_drums_gain_y();pointer(window,x,y,0);pointer(window,x+25.0,y,1);pointer(window,x+25.0,y,2);pump!();
        assert_ne!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].parameters.gain,old);ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].parameters.gain,old);
        ui.set_pad_midi(37);click(window,ui.get_drums_map_x(),ui.get_drums_map_y());pump!();assert_eq!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].midi_note,37);ui.invoke_undo();pump!();
        ui.invoke_rename_pad("UI Kick".into());pump!();assert!(last.as_ref().unwrap().keys.iter().any(|k|k.label=="35 UI Kick"));ui.invoke_undo();pump!();
        wheel(window,x,height as f32-140.0,0.0,-1000.0);pump!();ui.set_pad_import_path(source.to_str().unwrap().into());
        let iy=ui.get_drums_import_y();assert!(iy>140.0 && iy<height as f32-50.0,"Import reachable: {iy}");
        click(window,ui.get_drums_import_x(),iy);pump!();assert_eq!(ui.get_pad_import_path().as_str(),source.to_str().unwrap());assert!(matches!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].sample,Some(opendaw_app::drums::DrumSample::File{..})));
        click(window,ui.get_drums_clear_x(),ui.get_drums_clear_y());pump!();assert!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].sample.is_none());ui.invoke_undo();pump!();assert!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads[0].sample.is_some());
        snapshot(window,&format!("../evidence/m36-drums-import-{width}-{}x.ppm",scale as u32),pw,ph);ui.invoke_undo();pump!();
    }
    command!(Request::DuplicateTrack(track));let copy=last.as_ref().unwrap().track_id;let other=last.as_ref().unwrap().drum_kit.clone().unwrap();
    command!(Request::SelectTrack(track));assert!(last.as_ref().unwrap().drum_kit.as_ref().unwrap().pads.iter().all(|p|other.pads.iter().all(|q|p.id!=q.id)));
    let path=std::env::temp_dir().join(format!("opendaw-m36-ui-drums-{}",std::process::id()));command!(Request::Save(path.clone()));let p=opendaw_app::storage::load(&path).unwrap();assert!(p.track(track).unwrap().instrument.config.drums().is_some());std::fs::remove_dir_all(path).unwrap();
    command!(Request::RemoveTrack(copy));command!(Request::RemoveTrack(track));command!(Request::SelectTrack(1));
    assert_ne!(last.as_ref().unwrap().instance_id,instance);std::fs::remove_file(source).unwrap();ui.set_sound_tab(false);ui.set_inspector_scroll(0.0);
    window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.0});window.set_size(slint::PhysicalSize::new(1280,820));pump!();
    println!("PASS M3.5: pointer pads/preview/focus cancellation, gain gesture undo, mute/mode/map, naming, WAV replacement/clear/undo, independent duplicate and portable save at 1280/1000 logical pixels and 100/200% scaling");
}

fn fast_pump(ui:&AppWindow,mailbox:&RefCell<Mailbox>,connection:&controller::Connection,presenter:&mut ui_support::Presenter,last:&mut Option<ViewState>){
    let deadline=Instant::now()+Duration::from_secs(4);let mut viewed=false;
    loop{mailbox.borrow_mut().flush().unwrap();while let Ok(response)=connection.receiver.try_recv(){match response{
        Response::View(v)=>{presenter.apply(ui,(*v).clone());*last=Some(*v);viewed=true;},
        Response::Ack{request_id,error,..}=>{assert!(error.is_none(),"{error:?}");mailbox.borrow_mut().acknowledge(request_id);},
        Response::DrumMeter{track,instance,status}=>presenter.drum_meter(ui,track,instance,status),Response::Fatal(e)=>panic!("{e}"),_=>{}
    }}if viewed&&mailbox.borrow().pending_count()==0{return;}assert!(Instant::now()<deadline,"Large-project request timed out");std::thread::sleep(Duration::from_micros(100));}
}
fn large_scenarios(window:&MinimalSoftwareWindow,ui:&AppWindow,mailbox:&RefCell<Mailbox>,connection:&controller::Connection,presenter:&mut ui_support::Presenter,last:&mut Option<ViewState>){
    #[path="support/stress_project.rs"] mod fixture;
    macro_rules! pump{()=>{settle(ui,mailbox,connection,presenter,last)}}
    macro_rules! command{($r:expr)=>{{mailbox.borrow_mut().enqueue($r).unwrap();pump!();}}}
    let root=std::env::temp_dir().join(format!("opendaw-m36-large-ui-{}",std::process::id()));let previous=root.join("previous");let large=root.join("large");std::fs::create_dir_all(&large).unwrap();
    command!(Request::Save(previous));let p=fixture::project(100_000);std::fs::write(large.join("project.json"),serde_json::to_vec_pretty(&p).unwrap()).unwrap();command!(Request::Open(large.clone()));
    assert_eq!(last.as_ref().unwrap().tracks.len(),64);assert_eq!(ui.get_track_limit(),64);
    let pending=mailbox.borrow().pending_count();click(window,ui.get_add_track_x(),ui.get_add_track_y());click(window,ui.get_drums_add_x(),ui.get_drums_add_y());click(window,ui.get_duplicate_track_x(),ui.get_duplicate_track_y());assert_eq!(mailbox.borrow().pending_count(),pending,"Creation controls disabled at 64");
    let ids:Vec<_>=p.tracks.iter().map(|t|t.id).collect();
    for(width,height,scale)in[(1280,820,1.0),(1000,760,1.0),(1000,760,2.0)]{
        window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:scale});let pw=(width as f32*scale)as u32;let ph=(height as f32*scale)as u32;window.set_size(slint::PhysicalSize::new(pw,ph));pump!();
        ui.set_arrangement_y(0.0);let top=snapshot(window,&format!("../evidence/m36-64-top-{width}-{}x.ppm",scale as u32),pw,ph);check_alignment(&top,ui,scale);
        command!(Request::SelectTrack(ids[63]));ui.invoke_reveal_selected();assert!(ui.get_arrangement_y()< -60.0*ui.get_row_size());
        let bottom=snapshot(window,&format!("../evidence/m36-64-bottom-{width}-{}x.ppm",scale as u32),pw,ph);check_alignment(&bottom,ui,scale);
        command!(Request::ReorderTrack{track:ids[63],index:0});assert_eq!(last.as_ref().unwrap().tracks[0].id,ids[63]);ui.invoke_reveal_selected();let reordered=snapshot(window,&format!("../evidence/m36-64-reordered-{width}-{}x.ppm",scale as u32),pw,ph);check_alignment(&reordered,ui,scale);
        ui.invoke_undo();pump!();assert_eq!(last.as_ref().unwrap().tracks[63].id,ids[63]);ui.invoke_reveal_selected();
        for id in [ids[0],ids[63]]{command!(Request::SelectTrack(id));command!(Request::ZoomY(16.0/last.as_ref().unwrap().viewport.row_height));command!(Request::ScrollPiano(0.0,55.0*16.0));}
        let mut pixels=SharedPixelBuffer::<slint::Rgb8Pixel>::new(pw,ph);let mut responses=Vec::new();let mut frames=Vec::new();let mut max_notes=0;
        for i in 0..120{let action=match i%3{0=>Request::SelectTrack(ids[if i%2==0{0}else{63}]),1=>Request::ScrollPiano((i%8)as f64/8.0,55.0*16.0),_=>Request::Zoom(if i%2==0{1.01}else{1.0/1.01})};
            let begin=Instant::now();mailbox.borrow_mut().enqueue(action).unwrap();fast_pump(ui,mailbox,connection,presenter,last);let frame=Instant::now();window.request_redraw();assert!(window.draw_if_needed(|r|{r.render(pixels.make_mut_slice(),pw as usize);}));frames.push(frame.elapsed().as_secs_f64()*1000.0);responses.push(begin.elapsed().as_secs_f64()*1000.0);
            let count=last.as_ref().unwrap().notes.len();max_notes=max_notes.max(count);assert!(count<=1500);
        }
        frames.sort_by(f64::total_cmp);responses.sort_by(f64::total_cmp);println!("M3.6 UI {width}x{height} scale={scale}: 64 tracks/100000 notes, max_visible={max_notes}, n=120 response_to_paint_p95_ms={:.3} max_ms={:.3}; software_frame_p95_ms={:.3} max_ms={:.3}",responses[113],responses[119],frames[113],frames[119]);
        assert!(responses[113]<=50.0 && responses[119]<=100.0 && frames[113]<=16.7,"Declared software UI performance gates");
        assert!(max_notes>=1400,"Exercise close to 1500 visible notes, not just an empty viewport");
    }
    command!(Request::SelectTrack(ids[63]));command!(Request::Play);
    for i in 0..1000{ui.invoke_set_gain(ids[63].to_string().into(),i as f32/10000.0);}
    assert_eq!(mailbox.borrow().pending_count(),1,"1000 UI slider updates coalesce for track 64");ui.invoke_finish_mix();pump!();
    assert!((last.as_ref().unwrap().gain-0.0999).abs()<0.00001);assert_eq!(last.as_ref().unwrap().mix_label,"Sound synchronized");
    mailbox.borrow_mut().stop();pump!();assert!((last.as_ref().unwrap().gain-0.0999).abs()<0.00001,"Stop retains the latest persistent target");
    command!(Request::SetGain{track:ids[63],value:0.003});command!(Request::FinishMix);
    println!("PASS M3.6: 1000 UI slider updates on track 64 coalesce to the last target and synchronize with native audio; Stop retains edits");
    command!(Request::Save(large.clone()));let saved=opendaw_app::storage::load(&large).unwrap();assert_eq!(saved.tracks.len(),64);assert_eq!(saved.note_count(),100_000);std::fs::remove_dir_all(root).unwrap();
    window.dispatch_event(WindowEvent::ScaleFactorChanged{scale_factor:1.0});window.set_size(slint::PhysicalSize::new(1280,820));pump!();
    println!("PASS M3.6: 64 headers/lanes aligned <=1 logical pixel at top/bottom/reorder/undo, limit controls, selection reveal, 100k-note portable round trip at 100/200%");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui = AppWindow::new()?; let connection = controller::spawn(true);
    let mailbox = Rc::new(RefCell::new(Mailbox::new(connection.sender.clone(), connection.flags.clone(), connection.epoch.clone(),connection.audition_epoch.clone())));
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
    ui_support::bind_editor(&ui,mailbox.clone());
    ui.show()?; let mut presenter = ui_support::Presenter::new(&ui); let mut last = None;
    for (width,height,scale) in [(1280,820,1.0), (1000,760,1.0), (1000,760,2.0)] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor:scale });
        let physical_width = (width as f32*scale) as u32; let physical_height = (height as f32*scale) as u32;
        window.set_size(slint::PhysicalSize::new(physical_width,physical_height));
        settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
        assert!(ui.get_canvas_height() >= 180.0, "Grid must show all demo notes: {}",ui.get_canvas_height());
        let pixels=snapshot(&window,&format!("../evidence/m36-arrangement-{width}-{}x.ppm",scale as u32),physical_width,physical_height);
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
    clip_piano_scenarios(&window,&ui,&mailbox,&connection,&mut presenter,&mut last);
    synth_scenarios(&window,&ui,&mailbox,&connection,&mut presenter,&mut last);
    drum_scenarios(&window,&ui,&mailbox,&connection,&mut presenter,&mut last);
    for _ in 0..10 { mailbox.borrow_mut().enqueue(Request::DuplicateTrack(1)).unwrap(); }
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    let x=ui.get_rows_x(); let y=ui.get_rows_y();
    wheel(&window,x+80.0,y+55.0,0.0,-160.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_arrangement_y() < -50.0, "Headers and lanes must share vertical scrolling");
    let scroll=ui.get_arrangement_y(); mailbox.borrow_mut().enqueue(Request::SetMute{track:1,value:true}).unwrap();
    settle(&ui,&mailbox,&connection,&mut presenter,&mut last); assert_eq!(ui.get_arrangement_y(),scroll, "Model update must preserve scroll");
    wheel(&window,x+ui.get_header_size()+80.0,y+45.0,-120.0,0.0); settle(&ui,&mailbox,&connection,&mut presenter,&mut last);
    assert!(ui.get_arrangement_x() < 0.0, "Timeline must scroll independently of fixed headers");
    let pixels=snapshot(&window,"../evidence/m36-scrolled-12-tracks.ppm",1280,820); check_alignment(&pixels,&ui,1.0);
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
    snapshot(&window,"../evidence/m36-selected-track-revealed.ppm",1280,820);
    println!("PASS: common vertical scroll survives model edits; independent horizontal scroll; drag reorder with edge auto-scroll and one-step undo; Add / Duplicate / Show selected");
    large_scenarios(&window,&ui,&mailbox,&connection,&mut presenter,&mut last);
    click(&window,67.0,20.0); click(&window,80.0,60.0);
    assert!(ui.get_about_visible(), "Help opens About OpenDAW");
    snapshot(&window,"../evidence/m36-about.ppm",1280,820);
    let count=mailbox.borrow().pending_count(); click(&window,ui.get_play_x(),ui.get_play_y()); assert_eq!(mailbox.borrow().pending_count(),count,"About blocks transport");
    connection.flags.fetch_or(QUIT,Ordering::Release); connection.worker.join().unwrap();
    Ok(())
}
