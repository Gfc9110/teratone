//! UI-side bounded outbox. It never runs on the audio thread.
use crate::controller::{Envelope, Request, PANIC, STOP, RELEASE};
use std::{collections::{HashMap, HashSet, VecDeque}, sync::{Arc, atomic::{AtomicU32, AtomicU64, Ordering}, mpsc::{SyncSender, TrySendError}}};

pub struct Mailbox {
    sender: SyncSender<Envelope>, flags: Arc<AtomicU32>, epoch: Arc<AtomicU64>, audition_epoch: Arc<AtomicU64>, next_token: u64,
    pending: VecDeque<(u64, Request)>, in_flight: HashSet<u64>, next_id: u64,
    piano_scroll_views: HashMap<(u64,u64,u64),u64>, arrangement_views: HashMap<u64,u64>,
    last_piano_scroll: Option<((u64,u64,u64),u64,f64,f32)>,
}
fn coalesces(a: &Request, b: &Request) -> bool {
    match (a, b) {
        (Request::Viewport(..), Request::Viewport(..)) |
        (Request::ArrangementScroll(..), Request::ArrangementScroll(..)) |
        (Request::ArrangementViewport(..), Request::ArrangementViewport(..)) => true,
        (Request::ViewportForSession(a,..),Request::ViewportForSession(b,..)) |
        (Request::ArrangementViewportForSession(a,..),Request::ArrangementViewportForSession(b,..)) |
        (Request::ArrangementScrollForSession(a,..),Request::ArrangementScrollForSession(b,..)) |
        (Request::ScrollPianoForSession(a,..),Request::ScrollPianoForSession(b,..)) => a==b,
        (Request::ScrollPianoForClip(sa,ta,ca,..),Request::ScrollPianoForClip(sb,tb,cb,..)) => sa==sb && ta==tb && ca==cb,
        (Request::Move { token:a, .. },Request::Move { token:b, .. }) => a==b,
        (Request::ScrollPiano(..),Request::ScrollPiano(..)) => true,
        (Request::SetGain { track: a, .. }, Request::SetGain { track: b, .. }) |
        (Request::SetPan { track: a, .. }, Request::SetPan { track: b, .. }) |
        (Request::Mix { track: a, .. }, Request::Mix { track: b, .. }) => a == b,
        (Request::SynthParameter{track:a,instance:ia,field:fa,..},Request::SynthParameter{track:b,instance:ib,field:fb,..})=>a==b && ia==ib && fa==fb,
        (Request::PadParameter{track:a,instance:ia,pad:pa,field:fa,..},Request::PadParameter{track:b,instance:ib,pad:pb,field:fb,..})=>a==b && ia==ib && pa==pb && fa==fb,
        _ => false,
    }
}
fn resets_scroll_residual(request:&Request)->bool {
    matches!(request, Request::Zoom(..)|Request::ZoomAt(..)|Request::ZoomY(..)|Request::Pan(..)|Request::Pitch(..)|Request::FitNotes|Request::RestoreView|Request::SetMidiViewMode(..)|Request::SelectTrack(..)|Request::SelectClip{..}|Request::ActivateEditorClip{..}|Request::FollowSelection(..)|Request::SetNavigationLinked(..)|Request::Viewport(..)|Request::ViewportForSession(..)|Request::ArrangementViewport(..)|Request::ArrangementViewportForSession(..)|Request::Open(..)|Request::SwitchProject{..})
}
impl Mailbox {
    pub fn new(sender: SyncSender<Envelope>, flags: Arc<AtomicU32>, epoch: Arc<AtomicU64>, audition_epoch: Arc<AtomicU64>) -> Self {
        Self { sender, flags, epoch, audition_epoch, next_token:1, pending: VecDeque::new(), in_flight: HashSet::new(), next_id: 1, piano_scroll_views:HashMap::new(), arrangement_views:HashMap::new(), last_piano_scroll:None }
    }
    pub fn enqueue(&mut self, request: Request) -> Result<(), String> {
        let epoch = self.epoch.load(Ordering::Acquire);
        // Explicit navigation must not inherit a sub-tick remainder from an
        // earlier wheel gesture, even if its View has not arrived yet.
        if resets_scroll_residual(&request) {self.last_piano_scroll=None;}
        if let Some((old_epoch, old)) = self.pending.back_mut() {
            if *old_epoch == epoch && coalesces(old, &request) { *old = request; return Ok(()); }
        }
        if self.pending.len() == 256 {
            if matches!(request, Request::AuditionOff(_) | Request::CancelGesture(_)) { self.release_audition(); }
            return Err("Action not queued: controller is busy. Existing edits are preserved; retry this action.".into());
        }
        self.pending.push_back((epoch, request)); Ok(())
    }
    pub fn token(&mut self)->Result<u64,String> {
        if self.next_token>=(1u64<<63)-1 {return Err("Gesture identity space exhausted".into());}
        let token=self.next_token;self.next_token+=1;Ok(token)
    }
    pub fn audition_on(&mut self,track:u64,instance:u64,pitch:u8,velocity:u8)->Result<u64,String> {
        let token=self.token()?;let epoch=self.audition_epoch.load(Ordering::Acquire);
        self.enqueue(Request::AuditionOn{token,epoch,track,instance,pitch,velocity})?;Ok(token)
    }
    pub fn pad_on(&mut self,track:u64,instance:u64,pad:u64,velocity:u8)->Result<u64,String>{
        let token=self.token()?;let epoch=self.audition_epoch.load(Ordering::Acquire);self.enqueue(Request::PadAuditionOn{token,epoch,track,instance,pad,velocity})?;Ok(token)
    }
    pub fn release_audition(&mut self) {self.audition_epoch.fetch_add(1,Ordering::AcqRel);self.flags.fetch_or(RELEASE,Ordering::Release);}
    pub fn flush(&mut self) -> Result<(), String> {
        for _ in 0..16 {
            if self.in_flight.len() >= 64 { break; }
            let Some((epoch, request)) = self.pending.pop_front() else { break; };
            let request_id = self.next_id;
            if request_id == u64::MAX { self.pending.push_front((epoch, request)); return Err("Command identity space exhausted".into()); }
            let piano=match &request { Request::ScrollPianoForClip(s,t,c,beat,y)=>Some(((*s,*t,*c),*beat,*y)), _=>None };
            let arrangement=match &request {Request::ArrangementViewportForSession(s,..)=>Some(*s),_=>None};
            let reset_residual=resets_scroll_residual(&request);
            let envelope = Envelope { request_id, epoch, request };
            match self.sender.try_send(envelope) {
                Ok(()) => { self.next_id += 1; self.in_flight.insert(request_id); if reset_residual {self.last_piano_scroll=None;} if let Some((key,beat,y))=piano {self.piano_scroll_views.insert(key,request_id);self.last_piano_scroll=Some((key,request_id,beat,y));} if let Some(session)=arrangement {self.arrangement_views.insert(session,request_id);} }
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
    /// Acknowledgments can precede the view containing the command. Keep the
    /// local scroll position until a snapshot actually incorporates it.
    pub fn piano_scroll_unseen(&self, session:u64, track:u64, clip:u64, snapshot:u64) -> bool {
        self.piano_scroll_views.get(&(session,track,clip)).is_some_and(|id|*id>snapshot) ||
            self.pending.iter().any(|(_,r)|matches!(r,Request::ScrollPianoForClip(s,t,c,..) if (*s,*t,*c)==(session,track,clip)))
    }
    pub fn arrangement_view_unseen(&self,session:u64,snapshot:u64)->bool {
        self.arrangement_views.get(&session).is_some_and(|id|*id>snapshot) ||
            self.pending.iter().any(|(_,r)|matches!(r,Request::ArrangementViewportForSession(s,..) if *s==session))
    }
    pub fn piano_scroll_residual(&self,session:u64,track:u64,clip:u64,navigation_id:u64)->Option<(f64,f32)> {
        self.last_piano_scroll.filter(|(key,id,_,_)|*key==(session,track,clip) && *id==navigation_id).map(|(_,_,beat,y)|(beat,y))
    }
    pub fn retire_navigation_views(&mut self,session:u64,snapshot:u64) {
        self.piano_scroll_views.retain(|(s,_,_),id|*s==session && *id>snapshot);
        self.arrangement_views.retain(|s,id|*s==session && *id>snapshot);
        if self.last_piano_scroll.is_some_and(|(key,_,_,_)|key.0!=session) {self.last_piano_scroll=None;}
    }
    pub fn pending_count(&self) -> usize { self.pending.len() + self.in_flight.len() }
    pub fn stop(&mut self) { self.epoch.fetch_add(1, Ordering::AcqRel); self.flags.fetch_or(STOP, Ordering::Release); }
    pub fn panic(&mut self) { self.epoch.fetch_add(1, Ordering::AcqRel); self.flags.fetch_or(PANIC, Ordering::Release); }
}

#[cfg(test)] mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test] fn transient_view_updates_coalesce_only_within_the_same_project_session() {
        for (a,b,same) in [
            (Request::ViewportForSession(0,100.0,100.0),Request::ViewportForSession(0,200.0,200.0),true),
            (Request::ViewportForSession(0,100.0,100.0),Request::ViewportForSession(1,200.0,200.0),false),
            (Request::ArrangementViewportForSession(1,0.0,48.0,100.0),Request::ArrangementViewportForSession(2,1.0,48.0,100.0),false),
            (Request::ArrangementScrollForSession(1,-1.0),Request::ArrangementScrollForSession(2,-2.0),false),
            (Request::ScrollPianoForSession(1,1.0,2.0),Request::ScrollPianoForSession(2,3.0,4.0),false),
            (Request::ScrollPianoForClip(1,2,3,1.0,2.0),Request::ScrollPianoForClip(1,2,4,3.0,4.0),false),
            (Request::ScrollPianoForClip(1,2,3,1.0,2.0),Request::ScrollPianoForClip(1,2,3,3.0,4.0),true),
        ] {assert_eq!(coalesces(&a,&b),same);}
    }
    #[test] fn scroll_remains_unseen_after_ack_until_the_snapshot_includes_it() {
        let (tx,rx)=mpsc::sync_channel(8);let mut m=Mailbox::new(tx,Arc::new(AtomicU32::new(0)),Arc::new(AtomicU64::new(0)),Arc::new(AtomicU64::new(0)));
        m.enqueue(Request::ScrollPianoForClip(7,2,3,1.0,20.0)).unwrap();
        assert!(m.piano_scroll_unseen(7,2,3,0));m.flush().unwrap();let e=rx.try_recv().unwrap();m.acknowledge(e.request_id);
        assert!(m.piano_scroll_unseen(7,2,3,e.request_id-1),"ACK alone does not prove a view has arrived");
        assert!(!m.piano_scroll_unseen(7,2,3,e.request_id));
        assert!(!m.piano_scroll_unseen(7,2,4,0));assert!(!m.piano_scroll_unseen(8,2,3,0));
        m.retire_navigation_views(7,e.request_id);assert!(m.piano_scroll_views.is_empty());
    }
    #[test] fn slider_burst_coalesces_but_gesture_boundaries_and_edits_survive_stop() {
        let (tx, rx) = mpsc::sync_channel(64); let flags = Arc::new(AtomicU32::new(0)); let epoch = Arc::new(AtomicU64::new(0));
        let mut m = Mailbox::new(tx, flags.clone(), epoch.clone(),Arc::new(AtomicU64::new(0)));
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
        let (tx, rx) = mpsc::sync_channel(1); let mut m = Mailbox::new(tx, Arc::new(AtomicU32::new(0)), Arc::new(AtomicU64::new(0)),Arc::new(AtomicU64::new(0)));
        m.enqueue(Request::AddTrack { after: 1 }).unwrap(); m.enqueue(Request::DuplicateTrack(1)).unwrap(); m.flush().unwrap();
        assert_eq!(m.pending_count(), 2); let first = rx.try_recv().unwrap(); m.acknowledge(first.request_id); m.flush().unwrap();
        assert!(matches!(rx.try_recv().unwrap().request, Request::DuplicateTrack(1)));
        for _ in 0..256 { m.enqueue(Request::Undo).unwrap(); }
        assert!(m.enqueue(Request::RemoveTrack(1)).is_err()); assert_eq!(m.pending.len(), 256);
    }
    #[test] fn focus_release_invalidates_audition_epoch_and_preserves_durable_clip_commands() {
        let (tx,rx)=mpsc::sync_channel(64);let flags=Arc::new(AtomicU32::new(0));let epoch=Arc::new(AtomicU64::new(0));let audit=Arc::new(AtomicU64::new(0));
        let mut m=Mailbox::new(tx,flags.clone(),epoch.clone(),audit.clone());let first=m.audition_on(1,3,60,100).unwrap();
        m.enqueue(Request::RenameClip{track:1,clip:4,name:"Keep this edit".into()}).unwrap();m.release_audition();let second=m.audition_on(1,3,60,80).unwrap();
        assert_ne!(first,second);assert_eq!(epoch.load(Ordering::Acquire),0);assert_eq!(audit.load(Ordering::Acquire),1);assert_ne!(flags.load(Ordering::Acquire)&RELEASE,0);
        m.flush().unwrap();let messages:Vec<_>=rx.try_iter().collect();assert_eq!(messages.len(),3);
        assert!(matches!(messages[0].request,Request::AuditionOn{epoch:0,..}));assert!(matches!(messages[1].request,Request::RenameClip{..}));assert!(matches!(messages[2].request,Request::AuditionOn{epoch:1,..}));
    }

}
