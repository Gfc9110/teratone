use opendaw_app::project::{Edit, History, Note, Project, MAX_TRACKS, PPQ};
/// Deterministic 64-track fixture. Notes are bounded to <=1563/clip, <=1500 visible in the probe.
pub fn project(total_notes: usize) -> Project {
    let mut p = Project::demo();
    let mut h = History::default();
    for _ in p.tracks.len()..MAX_TRACKS {
        h.execute(&mut p, Edit::AddTrack { after: 1 }).unwrap();
    }
    p.name = "M3.6 · 64 tracks".into();
    p.length_ticks = 512 * PPQ;
    let mut next = p.next_id;
    for (t, track) in p.tracks.iter_mut().enumerate() {
        track.name = format!("Track {:02}", t + 1);
        track.gain = 0.003;
        let clip = &mut track.clips[0];
        clip.length_ticks = p.length_ticks;
        clip.notes.clear();
        let count = total_notes / MAX_TRACKS + usize::from(t < total_notes % MAX_TRACKS);
        for i in 0..count {
            clip.notes.push(Note {
                id: next,
                start: if t == 0 && i < 1490 {
                    (i / 16) as u64 * PPQ / 16
                } else {
                    i as u64 * PPQ / 8
                },
                duration: PPQ / 32,
                pitch: if t == 0 && i < 1490 {
                    60
                } else {
                    48 + (i % 24) as u8
                },
                velocity: 80,
            });
            next += 1;
        }
    }
    p.next_id = next;
    p.validate().unwrap();
    p
}
