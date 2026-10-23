//! Repeatable M3.6 fixture and retained-history/native replacement memory probe.
use opendaw_app::{
    bridge::Engine,
    project::{Edit, History, Instrument},
    storage,
    synth::{SynthField, SynthPreset},
};
use std::{path::PathBuf, sync::mpsc, time::Instant};
#[path = "support/stress_project.rs"]
mod fixture;
fn rss() -> Option<u64> {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
}
#[cfg(target_os = "macos")]
fn live_heap() -> Option<usize> {
    #[repr(C)]
    #[derive(Default)]
    struct Stats {
        blocks_in_use: u32,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }
    extern "C" {
        fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut Stats);
    }
    let mut stats = Stats::default();
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
    Some(stats.size_in_use)
}
#[cfg(not(target_os = "macos"))]
fn live_heap() -> Option<usize> {
    None
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--fixture") {
        let path = PathBuf::from(args.get(2).ok_or("Specify fixture output folder")?);
        let notes = args
            .get(3)
            .map_or(Ok(1024), |s| s.parse::<usize>().map_err(|e| e.to_string()))?;
        let p = fixture::project(notes);
        storage::save(&path, &p)?;
        println!(
            "Saved {}: {} tracks, {} notes",
            path.display(),
            p.tracks.len(),
            p.note_count()
        );
        return Ok(());
    }
    let root = std::env::temp_dir().join(format!("opendaw-m36-memory-{}", std::process::id()));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut p = fixture::project(1024);
    let first = p.tracks[0].id;
    let mut h = History::default();
    let last = p.tracks.last().unwrap().id;
    h.execute(&mut p, Edit::RemoveTrack { track: last })?;
    // One factory kit keeps the immutable CC0 data resident while snapshots/instances stay independent.
    let kit = p.tracks[62].id;
    h.execute(&mut p, Edit::UseDrumKit { track: kit })?;
    let mut e = Engine::new(48000, 256)?;
    e.load(&p)?;
    e.play()?;
    e.render(256)?;
    let snapshot = p.clone();
    let (ready, prepared) = mpsc::sync_channel(0);
    let output = root.join("parallel-export.wav");
    let output_clone = output.clone();
    let export = std::thread::spawn(move || -> Result<(), String> {
        let mut offline = Engine::new(48000, 256)?;
        offline.load(&snapshot)?;
        ready.send(()).map_err(|e| e.to_string())?;
        offline.export(&output_clone, 48000 * 5)
    });
    prepared
        .recv_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| format!("Export prepare: {e}"))?;
    let start = Instant::now();
    let mut measurements = Vec::new();
    for cycle in 0..300 {
        let preset = SynthPreset::factory(cycle % 5)?;
        let instance = p.track(first).unwrap().instrument.id;
        h.execute(
            &mut p,
            Edit::Synth {
                track: first,
                instance,
                instrument: Instrument::Synth {
                    parameters: preset.parameters,
                    preset: Some(preset),
                },
                field: Some(SynthField::Waveform),
            },
        )?;
        h.finish_gesture();
        h.execute(&mut p, Edit::DuplicateTrack { track: first })?;
        let copy = p.tracks[1].id;
        assert_eq!(p.tracks.len(), 64);
        e.load(&p)?;
        h.execute(&mut p, Edit::RemoveTrack { track: copy })?;
        assert_eq!(p.tracks.len(), 63);
        h.undo(&mut p)?;
        assert_eq!(p.tracks.len(), 64);
        h.redo(&mut p)?;
        assert_eq!(p.tracks.len(), 63);
        p.validate()?;
        if matches!(cycle, 24 | 49 | 74 | 99 | 149 | 199 | 249 | 299) {
            measurements.push((cycle + 1, rss(), live_heap()));
        }
    }
    export.join().map_err(|_| "Export worker panicked")??;
    assert!(output.exists());
    println!("M3.6 300 preset/duplicate/delete/undo/redo cycles, retained bounded history, 63/64 tracks incl. CC0 kit, parallel independent export; elapsed_s={:.3}; (cycle,current_rss_bytes,live_malloc_bytes)={measurements:?}",start.elapsed().as_secs_f64());
    // RSS includes allocator reservation/OS working sets. Check it separately from active heap,
    // after the first 100 cycles rather than treating warm-up RSS as a leak certificate.
    let after_warmup: Vec<_> = measurements.iter().filter(|x| x.0 >= 100).collect();
    let first = after_warmup[0];
    let last = after_warmup.last().unwrap();
    if let (Some(a), Some(b)) = (first.2, last.2) {
        let growth = b.saturating_sub(a);
        println!("Live malloc growth cycles 100..300: {growth} bytes; gate <=8 MiB including bounded history");
        assert!(
            growth <= 8 * 1024 * 1024,
            "Active heap keeps growing after warm-up"
        );
    }
    if let (Some(a), Some(b)) = (first.1, last.1) {
        let growth = b.saturating_sub(a);
        println!(
            "RSS growth cycles 100..300: {growth} bytes; reservation/working-set gate <=32 MiB"
        );
        assert!(
            growth <= 32 * 1024 * 1024 || args.iter().any(|a|a=="--observe"),
            "RSS has not reached a practical plateau; investigate"
        );
    }
    std::fs::remove_dir_all(root).map_err(|e| e.to_string())?;
    Ok(())
}
