use super::*;
use super::super::{ControllerIo, Register, ServiceStatus, TeardownPlatform};
use super::super::irq_control::{self, Backend, Bank, Handler, InterruptLine, SavedInterrupts, Table};
use irq_control::layout::GuestAddress;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct InterruptController(RefCell<BTreeMap<u32, u32>>);
impl Backend for InterruptController {
    fn load_word(&self, at: GuestAddress) -> u32 { *self.0.borrow().get(&at.0).unwrap_or(&0) }
    fn store_word(&self, at: GuestAddress, value: u32) { self.0.borrow_mut().insert(at.0, value); }
    fn mask_irq(&self) -> SavedInterrupts { SavedInterrupts(0) }
    fn restore_irq(&self, _: SavedInterrupts) {}
    fn mask_irq_fiq(&self) -> SavedInterrupts { SavedInterrupts(0) }
    fn restore_irq_fiq(&self, _: SavedInterrupts) {}
    fn bank(&self) -> Bank { Bank::Primary }
}
struct Registers;
impl ControllerIo for Registers {
    fn read32(&mut self, _: Register) -> u32 { 0 }
    fn read16(&mut self, _: Register) -> u16 { 0 }
    fn write32(&mut self, _: Register, _: u32) {}
    fn write16(&mut self, _: Register, _: u16) {}
}
struct Semaphore { status: ServiceStatus }
impl ChannelSemaphore for Semaphore {
    fn release(&mut self, _: Channel) -> ServiceStatus { self.status }
}

#[test]
fn irq_release_disables_hardware_resets_registration_and_invalidates_handle() {
    let backend = InterruptController::default();
    let table = Table::new(GuestAddress(0x40070000), Handler::from_static(0x42000));
    let mut entries = [None; 128];
    let mut sources = [u32::MAX; 128];
    let mut sequence = HandleSequence::new(0);
    let mut handles = HandleTable::new(&mut entries);
    let line = InterruptLine::from_static(85);
    let handle = handles.allocate(85, &mut sequence).unwrap();
    let registration = irq_control::Registration::new(0x43000, 13, 0, 1).unwrap();
    irq_control::configure(&backend, table, line, registration);
    irq_control::enable(&backend, table, line).unwrap();
    sources[85] = 0x46;
    let mut irq = IrqContext {
        backend: &backend, table, handles, sequence: &mut sequence,
        handlers: PublishedInterruptHandlers { zero: registration.handler, one: registration.handler },
        sources: &mut sources, channel_handles: [Some(handle), None],
    };
    release_interrupt_handle(&mut irq, handle).unwrap();
    assert_eq!(irq.handles.find(handle), Err(TransportError::InvalidChannel));
    assert_eq!(irq.sources[85], u32::MAX);
    assert_eq!(backend.load_word(GuestAddress(0xfffb3088)) & (1 << 21), 0);
    assert_eq!(backend.load_word(GuestAddress(0xfffb3554)), table.default_handler().get());
    assert_eq!(backend.load_word(table.entry(line)), table.default_handler().get());
    assert_eq!(backend.load_word(table.entry(line).offset(8)), 15);
    assert_eq!(release_interrupt_handle(&mut irq, handle), Err(TransportError::InvalidChannel));
}

#[test]
fn semaphore_error_after_channel_handle_free_can_resume_without_releasing_handle_twice() {
    let backend = InterruptController::default();
    let table = Table::new(GuestAddress(0x40070000), Handler::from_static(0x42000));
    let mut entries = [None; 128];
    let mut sources = [u32::MAX; 128];
    let mut sequence = HandleSequence::new(0);
    let mut runtime = ChannelRuntime {
        io: Registers,
        irq: IrqContext {
            backend: &backend, table, handles: HandleTable::new(&mut entries), sequence: &mut sequence,
            handlers: PublishedInterruptHandlers { zero: Handler::from_static(0x43000), one: Handler::from_static(0x44000) },
            sources: &mut sources, channel_handles: [None, None],
        },
        cache: (), timer: (),
    };
    let mut channel_entries = [None; 2];
    let mut channel_sequence = HandleSequence::new(0);
    let mut channel_handles = HandleTable::new(&mut channel_entries);
    let handle = channel_handles.allocate(0, &mut channel_sequence).unwrap();
    let mut platform = TeardownRuntime {
        runtime: &mut runtime, channel_handles, semaphores: Semaphore { status: ServiceStatus(-25) }, pending_semaphores: [None, None],
    };
    assert_eq!(platform.release_channel(Channel::Zero, handle), Err(TransportError::Release(ServiceStatus(-25))));
    assert_eq!(platform.channel_handles.find(handle), Err(TransportError::InvalidChannel));
    platform.semaphores.status = ServiceStatus(0);
    assert_eq!(platform.release_channel(Channel::Zero, handle), Ok(()));
    assert_eq!(platform.release_channel(Channel::Zero, handle), Err(TransportError::InvalidChannel));
}
