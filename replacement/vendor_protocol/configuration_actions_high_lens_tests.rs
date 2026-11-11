use super::*;
use crate::configuration_actions_high::{DisplayCompletionCode, PageBufferAddress, PageIdentifier};
use xm1_lens_transport::{FailureMode, TimingParameters};
use xm1_lens_transport::frame::RetryLimit;
use xm1_lens_transport::wire::{Address, CpuBackend, LensEndpoints, RequestedOutput, TimerInterrupt, TimerTicks, TimerToken, TransferToken};
use crate::lens_queue_runtime::replies::ReplyHalfword;

struct Physical;
impl Mmio for Physical {
    type SavedInterrupts = ();
    fn read_word(&self, _: Address) -> u32 { panic!("unexpected physical read") }
    fn write_word(&self, _: Address, _: u32) { panic!("unexpected physical write") }
    fn read_byte(&self, _: Address) -> u8 { panic!("unexpected physical read") }
    fn write_byte(&self, _: Address, _: u8) { panic!("unexpected physical write") }
    fn mask_interrupts(&self) {}
    fn restore_interrupts(&self, _: ()) {}
    fn publish_dma(&self) {}
}
impl Platform for Physical {
    type Error = ();
    fn acquire(&mut self, _: LensEndpoints) -> Result<(), ()> { panic!("unexpected physical acquire") }
    fn release(&mut self, _: LensEndpoints) -> Result<(), ()> { panic!("unexpected physical release") }
    fn request_output(&mut self, _: RequestedOutput) -> Result<(), ()> { panic!("unexpected output request") }
}
impl InterruptController for Physical {
    type Error = ();
    fn register_wire(&mut self, _: TransferToken) -> Result<(), ()> { panic!("unexpected wire interrupt") }
    fn unregister_wire(&mut self) -> Result<(), ()> { panic!("unexpected wire interrupt") }
    fn register_timer(&mut self, _: TimerInterrupt, _: TimerToken) -> Result<(), ()> { panic!("unexpected timer interrupt") }
    fn unregister_timer(&mut self, _: TimerInterrupt) -> Result<(), ()> { panic!("unexpected timer interrupt") }
}
fn runtime() -> LensQueueRuntime<Physical, Physical, Physical> {
    let ticks = TimerTicks::new(1).unwrap();
    let timing = TimingParameters { command: ticks, normal: ticks, deferred: ticks, accepted_page: ticks, rejected_page: ticks, recovery: ticks, recovery_limit: RetryLimit::new(1), busy_limit: RetryLimit::new(1), failure_mode: FailureMode::Notify };
    LensQueueRuntime::new(CpuBackend::new(Physical, Physical, Physical), timing, [0xff; 64])
}
fn callbacks() -> CallbackState {
    CallbackState { result: CpuReturn::REJECTED, high_byte: 0xa5, low_byte: 0x5a, page: PageIdentifier(0), page_buffer: PageBufferAddress(0), write_page: PageIdentifier(0), display_result: DisplayCompletionCode(0) }
}
fn status(status: Status) -> Publication {
    Publication::Command { callback: STATUS_CALLBACK, operation: ControlOperation::parse(RequestId(0x7248)).unwrap(), status, data: ReplyData::StatusOnly }
}
fn word(value: u16) -> Publication {
    Publication::Command { callback: WORD_CALLBACK, operation: ControlOperation::parse(RequestId(0x723c)).unwrap(), status: Status::Complete, data: ReplyData::Halfword(ReplyHalfword(value)) }
}

#[test]
fn pair_reads_publish_each_completed_pair_in_its_configuration_window() {
    for (action, start) in [(LensAction::ReadSelectedPairs, 0x83), (LensAction::ReadBothPairs, 0x81)] {
        let mut encoded = [0u8; 0x100];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
        let mut runtime = runtime();
        let mut actions = LensActions::new();
        let mut callbacks = callbacks();
        assert_eq!(actions.begin(action, &configuration, &mut runtime).unwrap(), None);
        assert_eq!(actions.publication(status(Status::Complete), &mut configuration, &mut callbacks, &mut runtime).unwrap(), None);
        assert_eq!(actions.publication(word(0x1234), &mut configuration, &mut callbacks, &mut runtime).unwrap(), None);
        assert_eq!(configuration.byte(start), Some(0x12));
        assert_eq!(configuration.byte(start + 1), Some(0x34));
        assert_eq!(configuration.byte(start + 2), Some(0));
        assert_eq!(actions.publication(word(0xabcd), &mut configuration, &mut callbacks, &mut runtime).unwrap(), Some(ActionCompletion::service_success()));
        assert_eq!(configuration.byte(start + 2), Some(0xab));
        assert_eq!(configuration.byte(start + 3), Some(0xcd));
    }
}

#[test]
fn completed_first_pair_survives_failed_second_pair() {
    let mut encoded = [0u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut runtime = runtime();
    let mut actions = LensActions::new();
    let mut callbacks = callbacks();
    actions.begin(LensAction::ReadBothPairs, &configuration, &mut runtime).unwrap();
    actions.publication(status(Status::Complete), &mut configuration, &mut callbacks, &mut runtime).unwrap();
    actions.publication(word(0x1234), &mut configuration, &mut callbacks, &mut runtime).unwrap();
    let failure = Publication::Command { callback: WORD_CALLBACK, operation: ControlOperation::parse(RequestId(0x723c)).unwrap(), status: Status::Stopped, data: ReplyData::Halfword(ReplyHalfword(0xffff)) };
    assert_eq!(actions.publication(failure, &mut configuration, &mut callbacks, &mut runtime).unwrap(), Some(ActionCompletion::service_rejected()));
    assert_eq!(configuration.byte(0x81), Some(0x12));
    assert_eq!(configuration.byte(0x82), Some(0x34));
    assert_eq!(configuration.byte(0x83), Some(0));
    assert_eq!(configuration.byte(0x84), Some(0));
}

#[test]
fn invalid_modes_reject_without_starting_a_wait() {
    for action in [LensAction::ReadSelectedPairs, LensAction::SetControlPair, LensAction::SetLevelSelection] {
        let mut encoded = [0u8; 0x100];
        encoded[0x81] = 0xff;
        let configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
        let mut runtime = runtime();
        let mut actions = LensActions::new();
        assert_eq!(actions.begin(action, &configuration, &mut runtime).unwrap(), Some(ActionCompletion::service_rejected()));
        assert_eq!(actions.wait_failed(), None);
        assert_eq!(runtime.pop_publication(), None);
    }
}

#[test]
fn timeout_finishes_wait_but_late_word_still_updates_callback_state() {
    let mut encoded = [0u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut runtime = runtime();
    let mut actions = LensActions::new();
    let mut callbacks = callbacks();
    actions.begin(LensAction::ReadBothPairs, &configuration, &mut runtime).unwrap();
    assert_eq!(actions.wait_failed(), Some(ActionCompletion::service_rejected()));
    assert_eq!(actions.wait_failed(), None);
    assert_eq!(actions.publication(word(0x5678), &mut configuration, &mut callbacks, &mut runtime).unwrap(), None);
    assert_eq!((callbacks.high_byte, callbacks.low_byte), (0x56, 0x78));
    assert_eq!(configuration.byte(0x81), Some(0));
}

#[test]
fn control_pair_and_level_selection_finish_only_on_completion() {
    for action in [LensAction::SetControlPair, LensAction::SetLevelSelection] {
        let mut encoded = [0u8; 0x100];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
        let mut runtime = runtime();
        let mut actions = LensActions::new();
        let mut callbacks = callbacks();
        assert_eq!(actions.begin(action, &configuration, &mut runtime).unwrap(), None);
        let first = actions.publication(status(Status::Complete), &mut configuration, &mut callbacks, &mut runtime).unwrap();
        let completed = if action == LensAction::SetControlPair {
            assert_eq!(first, None);
            actions.publication(status(Status::Complete), &mut configuration, &mut callbacks, &mut runtime).unwrap()
        } else { first };
        assert_eq!(completed, Some(ActionCompletion::service_success()));
    }
}

#[test]
fn transfer_queries_clear_previous_errors_and_publish_big_endian_values() {
    for (query, callback, operation, data) in [
        (TransferQuery::Word, TRANSFER_WORD_CALLBACK, 0x7239, ReplyData::Halfword(ReplyHalfword(0xa55a))),
        (TransferQuery::Bits, TRANSFER_BITS_CALLBACK, 0x7238, ReplyData::Flags(crate::lens_queue_runtime::replies::ReplyFlags16([false, true, false, true, true, false, true, false, true, false, true, false, false, true, false, true]))),
    ] {
        let mut encoded = [0u8; 0x100];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
        let mut runtime = runtime();
        let mut actions = LensActions::new();
        let mut callbacks = callbacks();
        assert_eq!(actions.begin_query(query, &configuration, &mut callbacks, &mut runtime).unwrap(), None);
        assert_eq!(callbacks.result, CpuReturn::SUCCESS);
        assert_eq!(actions.waiting_notification(), Some(CallbackNotification::Transfer));
        let publication = Publication::Command { callback, operation: ControlOperation::parse(RequestId(operation)).unwrap(), status: Status::Complete, data };
        assert_eq!(actions.publication(publication, &mut configuration, &mut callbacks, &mut runtime).unwrap(), Some(ActionCompletion::service_success()));
        assert_eq!(configuration.byte(0x82), Some(0xa5));
        assert_eq!(configuration.byte(0x83), Some(0x5a));
    }
}

#[test]
fn stopped_transfer_query_leaves_configuration_outputs_unchanged() {
    let mut encoded = [0u8; 0x100];
    encoded[0x82] = 0x12;
    encoded[0x83] = 0x34;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut runtime = runtime();
    let mut actions = LensActions::new();
    let mut callbacks = callbacks();
    actions.begin_query(TransferQuery::Word, &configuration, &mut callbacks, &mut runtime).unwrap();
    let publication = runtime.pop_publication().unwrap();
    assert_eq!(actions.publication(publication, &mut configuration, &mut callbacks, &mut runtime).unwrap(), Some(ActionCompletion::service_rejected()));
    assert_eq!(configuration.byte(0x82), Some(0x12));
    assert_eq!(configuration.byte(0x83), Some(0x34));
}
