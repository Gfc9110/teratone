//! Repeatable M3.2 smoke benchmark; headless ABI timings include Rust output-buffer allocation.
//! This is not a device-underrun or callback-only latency measurement.
use opendaw_app::{arrangement::{self, ArrangementViewport}, bridge::Engine, project::{Edit, History, Note, Project, PPQ}};
use std::time::Instant;
fn p95(values: &mut [f64]) -> f64 { values.sort_by(f64::total_cmp); values[(values.len()-1)*95/100] }
fn main() {
    let mut p=Project::demo(); let mut h=History::default();
    for _ in 2..16 { h.execute(&mut p,Edit::AddTrack{after:1}).unwrap(); }
    p.length_ticks=512*PPQ; let mut next=p.next_id;
    for track in &mut p.tracks {
        let clip=&mut track.clips[0]; clip.start_tick=0; clip.length_ticks=p.length_ticks; clip.notes.clear();
        for i in 0..6250 { clip.notes.push(Note{id:next,start:i*PPQ/16,duration:PPQ/32,pitch:48+(i%24) as u8,velocity:80}); next+=1; }
    }
    p.next_id=next; p.validate().unwrap();
    let mut scenes=Vec::new(); let mut max_rects=0;
    for i in 0..100 {
        let viewport=ArrangementViewport{start:i as f32*3.0,pixels_per_beat:48.0,width:600.0};
        let start=Instant::now(); let (rows,capped)=arrangement::scene(&p,viewport,1,Some(4));
        scenes.push(start.elapsed().as_secs_f64()*1000.0); assert!(!capped);
        let rects=rows.iter().flat_map(|t| &t.clips).map(|c|c.notes.len()).sum::<usize>(); max_rects=max_rects.max(rects);
    }
    println!("16 tracks / {} notes: arrangement scene p95 {:.3} ms; max {max_rects} thumbnail rectangles",p.note_count(),p95(&mut scenes));
    let mut piano_times=Vec::new();let mut keys_max=0;let mut notes_max=0;
    for i in 0..100 {
        let viewport=opendaw_app::editor::Viewport{start_tick:i*PPQ/2,pixels_per_beat:48.0,scroll_y:55.0*16.0,height:400.0,..Default::default()};
        let start=Instant::now();let (notes,capped)=viewport.scene(&p,1,4,None,None);let keys=viewport.keys();assert!(!capped);
        piano_times.push(start.elapsed().as_secs_f64()*1000.0);notes_max=notes_max.max(notes.len());keys_max=keys_max.max(keys.len());
    }
    println!("M3.3 piano scene + keyboard, same 100k-note project: p95 {:.3} ms; max {notes_max} note rectangles / {keys_max} keys",p95(&mut piano_times));
    let mut e=Engine::new(48000,256).unwrap(); let start=Instant::now(); e.load(&p).unwrap();
    println!("Control-thread prepare: {:.3} ms",start.elapsed().as_secs_f64()*1000.0); e.play().unwrap();
    let mut blocks=Vec::new();
    for i in 0..5000 {
        if i%4==0 { p.tracks[0].mute=i%8==0; p.tracks[1].solo=i%12==0; assert!(e.try_mix(&p).unwrap().is_some()); }
        let start=Instant::now(); let out=e.render(256).unwrap(); let elapsed=start.elapsed().as_secs_f64()*1000.0;
        assert!(out.iter().all(|v|v.is_finite())); blocks.push(elapsed);
    }
    let max=blocks.iter().copied().fold(0.0,f64::max); let p95=p95(&mut blocks);
    println!("5000 headless 256-frame calls, 48 kHz, periodic mixer snapshots: p95 {p95:.3} ms, max {max:.3} ms (Rust buffer allocation included)");
    println!("Not the M3.6 hardware gate: no physical device, 32/64 tracks, or fixed 8 voices/track claim.");
}
