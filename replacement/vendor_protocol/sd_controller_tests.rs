use super::*;
use std::{collections::VecDeque, vec::Vec};

struct Registers {
    halves: [u16; 32],
    words: [u32; 10],
    response: [u16; 8],
    received: VecDeque<u32>,
    sent: Vec<u32>,
}
impl Registers {
    fn new() -> Self {
        Self { halves: [0; 32], words: [0; 10], response: [0; 8],
            received: VecDeque::new(), sent: Vec::new() }
    }
    fn status(&mut self, value: u16) { self.halves[HalfRegister::Status as usize / 2] = value; }
}
impl Mmio for Registers {
    fn read_half(&mut self, register: HalfRegister) -> u16 { self.halves[register as usize / 2] }
    fn write_half(&mut self, register: HalfRegister, value: u16) {
        let slot = &mut self.halves[register as usize / 2];
        if matches!(register, HalfRegister::Status | HalfRegister::Errors) { *slot &= !value; }
        else { *slot = value; }
    }
    fn read_word(&mut self, register: WordRegister) -> u32 {
        if register == WordRegister::Fifo { self.received.pop_front().unwrap() }
        else { self.words[register as usize / 4] }
    }
    fn write_word(&mut self, register: WordRegister, value: u32) {
        if register == WordRegister::Fifo { self.sent.push(value); }
        else { self.words[register as usize / 4] = value; }
    }
    fn response_half(&mut self, index: u8) -> u16 { self.response[index as usize] }
}
struct Cache;
impl CacheInstructions for Cache {
    fn level_id(&mut self) -> u32 { 0 }
    fn select_level(&mut self, _: u32) { unreachable!() }
    fn instruction_barrier(&mut self) { unreachable!() }
    fn size_id(&mut self) -> u32 { unreachable!() }
    fn clean_invalidate_set_way(&mut self, _: u32) { unreachable!() }
    fn clean_invalidate_address(&mut self, _: u32) { unreachable!() }
}
fn host() -> Host { Host::new(TransferUnit::new(8).unwrap()) }
fn direct() -> Request<'static> {
    Request { operation: Operation::Direct52, argument: Argument::from_wire(0x1000_0004),
        execution: Execution::Polling, buffer: Buffer::None }
}
#[test]
fn direct_response_reaches_the_caller() {
    let mut host = host();
    let mut registers = Registers::new();
    registers.response[0] = 0x24;
    let mut transaction = host.begin(&mut registers, &mut Cache, direct(), Milliseconds::new(0)).unwrap();
    assert_eq!(transaction.poll(Milliseconds::new(0)), Poll::Pending);
    transaction.mmio.status(1);
    assert_eq!(transaction.poll(Milliseconds::new(1)), Poll::Ready(Completion {
        code: VendorCode::Success, physical: PhysicalResult::Completed, transferred: 0,
        response: 0x24, status: 0,
    }));
}
#[test]
fn timeout_is_not_a_physical_success() {
    let mut host = host();
    let mut registers = Registers::new();
    let mut transaction = host.begin(&mut registers, &mut Cache, direct(), Milliseconds::new(0)).unwrap();
    assert_eq!(transaction.poll(Milliseconds::new(0)), Poll::Pending);
    let Poll::Ready(completion) = transaction.poll(Milliseconds::new(500)) else { panic!() };
    assert_eq!(completion.code, VendorCode::Timeout);
    assert_eq!(completion.physical, PhysicalResult::TimedOut);
}
#[test]
fn response_errors_are_reported_as_result_status() {
    let mut host = host();
    let mut registers = Registers::new();
    registers.response[0] = 0xc900;
    let mut transaction = host.begin(&mut registers, &mut Cache, direct(), Milliseconds::new(0)).unwrap();
    assert_eq!(transaction.poll(Milliseconds::new(0)), Poll::Pending);
    transaction.mmio.status(1);
    let Poll::Ready(completion) = transaction.poll(Milliseconds::new(1)) else { panic!() };
    assert_eq!(completion.code, VendorCode::CommandError);
    assert_eq!(completion.status, 0x80c8_0000);
    assert_eq!(completion.physical, PhysicalResult::Error);
}
#[test]
fn fifo_read_drains_the_entire_unit_but_returns_only_requested_bytes() {
    let mut host = host();
    let mut registers = Registers::new();
    registers.received.extend([0x0403_0201, 0x0807_0605]);
    registers.words[WordRegister::Present as usize / 4] = 0x800;
    let mut bytes = [0; 3];
    let request = Request { operation: Operation::Extended53, argument: Argument::from_wire(3),
        execution: Execution::Polling, buffer: Buffer::Fifo(&mut bytes) };
    let mut transaction = host.begin(&mut registers, &mut Cache, request, Milliseconds::new(0)).unwrap();
    assert_eq!(transaction.poll(Milliseconds::new(0)), Poll::Pending);
    transaction.mmio.status(1);
    assert_eq!(transaction.poll(Milliseconds::new(1)), Poll::Pending);
    assert_eq!(transaction.poll(Milliseconds::new(2)), Poll::Pending);
    transaction.mmio.status(2);
    let Poll::Ready(completion) = transaction.poll(Milliseconds::new(3)) else { panic!() };
    assert_eq!(completion.code, VendorCode::Success);
    assert_eq!(transaction.buffer(), &[1, 2, 3]);
    assert!(transaction.mmio.received.is_empty());
}
#[test]
fn fifo_write_preserves_previous_word_upper_bytes_in_short_unit_tail() {
    let mut host = host();
    let mut registers = Registers::new();
    registers.words[WordRegister::Present as usize / 4] = 0x400;
    let mut bytes = [1, 2, 3, 4, 5];
    let request = Request { operation: Operation::Extended53, argument: Argument::from_wire(0x8000_0005),
        execution: Execution::Polling, buffer: Buffer::Fifo(&mut bytes) };
    let mut transaction = host.begin(&mut registers, &mut Cache, request, Milliseconds::new(0)).unwrap();
    assert_eq!(transaction.poll(Milliseconds::new(0)), Poll::Pending);
    transaction.mmio.status(1);
    assert_eq!(transaction.poll(Milliseconds::new(1)), Poll::Pending);
    assert_eq!(transaction.poll(Milliseconds::new(2)), Poll::Pending);
    transaction.mmio.status(2);
    assert!(matches!(transaction.poll(Milliseconds::new(3)), Poll::Ready(Completion { code: VendorCode::Success, .. })));
    assert_eq!(transaction.mmio.sent, [0x0403_0201, 0x0403_0205]);
}
#[test]
fn cancelled_transaction_does_not_consume_a_late_completion_as_success() {
    let mut host = host();
    let mut registers = Registers::new();
    let mut transaction = host.begin(&mut registers, &mut Cache, direct(), Milliseconds::new(0)).unwrap();
    assert_eq!(transaction.poll(Milliseconds::new(0)), Poll::Pending);
    transaction.cancel();
    transaction.mmio.status(1);
    transaction.interrupt();
    let Poll::Ready(completion) = transaction.poll(Milliseconds::new(1)) else { panic!() };
    assert_eq!(completion.physical, PhysicalResult::Cancelled);
}

struct Power { gate: bool, high: bool, registered: bool }
impl ClockPower for Power {
    fn unregister(&mut self, _: ClockRegistration) -> bool { self.registered = false; true }
    fn set_gate(&mut self, enabled: bool) { self.gate = enabled; }
    fn register(&mut self) -> Option<ClockRegistration> { self.registered = true; Some(ClockRegistration(1)) }
    fn select_source(&mut self, high: bool) { self.high = high; }
}
#[test]
fn clock_waits_for_hardware_stability_then_enables_output() {
    let mut control = ControlPlane::new();
    let mut registers = Registers::new();
    let mut power = Power { gate: false, high: false, registered: false };
    assert_eq!(control.start_clock(&mut registers, &mut power, ClockSelection::Mode4), Ok(ClockProgress::WaitingForStable));
    assert!(power.gate && power.high && power.registered);
    registers.halves[HalfRegister::Clock as usize / 2] |= 2;
    assert_eq!(control.poll_clock(&mut registers), ClockProgress::Complete);
    assert_eq!(registers.read_half(HalfRegister::Clock), 0x2007);
    assert_eq!(control.start_clock(&mut registers, &mut power, ClockSelection::Stopped), Ok(ClockProgress::Complete));
    assert!(!power.gate && !power.registered);
    assert_eq!(registers.read_half(HalfRegister::Clock), 1);
}
#[test]
fn control_updates_transfer_unit_and_mode_without_changing_other_register_bits() {
    let mut control = ControlPlane::new();
    let mut registers = Registers::new();
    let mut host = host();
    registers.write_half(HalfRegister::Length, 0xa008);
    registers.write_half(HalfRegister::Config, 0x4400);
    control.set_transfer_unit(&mut host, &mut registers, TransferUnit::BLOCK_512).unwrap();
    control.set_transfer_enabled(&mut host, &mut registers, true);
    assert_eq!(registers.read_half(HalfRegister::Length), 0xa200);
    assert_eq!(registers.read_half(HalfRegister::Config), 0x4402);
}
struct Lock { locked: bool }
impl OperationLock for Lock {
    fn acquire(&mut self) -> Result<(), ControlError> { assert!(!self.locked); self.locked = true; Ok(()) }
    fn release(&mut self) -> Result<(), ControlError> { assert!(self.locked); self.locked = false; Ok(()) }
}
#[test]
fn rejected_admission_releases_serialization() {
    let mut control = ControlPlane::new(); control.initialize();
    let mut registers = Registers::new();
    let mut host = host();
    let mut lock = Lock { locked: false };
    assert!(matches!(control.begin(DevicePort::Zero, &mut host, &mut registers, &mut Cache, &mut lock, direct(), Milliseconds::new(0)),
        Err(BeginError::Control(ControlError::Unavailable))));
    assert!(!lock.locked);
}
#[test]
fn dropping_admitted_transaction_releases_serialization_after_cancellation() {
    let mut control = ControlPlane::new(); control.initialize(); control.publish_available(true); control.publish_connected(true);
    let mut registers = Registers::new();
    let mut host = host();
    let mut lock = Lock { locked: false };
    let transfer = control.begin(DevicePort::Zero, &mut host, &mut registers, &mut Cache, &mut lock, direct(), Milliseconds::new(0)).unwrap();
    drop(transfer);
    assert!(!lock.locked);
    assert_eq!(registers.read_half(HalfRegister::StatusSignal), 0);
}

#[test]
fn card_notifications_are_delivered_without_an_active_transfer() {
    let mut host = host();
    let mut registers = Registers::new();
    host.set_notifications(true, true, true);
    registers.status(0x1c0);
    assert_eq!(host.interrupt(&mut registers), Notifications { inserted: true, removed: true, card_interrupt: true });
    assert_eq!(registers.read_half(HalfRegister::Status) & 0xc0, 0);
    assert_eq!(registers.read_half(HalfRegister::StatusMask) & 0x1c0, 0x1c0);
}
#[test]
fn cancelled_dma_drain_clears_late_status_before_another_command() {
    let mut host = host();
    let mut registers = Registers::new();
    let transfer = host.begin(&mut registers, &mut Cache, direct(), Milliseconds::new(0)).unwrap();
    drop(transfer);
    registers.status(1);
    registers.words[WordRegister::Present as usize / 4] = 1;
    assert_eq!(host.reap_cancelled(&mut registers), Poll::Pending);
    registers.words[WordRegister::Present as usize / 4] = 0;
    assert!(matches!(host.reap_cancelled(&mut registers), Poll::Ready(Completion { physical: PhysicalResult::Cancelled, .. })));
    let mut next = host.begin(&mut registers, &mut Cache, direct(), Milliseconds::new(1)).unwrap();
    assert_eq!(next.poll(Milliseconds::new(1)), Poll::Pending);
    assert_eq!(next.poll(Milliseconds::new(2)), Poll::Pending);
}
