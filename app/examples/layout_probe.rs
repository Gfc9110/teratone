use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{ComponentHandle, ModelRc, SharedPixelBuffer, VecModel};
use std::{cell::Cell, io::Write, path::Path, rc::Rc};
slint::include_modules!();

struct ProbePlatform(Rc<MinimalSoftwareWindow>);
impl Platform for ProbePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}
fn snapshot(
    window: &MinimalSoftwareWindow,
    path: &Path,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(pixels.make_mut_slice(), width as usize);
    }));
    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{width} {height}\n255\n")?;
    file.write_all(pixels.as_bytes())?;
    Ok(())
}
fn click(window: &MinimalSoftwareWindow, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ProbePlatform(window.clone())))?;
    let ui = AppWindow::new()?;
    let project = opendaw_app::project::Project::demo();
    ui.set_track_items(ModelRc::new(VecModel::from(project.tracks.iter().map(|t| TrackVisual {
        id: t.id.to_string().into(), name: t.name.clone().into(), instrument: t.instrument.config.label().into(),
        selected: t.id == 1, tint: slint::Color::from_argb_encoded(0xff000000 | u32::from_str_radix(&t.color[1..], 16).unwrap()),
    }).collect::<Vec<_>>())));
    ui.set_clip_names(ModelRc::new(VecModel::from(vec!["Main".into()])));
    ui.set_clip_ids(ModelRc::new(VecModel::from(vec!["4".into()])));
    let viewport = opendaw_app::editor::Viewport { top_note: 80, ..Default::default() };
    ui.set_top_note(viewport.top_note as i32);
    let (notes, _) = viewport.scene(&project, 1, 4, None, None);
    ui.set_note_items(ModelRc::new(VecModel::from(
        notes
            .into_iter()
            .map(|note| NoteVisual {
                x: note.x,
                y: note.y,
                w: note.width,
                h: note.height,
                selected: note.selected,
                label: note.label.into(),
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_status("Layout probe: audio is disabled.".into());
    ui.set_can_undo(true);
    let plays = Rc::new(Cell::new(0));
    let counter = plays.clone();
    ui.on_play(move || counter.set(counter.get() + 1));
    let undos = Rc::new(Cell::new(0));
    let counter = undos.clone();
    ui.on_undo(move || counter.set(counter.get() + 1));
    let additions = Rc::new(Cell::new(0)); let counter = additions.clone();
    ui.on_add_track(move |id| { assert_eq!(id.as_str(), "1"); counter.set(counter.get() + 1); });
    let duplicates = Rc::new(Cell::new(0)); let counter = duplicates.clone();
    ui.on_duplicate_track(move |id| { assert_eq!(id.as_str(), "1"); counter.set(counter.get() + 1); });
    ui.show()?;
    for (width, height) in [(1280, 820), (1000, 760)] {
        window.set_size(slint::PhysicalSize::new(width, height));
        snapshot(
            &window,
            Path::new(&format!("../evidence/m3-tracks-{width}.ppm")),
            width,
            height,
        )?;
        let previous_plays = plays.get();
        let previous_undos = undos.get();
        click(&window, 277.0, 77.0);
        click(&window, 270.0, 144.0);
        assert_eq!(
            plays.get(),
            previous_plays + 1,
            "Play must receive the pointer click"
        );
        assert_eq!(
            undos.get(),
            previous_undos + 1,
            "Undo must receive the pointer click"
        );
        let previous_adds = additions.get(); let previous_duplicates = duplicates.get();
        assert!(ui.get_add_track_x() > 0.0 && ui.get_add_track_y() < height as f32 - 48.0);
        assert!(ui.get_duplicate_track_x() < width as f32 && ui.get_duplicate_track_y() < height as f32 - 48.0);
        click(&window, ui.get_add_track_x(), ui.get_add_track_y());
        click(&window, ui.get_duplicate_track_x(), ui.get_duplicate_track_y());
        assert_eq!(additions.get(), previous_adds + 1, "Add track must be visible and receive clicks");
        assert_eq!(duplicates.get(), previous_duplicates + 1, "Duplicate must be visible and preserve the track ID");
        println!("PASS: Play, Undo, Add and Duplicate receive clicks at {width} x {height}");
    }
    click(&window, 24.0, 20.0);
    snapshot(
        &window,
        Path::new("../evidence/m3-help-menu.ppm"),
        1000,
        760,
    )?;
    click(&window, 80.0, 60.0);
    assert!(ui.get_about_visible(), "Help menu must open About OpenDAW");
    println!("PASS: Help menu opens About OpenDAW");
    snapshot(&window, Path::new("../evidence/m3-about.ppm"), 1000, 760)?;
    let previous_plays = plays.get();
    click(&window, 277.0, 77.0);
    assert_eq!(
        plays.get(),
        previous_plays,
        "About modal must block the transport"
    );
    click(&window, 500.0, 520.0);
    assert!(
        !ui.get_about_visible(),
        "Close must dismiss the About dialog"
    );
    println!("PASS: About dialog blocks underlying controls and closes");
    Ok(())
}
