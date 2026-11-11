use super::*;
use crate::controller::RequestKind;
use crate::controller_registration::{ControllerRegistrations, PollPeriod, TimerEvent, TimerRegistry};
use crate::transport_connection_cpu::{ConnectionNotification, ConnectionState};
use xm1_power_services::{device_timer_release::TimerPeriod, shutdown::TaskId,
    shutdown_task7_8::RegistryHandle, shutdown_task7_8_transport::RegistrationRegistry};

#[derive(Debug, Eq, PartialEq)]
enum Effect { Message(RequestId, QueueId, Vec<u8>), Priority(TaskId, u32), Release(RegistryHandle) }
struct Queue { effects: Vec<Effect>, failure: Option<KernelStatus> }
impl Queue { fn new() -> Self { Self { effects: Vec::new(), failure: None } } }
impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(9) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        self.effects.push(Effect::Message(message.operation, message.destination, message.payload.to_vec()));
        self.failure.map_or(Ok(()), Err)
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}
impl ControllerRtos for Queue {
    fn set_task_priority(&mut self, task: TaskId, priority: u32) -> Result<(), KernelStatus> {
        self.effects.push(Effect::Priority(task, priority)); Ok(())
    }
}

#[test]
fn queue36_preserves_the_fixed_payload_and_ignores_foreign_opcodes() {
    let mut payload = [0; 24];
    payload[0..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&0x12345678u32.to_le_bytes());
    payload[23] = 0x9a;
    let message = Queue36Message::parse(0x3408, &payload).unwrap().unwrap();
    let mut queue = Queue::new();
    message.publish(&mut queue).unwrap();
    assert_eq!(queue.effects, vec![Effect::Message(RequestId(0x3408), QueueId(36), payload.to_vec())]);
    assert_eq!(Queue36Message::parse(0x3500, &[]), Ok(None));
    assert_eq!(Queue36Message::parse(0x3408, &[0; 23]), Err(CompletionError::Payload));
}

#[test]
fn vendor_class_events_and_timer_events_have_distinct_routes() {
    assert_eq!(Queue36Event::parse(0x3410), Some(Queue36Event::ShortTransfer));
    assert_eq!(Queue36Event::parse(0x3411), Some(Queue36Event::LongTransfer));
    assert_eq!(Queue36Event::parse(0x3412), Some(Queue36Event::SubmitPrint));
    assert_eq!(operation(Queue36Event::SubmitPrint, ControllerState::PendingVendor), ControllerOperation::Defer);
    assert_eq!(operation(Queue36Event::SubmitPrint, ControllerState::ActiveVendor), ControllerOperation::SubmitPrint);
    assert_eq!(operation(Queue36Event::LongTransfer, ControllerState::PendingVendor), ControllerOperation::Ignore);
    assert_eq!(operation(Queue36Event::Operation, ControllerState::State4), ControllerOperation::OperationRejected(ServiceStatus(-14)));
}

#[test]
fn a_disconnect_during_the_file_phase_aborts_the_files_and_enters_state_six() {
    assert_eq!(operation(Queue36Event::Disconnected, ControllerState::State4), ControllerOperation::CloseProtocol);
    assert_eq!(table_next_state(Queue36Event::Disconnected, ControllerState::State4), Some(ControllerState::State6));
    assert_eq!(operation(Queue36Event::QueryStatus, ControllerState::State4), ControllerOperation::CloseProtocol);
    assert_eq!(operation(Queue36Event::ClassError, ControllerState::State6), ControllerOperation::ReportDisconnected);
}

#[test]
fn startup_completion_and_request_errors_reach_their_installed_consumers() {
    let consumers = ControllerConsumers { phase: InstalledConsumer::Task7, closed: InstalledConsumer::Task7,
        completion: InstalledConsumer::Task7, request: InstalledConsumer::Task7, status: InstalledConsumer::Task7 };
    let mut queue = Queue::new();
    for consumer in [StatusConsumer::Phase, StatusConsumer::Completion, StatusConsumer::Request, StatusConsumer::Status, StatusConsumer::Closed] {
        publish_status(consumer, consumers, CompletionHandle(23), RequestHandle(3), ServiceStatus(-13), &mut queue).unwrap();
    }
    assert_eq!(queue.effects, vec![
        Effect::Message(RequestId(0xb400), QueueId(7), (-13i32).to_le_bytes().to_vec()),
        Effect::Message(RequestId(0x1714), QueueId(7), vec![23, 0, 0, 0, 243, 255, 255, 255]),
        Effect::Message(RequestId(0xb403), QueueId(7), vec![3, 0, 0, 0, 243, 255, 255, 255]),
        Effect::Message(RequestId(0xb404), QueueId(7), (-13i32).to_le_bytes().to_vec()),
        Effect::Message(RequestId(0xb401), QueueId(7), vec![]),
    ]);
    let mut absent = Queue::new();
    publish_status(StatusConsumer::Closed, ControllerConsumers::new(), CompletionHandle(23), RequestHandle(3), ServiceStatus(0), &mut absent).unwrap();
    assert!(absent.effects.is_empty());
}

#[test]
fn authentication_snapshot_preserves_both_status_queries() {
    assert_eq!(ConnectionSnapshot::from_status(0, 1).completion_handle(), CompletionHandle(0));
    assert_eq!(ConnectionSnapshot::from_status(2, 0).completion_handle(), CompletionHandle(0));
    assert_eq!(ConnectionSnapshot::from_status(1, 0).completion_handle(), CompletionHandle(21));
    assert_eq!(ConnectionSnapshot::from_status(1, -1).completion_handle(), CompletionHandle(23));
}

#[test]
fn close_is_deferred_during_file_close_and_ignored_after_it_is_requested() {
    assert_eq!(operation(Queue36Event::Shutdown, ControllerState::State5), ControllerOperation::Defer);
    assert_eq!(operation(Queue36Event::Shutdown, ControllerState::State7), ControllerOperation::Ignore);
    assert_eq!(table_next_state(Queue36Event::Shutdown, ControllerState::State5), None);
    assert_eq!(table_next_state(Queue36Event::Shutdown, ControllerState::State4), Some(ControllerState::State7));
    assert_eq!(table_next_state(Queue36Event::Shutdown, ControllerState::State8), Some(ControllerState::Stopped));
}

#[test]
fn deferred_print_and_shutdown_messages_are_delivered_with_payloads_after_unblocking() {
    let mut deferred = DeferredMessages::new();
    let mut payload = [0; 24]; payload[0] = 7;
    let print = Queue36Message { event: Queue36Event::SubmitPrint, payload: Queue36Payload::parse(&payload).unwrap() };
    let close = Queue36Message { event: Queue36Event::Shutdown, payload: Queue36Payload::EMPTY };
    deferred.defer(print); deferred.defer(close);
    assert_eq!(deferred.next(), None);
    deferred.set_blocked(false);
    assert_eq!(deferred.next(), Some(print));
    assert_eq!(deferred.next(), Some(close));
    assert_eq!(deferred.next(), None);
}

struct TimerBus { next: u32, started: Vec<(TimerPeriod, TimerEvent)>, released: Vec<RegistryHandle>, failure: Option<ServiceStatus> }
impl TimerBus { fn new() -> Self { Self { next: 0, started: Vec::new(), released: Vec::new(), failure: None } } }
impl RegistrationRegistry for TimerBus {
    fn release(&mut self, handle: RegistryHandle) -> ServiceStatus { self.released.push(handle); ServiceStatus(0) }
}
impl TimerRegistry for TimerBus {
    fn start_periodic(&mut self, period: TimerPeriod, event: TimerEvent) -> Result<RegistryHandle, ServiceStatus> {
        if let Some(status) = self.failure { return Err(status); }
        self.started.push((period, event)); self.next += 1; Ok(RegistryHandle(self.next))
    }
}

#[test]
fn connection_poll_coalesces_until_consumed_and_wakes_even_when_post_fails() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut queue = Queue::new(); queue.failure = Some(KernelStatus(-50));
    assert_eq!(registrations.fire(TimerEvent::ConnectionPoll, &mut queue), Ok(()));
    registrations.fire(TimerEvent::ConnectionPoll, &mut queue).unwrap();
    assert_eq!(queue.effects, vec![Effect::Message(RequestId(0x340d), QueueId(36), vec![0; 24]), Effect::Priority(TaskId(36), 8)]);
    registrations.acknowledge_primary_poll();
    queue.failure = None;
    registrations.fire(TimerEvent::ConnectionPoll, &mut queue).unwrap();
    assert_eq!(queue.effects.len(), 4);
}

#[test]
fn timer_lifetime_uses_one_registry_for_polling_and_transfer_timeouts() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut registry = TimerBus::new();
    registrations.start_primary(PollPeriod::Connection, &mut registry).unwrap();
    registrations.stop_primary(&mut registry);
    registrations.start_primary(PollPeriod::Vendor, &mut registry).unwrap();
    let mut connection = ConnectionState::new();
    registrations.start_secondary(RequestKind::Service, &mut connection, &mut registry).unwrap();
    registrations.start_secondary(RequestKind::FileOnly, &mut connection, &mut registry).unwrap();
    registrations.start_secondary(RequestKind::Protocol, &mut connection, &mut registry).unwrap();
    registrations.start_short(&mut registry).unwrap();
    registrations.start_long(&mut registry).unwrap();
    assert_eq!(registry.started, vec![(TimerPeriod(25), TimerEvent::ConnectionPoll), (TimerPeriod(500), TimerEvent::ConnectionPoll),
        (TimerPeriod(50), TimerEvent::SessionPoll), (TimerPeriod(250), TimerEvent::ShortTransfer), (TimerPeriod(60_000), TimerEvent::LongTransfer)]);
    registrations.stop_short(&mut registry); registrations.stop_long(&mut registry);
    registrations.stop_secondary(&mut registry); registrations.stop_primary(&mut registry);
    assert_eq!(registry.released, vec![RegistryHandle(1), RegistryHandle(4), RegistryHandle(5), RegistryHandle(3), RegistryHandle(2)]);
    assert_eq!(registrations.primary(), None); assert_eq!(registrations.secondary(), None);
    assert_eq!(registrations.short(), None); assert_eq!(registrations.long(), None);
}

#[test]
fn connection_debounce_restarts_after_a_matching_sample() {
    let mut polling = ConnectionPolling::new();
    for _ in 0..15 {
        assert_eq!(polling.poll(ControllerState::State2, CompletionHandle(0), ConnectionLevel::Connected), ConnectionPollEffect::None);
    }
    assert_eq!(polling.poll(ControllerState::State2, CompletionHandle(0), ConnectionLevel::Disconnected), ConnectionPollEffect::None);
    for _ in 0..15 {
        assert_eq!(polling.poll(ControllerState::State2, CompletionHandle(0), ConnectionLevel::Connected), ConnectionPollEffect::None);
    }
    assert_eq!(polling.poll(ControllerState::State2, CompletionHandle(0), ConnectionLevel::Connected), ConnectionPollEffect::Change(ConnectionLevel::Connected));
}

#[test]
fn pending_vendor_resets_the_twenty_poll_completion_flag_timeout() {
    let mut polling = ConnectionPolling::new();
    for _ in 0..19 {
        assert_eq!(polling.poll(ControllerState::ActiveVendor, CompletionHandle(21), ConnectionLevel::Connected), ConnectionPollEffect::None);
    }
    assert_eq!(polling.poll(ControllerState::PendingVendor, CompletionHandle(21), ConnectionLevel::Connected), ConnectionPollEffect::None);
    for _ in 0..19 {
        assert_eq!(polling.poll(ControllerState::ActiveVendor, CompletionHandle(21), ConnectionLevel::Connected), ConnectionPollEffect::None);
    }
    assert_eq!(polling.poll(ControllerState::ActiveVendor, CompletionHandle(21), ConnectionLevel::Connected), ConnectionPollEffect::ClearFlag16);
}

struct OrderedRegistry<'a>(&'a core::cell::RefCell<Queue>);
impl RegistrationRegistry for OrderedRegistry<'_> {
    fn release(&mut self, handle: RegistryHandle) -> ServiceStatus {
        self.0.borrow_mut().effects.push(Effect::Release(handle));
        ServiceStatus(0)
    }
}

#[test]
fn secondary_notification_precedes_release_on_the_shared_rtos_engine() {
    use xm1_power_services::shutdown_task7_8_transport::unregister_secondary;
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut connection = ConnectionState::new();
    registrations.start_secondary(RequestKind::Protocol, &mut connection, &mut TimerBus::new()).unwrap();
    connection.update_connection(true, &mut registrations);
    let mut deferred = DeferredMessages::new();
    let queue = core::cell::RefCell::new(Queue::new());
    {
        let mut parts = registrations.shutdown_parts(RequestKind::Protocol, &mut connection, &mut deferred, &queue);
        assert_eq!(unregister_secondary(&mut parts.storage, &mut OrderedRegistry(&queue)), ServiceStatus(0));
    }
    assert_eq!(queue.borrow().effects, vec![
        Effect::Message(RequestId(0x171a), QueueId(7), (-11i32).to_le_bytes().to_vec()),
        Effect::Release(RegistryHandle(1)),
    ]);
    assert!(!connection.secondary_active());
    assert_eq!(registrations.secondary(), None);
}

#[test]
fn each_session_poll_reopens_its_coalesced_producer_and_a_restart_forgets_the_connection() {
    let producers = TimerProducers::new();
    let mut registrations = ControllerRegistrations::new(&producers);
    let mut connection = ConnectionState::new();
    let mut queue = Queue::new();
    registrations.fire(TimerEvent::SessionPoll, &mut queue).unwrap();
    registrations.fire(TimerEvent::SessionPoll, &mut queue).unwrap();
    assert_eq!(queue.effects, vec![Effect::Message(RequestId(0x340f), QueueId(36), vec![0; 24])]);
    assert_eq!(connection.update_connection(true, &mut registrations), Some(ConnectionNotification::Connected));
    registrations.fire(TimerEvent::SessionPoll, &mut queue).unwrap();
    assert_eq!(queue.effects.len(), 2);
    registrations.start_secondary(RequestKind::Protocol, &mut connection, &mut TimerBus::new()).unwrap();
    assert!(!connection.secondary_active());
    registrations.fire(TimerEvent::SessionPoll, &mut queue).unwrap();
    assert_eq!(queue.effects.len(), 3);
}

const STOCK_TABLE: &str = include_str!("controller_runtime_table.fixture");

struct StockCell { event: Queue36Event, state: ControllerState, tag: u8, handler: u32, next: u32 }

fn stock_cells() -> Vec<StockCell> {
    let mut cells = Vec::new();
    let mut index = 0;
    for line in STOCK_TABLE.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let bytes: Vec<u8> = line.split_whitespace().map(|byte| u8::from_str_radix(byte, 16).unwrap()).collect();
        assert_eq!(bytes.len(), 12, "malformed row {line}");
        let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        cells.push(StockCell {
            event: Queue36Event::ALL[index / 12],
            state: ControllerState::parse((index % 12) as u32).unwrap(),
            tag: bytes[0],
            handler: word(4),
            next: word(8),
        });
        index += 1;
    }
    cells
}

#[test]
fn every_stock_cell_matches_the_operation_class_tag_handler_and_next_phase() {
    let cells = stock_cells();
    assert_eq!(cells.len(), Queue36Event::ALL.len() * 12);
    let mut handlers = std::collections::BTreeSet::new();
    for cell in &cells {
        let cell_name = format!("{:?}/{:?}", cell.event, cell.state);
        assert!(matches!(cell.tag, 0 | 3), "{cell_name}: tag {}", cell.tag);
        assert_eq!(cell.tag == 0, cell.handler == 0, "{cell_name}");
        assert_eq!(cell.tag == 3, cell.handler != 0, "{cell_name}");
        assert_eq!(cell.handler & 3, 0, "{cell_name}");
        assert_eq!(operation(cell.event, cell.state) == ControllerOperation::Ignore, cell.handler == 0, "{cell_name}");
        assert_eq!(table_next_state(cell.event, cell.state), ControllerState::parse(cell.next).ok(), "{cell_name}");
        if cell.handler != 0 { assert!(handlers.insert(cell.handler), "duplicate handler {:08x}", cell.handler); }
    }
    assert_eq!(handlers.len(), 119);
}
