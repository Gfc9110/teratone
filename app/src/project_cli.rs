use opendaw_app::{bridge::Engine, project::Project, storage};
use std::{path::PathBuf, time::Duration};
fn main() {
    if let Err(e) = run() { eprintln!("OpenDAW: {e}"); std::process::exit(1); }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 { return Err("Usage: opendaw-project demo|validate|render|play PROJECT_DIR [OUTPUT.wav|--silent]".into()); }
    let root = PathBuf::from(&args[1]);
    match args[0].as_str() {
        "demo" => { storage::save(&root, &Project::demo())?; println!("Created {}", root.display()); }
        "validate" => {
            let p = storage::load(&root)?; let mut engine = Engine::new(48000, 256)?; engine.load(&p)?;
            println!("Valid project: {} tracks, {} notes", p.tracks.len(), p.tracks.iter().map(|t| t.notes.len()).sum::<usize>());
        }
        "render" => {
            let p = storage::load(&root)?; let output = PathBuf::from(args.get(2).ok_or("Specify an output WAV")?);
            storage::export_atomic(&output, &p)?; println!("Rendered {}", output.display());
        }
        "play" => {
            let p = storage::load(&root)?; let mut engine = Engine::new(48000, 256)?; engine.load(&p)?;
            let silent = args.get(2).is_some_and(|s| s == "--silent");
            engine.start_device(silent)?; engine.play()?;
            if silent { println!("SILENT CLOCK — no audio output"); }
            loop {
                std::thread::sleep(Duration::from_millis(50)); let t = engine.telemetry()?;
                if t.device_error != 0 { return Err(format!("Device failed: {}", t.device_error)); }
                if t.blocks > 0 && t.playing == 0 { break; }
            }
        }
        _ => return Err("Unknown command".into()),
    }
    Ok(())
}
