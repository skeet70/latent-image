use super::*;
use core::cell::Cell;
use std::{collections::BTreeMap, rc::Rc, vec::Vec};
use xm1_lens_transport::frame::RetryLimit;
use xm1_lens_transport::wire::{Address, LensEndpoints, RequestedOutput, TimerInterrupt, TimerTicks};

#[derive(Default)]
pub(crate) struct Physical {
    pub(crate) words: RefCell<BTreeMap<u32, u32>>,
    pub(crate) bytes: RefCell<BTreeMap<u32, u8>>,
    pub(crate) wire: Cell<Option<TransferToken>>,
    pub(crate) timer: Cell<Option<TimerToken>>,
    pub(crate) acquired: Cell<bool>,
    pub(crate) deny_acquire: Cell<bool>,
    pub(crate) requested: Cell<bool>,
}
#[derive(Clone)]
pub(crate) struct Registers(Rc<Physical>);
impl Mmio for Registers {
    type SavedInterrupts = ();
    fn read_word(&self, address: Address) -> u32 { *self.0.words.borrow().get(&address.get()).unwrap_or(&0) }
    fn write_word(&self, address: Address, value: u32) { self.0.words.borrow_mut().insert(address.get(), value); }
    fn read_byte(&self, address: Address) -> u8 { *self.0.bytes.borrow().get(&address.get()).unwrap_or(&0) }
    fn write_byte(&self, address: Address, value: u8) { self.0.bytes.borrow_mut().insert(address.get(), value); }
    fn mask_interrupts(&self) {}
    fn restore_interrupts(&self, _: ()) {}
    fn publish_dma(&self) {}
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalError { Denied }
pub(crate) struct Resources(Rc<Physical>);
impl Platform for Resources {
    type Error = PhysicalError;
    fn acquire(&mut self, _: LensEndpoints) -> Result<(), PhysicalError> { if self.0.deny_acquire.get() { Err(PhysicalError::Denied) } else { self.0.acquired.set(true); Ok(()) } }
    fn release(&mut self, _: LensEndpoints) -> Result<(), PhysicalError> { self.0.acquired.set(false); Ok(()) }
    fn request_output(&mut self, output: RequestedOutput) -> Result<(), PhysicalError> { self.0.requested.set(output == RequestedOutput::Active); Ok(()) }
}
pub(crate) struct Interrupts(Rc<Physical>);
impl InterruptController for Interrupts {
    type Error = PhysicalError;
    fn register_wire(&mut self, token: TransferToken) -> Result<(), PhysicalError> { self.0.wire.set(Some(token)); Ok(()) }
    fn unregister_wire(&mut self) -> Result<(), PhysicalError> { self.0.wire.set(None); Ok(()) }
    fn register_timer(&mut self, _: TimerInterrupt, token: TimerToken) -> Result<(), PhysicalError> { self.0.timer.set(Some(token)); Ok(()) }
    fn unregister_timer(&mut self, _: TimerInterrupt) -> Result<(), PhysicalError> { self.0.timer.set(None); Ok(()) }
}
pub(crate) type Runtime = LensQueueRuntime<Registers, Resources, Interrupts>;
pub(crate) fn setup() -> (Runtime, Rc<Physical>) {
    let physical = Rc::new(Physical::default());
    let ticks = TimerTicks::new(1).unwrap();
    let timing = TimingParameters { command: ticks, normal: ticks, deferred: ticks, accepted_page: ticks, rejected_page: ticks, recovery: ticks, recovery_limit: RetryLimit::new(2), busy_limit: RetryLimit::new(2), failure_mode: xm1_lens_transport::FailureMode::Notify };
    (Runtime::new(CpuBackend::new(Registers(physical.clone()), Resources(physical.clone()), Interrupts(physical.clone())), timing, [0xff; 64]), physical)
}
pub(crate) fn setup_from_configuration(source: configuration::LensSourceConfiguration) -> (Runtime, Rc<Physical>) {
    let physical = Rc::new(Physical::default());
    (Runtime::from_configuration(CpuBackend::new(Registers(physical.clone()), Resources(physical.clone()), Interrupts(physical.clone())), source), physical)
}
pub(crate) fn initialize(runtime: &mut Runtime) { runtime.submit(QueueRequest::Initialize { callback: CallbackId(1), observer: CallbackId(2) }, None).unwrap(); assert_eq!(runtime.pop_publication(), Some(Publication::Lifecycle { callback: CallbackId(1) })); }
pub(crate) fn packet(operation: u16, callback: u32, words: &[u32]) -> QueueRequest {
    let mut bytes = [0u8; 48];
    bytes[..4].copy_from_slice(&callback.to_le_bytes());
    for (index, word) in words.iter().enumerate() { bytes[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes()); }
    bytes[44] = words.len() as u8;
    QueueRequest::parse(RequestId(operation), &bytes).unwrap()
}
pub(crate) fn page(operation: u16, callback: u32, checksum: bool) -> QueueRequest {
    let mut bytes = [0u8; 20];
    bytes[..2].copy_from_slice(&0x1234u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&u32::from(checksum).to_le_bytes());
    bytes[16..].copy_from_slice(&callback.to_le_bytes());
    if operation == 0x7254 { bytes[8..12].copy_from_slice(&u32::from(checksum).to_le_bytes()); }
    QueueRequest::parse(RequestId(operation), &bytes).unwrap()
}
pub(crate) fn received(runtime: &mut Runtime, physical: &Physical, bytes: &[u8]) {
    for (index, byte) in bytes.iter().enumerate() { physical.bytes.borrow_mut().insert(0x4096_d820 + index as u32, *byte); }
    runtime.wire_completed(physical.wire.get().unwrap()).unwrap();
}
pub(crate) fn expire(runtime: &mut Runtime, physical: &Physical) { runtime.timer_expired(physical.timer.get().unwrap()).unwrap(); }
pub(crate) fn transmitted(physical: &Physical, length: usize) -> Vec<u8> { (0..length).map(|index| *physical.bytes.borrow().get(&(0x4096_d000 + index as u32)).unwrap()).collect() }
