use serde::{Deserialize, Serialize};
use crate::synth::{SynthParameters,SynthPreset,SynthField};
use std::collections::{HashSet, VecDeque};

pub const PPQ: u64 = 960;
pub const SCHEMA_VERSION: u32 = 2;
pub const MAX_TRACKS: usize = 16; // Raised with the native capacity in M3.6.
pub const MAX_NOTES: usize = 100_000;
pub const MAX_CLIPS: usize = 4096;
pub const TRACK_COLORS: [&str; 6] = ["#62BFA6", "#729FE8", "#D9AC64", "#B591DF", "#DD8398", "#76BCCB"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Note { pub id: u64, pub start: u64, pub duration: u64, pub pitch: u8, pub velocity: u8 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Instrument {
    Sine, // legacy v1/v2 spelling, exact compatible defaults
    Synth { parameters: SynthParameters, preset: Option<SynthPreset> },
    Sampler { sample_path: String, root_note: u8, one_shot: bool },
}
impl Instrument {
    pub fn synth(&self)->Option<SynthParameters>{match self{Self::Sine=>Some(SynthParameters::default()),Self::Synth{parameters,..}=>Some(*parameters),_=>None}}
    pub fn preset(&self)->Option<&SynthPreset>{match self{Self::Synth{preset,..}=>preset.as_ref(),_=>None}}

    pub fn label(&self) -> &'static str { match self { Self::Sine => "Oscillator · Sine", Self::Synth { parameters, .. } => parameters.waveform.label(), Self::Sampler { .. } => "Sampler" } }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InstrumentInstance { pub id: u64, pub config: Instrument }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MidiClip {
    pub id: u64, pub name: String, pub start_tick: u64, pub length_ticks: u64, pub notes: Vec<Note>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: u64, pub name: String, pub color: String, pub gain: f32, pub pan: f32,
    #[serde(default)] pub mute: bool,
    #[serde(default)] pub solo: bool,
    pub instrument: InstrumentInstance, pub clips: Vec<MidiClip>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32, pub name: String, pub bpm: f64, pub length_ticks: u64,
    pub next_id: u64, pub tracks: Vec<Track>,
}
fn valid_id(id: u64) -> bool { id != 0 && id < (1u64 << 63) }
fn take_id(next: &mut u64) -> Result<u64, String> {
    if !valid_id(*next) || *next >= (1u64 << 63) - 1 { return Err("ID space exhausted".into()); }
    let id = *next; *next += 1; Ok(id)
}
fn identity(ids: &mut HashSet<u64>, id: u64, next: u64) -> Result<(), String> {
    if !valid_id(id) || id >= next || !ids.insert(id) { return Err("Invalid, duplicate or unallocated identity".into()); }
    Ok(())
}
fn validate_note(note: &Note, length: u64) -> Result<(), String> {
    if !valid_id(note.id) || note.pitch > 127 || !(1..=127).contains(&note.velocity) || note.duration == 0 ||
       note.start.checked_add(note.duration).filter(|v| *v <= length).is_none() {
        return Err("Invalid note identity, MIDI value, or clip range".into());
    }
    Ok(())
}
fn validate_instrument(instrument: &Instrument) -> Result<(), String> {
    if let Instrument::Synth{parameters,preset}=instrument {parameters.validate()?;if let Some(p)=preset{p.validate()?;}}

    if let Instrument::Sampler { sample_path, root_note, .. } = instrument {
        if sample_path.is_empty() || sample_path.len() > 4096 || sample_path.contains('\0') || *root_note > 127 {
            return Err("Invalid sample path/root note".into());
        }
    }
    Ok(())
}
fn validate_mix(gain: f32, pan: f32) -> Result<(), String> {
    if !gain.is_finite() || !pan.is_finite() || !(0.0..=2.0).contains(&gain) || !(-1.0..=1.0).contains(&pan) {
        Err("Invalid gain/pan".into())
    } else { Ok(()) }
}
fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 120 || name.chars().any(char::is_control) { return Err("Name must contain 1–120 bytes without control characters".into()); }
    Ok(())
}
fn valid_color(color: &str) -> bool { color.len() == 7 && color.starts_with('#') && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) }
fn validate_track(track: &Track, length: u64, next: u64, ids: &mut HashSet<u64>) -> Result<(), String> {
    identity(ids, track.id, next)?; identity(ids, track.instrument.id, next)?;
    validate_name(&track.name)?;
    if !valid_color(&track.color) { return Err("Track color must be #RRGGBB".into()); }
    validate_mix(track.gain, track.pan)?; validate_instrument(&track.instrument.config)?;
    for clip in &track.clips {
        validate_clip(clip, length, next, ids)?;
    }
    Ok(())
}
fn validate_clip(clip: &MidiClip, length: u64, next: u64, ids: &mut HashSet<u64>) -> Result<(), String> {
    identity(ids, clip.id, next)?; validate_name(&clip.name)?;
    if clip.length_ticks == 0 || clip.start_tick.checked_add(clip.length_ticks).filter(|end| *end <= length).is_none() {
        return Err("Clip must fit inside the project".into());
    }
    for note in &clip.notes { validate_note(note, clip.length_ticks)?; identity(ids, note.id, next)?; }
    Ok(())
}
fn validate_limits(name: &str, bpm: f64, length: u64, next: u64, tracks: usize) -> Result<(), String> {
    if name.len() > 200 || !bpm.is_finite() || !(30.0..=300.0).contains(&bpm) || length == 0 ||
       length > 24 * 60 * 60 * 5 * PPQ || tracks == 0 || tracks > MAX_TRACKS || !valid_id(next) {
        return Err("Invalid project limits (1–16 tracks)".into());
    }
    if length as f64 * 60.0 / (bpm * PPQ as f64) > 86400.0 { return Err("Project exceeds 24 hours".into()); }
    Ok(())
}
impl Project {
    pub fn demo() -> Self {
        let notes = [60, 64, 67, 72, 67, 64, 62, 67].into_iter().enumerate().map(|(i, pitch)| Note {
            id: i as u64 + 10, start: i as u64 * PPQ, duration: PPQ * 3 / 4, pitch, velocity: 100,
        }).collect();
        Self { version: SCHEMA_VERSION, name: "First session".into(), bpm: 120.0,
            length_ticks: 16 * PPQ, next_id: 100,
            tracks: vec![Track { id: 1, name: "Sine lead".into(), color: TRACK_COLORS[0].into(), gain: 0.7, pan: -0.15, mute: false, solo: false,
                instrument: InstrumentInstance { id: 3, config: Instrument::Sine },
                clips: vec![MidiClip { id: 4, name: "Main".into(), start_tick: 0, length_ticks: 16 * PPQ, notes }] },
                Track { id: 2, name: "Bass".into(), color: TRACK_COLORS[1].into(), gain: 0.55, pan: 0.15, mute: false, solo: false,
                    instrument: InstrumentInstance { id: 5, config: Instrument::Sine },
                    clips: vec![MidiClip { id: 6, name: "Main".into(), start_tick: 0, length_ticks: 16 * PPQ,
                        notes: vec![Note { id: 20, start: 0, duration: 3 * PPQ, pitch: 48, velocity: 90 },
                            Note { id: 21, start: 4 * PPQ, duration: 3 * PPQ, pitch: 43, velocity: 90 }] }] }],
        }
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> { Self::decode_versioned(bytes).map(|(project, _)| project) }
    pub fn decode_versioned(bytes: &[u8]) -> Result<(Self, u32), String> {
        #[derive(Deserialize)] struct Header { version: u32 }
        let header: Header = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        // Parse original bytes again as the strict DTO, retaining duplicate-field checks.
        let project = match header.version {
            1 => serde_json::from_slice::<LegacyProject>(bytes).map_err(|e| e.to_string())?.migrate()?,
            2 => serde_json::from_slice::<Self>(bytes).map_err(|e| e.to_string())?,
            version => return Err(format!("Unsupported project version {version}")),
        };
        project.validate()?; Ok((project, header.version))
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != SCHEMA_VERSION { return Err(format!("Unsupported project version {}", self.version)); }
        validate_limits(&self.name, self.bpm, self.length_ticks, self.next_id, self.tracks.len())?;
        if self.note_count() > MAX_NOTES || self.clip_count() > MAX_CLIPS { return Err("Project note/clip limit exceeded".into()); }
        let mut ids = HashSet::new();
        for track in &self.tracks { validate_track(track, self.length_ticks, self.next_id, &mut ids)?; }
        Ok(())
    }
    pub fn note_count(&self) -> usize { self.tracks.iter().flat_map(|t| &t.clips).map(|c| c.notes.len()).sum() }
    pub fn clip_count(&self) -> usize { self.tracks.iter().map(|t| t.clips.len()).sum() }
    pub fn audible(&self, track: u64) -> bool {
        self.track(track).is_some_and(|t| !t.mute && (!self.tracks.iter().any(|t| t.solo) || t.solo))
    }
    pub fn frame_at(&self, tick: u64, sample_rate: u32) -> u64 {
        (tick as f64 * 60.0 * sample_rate as f64 / (self.bpm * PPQ as f64)).round() as u64
    }
    pub fn allocate_id(&mut self) -> Result<u64, String> { take_id(&mut self.next_id) }
    pub fn track(&self, id: u64) -> Option<&Track> { self.tracks.iter().find(|t| t.id == id) }
    pub fn clip(&self, track: u64, clip: u64) -> Option<&MidiClip> { self.track(track)?.clips.iter().find(|c| c.id == clip) }
    fn track_mut(&mut self, id: u64) -> Result<&mut Track, String> {
        self.tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| "Track no longer exists".into())
    }
    fn clip_mut(&mut self, track: u64, clip: u64) -> Result<&mut MidiClip, String> {
        self.track_mut(track)?.clips.iter_mut().find(|c| c.id == clip).ok_or_else(|| "Clip no longer exists".into())
    }
    fn ids(&self) -> HashSet<u64> {
        let mut ids = HashSet::new();
        for t in &self.tracks {
            ids.insert(t.id); ids.insert(t.instrument.id);
            for c in &t.clips { ids.insert(c.id); ids.extend(c.notes.iter().map(|n| n.id)); }
        }
        ids
    }
    fn check_length(&self, length: u64, excluding: Option<u64>) -> Result<(), String> {
        validate_limits(&self.name, self.bpm, length, self.next_id, self.tracks.len())?;
        if self.tracks.iter().flat_map(|t| &t.clips).filter(|c| Some(c.id) != excluding)
            .any(|c| c.start_tick.checked_add(c.length_ticks).is_none_or(|end| end > length)) {
            return Err("Project length would cut an existing clip".into());
        }
        Ok(())
    }
    fn insert_clip(&mut self, track: u64, index: usize, clip: MidiClip, next: u64, length: u64) -> Result<Edit, String> {
        let t = self.track(track).ok_or("Track no longer exists")?;
        if index > t.clips.len() || self.clip_count() >= MAX_CLIPS || self.note_count() + clip.notes.len() > MAX_NOTES {
            return Err("Invalid clip position or project clip/note limit reached".into());
        }
        self.check_length(length, None)?; validate_clip(&clip, length, next, &mut self.ids())?;
        let inverse = Edit::EraseClip { track, clip: clip.id, project_length: self.length_ticks };
        self.clip_mut_track(track)?.insert(index, clip); self.next_id = next; self.length_ticks = length; Ok(inverse)
    }
    fn clip_mut_track(&mut self, track: u64) -> Result<&mut Vec<MidiClip>, String> { Ok(&mut self.track_mut(track)?.clips) }
    fn insert_track(&mut self, index: usize, track: Track, next: u64) -> Result<Edit, String> {
        if self.tracks.len() >= MAX_TRACKS || index > self.tracks.len() { return Err("Track limit reached or invalid position".into()); }
        if self.note_count() + track.clips.iter().map(|c| c.notes.len()).sum::<usize>() > MAX_NOTES ||
           self.clip_count() + track.clips.len() > MAX_CLIPS { return Err("Project note/clip limit exceeded".into()); }
        validate_track(&track, self.length_ticks, next, &mut self.ids())?;
        let inverse = Edit::RemoveTrack { track: track.id };
        self.tracks.insert(index, track); self.next_id = next; Ok(inverse)
    }
}

// A separate strict DTO prevents defaults from silently accepting damaged/future formats.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProject { version: u32, name: String, bpm: f64, length_ticks: u64, next_id: u64, tracks: Vec<LegacyTrack> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyTrack { id: u64, name: String, gain: f32, pan: f32, instrument: Instrument, notes: Vec<Note> }
impl LegacyProject {
    fn migrate(self) -> Result<Project, String> {
        if self.version != 1 { return Err("Not a version 1 project".into()); }
        validate_limits(&self.name, self.bpm, self.length_ticks, self.next_id, self.tracks.len())?;
        let mut ids = HashSet::new(); let mut count = 0;
        for t in &self.tracks {
            identity(&mut ids, t.id, self.next_id)?;
            // v1 allowed blank track names; give only those a displayable migration name.
            if t.name.len() > 120 { return Err("Invalid legacy track name".into()); }
            validate_mix(t.gain, t.pan)?; validate_instrument(&t.instrument)?;
            if matches!(t.instrument,Instrument::Synth{..}) {return Err("Version 1 has no configurable synth".into());}
            count += t.notes.len(); if count > MAX_NOTES { return Err("Too many notes".into()); }
            for n in &t.notes { validate_note(n, self.length_ticks)?; identity(&mut ids, n.id, self.next_id)?; }
        }
        let mut next = self.next_id; let mut tracks = Vec::with_capacity(self.tracks.len());
        for (i, t) in self.tracks.into_iter().enumerate() {
            let name = if t.name.trim().is_empty() { format!("Track {}", i + 1) } else { t.name };
            tracks.push(Track { id: t.id, name, color: TRACK_COLORS[i % TRACK_COLORS.len()].into(), gain: t.gain, pan: t.pan, mute: false, solo: false,
                instrument: InstrumentInstance { id: take_id(&mut next)?, config: t.instrument },
                clips: vec![MidiClip { id: take_id(&mut next)?, name: "Main".into(), start_tick: 0, length_ticks: self.length_ticks, notes: t.notes }] });
        }
        Ok(Project { version: SCHEMA_VERSION, name: self.name, bpm: self.bpm, length_ticks: self.length_ticks, next_id: next, tracks })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact { Metadata, Realtime, Structural }
#[derive(Debug, Clone)]
pub enum Edit {
    AddNote { track: u64, clip: u64, start: u64, duration: u64, pitch: u8, velocity: u8 },
    Insert { track: u64, clip: u64, index: usize, note: Note }, Delete { track: u64, clip: u64, id: u64 },
    Replace { track: u64, clip: u64, note: Note }, Tempo(f64), Mix { track: u64, gain: f32, pan: f32 },
    Instrument { track: u64, instrument: Instrument },
    Synth { track:u64, instance:u64, instrument:Instrument, field:Option<SynthField> },
    Audibility { track: u64, mute: bool, solo: bool },
    AddTrack { after: u64 }, DuplicateTrack { track: u64 }, InsertTrack { index: usize, track: Track }, RemoveTrack { track: u64 },
    TrackInfo { track: u64, name: String, color: String }, MoveTrack { track: u64, index: usize },
    AddClip { track: u64, start: u64, length: u64 }, DuplicateClip { track: u64, clip: u64 },
    RemoveClip { track: u64, clip: u64 }, ClipName { track: u64, clip: u64, name: String },
    MoveClip { track: u64, clip: u64, destination: u64, start: u64 }, ResizeClip { track: u64, clip: u64, length: u64 },
    Velocity { track: u64, clip: u64, note: u64, value: u8 },
    // Inverse snapshots retain order, identities and any automatic project extension in one step.
    RestoreClip { track: u64, index: usize, clip: MidiClip, project_length: u64 },
    EraseClip { track: u64, clip: u64, project_length: u64 },
    PlaceClip { track: u64, clip: u64, destination: u64, index: usize, start: u64, length: u64, project_length: u64 },
}
impl Edit {
    pub fn impact(&self) -> Impact { match self { Self::TrackInfo { .. } | Self::ClipName { .. } => Impact::Metadata, Self::Mix { .. } | Self::Audibility { .. } | Self::Synth { .. } => Impact::Realtime, _ => Impact::Structural } }
    fn apply(self, p: &mut Project) -> Result<Self, String> {
        match self {
            Self::AddNote{track,clip,start,duration,pitch,velocity} => {
                let c=p.clip(track,clip).ok_or("Clip no longer exists")?;
                let mut next=p.next_id;let id=take_id(&mut next)?;let note=Note{id,start,duration,pitch,velocity};
                validate_note(&note,c.length_ticks)?;
                if p.note_count()>=MAX_NOTES {return Err("Project note limit reached".into());}
                p.clip_mut(track,clip)?.notes.push(note);p.next_id=next;Ok(Self::Delete{track,clip,id})
            }
            Self::Insert { track, clip, index, note } => {
                let c = p.clip(track, clip).ok_or("Clip no longer exists")?;
                validate_note(&note, c.length_ticks)?;
                if index > c.notes.len() || note.id >= p.next_id || p.ids().contains(&note.id) || p.note_count() >= MAX_NOTES {
                    return Err("Invalid note position, identity or note limit".into());
                }
                let inverse = Self::Delete { track, clip, id: note.id }; p.clip_mut(track, clip)?.notes.insert(index, note); Ok(inverse)
            }
            Self::Delete { track, clip, id } => {
                let c = p.clip_mut(track, clip)?;
                let index = c.notes.iter().position(|n| n.id == id).ok_or("Note not found")?;
                Ok(Self::Insert { track, clip, index, note: c.notes.remove(index) })
            }
            Self::Replace { track, clip, note } => {
                validate_note(&note, p.clip(track, clip).ok_or("Clip no longer exists")?.length_ticks)?;
                let old = p.clip_mut(track, clip)?.notes.iter_mut().find(|n| n.id == note.id).ok_or("Note not found")?;
                Ok(Self::Replace { track, clip, note: std::mem::replace(old, note) })
            }
            Self::AddClip { track, start, length } => {
                let index = p.track(track).ok_or("Track no longer exists")?.clips.len();
                let mut next = p.next_id; let id = take_id(&mut next)?;
                let end = start.checked_add(length).ok_or("Clip range overflow")?;
                let clip = MidiClip { id, name: "MIDI clip".into(), start_tick: start, length_ticks: length, notes: Vec::new() };
                p.insert_clip(track, index, clip, next, p.length_ticks.max(end))
            }
            Self::DuplicateClip { track, clip } => {
                let t = p.track(track).ok_or("Track no longer exists")?;
                let index = t.clips.iter().position(|c| c.id == clip).ok_or("Clip no longer exists")?;
                let mut copy = t.clips[index].clone(); let mut next = p.next_id;
                copy.start_tick = copy.start_tick.checked_add(copy.length_ticks).ok_or("Clip range overflow")?;
                let end = copy.start_tick.checked_add(copy.length_ticks).ok_or("Clip range overflow")?;
                copy.id = take_id(&mut next)?;
                while copy.name.len() > 115 { copy.name.pop(); } copy.name.push_str(" copy");
                for note in &mut copy.notes { note.id = take_id(&mut next)?; }
                p.insert_clip(track, index + 1, copy, next, p.length_ticks.max(end))
            }
            Self::RemoveClip { track, clip } => Self::EraseClip { track, clip, project_length: p.length_ticks }.apply(p),
            Self::RestoreClip { track, index, clip, project_length } => p.insert_clip(track, index, clip, p.next_id, project_length),
            Self::EraseClip { track, clip, project_length } => {
                let index = p.track(track).ok_or("Track no longer exists")?.clips.iter().position(|c| c.id == clip).ok_or("Clip no longer exists")?;
                p.check_length(project_length, Some(clip))?;
                let old_length = p.length_ticks; let removed = p.clip_mut_track(track)?.remove(index); p.length_ticks = project_length;
                Ok(Self::RestoreClip { track, index, clip: removed, project_length: old_length })
            }
            Self::ClipName { track, clip, name } => {
                validate_name(&name)?; let c = p.clip_mut(track, clip)?;
                Ok(Self::ClipName { track, clip, name: std::mem::replace(&mut c.name, name) })
            }
            Self::MoveClip { track, clip, destination, start } => {
                let c = p.clip(track, clip).ok_or("Clip no longer exists")?;
                let index = if track == destination { p.track(track).unwrap().clips.iter().position(|c| c.id == clip).unwrap() }
                    else { p.track(destination).ok_or("Destination track no longer exists")?.clips.len() };
                let length = c.length_ticks; let end = start.checked_add(length).ok_or("Clip range overflow")?;
                Self::PlaceClip { track, clip, destination, index, start, length, project_length: p.length_ticks.max(end) }.apply(p)
            }
            Self::ResizeClip { track, clip, length } => {
                let c = p.clip(track, clip).ok_or("Clip no longer exists")?;
                let index = p.track(track).unwrap().clips.iter().position(|c| c.id == clip).unwrap();
                let start = c.start_tick; let end = start.checked_add(length).ok_or("Clip range overflow")?;
                Self::PlaceClip { track, clip, destination: track, index, start, length, project_length: p.length_ticks.max(end) }.apply(p)
            }
            Self::PlaceClip { track, clip, destination, index, start, length, project_length } => {
                let source = p.track(track).ok_or("Track no longer exists")?;
                let old_index = source.clips.iter().position(|c| c.id == clip).ok_or("Clip no longer exists")?;
                let c = &source.clips[old_index]; let dest = p.track(destination).ok_or("Destination track no longer exists")?;
                if index > dest.clips.len() - usize::from(track == destination) { return Err("Invalid clip destination index".into()); }
                if length == 0 || start.checked_add(length).is_none_or(|end| end > project_length) { return Err("Invalid clip range".into()); }
                if c.notes.iter().any(|n| n.start.checked_add(n.duration).is_none_or(|end| end > length)) {
                    return Err("Resize refused: it would cut notes. Move/delete those notes first.".into());
                }
                p.check_length(project_length, Some(clip))?;
                let inverse = Self::PlaceClip { track: destination, clip, destination: track, index: old_index,
                    start: c.start_tick, length: c.length_ticks, project_length: p.length_ticks };
                let mut moved = p.clip_mut_track(track)?.remove(old_index); moved.start_tick = start; moved.length_ticks = length;
                p.clip_mut_track(destination)?.insert(index, moved); p.length_ticks = project_length; Ok(inverse)
            }
            Self::Velocity { track, clip, note, value } => {
                if !(1..=127).contains(&value) { return Err("Velocity must be 1–127".into()); }
                let n = p.clip_mut(track, clip)?.notes.iter_mut().find(|n| n.id == note).ok_or("Note no longer exists")?;
                Ok(Self::Velocity { track, clip, note, value: std::mem::replace(&mut n.velocity, value) })
            }
            Self::Tempo(bpm) => {
                if !bpm.is_finite() || !(30.0..=300.0).contains(&bpm) || p.length_ticks as f64 * 60.0 / (bpm * PPQ as f64) > 86400.0 {
                    return Err("Tempo must be 30–300 BPM and keep the project within 24 hours".into());
                }
                Ok(Self::Tempo(std::mem::replace(&mut p.bpm, bpm)))
            }
            Self::Mix { track, gain, pan } => {
                validate_mix(gain, pan)?; let t = p.track_mut(track)?;
                let inverse = Self::Mix { track, gain: t.gain, pan: t.pan }; t.gain = gain; t.pan = pan; Ok(inverse)
            }
            Self::Audibility { track, mute, solo } => {
                let t = p.track_mut(track)?;
                let inverse = Self::Audibility { track, mute: t.mute, solo: t.solo };
                t.mute = mute; t.solo = solo; Ok(inverse)
            }
            Self::Synth{track,instance,instrument,field}=>{
                validate_instrument(&instrument)?;
                let t=p.track_mut(track)?;
                if t.instrument.id!=instance || t.instrument.config.synth().is_none() || instrument.synth().is_none() {return Err("Synth instance changed or no longer exists".into());}
                Ok(Self::Synth{track,instance,instrument:std::mem::replace(&mut t.instrument.config,instrument),field})
            }
            Self::Instrument { track, instrument } => {
                validate_instrument(&instrument)?; let t = p.track_mut(track)?;
                Ok(Self::Instrument { track, instrument: std::mem::replace(&mut t.instrument.config, instrument) })
            }
            Self::AddTrack { after } => {
                let index = p.tracks.iter().position(|t| t.id == after).ok_or("Track no longer exists")? + 1;
                let mut next = p.next_id;
                let track = Track { id: take_id(&mut next)?, name: format!("Oscillator {}", p.tracks.len() + 1),
                    color: TRACK_COLORS[p.tracks.len() % TRACK_COLORS.len()].into(), gain: 0.7, pan: 0.0, mute: false, solo: false,
                    instrument: InstrumentInstance { id: take_id(&mut next)?, config: Instrument::Sine },
                    clips: vec![MidiClip { id: take_id(&mut next)?, name: "Main".into(), start_tick: 0, length_ticks: p.length_ticks, notes: Vec::new() }] };
                p.insert_track(index, track, next)
            }
            Self::DuplicateTrack { track } => {
                let index = p.tracks.iter().position(|t| t.id == track).ok_or("Track no longer exists")?;
                let mut copy = p.tracks[index].clone(); let mut next = p.next_id;
                let mut name = copy.name.clone(); while name.len() > 115 { name.pop(); } copy.name = format!("{name} copy");
                copy.id = take_id(&mut next)?; copy.instrument.id = take_id(&mut next)?;
                for clip in &mut copy.clips { clip.id = take_id(&mut next)?; for note in &mut clip.notes { note.id = take_id(&mut next)?; } }
                p.insert_track(index + 1, copy, next)
            }
            Self::InsertTrack { index, track } => p.insert_track(index, track, p.next_id),
            Self::RemoveTrack { track } => {
                if p.tracks.len() == 1 { return Err("Keep at least one track".into()); }
                let index = p.tracks.iter().position(|t| t.id == track).ok_or("Track no longer exists")?;
                Ok(Self::InsertTrack { index, track: p.tracks.remove(index) })
            }
            Self::TrackInfo { track, name, color } => {
                validate_name(&name)?; if !valid_color(&color) { return Err("Track color must be #RRGGBB".into()); }
                let t = p.track_mut(track)?;
                Ok(Self::TrackInfo { track, name: std::mem::replace(&mut t.name, name), color: std::mem::replace(&mut t.color, color) })
            }
            Self::MoveTrack { track, index } => {
                if index >= p.tracks.len() { return Err("Invalid destination".into()); }
                let old = p.tracks.iter().position(|t| t.id == track).ok_or("Track no longer exists")?;
                let t = p.tracks.remove(old); p.tracks.insert(index, t); Ok(Self::MoveTrack { track, index: old })
            }
        }
    }
}
#[derive(Default)]
pub struct History { undo: VecDeque<Edit>, redo: Vec<Edit>, merge_mix: bool }
impl History {
    pub fn finish_gesture(&mut self) { self.merge_mix = false; }
    pub fn execute(&mut self, p: &mut Project, edit: Edit) -> Result<(), String> {
        let inverse = edit.apply(p)?;
        let merge = self.merge_mix && matches!((&inverse, self.undo.back()),
            (Edit::Mix { track: a, .. }, Some(Edit::Mix { track: b, .. })) if a == b) || self.merge_mix && matches!((&inverse,self.undo.back()),
            (Edit::Synth{track:a,instance:ia,field:Some(fa),..},Some(Edit::Synth{track:b,instance:ib,field:Some(fb),..})) if a==b && ia==ib && fa==fb);
        self.merge_mix = matches!(&inverse, Edit::Mix { .. } | Edit::Synth{field:Some(_),..});
        if !merge { self.undo.push_back(inverse); }
        while self.undo.len() > 512 { self.undo.pop_front(); }
        self.redo.clear(); Ok(())
    }
    pub fn undo_impact(&self) -> Option<Impact> { self.undo.back().map(Edit::impact) }
    pub fn redo_impact(&self) -> Option<Impact> { self.redo.last().map(Edit::impact) }
    pub fn undo(&mut self, p: &mut Project) -> Result<bool, String> {
        self.finish_gesture(); let Some(edit) = self.undo.pop_back() else { return Ok(false); };
        match edit.clone().apply(p) { Ok(inverse) => { self.redo.push(inverse); Ok(true) }, Err(e) => { self.undo.push_back(edit); Err(e) } }
    }
    pub fn redo(&mut self, p: &mut Project) -> Result<bool, String> {
        self.finish_gesture(); let Some(edit) = self.redo.pop() else { return Ok(false); };
        match edit.clone().apply(p) { Ok(inverse) => { self.undo.push_back(inverse); Ok(true) }, Err(e) => { self.redo.push(edit); Err(e) } }
    }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
}

#[cfg(test)] mod tests {
    use super::*;
    const LEGACY: &[u8] = include_bytes!("../../examples/First-session.opendaw/project.json");
    #[test] fn sample_rate_independent_score() { let p = Project::demo(); p.validate().unwrap(); assert_eq!(p.frame_at(PPQ, 48000), 24000); assert_eq!(p.frame_at(PPQ, 44100), 22050); }
    #[test] fn legacy_migration_preserves_score_and_rejects_bad_ids() {
        let p = Project::decode(LEGACY).unwrap(); assert_eq!(p.version, 2); assert_eq!(p.note_count(), 10); assert_eq!(p.clip_count(), 2);
        let source: serde_json::Value = serde_json::from_slice(LEGACY).unwrap();
        for (index, track) in p.tracks.iter().enumerate() {
            assert_eq!(serde_json::to_value(&track.clips[0].notes).unwrap(), source["tracks"][index]["notes"]);
            assert_eq!(track.clips[0].start_tick, 0); assert_eq!(track.clips[0].length_ticks, p.length_ticks);
            assert_eq!(serde_json::to_value(&track.instrument.config).unwrap(), source["tracks"][index]["instrument"]);
        }
        let mut sampler = source.clone();
        sampler["tracks"][0]["instrument"] = serde_json::json!({"kind":"sampler", "sample_path":"assets/legacy.wav", "root_note":48, "one_shot":true});
        let migrated = Project::decode(&serde_json::to_vec(&sampler).unwrap()).unwrap();
        assert_eq!(migrated.tracks[0].instrument.config, Instrument::Sampler { sample_path: "assets/legacy.wav".into(), root_note: 48, one_shot: true });
        assert_eq!(migrated.tracks[0].clips[0].notes, p.tracks[0].clips[0].notes);
        let mut bad = source.clone(); bad["next_id"] = 10.into(); assert!(Project::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
        bad = source; bad["version"] = 999.into(); assert!(Project::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    #[test] fn undo_redo_and_invalid_edit() {
        let mut p = Project::demo(); let mut h = History::default(); let id = p.allocate_id().unwrap();
        h.execute(&mut p, Edit::Insert { track: 1, clip: 4, index: 0, note: Note { id, start: PPQ, duration: PPQ, pitch: 60, velocity: 100 } }).unwrap();
        assert_eq!(p.clip(1, 4).unwrap().notes.len(), 9); h.undo(&mut p).unwrap(); assert_eq!(p.clip(1, 4).unwrap().notes.len(), 8);
        h.redo(&mut p).unwrap(); assert_eq!(p.clip(1, 4).unwrap().notes.len(), 9);
        let before = p.clone(); assert!(h.execute(&mut p, Edit::Tempo(f64::NAN)).is_err()); assert_eq!(p, before);
    }
    #[test] fn synth_presets_are_owned_atomic_and_undoable_with_independent_copies() {
        let mut p=Project::demo();let mut h=History::default();let original=p.tracks[0].clone();
        let factory=SynthPreset::factory(2).unwrap();let parameters=factory.parameters;
        h.execute(&mut p,Edit::Synth{track:1,instance:3,instrument:Instrument::Synth{parameters,preset:Some(factory)},field:None}).unwrap();
        assert_eq!(p.tracks[1].instrument.config,Instrument::Sine);assert_eq!(h.undo_impact(),Some(Impact::Realtime));
        h.undo(&mut p).unwrap();assert_eq!(p.tracks[0],original);h.redo(&mut p).unwrap();
        h.execute(&mut p,Edit::DuplicateTrack{track:1}).unwrap();let copy=p.tracks[1].clone();assert_ne!(copy.instrument.id,3);
        let mut changed=parameters;changed.waveform=crate::synth::Waveform::Triangle;
        h.execute(&mut p,Edit::Synth{track:copy.id,instance:copy.instrument.id,instrument:Instrument::Synth{parameters:changed,preset:None},field:Some(SynthField::Waveform)}).unwrap();
        assert_eq!(p.tracks[0].instrument.config.synth(),Some(parameters));assert_eq!(p.tracks[1].instrument.config.synth(),Some(changed));
        assert!(h.execute(&mut p,Edit::Synth{track:copy.id,instance:3,instrument:Instrument::Sine,field:None}).is_err());
        assert_eq!(Project::decode(&serde_json::to_vec(&p).unwrap()).unwrap(),p);
    }
    #[test] fn synth_gestures_merge_only_the_same_field_and_preset_is_one_step() {
        let mut p=Project::demo();let mut h=History::default();
        for a in [0.1,0.2,0.3] {let mut parameters=SynthParameters::default();parameters.attack=a;
            h.execute(&mut p,Edit::Synth{track:1,instance:3,instrument:Instrument::Synth{parameters,preset:None},field:Some(SynthField::Attack)}).unwrap();}
        assert_eq!(h.undo.len(),1);h.finish_gesture();
        let factory=SynthPreset::factory(1).unwrap();h.execute(&mut p,Edit::Synth{track:1,instance:3,instrument:Instrument::Synth{parameters:factory.parameters,preset:Some(factory)},field:None}).unwrap();
        assert_eq!(h.undo.len(),2);h.undo(&mut p).unwrap();assert_eq!(p.tracks[0].instrument.config.synth().unwrap().attack,0.3);
        h.undo(&mut p).unwrap();assert_eq!(p.tracks[0].instrument.config,Instrument::Sine);
    }
    #[test] fn mix_gestures_and_metadata_impacts() {
        let mut p = Project::demo(); let mut h = History::default(); let before = p.tracks[0].gain;
        for gain in [0.2, 0.4, 0.9] { h.execute(&mut p, Edit::Mix { track: 1, gain, pan: 0.0 }).unwrap(); }
        h.finish_gesture(); h.execute(&mut p, Edit::Mix { track: 1, gain: 0.8, pan: 0.0 }).unwrap();
        h.undo(&mut p).unwrap(); assert_eq!(p.tracks[0].gain, 0.9);
        h.undo(&mut p).unwrap(); assert_eq!(p.tracks[0].gain, before); assert!(!h.can_undo());
        assert_eq!(Edit::TrackInfo { track: 1, name: "Lead".into(), color: TRACK_COLORS[0].into() }.impact(), Impact::Metadata);
    }
    #[test] fn duplicate_track_is_deep_and_restores_ids_on_redo() {
        let mut p = Project::demo(); let mut h = History::default(); let original = p.tracks[0].clone();
        h.execute(&mut p, Edit::DuplicateTrack { track: 1 }).unwrap(); let copy = p.tracks[1].clone(); p.validate().unwrap();
        assert_ne!(copy.id, original.id); assert_ne!(copy.instrument.id, original.instrument.id);
        assert_ne!(copy.clips[0].id, original.clips[0].id);
        for (a, b) in copy.clips[0].notes.iter().zip(&original.clips[0].notes) { assert_ne!(a.id, b.id); assert_eq!(a.pitch, b.pitch); }
        let next = p.next_id; h.undo(&mut p).unwrap(); assert_eq!(p.next_id, next); h.redo(&mut p).unwrap(); assert_eq!(p.tracks[1], copy);
        h.execute(&mut p, Edit::Instrument { track: copy.id, instrument: Instrument::Sampler { sample_path: "assets/test.wav".into(), root_note: 48, one_shot: true } }).unwrap();
        let mut changed = copy.clips[0].notes[0].clone(); changed.pitch = 90;
        h.execute(&mut p, Edit::Replace { track: copy.id, clip: copy.clips[0].id, note: changed }).unwrap();
        assert_eq!(p.tracks[0], original); p.validate().unwrap();
    }
    #[test] fn track_limits_and_invalid_commands_are_atomic() {
        let mut p = Project::demo(); let mut h = History::default();
        for _ in 2..MAX_TRACKS { h.execute(&mut p, Edit::AddTrack { after: 1 }).unwrap(); }
        let before = p.clone(); assert!(h.execute(&mut p, Edit::AddTrack { after: 1 }).is_err()); assert_eq!(p, before);
        for bad in [Edit::MoveTrack { track: 1, index: 99 }, Edit::RemoveTrack { track: 99 },
            Edit::TrackInfo { track: 1, name: "bad".into(), color: "#zzzzzz".into() }] {
            assert!(h.execute(&mut p, bad).is_err()); assert_eq!(p, before);
        }
        while p.tracks.len() > 1 { let id = p.tracks.last().unwrap().id; h.execute(&mut p, Edit::RemoveTrack { track: id }).unwrap(); }
        let before = p.clone(); assert!(h.execute(&mut p, Edit::RemoveTrack { track: 1 }).is_err()); assert_eq!(p, before);
    }
    #[test] fn track_history_sequences_keep_identity_and_order() {
        for seed in 0..100 {
            let mut p = Project::demo(); let before = p.clone(); let mut h = History::default();
            for i in 0..12 {
                let edit = match (seed + i) % 4 {
                    0 => Edit::DuplicateTrack { track: p.tracks[0].id },
                    1 => Edit::MoveTrack { track: p.tracks[0].id, index: p.tracks.len() - 1 },
                    2 => { let t = &p.tracks[0]; Edit::TrackInfo { track: t.id, name: format!("Track {seed}-{i}"), color: TRACK_COLORS[i % 6].into() } },
                    _ => Edit::RemoveTrack { track: p.tracks[0].id },
                };
                h.execute(&mut p, edit).unwrap(); p.validate().unwrap();
            }
            let end = p.clone(); while h.undo(&mut p).unwrap() { p.validate().unwrap(); }
            let next = p.next_id; p.next_id = before.next_id; assert_eq!(p, before); p.next_id = next;
            while h.redo(&mut p).unwrap() { p.validate().unwrap(); } assert_eq!(p, end);
        }
    }
    #[test] fn clip_bounds_and_instance_identity_are_validated() {
        let mut p = Project::demo(); p.tracks[1].instrument.id = 1; assert!(p.validate().is_err());
        p = Project::demo(); p.tracks[0].clips[0].start_tick = u64::MAX; assert!(p.validate().is_err());
        p = Project::demo(); p.tracks[0].clips[0].length_ticks = PPQ; assert!(p.validate().is_err());
        p = Project::demo(); p.tracks[1].clips[0].notes[0].id = p.tracks[0].clips[0].notes[0].id; assert!(p.validate().is_err());
    }
    #[test] fn audibility_flags_default_persist_and_undo_without_changing_faders() {
        let mut p = Project::demo(); let original = p.clone(); let mut h = History::default();
        let mut old = serde_json::to_value(&p).unwrap();
        for t in old["tracks"].as_array_mut().unwrap() { t.as_object_mut().unwrap().remove("mute"); t.as_object_mut().unwrap().remove("solo"); }
        assert_eq!(Project::decode(&serde_json::to_vec(&old).unwrap()).unwrap(), p);
        for mask in 0..16 {
            for i in 0..2 { p.tracks[i].mute = mask & (1 << (i*2)) != 0; p.tracks[i].solo = mask & (2 << (i*2)) != 0; }
            let q = Project::decode(&serde_json::to_vec(&p).unwrap()).unwrap(); assert_eq!(q, p);
            for t in &p.tracks { assert_eq!(p.audible(t.id), !t.mute && (!p.tracks.iter().any(|t| t.solo) || t.solo)); }
        }
        p = original.clone();
        h.execute(&mut p, Edit::Audibility { track: 1, mute: true, solo: true }).unwrap();
        h.execute(&mut p, Edit::Audibility { track: 2, mute: false, solo: true }).unwrap();
        assert!(!p.audible(1)); assert!(p.audible(2));
        h.undo(&mut p).unwrap(); h.undo(&mut p).unwrap(); assert_eq!(p, original);
        h.redo(&mut p).unwrap(); assert!(p.tracks[0].mute && p.tracks[0].solo);
        assert_eq!(p.tracks[0].gain, original.tracks[0].gain); assert_eq!(p.tracks[0].pan, original.tracks[0].pan);
        h.execute(&mut p, Edit::DuplicateTrack { track: 1 }).unwrap();
        assert!(p.tracks[1].mute && p.tracks[1].solo);
    }

    #[test] fn clip_transactions_restore_content_order_ids_and_project_extent() {
        let mut p=Project::demo();let original=p.clone();let mut h=History::default();
        h.execute(&mut p,Edit::DuplicateClip{track:1,clip:4}).unwrap();let copy=p.tracks[0].clips[1].clone();
        assert_eq!(copy.start_tick,16*PPQ);assert_eq!(p.length_ticks,32*PPQ);
        for (a,b) in copy.notes.iter().zip(&original.tracks[0].clips[0].notes) {assert_ne!(a.id,b.id);assert_eq!(a.pitch,b.pitch);assert_eq!(a.start,b.start);}
        h.execute(&mut p,Edit::MoveClip{track:1,clip:copy.id,destination:2,start:2*PPQ}).unwrap();
        assert!(p.clip(1,copy.id).is_none());assert_eq!(p.clip(2,copy.id).unwrap().notes,copy.notes);
        h.execute(&mut p,Edit::ResizeClip{track:2,clip:copy.id,length:10*PPQ}).unwrap();
        h.execute(&mut p,Edit::ClipName{track:2,clip:copy.id,name:"Verse".into()}).unwrap();
        let note=copy.notes[0].id;h.execute(&mut p,Edit::Velocity{track:2,clip:copy.id,note,value:37}).unwrap();
        h.execute(&mut p,Edit::RemoveClip{track:2,clip:copy.id}).unwrap();let end=p.clone();
        while h.undo(&mut p).unwrap(){p.validate().unwrap();}let next=p.next_id;p.next_id=original.next_id;assert_eq!(p,original);p.next_id=next;
        while h.redo(&mut p).unwrap(){p.validate().unwrap();}assert_eq!(p,end);
        assert_eq!(Project::decode(&serde_json::to_vec(&p).unwrap()).unwrap(),p);
    }
    #[test] fn invalid_clip_edits_are_atomic_and_empty_tracks_can_create_clips() {
        let mut p=Project::demo();let mut h=History::default();
        for edit in [Edit::ResizeClip{track:1,clip:4,length:PPQ},Edit::MoveClip{track:1,clip:4,destination:99,start:0},
            Edit::MoveClip{track:1,clip:4,destination:2,start:u64::MAX},Edit::AddClip{track:1,start:0,length:0},
            Edit::AddClip{track:1,start:u64::MAX,length:PPQ},Edit::ResizeClip{track:1,clip:4,length:500000*PPQ},
            Edit::ClipName{track:1,clip:4,name:"\n".into()},Edit::Velocity{track:1,clip:4,note:10,value:0},
            Edit::AddNote{track:1,clip:4,start:16*PPQ,duration:1,pitch:60,velocity:100}] {
            let before=p.clone();assert!(h.execute(&mut p,edit).is_err());assert_eq!(p,before);assert!(!h.can_undo());
        }
        h.execute(&mut p,Edit::RemoveClip{track:1,clip:4}).unwrap();assert!(p.tracks[0].clips.is_empty());
        let id=p.next_id;h.execute(&mut p,Edit::AddClip{track:1,start:2*PPQ,length:4*PPQ}).unwrap();assert!(p.clip(1,id).is_some());
        h.undo(&mut p).unwrap();h.undo(&mut p).unwrap();assert_eq!(p.tracks[0].clips[0].id,4);
    }
    #[test] fn clip_count_note_limits_and_restore_collisions_are_checked() {
        let mut p=Project::demo();let mut h=History::default();
        while p.clip_count()<MAX_CLIPS {h.execute(&mut p,Edit::AddClip{track:1,start:0,length:PPQ}).unwrap();}
        let before=p.clone();assert!(h.execute(&mut p,Edit::AddClip{track:1,start:0,length:PPQ}).is_err());assert_eq!(p,before);
        let copy=p.tracks[0].clips[0].clone();
        let length=p.length_ticks;assert!(h.execute(&mut p,Edit::RestoreClip{track:2,index:0,clip:copy,project_length:length}).is_err());
    }

}
