use super::mode_frontend::{LifecycleOperation, ModalController, ModalEvent, ModeFrontend, FRAME_BYTES};
use super::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId, TransitionToken};

struct Queue {
    messages: std::vec::Vec<(u16, u8, u8, std::vec::Vec<u8>)>,
    failure: Option<KernelStatus>,
}
impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(6) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if let Some(failure) = self.failure { return Err(failure); }
        self.messages.push((message.operation.0, message.sender.0, message.destination.0, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { Ok(()) }
}

#[test]
fn save_and_terminal_send_actual_queue_envelopes() {
    let mut frame = [0u8; FRAME_BYTES];
    let mut frontend = ModeFrontend::new(&mut frame);
    let mut queue = Queue { messages: std::vec::Vec::new(), failure: None };
    let mut residue = [0xa7; 16];
    assert_eq!(frontend.save_action(&mut queue, &mut residue).unwrap().0, 2);
    assert_eq!(queue.messages[0], (2, 6, 2, 0x1041u32.to_le_bytes().to_vec()));
    assert_eq!(frontend.terminal_action(&mut queue, &mut residue).unwrap().0, u32::MAX);
    assert_eq!(queue.messages[1].0, 3);
    assert_eq!(&queue.messages[1].3[..4], &0x81u32.to_le_bytes());
    assert_eq!(&queue.messages[1].3[4..12], &[0xa7; 8]);
    assert_eq!(&queue.messages[1].3[12..], &0x1081u32.to_le_bytes());
}

#[test]
fn completion_normalizer_uses_full_event_token_and_entry_precedence() {
    let mut frame = [0u8; FRAME_BYTES];
    frame[..4].copy_from_slice(&0x816u32.to_le_bytes());
    frame[4..8].copy_from_slice(&0x1041u32.to_le_bytes());
    frame[0xb15a..0xb15c].copy_from_slice(&0x1041u16.to_le_bytes());
    let selected = ModeFrontend::new(&mut frame).normalized_save().unwrap();
    assert_eq!(selected.controller, ModalController::Save);
    assert_eq!(selected.event, ModalEvent::Complete);
    frame[4..8].copy_from_slice(&0x11041u32.to_le_bytes());
    assert_eq!(ModeFrontend::new(&mut frame).normalized_save(), None);
    frame[0xb0cc..0xb0d0].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(ModeFrontend::new(&mut frame).normalized_save().unwrap().event, ModalEvent::Enter);
}

#[test]
fn token_generation_preserves_family_and_wraps_six_bit_sequence() {
    let mut frame = [0u8; FRAME_BYTES];
    frame[0xb15a..0xb15c].copy_from_slice(&0x107fu16.to_le_bytes());
    assert_eq!(ModeFrontend::new(&mut frame).issue_token(LifecycleOperation::Save), TransitionToken(0x1040));
    assert_eq!(ModeFrontend::new(&mut frame).issue_token(LifecycleOperation::Save), TransitionToken(0x1041));
    assert_eq!(ModeFrontend::new(&mut frame).issue_token(LifecycleOperation::Terminal), TransitionToken(0x1081));
}

#[test]
fn failed_post_keeps_issued_token_and_surfaces_native_status() {
    let mut frame = [0u8; FRAME_BYTES];
    let mut queue = Queue { messages: std::vec::Vec::new(), failure: Some(KernelStatus(-19)) };
    let mut residue = [0x56; 16];
    assert_eq!(ModeFrontend::new(&mut frame).save_action(&mut queue, &mut residue), Err(KernelStatus(-19)));
    queue.failure = None;
    ModeFrontend::new(&mut frame).save_action(&mut queue, &mut residue).unwrap();
    assert_eq!(&queue.messages[0].3, &0x1042u32.to_le_bytes());
}

#[test]
fn power_route_suppresses_only_actual_terminal_pending_modes() {
    use super::mode_frontend::{power_off_gate, PendingMode, PowerOffGate};
    use super::shutdown::OuterState;
    for pending in [12, 13, 14] {
        assert_eq!(power_off_gate(OuterState::from_abi(10), PendingMode(pending)), PowerOffGate::Suppressed);
    }
    for pending in [0, 3, 8, 11, 15] {
        assert_eq!(power_off_gate(OuterState::from_abi(10), PendingMode(pending)), PowerOffGate::Selected);
    }
    assert_eq!(power_off_gate(OuterState::from_abi(9), PendingMode(13)), PowerOffGate::NotSelected);
}

#[test]
fn repeated_matching_completions_remain_modal_dispatch_candidates() {
    let mut frame = [0u8; FRAME_BYTES];
    frame[..4].copy_from_slice(&0x816u32.to_le_bytes());
    frame[4..8].copy_from_slice(&0x1041u32.to_le_bytes());
    frame[0xb15a..0xb15c].copy_from_slice(&0x1041u16.to_le_bytes());
    let frontend = ModeFrontend::new(&mut frame);
    assert_eq!(frontend.normalized_save().unwrap().event, ModalEvent::Complete);
    assert_eq!(frontend.normalized_save().unwrap().event, ModalEvent::Complete);
}
