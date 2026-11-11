use super::*;
use irq_control::{Backend, Bank, Handler, SavedInterrupts, Table};
use irq_control::layout::GuestAddress;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Interrupts { words: RefCell<BTreeMap<u32, u32>>, masked: RefCell<bool> }
impl Backend for Interrupts {
    fn load_word(&self, at: GuestAddress) -> u32 { *self.words.borrow().get(&at.0).unwrap_or(&0) }
    fn store_word(&self, at: GuestAddress, value: u32) { self.words.borrow_mut().insert(at.0, value); }
    fn mask_irq(&self) -> SavedInterrupts { let previous = *self.masked.borrow(); *self.masked.borrow_mut() = true; SavedInterrupts(u32::from(previous)) }
    fn restore_irq(&self, saved: SavedInterrupts) { *self.masked.borrow_mut() = saved.0 != 0; }
    fn mask_irq_fiq(&self) -> SavedInterrupts { self.mask_irq() }
    fn restore_irq_fiq(&self, saved: SavedInterrupts) { self.restore_irq(saved); }
    fn bank(&self) -> Bank { Bank::Primary }
}

#[derive(Default)]
struct Device { words: BTreeMap<u32, u32>, clock: Vec<ClockLevel> }
impl Device {
    fn get(&self, address: u32) -> u32 { *self.words.get(&address).unwrap_or(&0) }
}
impl ControllerIo for Device {
    fn read32(&mut self, register: Register) -> u32 { self.get(register.address()) }
    fn read16(&mut self, register: Register) -> u16 { self.get(register.address()) as u16 }
    fn write32(&mut self, register: Register, value: u32) { self.words.insert(register.address(), value); }
    fn write16(&mut self, register: Register, value: u16) { self.words.insert(register.address(), u32::from(value)); }
}
impl StartIo for Device {
    fn start_read32(&mut self, register: StartRegister) -> u32 { self.get(register.address() as u32) }
    fn start_read16(&mut self, register: StartRegister) -> u16 { self.get(register.address() as u32) as u16 }
    fn start_write32(&mut self, register: StartRegister, value: u32) { self.words.insert(register.address() as u32, value); }
    fn start_write16(&mut self, register: StartRegister, value: u16) { self.words.insert(register.address() as u32, u32::from(value)); }
    fn request_clock(&mut self, level: ClockLevel) -> transport::ServiceStatus { self.clock.push(level); transport::ServiceStatus(0) }
}

#[test]
fn connection_votes_select_the_active_and_idle_clock_levels() {
    let mut io = Device::default();
    vote_connection(&mut io, true);
    vote_connection(&mut io, false);
    assert_eq!(io.clock, [ClockLevel::Active, ClockLevel::Idle]);
    assert_eq!(bulk_packet_size(&mut io), 64);
    io.write16(Register::BusSpeed, 0x4000);
    assert_eq!(bulk_packet_size(&mut io), 512);
}
struct Counter(u32);
impl cpu::DelayCounter for Counter {
    fn reload(&mut self) -> u32 { 10_000_000 }
    fn current(&mut self) -> u32 { self.0 = self.0.saturating_sub(10_000); self.0 }
}

fn with_runtime(test: impl FnOnce(&mut StartRuntime<'_, '_, Interrupts, Device, Counter>)) {
    let backend = Interrupts::default();
    let table = Table::new(GuestAddress(0x4007_0000), Handler::from_static(0x42000));
    let mut entries = [None; 128];
    let mut sources = [u32::MAX; 128];
    let mut sequence = cpu::HandleSequence::new(0);
    let mut irq = cpu::IrqContext {
        backend: &backend, table, handles: cpu::HandleTable::new(&mut entries), sequence: &mut sequence,
        handlers: cpu::PublishedInterruptHandlers::from_handlers(Handler::from_static(0x43000), Handler::from_static(0x44000)),
        sources: &mut sources, channel_handles: [None, None],
    };
    test(&mut StartRuntime { io: Device::default(), irq: &mut irq, timer: Counter(9_999_999), handler: Handler::from_static(0x45000) });
}

#[test]
fn reset_preserves_session_mode_reserved_bytes_and_other_usb_state() {
    let mut bytes = [0xa5; USB_MEMORY_BYTES];
    {
        let mut usb = UsbStart::new(UsbMemory::owned(&mut bytes));
        usb.set_session(SessionMode::from_abi(3));
        usb.reset_session();
        assert_eq!(usb.session(), SessionMode::from_abi(3));
    }
    assert_eq!(&bytes[0xb4..0xb8], &3u32.to_le_bytes());
    assert_eq!(&bytes[0xb8..0xba], &[0, 0]);
    assert_eq!(&bytes[0xba..0xbc], &[0xa5, 0xa5]);
    assert_eq!(&bytes[0xbc..0xca], &[0; 14]);
    assert_eq!(&bytes[0xca..], &[0xa5, 0xa5]);
    assert!(bytes[..0xb4].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn callback_publication_updates_guest_pointer_and_hardware_masks() {
    let mut bytes = [0xff; USB_MEMORY_BYTES];
    let mut io = Device::default();
    let region = transport::PublishedRegion::new(0x4100_0000, 64).unwrap();
    {
        let mut usb = UsbStart::new(UsbMemory::owned(&mut bytes));
        usb.bind_callbacks(CallbackTable::published(region, 16).unwrap(), &mut io);
    }
    assert_eq!(&bytes[0x18..0x1c], &0x4100_0010u32.to_le_bytes());
    assert_eq!(io.get(0xfff54040), 0xdffb);
    assert_eq!(io.get(0xfff54044), 0xffff);
    assert_eq!(io.get(0xfff54048), 0xffff);
    assert_eq!(CallbackTable::published(region, 36), Err(TransportError::OutsideRegion));
    assert_eq!(CallbackTable::published(region, 1), Err(TransportError::OutsideRegion));
}

#[test]
fn disconnected_link_debounces_without_programming_or_registering_irq() {
    with_runtime(|runtime| {
        let mut bytes = [0; USB_MEMORY_BYTES];
        let mut usb = UsbStart::new(UsbMemory::owned(&mut bytes));
        usb.begin_start();
        for _ in 0..99 { assert!(matches!(usb.poll_start(runtime), Ok(StartProgress::WaitTicks(ticks)) if ticks.get() == 10)); }
        assert_eq!(usb.poll_start(runtime), Ok(StartProgress::Disconnected));
        assert_eq!(usb.controller_irq(), None);
        assert_eq!(runtime.io.get(0xfff54090), 0);
        assert_eq!(usb.poll_start(runtime), Err(StartError::NotStarting));
    });
}

#[test]
fn stable_connected_link_programs_usb_registers_and_canonical_irq84() {
    with_runtime(|runtime| {
        runtime.io.start_write32(StartRegister::LinkStatus, 0x8000_0000);
        runtime.io.start_write16(StartRegister::PendingLow, 0x1234);
        runtime.io.start_write16(StartRegister::PendingMiddle, 0x5678);
        runtime.io.start_write16(StartRegister::PendingHigh, 0x9abc);
        let mut bytes = [0; USB_MEMORY_BYTES];
        let mut usb = UsbStart::new(UsbMemory::owned(&mut bytes));
        usb.begin_start();
        for _ in 0..100 { assert!(matches!(usb.poll_start(runtime), Ok(StartProgress::WaitTicks(ticks)) if ticks.get() == 10)); }
        assert_eq!(usb.poll_start(runtime), Ok(StartProgress::Ready));
        assert_eq!(runtime.io.get(0xfff54054), 0xedcb);
        assert_eq!(runtime.io.get(0xfff54058), 0xa987);
        assert_eq!(runtime.io.get(0xfff5405c), 0x6543);
        assert_eq!(runtime.io.get(0xfff54040), 0xdfe8);
        assert_eq!(runtime.io.get(0xfff54044), 0x7ffb);
        assert_eq!(runtime.io.get(0xfff54048), 0xafbd);
        assert_eq!(runtime.io.get(0xfff5512c), 0x81810303);
        assert_eq!(runtime.io.get(0xfff55114), 0x101);
        assert_eq!(runtime.io.get(0xfff54004), 12);
        assert_eq!(runtime.io.get(0xfff54068), 0x10d);
        assert_eq!(runtime.io.get(0xfff5406c), 0x2083);
        assert_eq!(runtime.io.get(0xfff54070), 0);
        assert_eq!(runtime.io.get(0xfff54080), 0xfff);
        assert_eq!(runtime.io.get(0xfff54090), 6);
        let handle = usb.controller_irq().unwrap();
        assert_eq!(runtime.irq.handles.find(handle), Ok(84));
        assert_eq!(runtime.irq.sources[84], 15);
        let line = irq_control::InterruptLine::from_static(84);
        assert_eq!(runtime.irq.backend.load_word(runtime.irq.table.entry(line)), 0x45000);
        assert_eq!(runtime.irq.backend.load_word(runtime.irq.table.entry(line).offset(8)), 13);
        assert_eq!(runtime.irq.backend.load_word(GuestAddress(0xfffb3088)) & (1 << 20), 1 << 20);
        assert!(!*runtime.irq.backend.masked.borrow());
    });
}

#[test]
fn falling_link_ends_start_before_any_irq_or_endpoint_enable() {
    with_runtime(|runtime| {
        let mut bytes = [0; USB_MEMORY_BYTES];
        let mut usb = UsbStart::new(UsbMemory::owned(&mut bytes));
        usb.begin_start();
        runtime.io.start_write32(StartRegister::LinkStatus, 0x8000_0000);
        assert!(matches!(usb.poll_start(runtime), Ok(StartProgress::WaitTicks(ticks)) if ticks.get() == 10));
        runtime.io.start_write32(StartRegister::LinkStatus, 0);
        assert_eq!(usb.poll_start(runtime), Ok(StartProgress::Disconnected));
        assert_eq!(usb.controller_irq(), None);
        assert_eq!(runtime.io.get(0xfff54090), 0);
    });
}

#[test]
fn rearm_replaces_irq_lease_and_republishes_masks_after_active_clear() {
    with_runtime(|runtime| {
        let mut bytes = [0; USB_MEMORY_BYTES];
        let mut usb = UsbStart::new(UsbMemory::owned(&mut bytes));
        usb.begin_start();
        usb.finalize(runtime).unwrap();
        assert_eq!(usb.controller_irq(), None);
        usb.clear_active();
        usb.finalize(runtime).unwrap();
        let old = usb.controller_irq().unwrap();
        usb.clear_active();
        usb.finalize(runtime).unwrap();
        let new = usb.controller_irq().unwrap();
        assert_ne!(old, new);
        assert_eq!(runtime.irq.handles.find(old), Err(TransportError::InvalidChannel));
        assert_eq!(runtime.irq.handles.find(new), Ok(84));
        assert_eq!(runtime.io.get(0xfff5512c), 0x81810303);
        assert_eq!(runtime.io.get(0xfff54040), 0xdfe8);
    });
}

#[test]
fn owned_transfer_storage_is_the_aperture_used_by_retained_transport() {
    let region = transport::PublishedRegion::new(0x4100_0000, 128).unwrap();
    let mut bytes = [0; 128];
    let span = region.span(64, 4).unwrap();
    {
        let mut buffer = TransferBuffer::owned(region, &mut bytes).unwrap();
        buffer.aperture_mut().bytes_mut(span).unwrap().copy_from_slice(&[1, 2, 3, 4]);
        let mut transfer = transport::EndpointTransfer::new(buffer.aperture(), span, transport::Endpoint::BulkIn).unwrap();
        let mut device = Device::default();
        assert_eq!(transfer.program_next(buffer.aperture(), &mut device), Ok(4));
        assert_eq!(device.get(Register::Fifo(transport::Endpoint::BulkIn).address()), 0x04030201);
    }
    assert_eq!(&bytes[64..68], &[1, 2, 3, 4]);
}

#[test]
fn bus_status_bit14_is_suppressed_without_the_connected_link_bit() {
    let mut device = Device::default();
    device.write16(Register::BusSpeed, 0x4000);
    assert!(!link_connected(&mut device));
    assert!(!bus_status_bit14(&mut device));
    device.start_write32(StartRegister::LinkStatus, 0x80000000);
    assert!(link_connected(&mut device));
    assert!(bus_status_bit14(&mut device));
    device.write16(Register::BusSpeed, 0xbfff);
    assert!(!bus_status_bit14(&mut device));
}
