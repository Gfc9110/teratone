//! UI-side bounded outbox. It never runs on the audio thread.
use crate::controller::{Envelope, Request, PANIC, STOP};
use std::{collections::{HashSet, VecDeque}, sync::{Arc, atomic::{AtomicU32, AtomicU64, Ordering}, mpsc::{SyncSender, TrySendError}}};

pub struct Mailbox {
    sender: SyncSender<Envelope>, flags: Arc<AtomicU32>, epoch: Arc<AtomicU64>,
    pending: VecDeque<(u64, Request)>, in_flight: HashSet<u64>, next_id: u64,
}
fn coalesces(a: &Request, b: &Request) -> bool {
    match (a, b) {
        (Request::Move(..), Request::Move(..)) | (Request::Viewport(..), Request::Viewport(..)) |
        (Request::ArrangementViewport(..), Request::ArrangementViewport(..)) => true,
        (Request::SetGain { track: a, .. }, Request::SetGain { track: b, .. }) |
        (Request::SetPan { track: a, .. }, Request::SetPan { track: b, .. }) |
        (Request::Mix { track: a, .. }, Request::Mix { track: b, .. }) => a == b,
        _ => false,
    }
}
impl Mailbox {
    pub fn new(sender: SyncSender<Envelope>, flags: Arc<AtomicU32>, epoch: Arc<AtomicU64>) -> Self {
        Self { sender, flags, epoch, pending: VecDeque::new(), in_flight: HashSet::new(), next_id: 1 }
    }
    pub fn enqueue(&mut self, request: Request) -> Result<(), String> {
        let epoch = self.epoch.load(Ordering::Acquire);
        if let Some((old_epoch, old)) = self.pending.back_mut() {
            if *old_epoch == epoch && coalesces(old, &request) { *old = request; return Ok(()); }
        }
        if self.pending.len() == 256 {
            if matches!(request, Request::Audition { on: false, .. }) { self.panic(); }
            return Err("Action not queued: controller is busy. Existing edits are preserved; retry this action.".into());
        }
        self.pending.push_back((epoch, request)); Ok(())
    }
    pub fn flush(&mut self) -> Result<(), String> {
        for _ in 0..16 {
            if self.in_flight.len() >= 64 { break; }
            let Some((epoch, request)) = self.pending.pop_front() else { break; };
            let request_id = self.next_id;
            if request_id == u64::MAX { self.pending.push_front((epoch, request)); return Err("Command identity space exhausted".into()); }
            let envelope = Envelope { request_id, epoch, request };
            match self.sender.try_send(envelope) {
                Ok(()) => { self.next_id += 1; self.in_flight.insert(request_id); }
                Err(TrySendError::Full(envelope)) => { self.pending.push_front((envelope.epoch, envelope.request)); break; }
                Err(TrySendError::Disconnected(envelope)) => {
                    self.pending.push_front((envelope.epoch, envelope.request));
                    return Err("Application controller stopped; the pending action was not applied.".into());
                }
            }
        }
        Ok(())
    }
    pub fn acknowledge(&mut self, request_id: u64) { self.in_flight.remove(&request_id); }
    pub fn pending_count(&self) -> usize { self.pending.len() + self.in_flight.len() }
    pub fn stop(&mut self) { self.epoch.fetch_add(1, Ordering::AcqRel); self.flags.fetch_or(STOP, Ordering::Release); }
    pub fn panic(&mut self) { self.epoch.fetch_add(1, Ordering::AcqRel); self.flags.fetch_or(PANIC, Ordering::Release); }
}

#[cfg(test)] mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test] fn slider_burst_coalesces_but_gesture_boundaries_and_edits_survive_stop() {
        let (tx, rx) = mpsc::sync_channel(64); let flags = Arc::new(AtomicU32::new(0)); let epoch = Arc::new(AtomicU64::new(0));
        let mut m = Mailbox::new(tx, flags.clone(), epoch.clone());
        for i in 0..1000 { m.enqueue(Request::SetGain { track: 1, value: i as f32 / 1000.0 }).unwrap(); }
        m.enqueue(Request::FinishMix).unwrap(); m.enqueue(Request::SetGain { track: 1, value: 0.3 }).unwrap();
        m.enqueue(Request::RenameTrack { track: 1, name: "Keep this edit".into() }).unwrap(); m.enqueue(Request::Play).unwrap();
        assert_eq!(m.pending_count(), 5); m.stop(); m.flush().unwrap();
        let envelopes: Vec<_> = rx.try_iter().collect(); assert_eq!(envelopes.len(), 5);
        assert!(matches!(envelopes[0].request, Request::SetGain { value, .. } if value == 0.999));
        assert!(!envelopes[3].request.canceled_by_stop()); assert!(envelopes[4].request.canceled_by_stop());
        assert_ne!(envelopes[4].epoch, epoch.load(Ordering::Acquire)); assert_ne!(flags.load(Ordering::Acquire) & STOP, 0);
        for e in envelopes { m.acknowledge(e.request_id); } assert_eq!(m.pending_count(), 0);
    }
    #[test] fn full_channel_retries_and_full_outbox_rejects_only_new_action() {
        let (tx, rx) = mpsc::sync_channel(1); let mut m = Mailbox::new(tx, Arc::new(AtomicU32::new(0)), Arc::new(AtomicU64::new(0)));
        m.enqueue(Request::AddTrack { after: 1 }).unwrap(); m.enqueue(Request::DuplicateTrack(1)).unwrap(); m.flush().unwrap();
        assert_eq!(m.pending_count(), 2); let first = rx.try_recv().unwrap(); m.acknowledge(first.request_id); m.flush().unwrap();
        assert!(matches!(rx.try_recv().unwrap().request, Request::DuplicateTrack(1)));
        for _ in 0..256 { m.enqueue(Request::Undo).unwrap(); }
        assert!(m.enqueue(Request::RemoveTrack(1)).is_err()); assert_eq!(m.pending.len(), 256);
    }
}
