use opendaw_app::{
    bridge::Engine,
    drums::{self, DrumKit, DrumSample, PadField},
    project::{Edit, History, Instrument, Project},
    storage,
};
use std::path::PathBuf;
fn kit_project() -> Project {
    let mut p = Project::demo();
    p.tracks.pop();
    History::default()
        .execute(&mut p, Edit::UseDrumKit { track: 1 })
        .unwrap();
    p.tracks[0].clips[0].notes.clear();
    p.validate().unwrap();
    p
}
fn root(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("opendaw-m35-{name}-{}", std::process::id()));
    if p.exists() {
        std::fs::remove_dir_all(&p).unwrap();
    }
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn all_twelve_factory_assets_verify_and_sound_through_v4() {
    assert_eq!(
        drums::sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let p = kit_project();
    let kit = p.tracks[0].instrument.config.drums().unwrap();
    assert_eq!(kit.pads.len(), 16);
    assert_eq!(kit.pads.iter().filter(|p| p.sample.is_some()).count(), 12);
    let mut e = Engine::new(48000, 256).unwrap();
    e.load(&p).unwrap();
    for (i, pad) in kit.pads.iter().enumerate() {
        if let Some(sample) = &pad.sample {
            sample.bytes().unwrap();
            e.live_note_velocity(0, i as u64 + 1, pad.midi_note, 100, true)
                .unwrap();
            let out = e.render(8192).unwrap();
            assert!(out.iter().all(|x| x.is_finite()));
            assert!(out.iter().any(|x| x.abs() > 0.0001), "{}", pad.name);
            assert_eq!(e.drum_status(0).unwrap().triggers[i], 1);
            e.cancel_live().unwrap();
            let tail = e.render(512).unwrap();
            assert!(tail[tail.len() - 64..].iter().all(|x| *x == 0.));
        }
    }
    assert_eq!(e.drum_status(0).unwrap().active_voices, 0);
}
#[test]
fn pad_identities_parameters_and_atomic_undo_are_independent() {
    let mut p = kit_project();
    let mut h = History::default();
    let original = p.tracks[0].clone();
    h.execute(&mut p, Edit::DuplicateTrack { track: 1 })
        .unwrap();
    let duplicate = p.tracks[1].clone();
    let a = original.instrument.config.drums().unwrap();
    let b = duplicate.instrument.config.drums().unwrap();
    assert!(a.pads.iter().all(|a| b.pads.iter().all(|b| a.id != b.id)));
    let mut pad = b.pads[0].clone();
    pad.parameters.gain = 0.123;
    pad.name = "Copy Kick".into();
    h.execute(
        &mut p,
        Edit::Pad {
            track: duplicate.id,
            instance: duplicate.instrument.id,
            pad: pad.clone(),
            field: PadField::Gain,
        },
    )
    .unwrap();
    assert_eq!(p.tracks[0], original);
    h.undo(&mut p).unwrap();
    assert_eq!(p.tracks[1], duplicate);
    h.redo(&mut p).unwrap();
    p.validate().unwrap();
    let before = p.clone();
    pad.midi_note = b.pads[1].midi_note;
    assert!(h
        .execute(
            &mut p,
            Edit::Pad {
                track: duplicate.id,
                instance: duplicate.instrument.id,
                pad,
                field: PadField::MidiNote
            }
        )
        .is_err());
    assert_eq!(p, before);
    assert_eq!(
        Project::decode(&serde_json::to_vec(&p).unwrap()).unwrap(),
        p
    );
    // A pad ID colliding with a note/track is rejected globally, including empty pads.
    let collision = p.tracks[0].id;
    if let Instrument::DrumKit { kit } = &mut p.tracks[0].instrument.config {
        kit.pads[15].id = collision;
    }
    assert!(p.validate().is_err());
}
#[test]
fn live_pad_mix_queue_recovers_and_export_uses_accepted_controls() {
    let mut p = kit_project();
    let id = p.allocate_id().unwrap();
    p.tracks[0].clips[0].notes.push(opendaw_app::project::Note {
        id,
        start: 0,
        duration: 960,
        pitch: 35,
        velocity: 100,
    });
    let mut live = Engine::new(48000, 256).unwrap();
    live.load(&p).unwrap();
    live.play().unwrap();
    live.render(2048).unwrap();
    let dir = root("export");
    if let Instrument::DrumKit { kit } = &mut p.tracks[0].instrument.config {
        kit.pads[0].parameters.mute = true;
    }
    assert!(live.try_mix(&p).unwrap().is_some());
    let silent = live.render(1024).unwrap();
    assert!(silent[silent.len() - 64..].iter().all(|x| *x == 0.));
    for i in 0..8 {
        if let Instrument::DrumKit { kit } = &mut p.tracks[0].instrument.config {
            kit.pads[0].parameters.gain = i as f32 / 8.;
        }
        assert!(live.try_mix(&p).unwrap().is_some());
    }
    if let Instrument::DrumKit { kit } = &mut p.tracks[0].instrument.config {
        kit.pads[0].parameters.mute = false;
        kit.pads[0].parameters.pan = -0.7;
    }
    assert_eq!(live.try_mix(&p).unwrap(), None);
    live.stop().unwrap();
    live.render(256).unwrap();
    assert!(live.try_mix(&p).unwrap().is_some());
    live.export(&dir.join("accepted.wav"), 24000).unwrap();
    let mut reference = Engine::new(48000, 256).unwrap();
    reference.load(&p).unwrap();
    reference.export(&dir.join("prepared.wav"), 24000).unwrap();
    assert_eq!(
        std::fs::read(dir.join("accepted.wav")).unwrap(),
        std::fs::read(dir.join("prepared.wav")).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn immutable_import_undo_and_portable_save_include_every_pad() {
    let mut p = kit_project();
    let dir = root("portable");
    let original = dir.join("source.wav");
    std::fs::write(&original, drums::FACTORY_ASSETS[1].bytes).unwrap();
    let mut staging = storage::DrumStaging::default();
    let imported = staging.import(&original).unwrap();
    std::fs::remove_file(&original).unwrap();
    imported.bytes().unwrap();
    let mut h = History::default();
    let mut pad = p.tracks[0].instrument.config.drums().unwrap().pads[12].clone();
    pad.sample = Some(imported.clone());
    h.execute(
        &mut p,
        Edit::Pad {
            track: 1,
            instance: 3,
            pad: pad.clone(),
            field: PadField::Sample,
        },
    )
    .unwrap();
    h.undo(&mut p).unwrap();
    assert!(p.tracks[0].instrument.config.drums().unwrap().pads[12]
        .sample
        .is_none());
    h.redo(&mut p).unwrap();
    let saved = storage::save(&dir.join("Project"), &p).unwrap();
    assert!(dir.join("Project/assets/VCSL-CC0-MANIFEST.json").exists());
    assert!(dir.join("Project/assets/VCSL-CC0-LICENSE.txt").exists());
    let stored = std::fs::read_to_string(dir.join("Project/project.json")).unwrap();
    assert!(!stored.contains("\"kind\": \"factory\""));
    assert!(stored.contains("assets/drum-"));
    // The imported snare duplicates factory bytes; content-addressed persistence deduplicates it.
    assert_eq!(
        std::fs::read_dir(dir.join("Project/assets"))
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "wav"))
            .count(),
        12
    );
    std::fs::rename(dir.join("Project"), dir.join("Moved")).unwrap();
    drop(staging);
    let portable = storage::load(&dir.join("Moved")).unwrap();
    let mut a = Engine::new(48000, 256).unwrap();
    a.load(&saved).unwrap_err(); // old folder no longer exists
    a.load(&portable).unwrap();
    a.live_note(0, 1, 36, true).unwrap();
    assert!(a.render(8192).unwrap().iter().any(|x| x.abs() > 0.001));
    for pad in &portable.tracks[0].instrument.config.drums().unwrap().pads {
        if let Some(s) = &pad.sample {
            s.bytes().unwrap();
        }
    }
    let sample = portable.tracks[0].instrument.config.drums().unwrap().pads[0]
        .sample
        .as_ref()
        .unwrap();
    let DrumSample::File { sample_path, .. } = sample else {
        panic!()
    };
    let mut bytes = std::fs::read(sample_path).unwrap();
    let end = bytes.len() - 1;
    bytes[end] ^= 1;
    std::fs::write(sample_path, bytes).unwrap();
    assert!(storage::load(&dir.join("Moved")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn sixteen_kits_share_immutable_decoded_data_within_the_media_budget() {
    let mut p = kit_project();
    let mut h = History::default();
    for _ in 1..16 {
        h.execute(&mut p, Edit::AddDrumTrack { after: 1 }).unwrap();
    }
    p.validate().unwrap();
    let mut engine = Engine::new(48000, 256).unwrap();
    engine.load(&p).unwrap();
    for i in 0..16 {
        engine.live_note(i, i as u64 + 1, 35, true).unwrap();
    }
    assert!(engine.render(4096).unwrap().iter().any(|x| x.abs() > 0.001));
}
#[test]
fn strict_kit_metadata_and_hash_checks_reject_bad_data() {
    let mut next = 1;
    let mut kit = DrumKit::factory(|| {
        let id = next;
        next += 1;
        Ok(id)
    })
    .unwrap();
    kit.validate().unwrap();
    for bad in [f32::NAN, f32::INFINITY, -1., 5.] {
        let mut p = kit.pads[0].parameters;
        p.gain = bad;
        assert!(p.validate().is_err());
    }
    kit.pads[1].midi_note = kit.pads[0].midi_note;
    assert!(kit.validate().is_err());
    let s = DrumSample::Factory {
        asset_id: "kick".into(),
        sha256: "0".repeat(64),
    };
    assert!(s.bytes().is_err());
}
