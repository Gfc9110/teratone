//! Software UI exercise with the production project callbacks, isolated settings and no audio output.
use opendaw_app::{
    controller::{self, Request, Response, QUIT},
    mailbox::Mailbox,
    project_library::ProjectLibrary,
    storage,
};
use slint::{
    platform::{
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent,
    },
    ComponentHandle, Model, SharedPixelBuffer,
};
use std::{
    cell::RefCell,
    io::Write,
    rc::Rc,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
slint::include_modules!();
#[path = "../src/project_ui.rs"]
mod project_ui;
#[path = "../src/ui_support.rs"]
mod ui_support;

struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}
fn click(window: &MinimalSoftwareWindow, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}
fn pump(
    ui: &AppWindow,
    mailbox: &RefCell<Mailbox>,
    connection: &controller::Connection,
    presenter: &mut ui_support::Presenter,
    expect_error: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut quiet = 0;
    let mut failed = false;
    loop {
        mailbox.borrow_mut().flush().unwrap();
        while let Ok(response) = connection.receiver.try_recv() {
            match response {
                Response::View(view) => presenter.apply(ui, *view),
                Response::Ack {
                    request_id, error, ..
                } => {
                    mailbox.borrow_mut().acknowledge(request_id);
                    if let Some(error) = error {
                        assert!(expect_error, "{error}");
                        ui.set_action_error(error.into());
                        failed = true;
                    }
                }
            Response::DrumMeter{track,instance,status}=>presenter.drum_meter(ui,track,instance,status),
            Response::Fatal(error) => panic!("{error}"),
                _ => {}
            }
        }
        ui.set_pending_actions(mailbox.borrow().pending_count() as i32);
        if mailbox.borrow().pending_count() == 0 {
            quiet += 1;
            if quiet >= 8 {
                assert_eq!(failed, expect_error);
                return;
            }
        } else {
            quiet = 0;
        }
        assert!(Instant::now() < deadline, "Project controller timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn snapshot(window: &MinimalSoftwareWindow, name: &str, width: u32, height: u32) {
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    window.request_redraw();
    assert!(window.draw_if_needed(|r| {
        r.render(pixels.make_mut_slice(), width as usize);
    }));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../evidence/projects-{name}.ppm"));
    let mut file = std::fs::File::create(path).unwrap();
    write!(file, "P6\n{width} {height}\n255\n").unwrap();
    file.write_all(pixels.as_bytes()).unwrap();
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui = AppWindow::new()?;
    ui.set_home_visible(true);
    let root = std::env::temp_dir().join(format!("opendaw-project-ui-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let connection = controller::spawn_with_library(
        true,
        ProjectLibrary::load(root.join("settings/projects.json"))?,
    );
    let mailbox = Rc::new(RefCell::new(Mailbox::new(
        connection.sender.clone(),
        connection.flags.clone(),
        connection.epoch.clone(),
        connection.audition_epoch.clone(),
    )));
    project_ui::bind(&ui, mailbox.clone());
    ui_support::bind_editor(&ui, mailbox.clone());
    let m = mailbox.clone();
    ui.on_stop(move || m.borrow_mut().stop());
    let m = mailbox.clone();
    ui.on_rename_track(move |track, name| {
        m.borrow_mut()
            .enqueue(Request::RenameTrack {
                track: track.parse().unwrap(),
                name: name.into(),
            })
            .unwrap()
    });
    ui.show()?;
    let mut presenter = ui_support::Presenter::new(&ui);
    macro_rules! pump {
        () => {
            pump(&ui, &mailbox, &connection, &mut presenter, false)
        };
    }
    for (width, height, scale) in [(1280, 820, 1.0), (1000, 760, 1.0), (1000, 760, 2.0)] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        let (pw, ph) = (
            (width as f32 * scale) as u32,
            (height as f32 * scale) as u32,
        );
        window.set_size(slint::PhysicalSize::new(pw, ph));
        pump!();
        assert!(ui.get_home_visible());
        assert!(!ui.get_has_project());
        assert!(ui.get_home_create_x() > 0.0 && ui.get_home_create_x() < width as f32);
        assert!(ui.get_home_create_y() > 0.0 && ui.get_home_create_y() < height as f32 - 42.0);
        snapshot(
            &window,
            &format!("welcome-{width}-{}x", scale as u32),
            pw,
            ph,
        );
    }
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.set_size(slint::PhysicalSize::new(1280, 820));
    ui.set_new_name("First song".into());
    ui.set_new_parent(root.to_string_lossy().into_owned().into());
    pump!();
    snapshot(&window,"before-create-1280-1x",1280,820);
    click(&window, ui.get_home_create_x(), ui.get_home_create_y());
    pump!();
    assert!(!ui.get_home_visible());
    assert!(ui.get_has_project());
    assert_eq!(ui.get_track_items().row_count(), 1);
    let first = root.join("First song.opendaw");
    assert_eq!(storage::load(&first)?.note_count(), 0);
    assert_eq!(
        ui.get_project_path().as_str(),
        first.canonicalize()?.to_str().unwrap()
    );
    ui.set_project_tab(true);
    snapshot(&window, "project-folder-1280-1x", 1280, 820);
    ui.invoke_rename_track("1".into(), "Saved before switch".into());
    pump!();
    assert!(ui.get_dirty());
    ui.invoke_show_projects();
    pump!();
    assert!(ui.get_home_visible());
    assert_eq!(ui.get_recent_projects().row_count(), 1);
    ui.set_new_name("Second song".into());
    click(&window, ui.get_home_create_x(), ui.get_home_create_y());
    pump!();
    assert!(ui.get_confirm_switch());
    snapshot(&window, "unsaved-switch-1280-1x", 1280, 820);
    click(&window, ui.get_switch_save_x(), ui.get_switch_save_y());
    pump!();
    assert!(!ui.get_confirm_switch());
    assert!(!ui.get_home_visible());
    assert_eq!(ui.get_project_name().as_str(), "Second song");
    assert_eq!(storage::load(&first)?.tracks[0].name, "Saved before switch");
    ui.invoke_rename_track("1".into(), "Discard this edit".into());
    pump!();
    ui.invoke_show_projects();
    pump!();
    ui.invoke_request_switch(0, first.to_string_lossy().into_owned().into(), "".into());
    pump!();
    assert!(ui.get_confirm_switch());
    click(
        &window,
        ui.get_switch_discard_x(),
        ui.get_switch_discard_y(),
    );
    pump!();
    assert_eq!(ui.get_track_name_draft().as_str(), "Saved before switch");
    assert_eq!(
        storage::load(&root.join("Second song.opendaw"))?.tracks[0].name,
        "Track 1"
    );
    ui.invoke_show_save_as();
    snapshot(&window, "save-as-1280-1x", 1280, 820);
    ui.invoke_save_project_as(root.to_string_lossy().into_owned().into(), "Copy".into());
    pump!();
    assert!(!ui.get_save_as_visible());
    assert!(ui.get_project_path().ends_with("Copy.opendaw"));
    ui.invoke_show_projects();
    pump!();
    snapshot(&window, "recent-1280-1x", 1280, 820);
    assert_eq!(ui.get_recent_projects().row_count(), 3);
    // Open the actual recent-project row with a pointer, not just a callback.
    click(&window, ui.get_recent_list_x()+100.0, ui.get_recent_list_y()+80.0+36.0);
    pump!();
    assert!(!ui.get_home_visible());
    assert!(ui.get_project_path().ends_with("First song.opendaw"));
    let before = ui.get_project_path();
    ui.invoke_switch_project(
        0,
        root.join("missing").to_string_lossy().into_owned().into(),
        "".into(),
        2,
    );
    pump(&ui, &mailbox, &connection, &mut presenter, true);
    assert_eq!(ui.get_project_path(), before);
    let restored = ProjectLibrary::load(root.join("settings/projects.json"))?;
    assert_eq!(restored.recent().len(), 3);
    connection.flags.fetch_or(QUIT, Ordering::Release);
    connection.worker.join().unwrap();
    std::fs::remove_dir_all(root)?;
    println!("PASS: welcome at 1280/1000 and 100/200%, empty project creation, folder display, Save/Discard switching, recent-row click, Save as, missing-project preservation and persisted recent history. Native folder chooser and Finder launch require a windowed manual check.");
    Ok(())
}
