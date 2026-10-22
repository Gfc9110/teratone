use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

pub const PPQ: u64 = 960;
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_NOTES: usize = 100_000;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Note { pub id: u64, pub start: u64, pub duration: u64, pub pitch: u8, pub velocity: u8 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Instrument {
    Sine,
    Sampler { sample_path: String, root_note: u8, one_shot: bool },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: u64, pub name: String, pub gain: f32, pub pan: f32,
    pub instrument: Instrument, pub notes: Vec<Note>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32, pub name: String, pub bpm: f64, pub length_ticks: u64,
    pub next_id: u64, pub tracks: Vec<Track>,
}
fn valid_id(id: u64) -> bool { id != 0 && id < (1u64 << 63) }
fn validate_note(note: &Note, length: u64) -> Result<(), String> {
    if !valid_id(note.id) || note.pitch > 127 || !(1..=127).contains(&note.velocity) || note.duration == 0 ||
       note.start.checked_add(note.duration).filter(|v| *v <= length).is_none() {
        return Err("Invalid note identity, MIDI value, or musical range".into());
    }
    Ok(())
}
fn validate_instrument(instrument: &Instrument) -> Result<(), String> {
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
impl Project {
    pub fn demo() -> Self {
        let notes = [60, 64, 67, 72, 67, 64, 62, 67].into_iter().enumerate().map(|(i, pitch)| Note {
            id: i as u64 + 10, start: i as u64 * PPQ, duration: PPQ * 3 / 4, pitch, velocity: 100,
        }).collect();
        Self { version: SCHEMA_VERSION, name: "First session".into(), bpm: 120.0,
            length_ticks: 16 * PPQ, next_id: 100,
            tracks: vec![Track { id: 1, name: "Sine lead".into(), gain: 0.7, pan: -0.15, instrument: Instrument::Sine, notes },
                Track { id: 2, name: "Bass".into(), gain: 0.55, pan: 0.15, instrument: Instrument::Sine,
                    notes: vec![Note { id: 20, start: 0, duration: 3 * PPQ, pitch: 48, velocity: 90 },
                        Note { id: 21, start: 4 * PPQ, duration: 3 * PPQ, pitch: 43, velocity: 90 }] }],
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != SCHEMA_VERSION { return Err(format!("Unsupported project version {}", self.version)); }
        if self.name.len() > 200 || !self.bpm.is_finite() || !(30.0..=300.0).contains(&self.bpm) ||
           self.length_ticks == 0 || self.length_ticks > 24 * 60 * 60 * 5 * PPQ ||
           self.tracks.is_empty() || self.tracks.len() > 16 || !valid_id(self.next_id) {
            return Err("Invalid project limits".into());
        }
        if self.length_ticks as f64 * 60.0 / (self.bpm * PPQ as f64) > 86400.0 {
            return Err("Project exceeds 24 hours".into());
        }
        let mut ids = HashSet::new();
        let mut count = 0;
        for track in &self.tracks {
            if !valid_id(track.id) || !ids.insert(track.id) || track.name.len() > 120 { return Err("Invalid track identity/name".into()); }
            validate_mix(track.gain, track.pan)?; validate_instrument(&track.instrument)?;
            count += track.notes.len(); if count > MAX_NOTES { return Err("Too many notes".into()); }
            for note in &track.notes {
                validate_note(note, self.length_ticks)?;
                if !ids.insert(note.id) { return Err("Duplicate identity".into()); }
            }
        }
        if ids.iter().any(|id| *id >= self.next_id) { return Err("next_id must exceed all persisted identities".into()); }
        Ok(())
    }
    pub fn frame_at(&self, tick: u64, sample_rate: u32) -> u64 {
        (tick as f64 * 60.0 * sample_rate as f64 / (self.bpm * PPQ as f64)).round() as u64
    }
    pub fn allocate_id(&mut self) -> Result<u64, String> {
        if self.next_id >= (1u64 << 63) - 1 { return Err("ID space exhausted".into()); }
        let id = self.next_id; self.next_id += 1; Ok(id)
    }
    pub fn track(&self, id: u64) -> Option<&Track> { self.tracks.iter().find(|t| t.id == id) }
    fn track_mut(&mut self, id: u64) -> Result<&mut Track, String> {
        self.tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| "Track no longer exists".into())
    }
}

#[derive(Debug, Clone)]
pub enum Edit {
    Insert { track: u64, note: Note }, Delete { track: u64, id: u64 },
    Replace { track: u64, note: Note }, Tempo(f64), Mix { track: u64, gain: f32, pan: f32 },
    Instrument { track: u64, instrument: Instrument },
}
impl Edit {
    // Validate before mutation, then return the inverse. Audio buffers are never part of undo.
    fn apply(self, p: &mut Project) -> Result<Self, String> {
        match self {
            Self::Insert { track, note } => {
                validate_note(&note, p.length_ticks)?;
                if p.tracks.iter().any(|t| t.id == note.id || t.notes.iter().any(|n| n.id == note.id)) {
                    return Err("Duplicate note identity".into());
                }
                if p.tracks.iter().map(|t| t.notes.len()).sum::<usize>() >= MAX_NOTES { return Err("Note limit reached".into()); }
                if note.id >= p.next_id { return Err("Allocate an ID before inserting a note".into()); }
                let inverse = Self::Delete { track, id: note.id }; p.track_mut(track)?.notes.push(note); Ok(inverse)
            }
            Self::Delete { track, id } => {
                let t = p.track_mut(track)?;
                let index = t.notes.iter().position(|n| n.id == id).ok_or("Note not found")?;
                Ok(Self::Insert { track, note: t.notes.remove(index) })
            }
            Self::Replace { track, note } => {
                validate_note(&note, p.length_ticks)?;
                let t = p.track_mut(track)?;
                let old = t.notes.iter_mut().find(|n| n.id == note.id).ok_or("Note not found")?;
                Ok(Self::Replace { track, note: std::mem::replace(old, note) })
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
            Self::Instrument { track, instrument } => {
                validate_instrument(&instrument)?; let t = p.track_mut(track)?;
                Ok(Self::Instrument { track, instrument: std::mem::replace(&mut t.instrument, instrument) })
            }
        }
    }
}
#[derive(Default)]
pub struct History { undo: VecDeque<Edit>, redo: Vec<Edit> }
impl History {
    pub fn execute(&mut self, p: &mut Project, edit: Edit) -> Result<(), String> {
        let inverse = edit.apply(p)?;
        // Coalesce consecutive mixer changes. Note drags commit exactly once on release.
        let merge = matches!((&inverse, self.undo.back()),
            (Edit::Mix { track: a, .. }, Some(Edit::Mix { track: b, .. })) if a == b);
        if !merge { self.undo.push_back(inverse); }
        while self.undo.len() > 512 { self.undo.pop_front(); }
        self.redo.clear(); Ok(())
    }
    pub fn undo(&mut self, p: &mut Project) -> Result<bool, String> {
        let Some(edit) = self.undo.pop_back() else { return Ok(false); };
        match edit.clone().apply(p) { Ok(inverse) => { self.redo.push(inverse); Ok(true) }, Err(e) => { self.undo.push_back(edit); Err(e) } }
    }
    pub fn redo(&mut self, p: &mut Project) -> Result<bool, String> {
        let Some(edit) = self.redo.pop() else { return Ok(false); };
        match edit.clone().apply(p) { Ok(inverse) => { self.undo.push_back(inverse); Ok(true) }, Err(e) => { self.redo.push(edit); Err(e) } }
    }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn sample_rate_independent_score() { let p = Project::demo(); p.validate().unwrap(); assert_eq!(p.frame_at(PPQ, 48000), 24000); assert_eq!(p.frame_at(PPQ, 44100), 22050); }
    #[test] fn undo_redo_and_invalid_edit() {
        let mut p = Project::demo(); let mut h = History::default(); let id = p.allocate_id().unwrap();
        h.execute(&mut p, Edit::Insert { track: 1, note: Note { id, start: PPQ, duration: PPQ, pitch: 60, velocity: 100 } }).unwrap();
        assert_eq!(p.track(1).unwrap().notes.len(), 9); h.undo(&mut p).unwrap(); assert_eq!(p.track(1).unwrap().notes.len(), 8);
        h.redo(&mut p).unwrap(); assert_eq!(p.track(1).unwrap().notes.len(), 9);
        let before = p.clone(); assert!(h.execute(&mut p, Edit::Tempo(f64::NAN)).is_err()); assert_eq!(p, before);
    }
    #[test] fn coalesced_mix() {
        let mut p = Project::demo(); let before = p.tracks[0].gain; let mut h = History::default();
        for gain in [0.2, 0.4, 0.9] { h.execute(&mut p, Edit::Mix { track: 1, gain, pan: 0.0 }).unwrap(); }
        h.undo(&mut p).unwrap(); assert_eq!(p.tracks[0].gain, before); assert!(!h.can_undo());
    }
    #[test] fn duplicate_and_unknown_schema() { let mut p = Project::demo(); p.tracks[1].notes[0].id = p.tracks[0].notes[0].id; assert!(p.validate().is_err()); p.version = 999; assert!(p.validate().is_err()); }
}
