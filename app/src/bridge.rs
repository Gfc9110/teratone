use crate::project::{Instrument, Project};
use std::{ffi::{c_char, CStr, CString}, marker::PhantomData, path::Path, ptr::NonNull, rc::Rc};

#[repr(C)] struct RawEngine { _private: [u8; 0] }
#[repr(C)] struct RawNote { id: u64, start_frame: u64, end_frame: u64, track: u32, pitch: u32, velocity: u32 }
#[repr(C)] struct RawTrack { gain: f32, pan: f32, sample_path_utf8: *const c_char, root_note: u32, one_shot: u32 }
#[repr(C)] struct RawTrackV2 { struct_size: u32, version: u32, base: RawTrack, mute: u32, solo: u32 }
#[repr(C)] #[derive(Default, Clone, Copy)] struct RawTrackMix { gain: f32, pan: f32, mute: u32, solo: u32 }
#[repr(C)] #[derive(Default, Debug, Clone, Copy)]
pub struct MixStatus { pub generation: u64, pub submitted: u64, pub applied: u64 }
#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct Telemetry {
    pub position: u64, pub rendered_frames: u64, pub blocks: u64, pub last_applied: u64,
    pub rejected_commands: u64, pub clipped_samples: u64,
    pub peak_left: f32, pub peak_right: f32,
    pub playing: u32, pub looping: u32, pub device_open: u32, pub device_error: u32,
}
extern "C" {
    fn od_abi_version() -> u32;
    fn od_error_message() -> *const c_char;
    fn od_create(rate: u32, block: u32) -> *mut RawEngine;
    fn od_destroy(e: *mut RawEngine);
    fn od_load_v2(e: *mut RawEngine, tracks: *const RawTrackV2, count: usize, notes: *const RawNote, note_count: usize, length: u64) -> i32;
    fn od_set_mix_v1(e: *mut RawEngine, generation: u64, tracks: *const RawTrackMix, count: usize, serial: *mut u64) -> i32;
    fn od_read_mix_status_v1(e: *const RawEngine, status: *mut MixStatus) -> i32;
    fn od_play(e: *mut RawEngine) -> i32;
    fn od_pause(e: *mut RawEngine) -> i32;
    fn od_stop(e: *mut RawEngine) -> i32;
    fn od_panic(e: *mut RawEngine) -> i32;
    fn od_seek(e: *mut RawEngine, frame: u64) -> i32;
    fn od_loop(e: *mut RawEngine, enabled: u32, begin: u64, end: u64) -> i32;
    fn od_live_note(e: *mut RawEngine, track: u32, id: u64, pitch: u32, velocity: u32, on: u32) -> i32;
    fn od_gain_pan(e: *mut RawEngine, track: u32, gain: f32, pan: f32) -> i32;
    fn od_read_telemetry(e: *const RawEngine, out: *mut Telemetry) -> i32;
    fn od_render(e: *mut RawEngine, stereo: *mut f32, frames: u32) -> i32;
    fn od_device_start(e: *mut RawEngine, silent_clock: u32) -> i32;
    fn od_device_stop(e: *mut RawEngine) -> i32;
    fn od_export(e: *mut RawEngine, path: *const c_char, frames: u64) -> i32;
}
fn last_error(code: i32) -> String {
    // The API returns a valid thread-local, NUL-terminated buffer; copy before any next API call.
    let text = unsafe { CStr::from_ptr(od_error_message()) }.to_string_lossy();
    format!("Native engine ({code}): {text}")
}
fn check(code: i32) -> Result<(), String> { if code == 0 { Ok(()) } else { Err(last_error(code)) } }

/// Exactly one control-thread owner. Not Send/Sync; no raw pointer exposed to UI code.
/// C++ owns the audio consumer and joins it before destroying/replacing its graph.
pub struct Engine {
    raw: NonNull<RawEngine>, rate: u32, generation: u64, track_ids: Vec<u64>, _single_owner: PhantomData<Rc<()>>,
}
impl Engine {
    pub fn new(rate: u32, block: u32) -> Result<Self, String> {
        // No Rust callbacks cross the audio boundary.
        unsafe {
            if od_abi_version() != 1 { return Err("Native ABI version mismatch".into()); }
            let raw = NonNull::new(od_create(rate, block)).ok_or_else(|| last_error(4))?;
            Ok(Self { raw, rate, generation: 1, track_ids: Vec::new(), _single_owner: PhantomData })
        }
    }
    pub fn load(&mut self, project: &Project) -> Result<(), String> {
        project.validate()?;
        let paths: Vec<Option<CString>> = project.tracks.iter().map(|track| match &track.instrument.config {
            Instrument::Sine => Ok(None),
            Instrument::Sampler { sample_path, .. } => CString::new(sample_path.as_bytes()).map(Some).map_err(|e| e.to_string()),
        }).collect::<Result<_, String>>()?;
        let tracks: Vec<_> = project.tracks.iter().zip(paths.iter()).map(|(track, path)| {
            let (root_note, one_shot) = match &track.instrument.config {
                Instrument::Sine => (60, 0), Instrument::Sampler { root_note, one_shot, .. } => (*root_note as u32, u32::from(*one_shot)),
            };
            RawTrackV2 { struct_size: std::mem::size_of::<RawTrackV2>() as u32, version: 2,
                base: RawTrack { gain: track.gain, pan: track.pan, sample_path_utf8: path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()), root_note, one_shot },
                mute: u32::from(track.mute), solo: u32::from(track.solo) }
        }).collect();
        let rate = self.rate;
        let mut notes = Vec::with_capacity(project.note_count());
        for (index, track) in project.tracks.iter().enumerate() {
            for clip in &track.clips {
                for note in &clip.notes {
                    // Project validation checks both additions for overflow and clip bounds.
                    let start_frame = project.frame_at(clip.start_tick + note.start, rate);
                    let end_frame = project.frame_at(clip.start_tick + note.start + note.duration, rate);
                    if start_frame >= end_frame { return Err("Note duration rounds below one audio frame".into()); }
                    notes.push(RawNote { id: note.id, start_frame, end_frame,
                        track: index as u32, pitch: note.pitch as u32, velocity: note.velocity as u32 });
                }
            }
        }
        // All arrays/C strings remain alive for this synchronous, copying call.
        check(unsafe { od_load_v2(self.raw.as_ptr(), tracks.as_ptr(), tracks.len(), notes.as_ptr(), notes.len(), project.frame_at(project.length_ticks, self.rate)) })?;
        self.generation = self.mix_status()?.generation;
        self.track_ids = project.tracks.iter().map(|t| t.id).collect(); Ok(())
    }
    pub fn play(&mut self) -> Result<(), String> { check(unsafe { od_play(self.raw.as_ptr()) }) }
    pub fn pause(&mut self) -> Result<(), String> { check(unsafe { od_pause(self.raw.as_ptr()) }) }
    pub fn stop(&mut self) -> Result<(), String> { check(unsafe { od_stop(self.raw.as_ptr()) }) }
    pub fn panic(&mut self) -> Result<(), String> { check(unsafe { od_panic(self.raw.as_ptr()) }) }
    pub fn seek(&mut self, frame: u64) -> Result<(), String> { check(unsafe { od_seek(self.raw.as_ptr(), frame) }) }
    pub fn set_loop(&mut self, enabled: bool, begin: u64, end: u64) -> Result<(), String> {
        check(unsafe { od_loop(self.raw.as_ptr(), u32::from(enabled), begin, end) })
    }
    pub fn live_note(&mut self, track: u32, id: u64, pitch: u8, on: bool) -> Result<(), String> {
        if id == 0 || id >= (1u64 << 63) { return Err("Invalid live ID".into()); }
        check(unsafe { od_live_note(self.raw.as_ptr(), track, id | (1u64 << 63), pitch as u32, 100, u32::from(on)) })
    }
    pub fn mix(&mut self, index: u32, gain: f32, pan: f32) -> Result<(), String> {
        check(unsafe { od_gain_pan(self.raw.as_ptr(), index, gain, pan) })
    }
    pub fn try_mix(&mut self, project: &Project) -> Result<Option<u64>, String> {
        if project.tracks.len() > 16 || !project.tracks.iter().map(|t| t.id).eq(self.track_ids.iter().copied()) {
            return Err("Mixer slots changed; compile the current project before live updates".into());
        }
        let mut tracks = [RawTrackMix::default(); 16];
        for (out, t) in tracks.iter_mut().zip(&project.tracks) { *out = RawTrackMix { gain: t.gain, pan: t.pan, mute: u32::from(t.mute), solo: u32::from(t.solo) }; }
        let mut serial = 0;
        let code = unsafe { od_set_mix_v1(self.raw.as_ptr(), self.generation, tracks.as_ptr(), project.tracks.len(), &mut serial) };
        if code == 2 { return Ok(None); }
        check(code)?; Ok(Some(serial))
    }
    pub fn mix_status(&self) -> Result<MixStatus, String> {
        let mut status = MixStatus::default(); check(unsafe { od_read_mix_status_v1(self.raw.as_ptr(), &mut status) })?; Ok(status)
    }
    pub fn telemetry(&self) -> Result<Telemetry, String> {
        let mut result = Telemetry::default(); check(unsafe { od_read_telemetry(self.raw.as_ptr(), &mut result) })?; Ok(result)
    }
    pub fn start_device(&mut self, silent: bool) -> Result<(), String> {
        check(unsafe { od_device_start(self.raw.as_ptr(), u32::from(silent)) })
    }
    pub fn close_device(&mut self) -> Result<(), String> { check(unsafe { od_device_stop(self.raw.as_ptr()) }) }
    pub fn render(&mut self, frames: u32) -> Result<Vec<f32>, String> {
        if frames > 1_048_576 { return Err("Headless block too large".into()); }
        let mut result = vec![0.0; frames as usize * 2];
        check(unsafe { od_render(self.raw.as_ptr(), result.as_mut_ptr(), frames) })?; Ok(result)
    }
    pub fn export(&mut self, path: &Path, frames: u64) -> Result<(), String> {
        let path = CString::new(path.to_str().ok_or("Export path must be UTF-8")?).map_err(|e| e.to_string())?;
        check(unsafe { od_export(self.raw.as_ptr(), path.as_ptr(), frames) })
    }
}
impl Drop for Engine { fn drop(&mut self) { unsafe { od_destroy(self.raw.as_ptr()); } } }

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn native_bridge_roundtrip() {
        let mut e = Engine::new(48000, 256).unwrap(); let p = Project::demo(); e.load(&p).unwrap(); e.play().unwrap();
        assert!(e.render(512).unwrap().iter().any(|x| x.abs() > 0.001));
        assert_eq!(e.telemetry().unwrap().position, 512); e.stop().unwrap(); e.render(256).unwrap();
        assert_eq!(e.telemetry().unwrap().position, 0);
    }
    #[test] fn clip_offsets_and_track_order_match_flat_score() {
        let mut p = Project::demo();
        p.tracks[0].clips[0].start_tick = crate::project::PPQ;
        p.tracks[0].clips[0].length_ticks = 10 * crate::project::PPQ;
        let mut extra = p.tracks[0].clips[0].clone(); extra.id = p.allocate_id().unwrap();
        extra.start_tick = 0; extra.length_ticks = crate::project::PPQ; extra.notes.truncate(1);
        extra.notes[0].id = p.allocate_id().unwrap(); extra.notes[0].pitch = 75;
        p.tracks[0].clips.push(extra); p.validate().unwrap();
        let mut flat = p.clone();
        let mut notes = Vec::new();
        for c in &p.tracks[0].clips { for n in &c.notes { let mut n = n.clone(); n.start += c.start_tick; notes.push(n); } }
        flat.tracks[0].clips.truncate(1); flat.tracks[0].clips[0].start_tick = 0;
        flat.tracks[0].clips[0].length_ticks = flat.length_ticks; flat.tracks[0].clips[0].notes = notes;
        let mut a = Engine::new(48000, 256).unwrap(); let mut b = Engine::new(48000, 256).unwrap();
        a.load(&p).unwrap(); b.load(&flat).unwrap(); a.play().unwrap(); b.play().unwrap();
        assert_eq!(a.render(96000).unwrap(), b.render(96000).unwrap());
        flat.tracks.swap(0, 1); b.load(&flat).unwrap(); b.play().unwrap();
        a.load(&p).unwrap(); a.play().unwrap(); assert_eq!(a.render(48000).unwrap(), b.render(48000).unwrap());
    }
    #[test] fn abi_layout_on_64_bit_targets() { assert_eq!(std::mem::size_of::<RawNote>(), 40); assert_eq!(std::mem::size_of::<RawTrack>(), 24); assert_eq!(std::mem::size_of::<RawTrackV2>(), 40);
        assert_eq!(std::mem::size_of::<RawTrackMix>(), 16); assert_eq!(std::mem::size_of::<MixStatus>(), 24); assert_eq!(std::mem::size_of::<Telemetry>(), 72); }
    #[test] fn mixer_masks_equal_rendering_only_audible_tracks() {
        let base = Project::demo();
        for mask in 0..16 {
            let mut p = base.clone();
            for i in 0..2 { p.tracks[i].mute = mask & (1 << (i*2)) != 0; p.tracks[i].solo = mask & (2 << (i*2)) != 0; }
            let mut expected = p.clone(); let any_solo = p.tracks.iter().any(|t| t.solo);
            for t in &mut expected.tracks { if t.mute || (any_solo && !t.solo) { t.clips.clear(); } t.mute = false; t.solo = false; }
            let mut a = Engine::new(48000, 256).unwrap(); let mut b = Engine::new(48000, 256).unwrap();
            a.load(&p).unwrap(); b.load(&expected).unwrap(); a.play().unwrap(); b.play().unwrap();
            assert_eq!(a.render(1024).unwrap(), b.render(1024).unwrap(), "mask {mask}");
        }
    }
    #[test] fn mix_queue_backpressure_retains_latest_document_and_rejects_stale_order() {
        let mut e = Engine::new(48000, 256).unwrap(); let mut p = Project::demo(); e.load(&p).unwrap();
        for _ in 0..8 { assert!(e.try_mix(&p).unwrap().is_some()); }
        p.tracks[0].mute = true; assert!(e.try_mix(&p).unwrap().is_none());
        e.stop().unwrap(); e.render(256).unwrap(); // Stop cannot discard the independent mixer queue.
        assert_eq!(e.mix_status().unwrap().applied, 8);
        let serial = e.try_mix(&p).unwrap().unwrap(); e.render(256).unwrap(); assert_eq!(e.mix_status().unwrap().applied, serial);
        p.tracks.swap(0, 1); assert!(e.try_mix(&p).is_err());
        e.load(&p).unwrap(); assert!(e.try_mix(&p).unwrap().is_some());
    }

}
