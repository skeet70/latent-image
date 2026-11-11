use super::*;
use crate::shutdown::TaskId;
use std::{collections::VecDeque, vec::Vec};

#[derive(Default)]
struct Transport { messages: VecDeque<UiPacket>, statuses: VecDeque<KernelStatus>, attempts: usize }
impl ShutdownRtos for Transport {
    fn current_task(&mut self) -> TaskId { TaskId(7) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        assert_eq!(message.operation, RequestId(0x3100));
        assert_eq!(message.destination, QueueId(33));
        assert_eq!(message.sender, TaskId(7));
        self.attempts += 1;
        if let Some(status) = self.statuses.pop_front() { return Err(status); }
        self.messages.push_back(UiPacket(message.payload.try_into().unwrap()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { panic!("unexpected event flag") }
}
impl Queue33Receive for Transport {
    fn receive(&mut self) -> Result<UiPacket, KernelStatus> { self.messages.pop_front().ok_or(KernelStatus(-50)) }
}
#[derive(Default)]
struct Timing { delays: Vec<u32>, notifications: Vec<(UiCode, u32)> }
impl QueueTiming for Timing {
    fn delay_ticks(&mut self, ticks: u32) { self.delays.push(ticks); }
    fn posted(&mut self, code: UiCode, phase: u32) { self.notifications.push((code, phase)); }
}
struct Menu { context: ResumeMenuContext, selected: Vec<u8> }
impl MenuResume for Menu {
    fn context(&self) -> ResumeMenuContext { self.context }
    fn select_and_snapshot(&mut self, low: u8) { self.selected.push(low); }
}
fn menu() -> Menu { Menu { context: ResumeMenuContext { current: UiCode(0x4011), mode: MenuMode(0), alternative: false, selection: MenuMode(0) }, selected: Vec::new() } }
fn residue() -> CallerResidue { CallerResidue { packet: [0xa5; 12], application_record: [0x6b; 76] } }
#[derive(Default)]
struct Events(Vec<(ApplicationEvent, [u8; 76])>);
impl ApplicationEvents for Events { fn post(&mut self, event: ApplicationEvent, record: &[u8; 76]) { self.0.push((event, *record)); } }

#[test]
fn delivery_requires_actual_queue_receive_and_preserves_event_tail() {
    let mut route = UiCompletion::new();
    let mut transport = Transport::default();
    let mut timing = Timing::default();
    let mut menu = menu();
    let mut residue = residue();
    let mut events = Events::default();
    let mut consumer = PowerConsumer { initialized: true, blocked: false, block_on_delivery: false };
    route.request(UiRequest::PowerOff, &mut transport, &mut timing, &mut menu, &mut residue).unwrap();
    assert!(events.0.is_empty());
    assert_eq!(route.last_posted(), UiCode(0x500e));
    assert_eq!(route.receive(&mut transport, &mut consumer, &mut events, &mut residue), Ok(Delivery::PowerOff { events: 2 }));
    for (event, record) in &events.0 {
        assert_eq!(*event, ApplicationEvent(0xa2f));
        assert_eq!(&record[..8], &[0x2f, 0xa, 0, 0, 0x10, 1, 0, 0]);
        assert_eq!(&record[8..], &[0x6b; 68]);
    }
    assert_eq!(route.receive(&mut transport, &mut consumer, &mut events, &mut residue), Err(KernelStatus(-50)));
    assert_eq!(events.0.len(), 2);
}

#[test]
fn blocked_and_uninitialized_consumers_do_not_invent_a_second_post() {
    let mut bytes = [0; 12]; bytes[..2].copy_from_slice(&0x500eu16.to_le_bytes());
    let mut residue = residue();
    let mut events = Events::default();
    let mut consumer = PowerConsumer { initialized: false, blocked: false, block_on_delivery: true };
    assert_eq!(consumer.deliver(UiPacket(bytes), &mut events, &mut residue), Delivery::NotInitialized);
    assert!(events.0.is_empty());
    consumer.initialized = true;
    assert_eq!(consumer.deliver(UiPacket(bytes), &mut events, &mut residue), Delivery::PowerOff { events: 1 });
    assert_eq!(events.0.len(), 1);
}

#[test]
fn suspended_table_accepts_thirty_entries_and_checks_bound_before_duplicate() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut menu, mut residue) = (Transport::default(), Timing::default(), menu(), residue());
    route.initialize(true, &mut transport, &mut timing, &mut residue);
    for i in 0..30 {
        let request = if i % 2 == 0 { UiRequest::PowerOff } else { UiRequest::PowerPrepare };
        assert_eq!(route.request(request, &mut transport, &mut timing, &mut menu, &mut residue), Ok(RequestOutcome::Queued));
    }
    assert_eq!(route.request(UiRequest::PowerPrepare, &mut transport, &mut timing, &mut menu, &mut residue), Err(RouteError::SuspendedTableFull));
    assert!(transport.messages.is_empty());
    assert_eq!(route.pending_count(), 30);
}

#[test]
fn idle_resume_drains_then_posts_boundary_and_menu_without_clearing_count() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut menu, mut residue) = (Transport::default(), Timing::default(), menu(), residue());
    route.initialize(true, &mut transport, &mut timing, &mut residue);
    route.request(UiRequest::PowerOff, &mut transport, &mut timing, &mut menu, &mut residue).unwrap();
    assert_eq!(route.request(UiRequest::PowerOff, &mut transport, &mut timing, &mut menu, &mut residue), Ok(RequestOutcome::Duplicate));
    route.resume(false, &mut transport, &mut timing, &mut menu, &mut residue).unwrap();
    assert_eq!(transport.messages.iter().map(UiPacket::code).collect::<Vec<_>>(), [UiCode(0x1000), UiCode(0x500e), UiCode(0xb000), UiCode(0x4002)]);
    assert_eq!(menu.selected, [2]);
    assert_eq!(route.pending_count(), 1);
    assert!(!route.suspended());
}

#[test]
fn sequence_acknowledgment_controls_repeated_advance() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut residue) = (Transport::default(), Timing::default(), residue());
    assert_eq!(route.post(UiCode(0x4002), SequenceMode::Advance, &mut transport, &mut timing, &mut residue), RequestOutcome::Posted);
    assert_eq!(route.post(UiCode(0x4002), SequenceMode::Advance, &mut transport, &mut timing, &mut residue), RequestOutcome::Suppressed);
    let mut consumer = PowerConsumer { initialized: true, blocked: false, block_on_delivery: false };
    route.receive(&mut transport, &mut consumer, &mut Events::default(), &mut residue).unwrap();
    assert_eq!(route.expected_sequence(), Sequence(1));
    assert_eq!(route.post(UiCode(0x4002), SequenceMode::Advance, &mut transport, &mut timing, &mut residue), RequestOutcome::Posted);
    assert_eq!(transport.messages.back().unwrap().sequence(), Sequence(2));
}

#[test]
fn retries_only_pool_statuses_and_delays_after_last_retry() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut residue) = (Transport::default(), Timing::default(), residue());
    transport.statuses = [KernelStatus(-62); 10].into();
    route.post(UiCode(0x500e), SequenceMode::Unsequenced, &mut transport, &mut timing, &mut residue);
    assert_eq!(transport.attempts, 10);
    assert_eq!(timing.delays, [100; 10]);
    assert!(transport.messages.is_empty());
    transport.statuses.push_back(KernelStatus(-50));
    route.post(UiCode(0x500e), SequenceMode::Unsequenced, &mut transport, &mut timing, &mut residue);
    assert_eq!(transport.attempts, 11);
    assert_eq!(timing.delays.len(), 10);
}

#[test]
fn sequence_wraps_to_one_and_frontend_guards_produce_no_message() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut menu, mut residue) = (Transport::default(), Timing::default(), menu(), residue());
    let gate = FrontendGate { disabled: true, transition_active: false, restrict_lower_family: false };
    assert_eq!(route.frontend_request(gate, UiRequest::PowerOff, &mut transport, &mut timing, &mut menu, &mut residue), Ok(RequestOutcome::Suppressed));
    assert!(transport.messages.is_empty());
    for index in 0..256 {
        route.post(UiCode(if index % 2 == 0 { 0x4002 } else { 0x4003 }), SequenceMode::Advance, &mut transport, &mut timing, &mut residue);
    }
    assert_eq!(transport.messages.back().unwrap().sequence(), Sequence(1));
    assert!(transport.messages.iter().all(|packet| packet.sequence().0 != 0));
}

#[test]
fn already_resumed_entry_posts_boundary_only() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut menu, mut residue) = (Transport::default(), Timing::default(), menu(), residue());
    route.resume(false, &mut transport, &mut timing, &mut menu, &mut residue).unwrap();
    assert_eq!(transport.messages.iter().map(UiPacket::code).collect::<Vec<_>>(), [UiCode(0xb000)]);
    assert!(menu.selected.is_empty());
}

#[test]
fn resume_refresh_preserves_unwritten_flags_and_reads_owned_properties() {
    use super::menu::{Field, Properties, PropertyId, SelectionContext, SelectionFlags};
    struct Values;
    impl Properties for Values { fn read(&mut self, property: PropertyId) -> u32 { if property == PropertyId(0x18a) { 1 } else { 2 } } }
    let mut flags = SelectionFlags([9; 30]);
    let context = SelectionContext { category: MenuMode(0), side: MenuMode(0), bank: MenuMode(8), scene: MenuMode(0), scene_submode: MenuMode(0), bf88: MenuMode(0) };
    flags.select(0x5c, context, &mut Values);
    assert_eq!(flags.0[Field::F994 as usize], 0);
    assert_eq!(flags.0[Field::F9a0 as usize], 1);
    assert_eq!(flags.0[Field::F990 as usize], 0);
    assert_eq!(flags.0[Field::F9e0 as usize], 9);
    assert_eq!(flags.0[Field::F9d0 as usize], 0);
    assert_eq!(flags.0[Field::F9d4 as usize], 1);
}

#[test]
fn resume_refresh_bank_five_keeps_code_1d_a4_disabled() {
    use super::menu::{Field, Properties, PropertyId, SelectionContext, SelectionFlags};
    struct Values;
    impl Properties for Values { fn read(&mut self, _: PropertyId) -> u32 { 0 } }
    let mut flags = SelectionFlags([9; 30]);
    let context = SelectionContext { category: MenuMode(0), side: MenuMode(0), bank: MenuMode(5), scene: MenuMode(0), scene_submode: MenuMode(0), bf88: MenuMode(0) };
    flags.select(0x1d, context, &mut Values);
    assert_eq!(flags.0[Field::F9a4 as usize], 0);
    assert_eq!(flags.0[Field::F9d4 as usize], 0);
    assert_eq!(flags.0[Field::F9b8 as usize], 1);
    assert_eq!(flags.0[Field::F9bc as usize], 1);
}

#[test]
fn busy_resume_requeues_requests_and_still_posts_boundary_at_capacity() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut menu, mut residue) = (Transport::default(), Timing::default(), menu(), residue());
    route.initialize(true, &mut transport, &mut timing, &mut residue);
    for request in [UiRequest::PowerOff, UiRequest::PowerPrepare] {
        route.request(request, &mut transport, &mut timing, &mut menu, &mut residue).unwrap();
    }
    assert_eq!(route.resume(true, &mut transport, &mut timing, &mut menu, &mut residue), Err(RouteError::SuspendedTableFull));
    assert_eq!(route.pending_count(), 30);
    assert!(route.suspended());
    assert_eq!(transport.messages.iter().map(UiPacket::code).collect::<Vec<_>>(), [UiCode(0xb000)]);
}

#[test]
fn power_family_completion_is_not_discarded_for_an_old_sequence() {
    let mut route = UiCompletion::new();
    let (mut transport, mut timing, mut residue) = (Transport::default(), Timing::default(), residue());
    route.post(UiCode(0x4002), SequenceMode::Advance, &mut transport, &mut timing, &mut residue);
    let mut packet = UiPacket([0; 12]);
    packet.0[..2].copy_from_slice(&0x500eu16.to_le_bytes());
    packet.0[2] = 9;
    transport.messages.clear();
    transport.messages.push_back(packet);
    let mut consumer = PowerConsumer { initialized: true, blocked: false, block_on_delivery: false };
    let mut events = Events::default();
    assert_eq!(route.receive(&mut transport, &mut consumer, &mut events, &mut residue), Ok(Delivery::PowerOff { events: 2 }));
    assert_eq!(route.expected_sequence(), Sequence(9));
    assert_eq!(events.0.len(), 2);
}
