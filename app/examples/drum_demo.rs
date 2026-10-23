//! Reproducible headless factory-kit preview; first 12 beats enumerate sounds, then a 2-bar groove.
use opendaw_app::{
    bridge::Engine,
    project::{Edit, History, Note, Project},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/drum-preview.wav".into());
    let mut p = Project::demo();
    p.tracks.pop();
    p.name = "VCSL CC0 · kit preview + groove".into();
    p.length_ticks = 32 * 960;
    p.bpm = 120.;
    History::default().execute(&mut p, Edit::UseDrumKit { track: 1 })?;
    p.tracks[0].name = "CC0 Percussion".into();
    p.tracks[0].gain = 0.8;
    p.tracks[0].clips[0].name = "12 sounds, then 2-bar groove".into();
    p.tracks[0].clips[0].length_ticks = p.length_ticks;
    let pitches: Vec<_> = p.tracks[0]
        .instrument
        .config
        .drums()
        .unwrap()
        .pads
        .iter()
        .filter(|p| p.sample.is_some())
        .map(|p| p.midi_note)
        .collect();
    let mut events: Vec<(u64, u8, u8)> = pitches
        .iter()
        .enumerate()
        .map(|(i, &pitch)| (i as u64 * 960, pitch, 100))
        .collect();
    for step in 0..16 {
        let tick = 16 * 960 + step * 480;
        events.push((tick, 42, if step % 2 == 0 { 85 } else { 55 }));
        if step % 8 == 0 || step % 8 == 6 {
            events.push((tick, 35, 110));
        }
        if step % 8 == 2 || step % 8 == 6 {
            events.push((tick, 38, 100));
        }
    }
    events.push((16 * 960, 49, 85));
    events.push((23 * 960 + 480, 46, 80));
    let mut notes = Vec::new();
    for (start, pitch, velocity) in events {
        notes.push(Note {
            id: p.allocate_id()?,
            start,
            duration: 120,
            pitch,
            velocity,
        });
    }
    p.tracks[0].clips[0].notes = notes;
    p.validate()?;
    let mut engine = Engine::new(48000, 256)?;
    engine.load(&p)?;
    engine.export(std::path::Path::new(&output), 16 * 48000)?;
    if let Some(project) = std::env::args().nth(2) {
        let root = std::path::Path::new(&project);
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("project.json"), serde_json::to_vec_pretty(&p)?)?;
    }
    println!("Rendered 12 verified CC0 factory sounds + groove to {output} (16s, 48kHz stereo PCM16); human listening pending.");
    Ok(())
}
