use core::cell::{Cell, RefCell};
use super::{Io, PhysicalAddress, SavedInterrupts, UART};

struct Registers { entries: [(u32, u32); 256], used: usize }
impl Registers {
    fn get(&self, address: u32) -> u32 { self.entries[..self.used].iter().find(|(a, _)| *a == address).map_or(0, |(_, v)| *v) }
    fn set(&mut self, address: u32, value: u32) {
        if let Some(entry) = self.entries[..self.used].iter_mut().find(|(a, _)| *a == address) { entry.1 = value; }
        else { assert!(self.used < self.entries.len()); self.entries[self.used] = (address, value); self.used += 1; }
    }
}
struct Fifo { bytes: [u8; 256], read: usize, len: usize }
impl Fifo {
    fn push(&mut self, byte: u8) { assert!(self.len < 256); self.bytes[(self.read + self.len) % 256] = byte; self.len += 1; }
    fn pop(&mut self) -> Option<u8> { if self.len == 0 { None } else { let byte = self.bytes[self.read]; self.read = (self.read + 1) % 256; self.len -= 1; Some(byte) } }
}

pub struct Emulator {
    registers: RefCell<Registers>,
    received: RefCell<Fifo>,
    transmitted: RefCell<Fifo>,
    in_flight: Cell<Option<u8>>,
    masks: Cell<u32>,
}
impl Default for Emulator { fn default() -> Self { Self::new() } }
impl Emulator {
    pub const fn new() -> Self {
        Self { registers: RefCell::new(Registers { entries: [(0, 0); 256], used: 0 }),
            received: RefCell::new(Fifo { bytes: [0; 256], read: 0, len: 0 }),
            transmitted: RefCell::new(Fifo { bytes: [0; 256], read: 0, len: 0 }),
            in_flight: Cell::new(None), masks: Cell::new(0) }
    }
    pub fn register(&self, address: PhysicalAddress) -> u32 { self.registers.borrow().get(address.0) }
    pub fn set_register(&self, address: PhysicalAddress, value: u32) { self.registers.borrow_mut().set(address.0, value); }
    pub fn receive(&self, bytes: &[u8]) {
        for &byte in bytes { self.received.borrow_mut().push(byte); }
        self.set_register(PhysicalAddress(UART + 4), 4);
    }
    pub fn line_error(&self, status: u16) {
        self.set_register(PhysicalAddress(UART + 4), 6);
        self.set_register(PhysicalAddress(UART + 0xc), status as u32);
    }
    pub fn complete_transmit(&self) -> Option<u8> {
        let byte = self.in_flight.take()?;
        self.transmitted.borrow_mut().push(byte);
        self.set_register(PhysicalAddress(UART + 0xc), 0x60);
        Some(byte)
    }
    pub fn take_transmitted(&self) -> Option<u8> { self.transmitted.borrow_mut().pop() }
}
impl Io for Emulator {
    fn read8(&self, address: PhysicalAddress) -> u8 {
        if address.0 == UART { self.received.borrow_mut().pop().expect("UART receive FIFO empty") } else { self.register(address) as u8 }
    }
    fn write8(&self, address: PhysicalAddress, value: u8) {
        if address.0 == UART {
            assert!(self.in_flight.get().is_none(), "UART transmitter occupied");
            self.in_flight.set(Some(value));
            let status = self.register(PhysicalAddress(UART + 0xc));
            self.set_register(PhysicalAddress(UART + 0xc), status & !0x60);
        } else { self.set_register(address, value as u32); }
    }
    fn read16(&self, address: PhysicalAddress) -> u16 {
        let word = self.register(address) as u16;
        if address.0 == UART + 0xc { (word & !1) | u16::from(self.received.borrow().len != 0) } else { word }
    }
    fn write16(&self, address: PhysicalAddress, value: u16) { self.set_register(address, value as u32); }
    fn read32(&self, address: PhysicalAddress) -> u32 { self.register(address) }
    fn write32(&self, address: PhysicalAddress, value: u32) { self.set_register(address, value); }
    fn mask_interrupts(&self) -> SavedInterrupts { SavedInterrupts(self.masks.replace(0xc0)) }
    fn restore_interrupts(&self, saved: SavedInterrupts) { self.masks.set(saved.0); }
}
