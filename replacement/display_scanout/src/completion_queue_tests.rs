use crate::{commit::{CallbackId, CommitPending, RuntimeState}, completion_queue::{self, CallbackPacket, CommitDeferral, CommitEvent, Critical, QueuedRuntime}, Error};
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};
#[derive(Default)]
struct Interrupts(Vec<u32>);
impl Critical for Interrupts {
    type Token = u32;
    fn enter(&mut self) -> u32 { self.0.push(0x51); 0xa5 }
    fn leave(&mut self, token: u32) { self.0.push(token); }
}
fn initialized() -> RuntimeState { RuntimeState { capture_count: 1, ..RuntimeState::default() } }
#[test]
fn six_word_packet_preserves_occupied_slots_and_requests_one_commit() {
    let mut state = initialized();
    state.secondary_callbacks = [Some(CallbackId(0x91)), None, Some(CallbackId(0x93)), None, None, Some(CallbackId(0x96))];
    let mut packet = [Some(CallbackId(0x11)), Some(CallbackId(0x22)), Some(CallbackId(0x33)), Some(CallbackId(0x44)), None, Some(CallbackId(0x66))];
    let mut critical = Interrupts::default();
    assert_eq!(completion_queue::packet(&mut QueuedRuntime { runtime: &mut state, deferral: CommitDeferral(0) }, Some(&mut packet), &mut critical), Ok(()));
    assert_eq!(state.secondary_callbacks, [Some(CallbackId(0x91)), Some(CallbackId(0x22)), Some(CallbackId(0x93)), Some(CallbackId(0x44)), None, Some(CallbackId(0x96))]);
    assert_eq!(state.pending, CommitPending(1));
    assert_eq!(critical.0, vec![0x51, 0xa5]);
}
#[test]
fn event_six_aliases_secondary_zero_without_replacing_a_live_callback() {
    let mut state = initialized();
    state.secondary_callbacks[0] = Some(CallbackId(0x71));
    let mut critical = Interrupts::default();
    assert_eq!(completion_queue::event(&mut QueuedRuntime { runtime: &mut state, deferral: CommitDeferral(0) }, CommitEvent::parse(6).unwrap(), Some(CallbackId(0x72)), &mut critical), Ok(()));
    assert_eq!(state.secondary_callbacks[0], Some(CallbackId(0x71)));
    assert_eq!(state.completion_callbacks, [None; 6]);
    assert_eq!(state.pending, CommitPending(1));
    assert_eq!(critical.0, vec![0x51, 0xa5]);
    assert_eq!(CommitEvent::parse(7), None);
    assert_eq!(CommitEvent::parse(u32::MAX), None);
}
#[test]
fn null_packet_still_requests_commit_and_nonzero_deferral_preserves_pending() {
    let mut state = initialized();
    let mut critical = Interrupts::default();
    assert_eq!(completion_queue::packet::<[Option<CallbackId>; 6]>(&mut QueuedRuntime { runtime: &mut state, deferral: CommitDeferral(0) }, None, &mut critical), Ok(()));
    assert_eq!(state.pending, CommitPending(1));
    state.pending = CommitPending(0x44);
    assert_eq!(completion_queue::event(&mut QueuedRuntime { runtime: &mut state, deferral: CommitDeferral(0x80000000) }, CommitEvent::parse(2).unwrap(), Some(CallbackId(0x81)), &mut critical), Ok(()));
    assert_eq!(state.pending, CommitPending(0x44));
    assert_eq!(state.completion_callbacks[2], Some(CallbackId(0x81)));
}
#[test]
fn uninitialized_request_has_no_critical_or_callback_effect() {
    let mut state = RuntimeState::default();
    let before = state.clone();
    let mut critical = Interrupts::default();
    let mut packet = [Some(CallbackId(0x11)); 6];
    assert_eq!(completion_queue::packet(&mut QueuedRuntime { runtime: &mut state, deferral: CommitDeferral(0) }, Some(&mut packet), &mut critical), Err(Error::Uninitialized));
    assert_eq!(state, before);
    assert!(critical.0.is_empty());
}
struct Reads { values: [Option<CallbackId>; 6], reads: Rc<RefCell<Vec<usize>>> }
impl CallbackPacket for Reads {
    fn read(&mut self, index: usize) -> Option<CallbackId> { self.reads.borrow_mut().push(index); self.values[index] }
}
#[test]
fn packet_does_not_read_inputs_for_occupied_destinations() {
    let mut state = initialized();
    state.secondary_callbacks = [Some(CallbackId(1)), None, Some(CallbackId(3)), None, Some(CallbackId(5)), None];
    let reads = Rc::new(RefCell::new(Vec::new()));
    let mut packet = Reads { values: [Some(CallbackId(0xa)); 6], reads: reads.clone() };
    assert_eq!(completion_queue::packet(&mut QueuedRuntime { runtime: &mut state, deferral: CommitDeferral(0) }, Some(&mut packet), &mut Interrupts::default()), Ok(()));
    assert_eq!(*reads.borrow(), vec![1, 3, 5]);
    assert_eq!(state.secondary_callbacks, [Some(CallbackId(1)), Some(CallbackId(0xa)), Some(CallbackId(3)), Some(CallbackId(0xa)), Some(CallbackId(5)), Some(CallbackId(0xa))]);
}
