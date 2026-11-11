use super::*;
use scheduler::{InterruptState, Machine, SwitchReason, TimerEntry};
use scheduler::layout::{Domain, GuestAddress, KernelContext, LockRecord, Task};
use std::{cell::{Cell, RefCell}, collections::BTreeMap, vec::Vec};

struct Clock {
    memory: RefCell<BTreeMap<u32, u32>>,
    timer: Cell<Option<(TimerEntry, u32)>>,
    now: Cell<u32>,
    interrupted: Cell<bool>,
    cancel_after: Cell<Option<u32>>,
}
impl Clock {
    fn new() -> Self {
        let clock = Self { memory: RefCell::new(BTreeMap::new()), timer: Cell::new(None), now: Cell::new(0), interrupted: Cell::new(false), cancel_after: Cell::new(None) };
        let words = Domain::A.words();
        let context = words.context(Domain::A);
        let task = Task(words.task_table);
        clock.store_word(words.current_context, context.0.0);
        clock.store_word(words.running, task.0.0);
        clock.store_word(words.dispatch_enabled, 1);
        clock.store_word(task.definition(), 0x40060000);
        clock.store_word(GuestAddress(0x40060020), 1);
        clock.store_byte(task.state(), 1);
        clock.store_word(context.tick_now(), 0x40061000);
        clock.store_word(GuestAddress(0x40061000), 0);
        clock
    }
}
impl Machine for Clock {
    fn load_word(&self, at: GuestAddress) -> u32 { *self.memory.borrow().get(&at.0).unwrap_or(&0) }
    fn store_word(&self, at: GuestAddress, value: u32) { self.memory.borrow_mut().insert(at.0, value); }
    fn load_byte(&self, at: GuestAddress) -> u8 { self.load_word(at) as u8 }
    fn store_byte(&self, at: GuestAddress, value: u8) { self.store_word(at, value as u32); }
    fn save_and_mask(&self) -> InterruptState { InterruptState(0) }
    fn restore(&self, _: InterruptState) {}
    fn masked(&self) -> bool { false }
    fn irq_enable(&self) {}
    fn irq_disable(&self) {}
    fn lock_acquire(&self, _: LockRecord) -> bool { true }
    fn lock_release(&self, _: LockRecord) {}
    fn remove_ready(&self, _: KernelContext, _: Task) -> bool { false }
    fn insert_ready(&self, _: KernelContext, task: Task) -> bool { self.store_byte(task.state(), 1); false }
    fn reset_task(&self, _: Task) { panic!("unexpected task reset") }
    fn restart_task(&self, _: KernelContext, _: Task) -> bool { panic!("unexpected task restart") }
    fn notify_dispatch(&self, _: KernelContext) -> bool { false }
    fn switch(&self, reason: SwitchReason) {
        assert_eq!(reason, SwitchReason::Delay);
        let (timer, expiry) = self.timer.take().expect("owned delay must arm a timer");
        let words = Domain::A.words();
        let task = Task(GuestAddress(self.load_word(timer.argument())));
        if self.interrupted.get() || self.cancel_after.get() == Some(self.now.get()) {
            self.store_word(GuestAddress(self.load_word(task.wait())), (-49i32) as u32);
            self.store_byte(task.state(), 1);
        } else {
            self.now.set(expiry);
            self.store_word(GuestAddress(0x40061000), expiry);
            scheduler::timeout::delay(self, Domain::A, task);
        }
        assert_eq!(self.load_byte(task.state()), 1);
        assert_eq!(self.load_word(words.running), task.0.0);
    }
    fn timer_insert(&self, _: KernelContext, entry: TimerEntry, expiry: u32) {
        assert!(self.timer.replace(Some((entry, expiry))).is_none());
    }
    fn timer_remove(&self, _: KernelContext, entry: TimerEntry) {
        assert_eq!(self.timer.take().map(|value| value.0), Some(entry));
    }
    fn timer_active(&self, _: KernelContext, entry: TimerEntry) -> bool { self.timer.get().map(|value| value.0) == Some(entry) }
}

struct Device {
    zero_status: u32,
    one_status: u32,
    reset: u16,
    enabled: u16,
    disconnect: u16,
    closed: Vec<Channel>,
    interrupts: Vec<Channel>,
    released: Vec<cpu::DriverHandle>,
}
impl Device {
    fn new() -> Self { Self { zero_status: 0, one_status: 0, reset: 0x80, enabled: 7, disconnect: 8, closed: Vec::new(), interrupts: Vec::new(), released: Vec::new() } }
}
impl ControllerIo for Device {
    fn read32(&mut self, register: Register) -> u32 {
        match register { Register::ChannelStatus(Channel::Zero) => self.zero_status, Register::ChannelStatus(Channel::One) => self.one_status, _ => 0 }
    }
    fn read16(&mut self, register: Register) -> u16 {
        match register { Register::ResetPulse => self.reset, Register::ControllerEnable => self.enabled, Register::ControllerDisconnect => self.disconnect, _ => 0 }
    }
    fn write32(&mut self, _: Register, _: u32) {}
    fn write16(&mut self, register: Register, value: u16) {
        match register { Register::ResetPulse => self.reset = value, Register::ControllerEnable => self.enabled = value, Register::ControllerDisconnect => self.disconnect = value, _ => {} }
    }
}
impl TeardownPlatform for Device {
    fn release_interrupt(&mut self, handle: cpu::DriverHandle) -> Result<(), TransportError> { self.released.push(handle); Ok(()) }
    fn release_channel_interrupt(&mut self, channel: Channel) -> Result<(), TransportError> { self.interrupts.push(channel); Ok(()) }
    fn release_channel(&mut self, channel: Channel, _: cpu::DriverHandle) -> Result<(), TransportError> { self.closed.push(channel); Ok(()) }
}
fn teardown() -> ControllerTeardown {
    let mut zero = DmaChannel::open(Channel::Zero);
    zero.state = ChannelState::Armed;
    zero.callback = Some(ChannelCallback::Receive);
    let mut one = DmaChannel::open(Channel::One);
    one.state = ChannelState::Armed;
    one.callback = Some(ChannelCallback::Transmit);
    ControllerTeardown::new(Some(cpu::DriverHandle::from_raw(3).unwrap()), [
        Some(ChannelLease::new(zero, cpu::DriverHandle::from_raw(1).unwrap())),
        Some(ChannelLease::new(one, cpu::DriverHandle::from_raw(2).unwrap())),
    ], false)
}

#[test]
fn teardown_waits_for_both_dma_channels_before_releasing_or_resetting() {
    let clock = Clock::new();
    let mut device = Device::new();
    device.zero_status = 1;
    let mut shutdown = teardown();
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Pending));
    assert!(device.closed.is_empty());
    assert_eq!(device.reset, 0x80);
    device.zero_status = 0;
    device.one_status = 7;
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Pending));
    assert!(device.closed.is_empty());
    assert_eq!(clock.now.get(), 0);
    device.one_status = 0;
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Complete));
    assert_eq!(device.closed, [Channel::Zero, Channel::One]);
    assert_eq!(device.reset, 0x80);
    assert_eq!(device.enabled, 0);
    assert_eq!(device.disconnect, 10);
    assert_eq!(clock.now.get(), 200);
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Complete));
    assert_eq!(clock.now.get(), 200);
    assert_eq!(device.closed.len(), 2);
}

#[test]
fn a_channel_that_was_never_opened_is_neither_stopped_nor_closed() {
    let clock = Clock::new();
    let mut device = Device::new();
    let mut zero = DmaChannel::open(Channel::Zero);
    zero.state = ChannelState::Armed;
    let mut shutdown = ControllerTeardown::new(None, [
        Some(ChannelLease::new(zero, cpu::DriverHandle::from_raw(1).unwrap())),
        None,
    ], true);
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Complete));
    assert_eq!(device.interrupts, [Channel::Zero]);
    assert_eq!(device.closed, [Channel::Zero]);
    assert_eq!(device.reset, 0x80);
    assert_eq!(clock.now.get(), 0);
}

#[test]
fn cancelled_delay_cannot_advance_reset_and_late_dma_completion_has_no_callback() {
    let clock = Clock::new();
    clock.interrupted.set(true);
    let mut device = Device::new();
    let mut shutdown = teardown();
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Err(TransportError::Scheduler(ServiceStatus(-49))));
    assert_eq!(device.reset, 0x81);
    assert_eq!(device.enabled, 7);
    assert_eq!(clock.now.get(), 0);
    assert_eq!(shutdown.completion_interrupt(Channel::Zero, &mut device), None);
    assert_eq!(shutdown.completion_interrupt(Channel::One, &mut device), None);
    clock.interrupted.set(false);
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Complete));
    assert_eq!(clock.now.get(), 200);
    assert_eq!(device.closed.len(), 2);
}

#[test]
fn second_delay_cancellation_keeps_reset_deasserted_until_owned_completion() {
    let clock = Clock::new();
    clock.cancel_after.set(Some(100));
    let mut device = Device::new();
    let mut shutdown = teardown();
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Err(TransportError::Scheduler(ServiceStatus(-49))));
    assert_eq!(clock.now.get(), 100);
    assert_eq!(device.reset, 0x80);
    assert_eq!(device.enabled, 7);
    clock.cancel_after.set(None);
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Complete));
    assert_eq!(clock.now.get(), 200);
    assert_eq!(device.enabled, 0);
    assert_eq!(device.closed.len(), 2);
}

#[test]
fn timer_cancellation_preserves_registry_status_and_clears_handle_even_on_error() {
    struct Registry { seen: Vec<RegistryHandle> }
    impl RegistrationRegistry for Registry {
        fn release(&mut self, handle: RegistryHandle) -> ServiceStatus { self.seen.push(handle); ServiceStatus(-25) }
    }
    let mut registry = Registry { seen: Vec::new() };
    let mut handle = Some(RegistryHandle(17));
    assert_eq!(cancel_transfer_timer(&mut handle, &mut registry), ServiceStatus(-25));
    assert_eq!(handle, None);
    assert_eq!(registry.seen, [RegistryHandle(17)]);
    assert_eq!(cancel_transfer_timer(&mut handle, &mut registry), ServiceStatus(-25));
    assert_eq!(registry.seen, [RegistryHandle(17), RegistryHandle(0)]);
}

#[test]
fn invalid_scheduler_context_does_not_complete_either_reset_delay() {
    let clock = Clock::new();
    clock.store_word(Domain::A.words().dispatch_enabled, 0);
    let mut device = Device::new();
    let mut shutdown = teardown();
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Err(TransportError::Scheduler(ServiceStatus(-25))));
    assert_eq!(device.reset, 0x81);
    assert_eq!(device.enabled, 7);
    assert_eq!(clock.now.get(), 0);
    clock.store_word(Domain::A.words().dispatch_enabled, 1);
    assert_eq!(shutdown.resume(&mut device, &clock, Domain::A), Ok(TeardownProgress::Complete));
    assert_eq!(clock.now.get(), 200);
}
