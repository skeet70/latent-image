use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect { Word(Register, u32), Half(Register, u16), Event(TransportEvent) }
struct Recorder { effects: [Option<Effect>; 128], count: usize }
impl Recorder {
    fn new() -> Self { Self { effects: [None; 128], count: 0 } }
    fn push(&mut self, effect: Effect) { self.effects[self.count] = Some(effect); self.count += 1; }
    fn observed(&self) -> &[Option<Effect>] { &self.effects[..self.count] }
}
impl ControllerIo for Recorder {
    fn read32(&mut self, _: Register) -> u32 { 0 }
    fn read16(&mut self, _: Register) -> u16 { 0 }
    fn write32(&mut self, register: Register, value: u32) { self.push(Effect::Word(register, value)); }
    fn write16(&mut self, register: Register, value: u16) { self.push(Effect::Half(register, value)); }
}
impl EventSink for Recorder { fn emit(&mut self, event: TransportEvent) { self.push(Effect::Event(event)); } }

#[test]
fn published_aperture_refuses_overflow_zero_and_excess_spans() {
    assert_eq!(PublishedRegion::new(u32::MAX, 1), Err(TransportError::AddressOverflow));
    let region = PublishedRegion::new(0x1000, 64).unwrap();
    assert_eq!(region.span(0, 0), Err(TransportError::EmptySpan));
    assert_eq!(region.span(32, 33), Err(TransportError::OutsideRegion));
    assert_eq!(region.span(u32::MAX, 2), Err(TransportError::AddressOverflow));
    let span = region.span(32, 32).unwrap();
    assert_eq!((span.address(), span.offset(), span.length()), (0x1020, 32, 32));
}

#[test]
fn bulk_source_consumes_both_readiness_flags_only_on_selection() {
    let region = PublishedRegion::new(0x1000, 64).unwrap();
    let mut transport = TransportState::new();
    let (mut armed, mut ready) = (true, false);
    assert_eq!(transport.bulk_in_source(region, 0, 16, 16, false, &mut armed, &mut ready), Ok(SourceAction::InNotReady));
    assert!(armed);
    ready = true;
    let action = transport.bulk_in_source(region, 0, 16, 16, false, &mut armed, &mut ready).unwrap();
    assert_eq!(action, SourceAction::Transfer(region.span(0, 16).unwrap()));
    assert!(!armed && !ready);
}

#[test]
fn failed_completion_does_not_publish_status_or_consume_response() {
    let mut transport = TransportState::new();
    transport.set_response(ResponseKind::Bulk, [0x24, 1]);
    assert_eq!(transport.bulk_in_complete(CompletionStatus::Failed(-1)), CompletionAction::None);
    assert_eq!(transport.bulk_in_complete(CompletionStatus::Success), CompletionAction::SendStatus([0x24, 1]));
    assert_eq!(transport.bulk_in_complete(CompletionStatus::Success), CompletionAction::None);
}

#[test]
fn setup_only_accepts_vendor_host_to_device() {
    let mut transport = TransportState::new();
    assert_eq!(transport.setup(0x40), SetupAction::ReceiveControl { length: 12 });
    assert_eq!(transport.setup(0xc0), SetupAction::Reject);
    assert_eq!(transport.setup(0x20), SetupAction::Reject);
}

#[test]
fn interrupt_programs_clear_registers_and_always_queues_status() {
    let mut transport = TransportState::new();
    let mut io = Recorder::new();
    let mut events = Recorder::new();
    transport.interrupt(&mut io, &mut events);
    transport.interrupt(&mut io, &mut events);
    assert_eq!(&io.observed()[..4], &[
        Some(Effect::Word(Register::InterruptReset, 0x81810303)),
        Some(Effect::Half(Register::EventClearLow, 0xffff)),
        Some(Effect::Half(Register::EventClearMiddle, 0xffff)),
        Some(Effect::Half(Register::EventClearHigh, 0xffff)),
    ]);
    assert_eq!(events.observed(), &[
        Some(Effect::Event(TransportEvent::QueueStatus)),
        Some(Effect::Event(TransportEvent::QueueStatus)),
    ]);
}

#[test]
fn fifo_emits_little_endian_words_and_tail_commit() {
    let mut io = Recorder::new();
    write_fifo(Endpoint::BulkIn, &[1, 2, 3, 4, 5, 6], &mut io);
    assert_eq!(io.observed(), &[
        Some(Effect::Word(Register::Fifo(Endpoint::BulkIn), 0x04030201)),
        Some(Effect::Word(Register::FifoMode, 0xc0000)),
        Some(Effect::Word(Register::Fifo(Endpoint::BulkIn), 0x0605)),
        Some(Effect::Word(Register::FifoMode, 18)),
        Some(Effect::Half(Register::FifoCommit, 4)),
    ]);
}

#[test]
fn handle_generation_skips_zero_and_rejects_occupied_slots() {
    use super::cpu::{HandleSequence, HandleTable};
    let mut entries = [None; 2];
    let mut handles = HandleTable::new(&mut entries);
    let mut sequence = HandleSequence::new(u32::MAX);
    let first = handles.allocate(0, &mut sequence).unwrap();
    let second = handles.allocate(1, &mut sequence).unwrap();
    assert_eq!((first.get(), second.get()), (1, 2));
    assert_eq!(handles.allocate(0, &mut sequence), Err(TransportError::ChannelBusy));
    assert_eq!(handles.channel(second), Ok(Channel::One));
    handles.release(first).unwrap();
    assert_eq!(handles.find(first), Err(TransportError::InvalidChannel));
}

#[test]
fn selector_reset_is_distinct_from_profile_index_operations() {
    assert_eq!(profile_selector(5, 255, 0), Ok(ProfileAction::ResetModes));
    assert_eq!(profile_selector(1, 16, 0), Err(TransportError::InvalidSelector));
    assert_eq!(TransportEvent::Disconnect.wire_message(), Some((1, 0x90, 0)));
}

#[test]
fn status_submission_refuses_busy_without_touching_controller() {
    let mut transmitter = StatusTransmitter::new();
    let mut io = Recorder::new();
    transmitter.set_phase(TransmitPhase::Busy);
    assert_eq!(transmitter.submit(&[0x24, 1], &mut io), Err(TransportError::ChannelBusy));
    assert!(io.observed().is_empty());
    transmitter.set_phase(TransmitPhase::Idle);
    assert_eq!(transmitter.submit(&[0x24, 1], &mut io), Ok(2));
    assert_eq!(io.observed(), &[
        Some(Effect::Word(Register::FifoMode, 0xc0000)),
        Some(Effect::Word(Register::Fifo(Endpoint::BulkIn), 0x124)),
        Some(Effect::Word(Register::FifoMode, 18)),
        Some(Effect::Half(Register::FifoCommit, 4)),
    ]);
}

#[test]
fn whole_cache_clean_visits_selected_geometry_in_set_way_order() {
    struct Cache { selected: Option<u32>, operands: [u32; 4], count: usize }
    impl cpu::CacheInstructions for Cache {
        fn level_id(&mut self) -> u32 { 0x0100_0003 }
        fn select_level(&mut self, selector: u32) { self.selected = Some(selector); }
        fn instruction_barrier(&mut self) {}
        fn size_id(&mut self) -> u32 { (1 << 3) | (1 << 13) }
        fn clean_invalidate_set_way(&mut self, operand: u32) { self.operands[self.count] = operand; self.count += 1; }
        fn clean_invalidate_address(&mut self, _: u32) { panic!("range operation on whole-cache clean"); }
    }
    let mut cache = Cache { selected: None, operands: [0; 4], count: 0 };
    cpu::clean_invalidate_all(&mut cache);
    assert_eq!(cache.selected, Some(0));
    assert_eq!(cache.operands, [0x8000_0010, 0x10, 0x8000_0000, 0]);
}

#[test]
fn pending_interrupt_publishes_one_queue_status_per_take() {
    let mut transport = TransportState::new();
    let mut events = Recorder::new();
    transport.mark_pending(&mut events);
    assert!(transport.take_pending());
    assert!(!transport.take_pending());
    assert_eq!(events.observed(), &[
        Some(Effect::Event(TransportEvent::QueueStatus)),
    ]);
}

#[test]
fn upload_completion_clears_receive_enable_without_changing_other_flags() {
    let mut state = ReceiveProgramming::new(0xa5, 0x40, 4);
    state.begin_receive();
    assert!(state.receive_enabled());
    state.clear_receive_state();
    assert!(!state.receive_enabled());
    assert_eq!((state.endpoint_mask(), state.pending_events(), state.dma_events()), (0xa5, 0x40, 4));
    assert!(!state.active());
}

impl ChannelPlatform for Recorder {
    fn register_interrupt(&mut self, _: Channel, _: ChannelCallback) -> Result<(), TransportError> { Ok(()) }
    fn synchronize(&mut self) {}
    fn delay_start(&mut self) -> Result<(), TransportError> { Ok(()) }
}

#[test]
fn admitted_cpu_alias_is_translated_after_rounded_aperture_check() {
    let mut aperture = [0u8; 128];
    let region = PublishedRegion::new(0x4104_0000, 128).unwrap();
    let transport = PublishedAperture::publish(region, &mut aperture).unwrap();
    let mut handles = [None; 2];
    let mut table = cpu::HandleTable::new(&mut handles);
    let mut sequence = cpu::HandleSequence::new(0);
    let mut opening = Recorder::new();
    let (mut channel, _) = cpu::open_channel(Channel::Zero, &mut table, &mut sequence, &mut opening).unwrap();
    let mut state = ReceiveProgramming::new(0, 0, 0);
    let mut io = Recorder::new();
    assert_eq!(channel.program_receive(&transport, region.span(64, 32).unwrap(), &mut state, &mut io), Ok(()));
    assert!(io.observed().contains(&Some(Effect::Word(Register::ChannelAddress(Channel::Zero), 0x0104_0040))));
    assert!(io.observed().contains(&Some(Effect::Word(Register::ChannelLength(Channel::Zero), 64))));
    assert!(state.active());

    let mut shorter_aperture = [0u8; 95];
    let shorter_region = PublishedRegion::new(0x4104_0000, 95).unwrap();
    let shorter_transport = PublishedAperture::publish(shorter_region, &mut shorter_aperture).unwrap();
    let mut other_handles = [None; 2];
    let mut other_table = cpu::HandleTable::new(&mut other_handles);
    let (mut other_channel, _) = cpu::open_channel(Channel::Zero, &mut other_table, &mut sequence, &mut opening).unwrap();
    let mut untouched = Recorder::new();
    assert_eq!(other_channel.program_receive(&shorter_transport, shorter_region.span(64, 31).unwrap(), &mut state, &mut untouched), Err(TransportError::OutsideRegion));
    assert!(untouched.observed().is_empty());
}

#[test]
fn callback_state_is_shared_across_independently_published_buffers() {
    let mut request_bytes = [0u8; 64];
    let mut reply_bytes = [0u8; 32];
    let request = PublishedAperture::publish(PublishedRegion::new(0x4104_0000, 64).unwrap(), &mut request_bytes).unwrap();
    let reply = PublishedAperture::publish(PublishedRegion::new(0x42e4_0000, 32).unwrap(), &mut reply_bytes).unwrap();
    let mut state = TransportState::new();
    state.arm_receive(request.region(), 0, 8, 8, false).unwrap();
    state.set_response(ResponseKind::Control, [0x24, 1]);
    assert_eq!(state.control_complete(CompletionStatus::Success), CompletionAction::SendStatus([0x24, 1]));
    let (mut armed, mut ready) = (true, true);
    assert_eq!(state.bulk_in_source(reply.region(), 0, 2, 2, false, &mut armed, &mut ready), Ok(SourceAction::InNotReady));
    assert!(armed && ready);
    assert_eq!(state.bulk_out_source(), Ok(SourceAction::Transfer(request.region().span(0, 8).unwrap())));
    assert_eq!(state.bulk_in_source(reply.region(), 0, 2, 2, false, &mut armed, &mut ready), Ok(SourceAction::Transfer(reply.region().span(0, 2).unwrap())));
    assert!(!armed && !ready);
    assert_eq!(reply.bytes(request.region().span(0, 8).unwrap()), Err(TransportError::OutsideRegion));
}

#[test]
fn receive_progress_retains_selected_region_without_borrowing_an_aperture() {
    let request = PublishedRegion::new(0x4104_0000, 64).unwrap();
    let mut state = TransportState::new();
    state.arm_receive(request, 4, 12, 4, true).unwrap();
    assert_eq!(state.bulk_out_source(), Ok(SourceAction::Transfer(request.span(4, 4).unwrap())));
    assert_eq!(state.bulk_out_complete(CompletionStatus::Success), Ok(CompletionAction::None));
    assert_eq!(state.bulk_out_source(), Ok(SourceAction::Transfer(request.span(8, 4).unwrap())));
    assert_eq!(state.bulk_out_complete(CompletionStatus::Success), Ok(CompletionAction::None));
    assert_eq!(state.bulk_out_source(), Ok(SourceAction::Transfer(request.span(12, 4).unwrap())));
    assert_eq!(state.bulk_out_complete(CompletionStatus::Success), Ok(CompletionAction::ReceiveComplete { start: 4, length: 12, status: None }));
}

#[test]
fn clearing_receive_preserves_status_suppression_and_pending_notification() {
    let mut state = TransportState::new();
    state.arm_receive(PublishedRegion::new(0x1000, 64).unwrap(), 4, 12, 4, true).unwrap();
    state.set_response(ResponseKind::Control, [0, 1]);
    assert_eq!(state.control_complete(CompletionStatus::Success), CompletionAction::SendStatus([0, 1]));
    state.set_response(ResponseKind::Bulk, [9, 1]);
    let mut events = Recorder::new();
    state.mark_pending(&mut events);
    state.clear_receive();
    assert_eq!(state.bulk_out_source(), Ok(SourceAction::OutSuppressed));
    assert_eq!(state.bulk_out_source(), Err(TransportError::BufferTooSmall));
    assert!(state.take_pending());
    assert_eq!(state.bulk_in_complete(CompletionStatus::Success), CompletionAction::SendStatus([9, 1]));
    assert_eq!(state.bulk_out_complete(CompletionStatus::Success), Ok(CompletionAction::ProcessReceive { status: None }));
}
