extern crate std;

use super::{ApplicationDispatcher, ApplicationGate, ApplicationMarker, BufferedCount, EarlyApplicationBuffer, InstalledCallback, SequenceNumber};
use crate::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId, UiRouting};
use std::vec::Vec;

struct Queue {
    messages: Vec<(u16, u8, Vec<u8>)>,
    failure: Option<KernelStatus>,
}

impl Queue {
    fn new() -> Self { Self { messages: Vec::new(), failure: None } }
}

impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(17) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if let Some(status) = self.failure.take() { return Err(status); }
        self.messages.push((message.operation.0, message.destination.0, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}

fn record(event: u32) -> [u8; 76] {
    let mut result = [0xa5; 76];
    result[..4].copy_from_slice(&event.to_le_bytes());
    result[4..8].copy_from_slice(&0x11223344u32.to_le_bytes());
    result
}

fn dispatcher(routing: UiRouting) -> ApplicationDispatcher {
    ApplicationDispatcher {
        callback: InstalledCallback::Default,
        routing,
        first_request_task: TaskId(0),
        gate: ApplicationGate { issued: SequenceNumber(0), consumed: SequenceNumber(0) },
    }
}

#[test]
fn buffered_records_preserve_unwritten_slot_bytes() {
    let mut slots = [[0x5a; 76]; 64];
    let mut count = BufferedCount(0);
    let input = record(0x502);
    let mut buffer = EarlyApplicationBuffer { slots: &mut slots, count: &mut count };
    assert_eq!(buffer.dispatch(&input, |_| unreachable!()), Ok(()));
    assert_eq!(buffer.count.0, 1);
    assert_eq!(&buffer.slots[0][..8], &input[..8]);
    assert_eq!(&buffer.slots[0][8..], &[0x5a; 68]);
    assert_eq!(buffer.slots[1], [0x5a; 76]);
}

#[test]
fn full_early_buffer_reports_error_without_overwriting_records() {
    let mut slots = [[0x5a; 76]; 64];
    let mut count = BufferedCount(64);
    let mut marker = None;
    let mut buffer = EarlyApplicationBuffer { slots: &mut slots, count: &mut count };
    assert_eq!(buffer.dispatch(&record(0x502), |value| marker = Some(value)), Err(KernelStatus(-63)));
    assert_eq!(marker, Some(ApplicationMarker::EarlyBufferFull));
    assert_eq!(buffer.count.0, 64);
    assert_eq!(buffer.slots, &[[0x5a; 76]; 64]);
}

#[test]
fn active_dispatch_routes_and_copies_the_complete_record() {
    let mut queue = Queue::new();
    let mut application = dispatcher(UiRouting::Primary);
    let input = record(0x502);
    assert_eq!(application.dispatch(&mut queue, &input, |_| unreachable!()), Ok(()));
    assert_eq!(queue.messages, [(0, 7, input.to_vec())]);
    assert_eq!(application.first_request_task, TaskId(0));
}

#[test]
fn diagnostic_first_task_is_saved_after_queue_failure() {
    let mut queue = Queue::new();
    queue.failure = Some(KernelStatus(-50));
    let mut application = dispatcher(UiRouting::Other);
    assert_eq!(application.dispatch(&mut queue, &record(0x91b), |_| unreachable!()), Err(KernelStatus(-50)));
    assert!(queue.messages.is_empty());
    assert_eq!(application.first_request_task, TaskId(17));
}

#[test]
fn occupied_gate_reports_without_publishing_or_advancing_sequence() {
    let mut queue = Queue::new();
    let mut application = dispatcher(UiRouting::Other);
    application.gate.issued = SequenceNumber(1);
    let mut marker = None;
    assert_eq!(application.dispatch(&mut queue, &record(0x82e), |value| marker = Some(value)), Ok(()));
    assert_eq!(marker, Some(ApplicationMarker::GateOccupied));
    assert!(queue.messages.is_empty());
    assert_eq!(application.gate.issued, SequenceNumber(1));
}
