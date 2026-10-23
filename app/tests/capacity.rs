use opendaw_app::{
    bridge::Engine,
    project::{Edit, History, Instrument, MAX_TRACKS},
    storage,
    synth::{SynthField, SynthParameters, Waveform},
};
#[path = "../examples/support/stress_project.rs"]
mod fixture;
#[test]
fn sixty_four_tracks_round_trip_last_slot_mix_reorder_undo_and_export() {
    let mut p = fixture::project(1024);
    assert_eq!(MAX_TRACKS, 64);
    let mut h = History::default();
    let last = p.tracks[63].id;
    let instance = p.tracks[63].instrument.id;
    let before = p.clone();
    assert!(h.execute(&mut p, Edit::AddTrack { after: last }).is_err());
    assert_eq!(p, before);
    assert!(h
        .execute(&mut p, Edit::DuplicateTrack { track: last })
        .is_err());
    assert_eq!(p, before);
    h.execute(
        &mut p,
        Edit::MoveTrack {
            track: last,
            index: 0,
        },
    )
    .unwrap();
    assert_eq!(p.tracks[0].id, last);
    h.undo(&mut p).unwrap();
    assert_eq!(p.tracks, before.tracks);
    h.redo(&mut p).unwrap();
    h.execute(
        &mut p,
        Edit::Synth {
            track: last,
            instance,
            instrument: Instrument::Synth {
                parameters: SynthParameters {
                    waveform: Waveform::Triangle,
                    ..Default::default()
                },
                preset: None,
            },
            field: Some(SynthField::Waveform),
        },
    )
    .unwrap();
    assert!(matches!(p.tracks[1].instrument.config, Instrument::Sine));
    let root = std::env::temp_dir().join(format!("opendaw-m36-capacity-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let p = storage::save(&root, &p).unwrap();
    let loaded = storage::load(&root).unwrap();
    assert_eq!(p, loaded);
    let mut e = Engine::new(48000, 256).unwrap();
    e.load(&loaded).unwrap();
    e.play().unwrap();
    assert!(e.render(2048).unwrap().iter().any(|x| x.abs() > 0.0001));
    let mut muted = loaded.clone();
    for t in &mut muted.tracks {
        t.mute = true;
    }
    assert!(e.try_mix(&muted).unwrap().is_some());
    let x = e.render(2048).unwrap();
    assert!(x[x.len() - 64..].iter().all(|x| *x == 0.));
    e.export(&root.join("muted.wav"), 8192).unwrap();
    let bytes = std::fs::read(root.join("muted.wav")).unwrap();
    assert!(bytes[44..].iter().all(|b| *b == 0));
    muted.tracks[63].mute = false;
    assert!(e.try_mix(&muted).unwrap().is_some());
    e.export(&root.join("live-settings.wav"), 8192).unwrap();
    let mut reference = Engine::new(48000, 256).unwrap();
    reference.load(&muted).unwrap();
    reference.export(&root.join("prepared.wav"), 8192).unwrap();
    assert_eq!(
        std::fs::read(root.join("live-settings.wav")).unwrap(),
        std::fs::read(root.join("prepared.wav")).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn one_hundred_thousand_notes_with_sixty_four_tracks_are_valid_without_truncation() {
    let p = fixture::project(100_000);
    assert_eq!(p.note_count(), 100_000);
    let bytes = serde_json::to_vec_pretty(&p).unwrap();
    assert!(bytes.len() < 32 * 1024 * 1024);
    assert_eq!(opendaw_app::project::Project::decode(&bytes).unwrap(), p);
    let mut engine = Engine::new(48000, 256).unwrap();
    engine.load(&p).unwrap();
    engine.play().unwrap();
    assert!(engine.render(4096).unwrap().iter().all(|x| x.is_finite()));
}
#[test]
fn factory_dedup_never_skips_a_file_claiming_the_same_hash() {
    use opendaw_app::drums::{DrumSample, FACTORY_ASSETS};
    let mut p = fixture::project(0);
    let first = p.tracks[0].id;
    let last = p.tracks[63].id;
    let mut h = History::default();
    for track in [first, last] {
        h.execute(&mut p, Edit::UseDrumKit { track }).unwrap();
    }
    let mut e = Engine::new(48000, 256).unwrap();
    e.load(&p).unwrap();
    e.play().unwrap();
    e.live_note(0, 1, 35, true).unwrap();
    e.render(256).unwrap();
    let before = e.telemetry().unwrap();
    let path = std::env::temp_dir().join(format!(
        "opendaw-m36-forged-hash-{}.wav",
        std::process::id()
    ));
    let mut bytes = FACTORY_ASSETS[0].bytes.to_vec();
    let n = bytes.len();
    bytes[n - 1] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    if let Instrument::DrumKit { kit } = &mut p.tracks[63].instrument.config {
        kit.pads[0].sample = Some(DrumSample::File {
            sample_path: path.to_str().unwrap().into(),
            sha256: FACTORY_ASSETS[0].hash.into(),
            origin: None,
        });
    }
    p.validate().unwrap();
    assert!(e.load(&p).is_err());
    let after = e.telemetry().unwrap();
    assert_eq!(after.position, before.position);
    assert_eq!(after.playing, before.playing);
    assert!(e.render(512).unwrap().iter().any(|x| *x != 0.));
    std::fs::remove_file(path).unwrap();
}
