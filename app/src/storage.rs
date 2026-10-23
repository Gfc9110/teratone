use crate::project::{Instrument, Project};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Component, Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
static SERIAL: AtomicU64 = AtomicU64::new(0);
const MANIFEST_LIMIT: u64 = 32 * 1024 * 1024;
fn err(e: impl std::fmt::Display) -> String { e.to_string() }
fn unique_name(prefix: &str) -> String {
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{prefix}-{time}-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed))
}
fn temp_file(parent: &Path, prefix: &str) -> Result<(PathBuf, File), String> {
    let path = parent.join(unique_name(prefix));
    let file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(err)?;
    Ok((path, file))
}
#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> { fs::rename(source, destination).map_err(err) }
#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" { fn MoveFileExW(old: *const u16, new: *const u16, flags: u32) -> i32; }
    let a: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<_> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    // REPLACE_EXISTING | WRITE_THROUGH; both paths remain alive across this call.
    if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 1 | 8) } == 0 { Err(err(std::io::Error::last_os_error())) } else { Ok(()) }
}
fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)] File::open(path).and_then(|f| f.sync_all()).map_err(err)?;
    #[cfg(not(unix))] let _ = path;
    Ok(())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let (temp, mut file) = temp_file(parent, ".pending")?;
    let result = (|| {
        file.write_all(bytes).map_err(err)?; file.sync_all().map_err(err)?; drop(file);
        replace_file(&temp, path)?; sync_directory(parent)
    })();
    if result.is_err() { let _ = fs::remove_file(temp); }
    result
}
fn read_manifest(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > MANIFEST_LIMIT { return Err("Project manifest is too large".into()); }
    let mut bytes = Vec::new(); file.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() as u64 > MANIFEST_LIMIT { return Err("Project manifest grew beyond its limit".into()); } Ok(bytes)
}
pub fn save_synth_preset(path:&Path,preset:&crate::synth::SynthPreset)->Result<(),String>{
    preset.validate()?; atomic_write(path,&serde_json::to_vec_pretty(preset).map_err(err)?)
}
pub fn load_synth_preset(path:&Path)->Result<crate::synth::SynthPreset,String>{
    let file=File::open(path).map_err(err)?;let mut bytes=Vec::new();file.take(4097).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len()>4096{return Err("Synth preset exceeds 4 KiB".into());}
    let preset:crate::synth::SynthPreset=serde_json::from_slice(&bytes).map_err(err)?;preset.validate()?;Ok(preset)
}
fn resolve_samples(root: &Path, p: &mut Project) -> Result<(), String> {
    let root = root.canonicalize().map_err(err)?;
    for t in &mut p.tracks {
        if let Instrument::Sampler { sample_path, .. } = &mut t.instrument.config {
            let relative = Path::new(sample_path);
            if relative.is_absolute() || relative.components().any(|c| !matches!(c, Component::Normal(_))) || !relative.starts_with("assets") {
                return Err("Saved samples must use relative assets/ paths without traversal".into());
            }
            let resolved = root.join(relative).canonicalize().map_err(err)?;
            if !resolved.starts_with(&root) { return Err("Sample symlink escapes project directory".into()); }
            *sample_path = resolved.to_str().ok_or("Sample path must be UTF-8")?.to_string();
        }
    }
    Ok(())
}
pub fn load(root: &Path) -> Result<Project, String> { load_versioned(root).map(|(project, _)| project) }
pub fn load_versioned(root: &Path) -> Result<(Project, u32), String> {
    let bytes = read_manifest(&root.join("project.json"))?;
    let (mut p, version) = Project::decode_versioned(&bytes)?;
    resolve_samples(root, &mut p)?; Ok((p, version))
}
/// Save immutable asset copies first, backup the prior manifest, then atomically replace project.json.
/// A failed save may leave orphan assets; it does not delete the prior checkpoint.
pub fn save(root: &Path, project: &Project) -> Result<Project, String> {
    project.validate()?;
    fs::create_dir_all(root.join("assets")).map_err(err)?;
    fs::create_dir_all(root.join("recovery")).map_err(err)?;
    let root = root.canonicalize().map_err(err)?;
    let assets = root.join("assets").canonicalize().map_err(err)?;
    let recovery = root.join("recovery").canonicalize().map_err(err)?;
    if !assets.starts_with(&root) || !recovery.starts_with(&root) {
        return Err("Asset or recovery directory escapes project".into());
    }
    if root.join("project.json").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("The project manifest must not be a symlink".into());
    }
    let mut stored = project.clone();
    let mut imported = std::collections::HashMap::<PathBuf, String>::new();
    let mut copied = 0u64;
    for track in &mut stored.tracks {
        if let Instrument::Sampler { sample_path, .. } = &mut track.instrument.config {
            let source = Path::new(sample_path).canonicalize().map_err(err)?;
            if let Some(relative) = imported.get(&source) { *sample_path = relative.clone(); continue; }
            let relative = if source.starts_with(&assets) {
                source.strip_prefix(&root).map_err(err)?.to_str().ok_or("Non-UTF-8 asset name")?.replace('\\', "/")
            } else {
                let size = fs::metadata(&source).map_err(err)?.len();
                copied = copied.checked_add(size).ok_or("Asset budget overflow")?;
                if copied > 128 * 1024 * 1024 { return Err("Asset import exceeds 128 MiB save budget".into()); }
                let relative = format!("assets/{}.wav", unique_name("sample"));
                let destination = root.join(&relative);
                let mut input = File::open(&source).map_err(err)?;
                let mut output = OpenOptions::new().create_new(true).write(true).open(&destination).map_err(err)?;
                let count = std::io::copy(&mut (&mut input).take(size + 1), &mut output).map_err(err)?;
                if count != size { let _ = fs::remove_file(&destination); return Err("Sample changed during save".into()); }
                output.sync_all().map_err(err)?;
                relative
            };
            imported.insert(source, relative.clone()); *sample_path = relative;
        }
    }
    let manifest = root.join("project.json");
    let bytes = serde_json::to_vec_pretty(&stored).map_err(err)?;
    if bytes.len() as u64 > MANIFEST_LIMIT { return Err("Manifest exceeds limit".into()); }
    sync_directory(&assets)?;
    if manifest.exists() {
        let previous = read_manifest(&manifest)?;
        let (_, version) = Project::decode_versioned(&previous)
            .map_err(|e| format!("Existing manifest is invalid; save to a new folder: {e}"))?;
        let legacy_backup = recovery.join("pre-m3-v1.json");
        if version == 1 {
            if let Ok(metadata) = legacy_backup.symlink_metadata() {
                if metadata.file_type().is_symlink() { return Err("The v1 migration backup must not be a symlink".into()); }
                let (_, backup_version) = Project::decode_versioned(&read_manifest(&legacy_backup)?)?;
                if backup_version != 1 { return Err("Existing v1 migration backup is not version 1; save to a new folder".into()); }
            } else {
                // Keep the original v1 even after subsequent v2 saves replace previous.json.
                atomic_write(&legacy_backup, &previous)?;
            }
        }
        atomic_write(&recovery.join("previous.json"), &previous)?;
    }
    atomic_write(&manifest, &bytes)?;
    resolve_samples(&root, &mut stored)?; Ok(stored)
}
pub fn export_atomic(path: &Path, project: &Project) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() { return Err("Export directory does not exist".into()); }
    let (temp, file) = temp_file(parent, ".export")?; drop(file);
    let result = (|| {
        let mut engine = crate::bridge::Engine::new(48000, 256)?; engine.load(project)?;
        engine.export(&temp, project.frame_at(project.length_ticks, 48000))?;
        OpenOptions::new().read(true).write(true).open(&temp)
            .and_then(|f| f.sync_all()).map_err(err)?;
        replace_file(&temp, path)?; sync_directory(parent)
    })();
    if result.is_err() { let _ = fs::remove_file(temp); }
    result
}

#[cfg(test)] mod tests {
    use super::*;
    fn directory() -> PathBuf { let p = std::env::temp_dir().join(unique_name("opendaw-test")); fs::create_dir(&p).unwrap(); p }
    #[test] fn roundtrip_backup_and_version_rejection() {
        let path = directory(); let p = Project::demo(); save(&path, &p).unwrap(); assert_eq!(load(&path).unwrap(), p);
        let mut second = p.clone(); second.name = "Second".into(); save(&path, &second).unwrap();
        let old: Project = serde_json::from_slice(&read_manifest(&path.join("recovery/previous.json")).unwrap()).unwrap(); assert_eq!(old, p);
        let mut wrong = second.clone(); wrong.version = 999; assert!(save(&path, &wrong).is_err()); assert_eq!(load(&path).unwrap(), second);
        fs::remove_dir_all(path).unwrap();
    }
    #[test] fn migration_keeps_original_v1_across_repeated_saves() {
        let path = directory();
        let bytes = include_bytes!("../../examples/First-session.opendaw/project.json");
        fs::write(path.join("project.json"), bytes).unwrap();
        let (p, version) = load_versioned(&path).unwrap(); assert_eq!(version, 1); assert_eq!(p.version, 2);
        assert_eq!(fs::read(path.join("project.json")).unwrap(), bytes);
        save(&path, &p).unwrap();
        assert_eq!(fs::read(path.join("recovery/previous.json")).unwrap(), bytes);
        assert_eq!(fs::read(path.join("recovery/pre-m3-v1.json")).unwrap(), bytes);
        let mut second = p.clone(); second.name = "Second v2 save".into(); save(&path, &second).unwrap();
        assert_eq!(fs::read(path.join("recovery/pre-m3-v1.json")).unwrap(), bytes);
        assert_eq!(load_versioned(&path).unwrap(), (second, 2));
        fs::remove_dir_all(path).unwrap();
    }
    #[test] fn duplicated_sampler_assets_survive_portable_save() {
        let path = directory(); let source = directory(); let wav = source.join("tone.wav");
        let mut e = crate::bridge::Engine::new(48000, 256).unwrap(); e.load(&Project::demo()).unwrap(); e.export(&wav, 1024).unwrap();
        let mut p = Project::demo(); p.tracks[0].instrument.config = Instrument::Sampler { sample_path: wav.to_str().unwrap().into(), root_note: 60, one_shot: false };
        let mut h = crate::project::History::default(); h.execute(&mut p, crate::project::Edit::DuplicateTrack { track: 1 }).unwrap();
        let saved = save(&path, &p).unwrap(); fs::remove_dir_all(&source).unwrap();
        let loaded = load(&path).unwrap(); assert_eq!(loaded, saved);
        assert_ne!(loaded.tracks[0].instrument.id, loaded.tracks[1].instrument.id);
        assert_eq!(loaded.tracks[0].instrument.config, loaded.tracks[1].instrument.config);
        assert_eq!(fs::read_dir(path.join("assets")).unwrap().count(), 1);
        e.load(&loaded).unwrap(); e.play().unwrap(); assert!(e.render(512).unwrap().iter().any(|v| v.abs() > 0.001));
        fs::remove_dir_all(path).unwrap();
    }
    #[cfg(unix)]
    #[test] fn reject_escape_recovery_symlink() {
        let path = directory(); let outside = directory();
        std::os::unix::fs::symlink(&outside, path.join("recovery")).unwrap();
        assert!(save(&path, &Project::demo()).is_err());
        assert!(!outside.join("previous.json").exists());
        fs::remove_dir_all(path).unwrap(); fs::remove_dir_all(outside).unwrap();
    }
    #[test] fn reject_escape_path() {
        let path = directory(); let mut p = Project::demo();
        p.tracks[0].instrument.config = Instrument::Sampler { sample_path: "../outside.wav".into(), root_note: 60, one_shot: false };
        assert!(resolve_samples(&path, &mut p).is_err()); fs::remove_dir_all(path).unwrap();
    }
    #[test] fn audibility_roundtrip_and_all_muted_export() {
        let path = directory(); let mut p = Project::demo();
        p.tracks[0].mute = true; p.tracks[0].solo = true; p.tracks[1].mute = true;
        save(&path, &p).unwrap(); let loaded = load(&path).unwrap(); assert_eq!(loaded, p);
        let wav = path.join("muted.wav"); export_atomic(&wav, &loaded).unwrap();
        let data = fs::read(wav).unwrap(); assert_eq!(data.len(), 44 + p.frame_at(p.length_ticks, 48000) as usize * 4);
        assert!(data[44..].iter().all(|b| *b == 0)); fs::remove_dir_all(path).unwrap();
    }

    #[test] fn two_clip_twenty_note_duplicate_is_independent_after_save_reopen() {
        use crate::project::{Edit,History,PPQ};let path=directory();let mut p=Project::demo();let mut h=History::default();let clip=p.next_id;
        h.execute(&mut p,Edit::AddClip{track:1,start:8*PPQ,length:8*PPQ}).unwrap();
        for i in 0..12 {h.execute(&mut p,Edit::AddNote{track:1,clip,start:i*PPQ/4,duration:PPQ/4,pitch:60+(i%12) as u8,velocity:90}).unwrap();}
        let original=p.tracks[0].clone();assert_eq!(original.clips.iter().map(|c|c.notes.len()).sum::<usize>(),20);
        let copy=p.next_id;h.execute(&mut p,Edit::DuplicateTrack{track:1}).unwrap();let copied=p.track(copy).unwrap();assert_eq!(copied.clips.len(),2);
        assert_ne!(copied.instrument.id,original.instrument.id);
        let (copy_clip,note)=(copied.clips[1].id,copied.clips[1].notes[0].id);
        h.execute(&mut p,Edit::Velocity{track:copy,clip:copy_clip,note,value:12}).unwrap();
        h.execute(&mut p,Edit::MoveClip{track:copy,clip:copy_clip,destination:2,start:4*PPQ}).unwrap();
        assert_eq!(p.track(1).unwrap(),&original);save(&path,&p).unwrap();let loaded=load(&path).unwrap();assert_eq!(loaded,p);
        assert_eq!(loaded.clip(2,copy_clip).unwrap().notes[0].velocity,12);assert_eq!(loaded.track(1).unwrap(),&original);
        fs::remove_dir_all(path).unwrap();
    }

}
