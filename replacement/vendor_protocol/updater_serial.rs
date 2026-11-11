//! X-M1 1.50 updater UART at 0xfffaa000, distinct from the SPI transport.
//! Source pin 55436fc85c0560dd252b0410af6f2427fe198cd3; raw image-4 extents
//! 0x65bb8c..0x65df68 authenticate against the pinned function-range hashes.

#[path = "updater_serial_irq.rs"]
mod interrupt;
pub use interrupt::{InterruptBinding, PhysicalAddress};
use core::task::Poll;

#[cfg(not(target_arch = "arm"))]
#[path = "updater_serial_emulator.rs"]
pub mod emulator;

#[cfg(target_arch = "arm")]
#[path = "updater_serial_native.rs"]
pub mod native;

pub const RX_CAPACITY: usize = 100;
const UART: u32 = 0xfffa_a000;

/// Implementations expose physical accesses, not UART operations or completion decisions.
pub trait Io {
    fn read8(&self, address: PhysicalAddress) -> u8;
    fn write8(&self, address: PhysicalAddress, value: u8);
    fn read16(&self, address: PhysicalAddress) -> u16;
    fn write16(&self, address: PhysicalAddress, value: u16);
    fn read32(&self, address: PhysicalAddress) -> u32;
    fn write32(&self, address: PhysicalAddress, value: u32);
    fn mask_interrupts(&self) -> SavedInterrupts;
    fn restore_interrupts(&self, saved: SavedInterrupts);
}
impl<I: Io> Io for &I {
    fn read8(&self, address: PhysicalAddress) -> u8 { I::read8(self, address) }
    fn write8(&self, address: PhysicalAddress, value: u8) { I::write8(self, address, value); }
    fn read16(&self, address: PhysicalAddress) -> u16 { I::read16(self, address) }
    fn write16(&self, address: PhysicalAddress, value: u16) { I::write16(self, address, value); }
    fn read32(&self, address: PhysicalAddress) -> u32 { I::read32(self, address) }
    fn write32(&self, address: PhysicalAddress, value: u32) { I::write32(self, address, value); }
    fn mask_interrupts(&self) -> SavedInterrupts { I::mask_interrupts(self) }
    fn restore_interrupts(&self, saved: SavedInterrupts) { I::restore_interrupts(self, saved); }
}

#[derive(Clone, Copy)]
pub struct SavedInterrupts(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxEpoch(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadPosition(usize);
impl ReadPosition { pub const fn get(self) -> usize { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Baud(u32);
impl Baud {
    pub const fn new(value: u32) -> Result<Self, Error> {
        if value > 22 && value <= 1_500_000 { Ok(Self(value)) } else { Err(Error::InvalidBaud) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Busy, InvalidHandle, NotReceiving, AlreadyReceiving, InvalidBaud, Exhausted, WriteBusy, Cancelled }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReceiveErrors { pub overrun: u32, pub parity: u32, pub framing: u32, pub break_signal: u32 }
#[derive(Clone, Copy, Eq, PartialEq)]
enum State { Closed, Open, Receiving }

pub struct Write<'a> {
    bytes: &'a [u8],
    handle: Handle,
    identity: u32,
    sent: usize,
    awaiting_wire: bool,
}
impl Write<'_> { pub fn bytes_completed(&self) -> usize { self.sent } }

pub struct UpdaterSerial<I> {
    io: I,
    binding: InterruptBinding,
    state: State,
    generation: u32,
    epoch: u32,
    write_sequence: u32,
    pending_write: Option<u32>,
    wire_pending: bool,
    buffer: [u8; RX_CAPACITY],
    read: usize,
    write: usize,
    errors: ReceiveErrors,
}
impl<I: Io> UpdaterSerial<I> {
    pub const fn new(io: I, binding: InterruptBinding) -> Self {
        Self { io, binding, state: State::Closed, generation: 0, epoch: 0, write_sequence: 0,
            pending_write: None, wire_pending: false, buffer: [0; RX_CAPACITY], read: 0, write: 0, errors: ReceiveErrors { overrun: 0, parity: 0, framing: 0, break_signal: 0 } }
    }
    pub fn io(&self) -> &I { &self.io }
    fn valid(&self, handle: Handle) -> Result<(), Error> {
        if self.state != State::Closed && handle.0 == self.generation { Ok(()) } else { Err(Error::InvalidHandle) }
    }
    fn h(&self, offset: u32, value: u16) { self.io.write16(PhysicalAddress(UART + offset), value); }
    fn rh(&self, offset: u32) -> u16 { self.io.read16(PhysicalAddress(UART + offset)) }
    fn modify_word(&self, address: u32, clear: u32, set: u32) {
        let address = PhysicalAddress(address);
        self.io.write32(address, (self.io.read32(address) & !clear) | set);
    }
    /// Opens native channel zero. Both stock logical configurations select that channel.
    pub fn open(&mut self) -> Result<Handle, Error> {
        if self.state != State::Closed { return Err(Error::Busy); }
        let generation = self.generation.checked_add(1).ok_or(Error::Exhausted)?;
        let saved = self.io.mask_interrupts();
        // 65cbd8 configures C0/C1 before selecting their UART peripheral function.
        self.modify_word(0xfff8_9318, 0xf, 0);
        self.io.write32(PhysicalAddress(0xfff8_930c), 0x000f_000f);
        self.io.write32(PhysicalAddress(0xfff8_9308), 0x0003_0002);
        self.io.write32(PhysicalAddress(0xfff8_9304), 0x0003_0000);
        self.modify_word(0xfffb_2004, 0xf0, 0x50);
        self.modify_word(0xfffb_2010, 0xffff, 0x120);
        self.modify_word(0xfffb_2014, 0xffff, 0x120);
        self.modify_word(0xfff8_802c, 0, 1);
        self.h(0x16, 0x80);
        self.h(0x16, 0);
        self.h(2, 0);
        self.program_baud(Baud(19_200));
        self.h(8, 3);
        self.h(0xa, 0);
        self.h(6, (self.rh(6) & !1) | 6);
        self.h(6, 0x81);
        self.h(0x16, 0);
        self.io.restore_interrupts(saved);
        self.generation = generation;
        self.state = State::Open;
        Ok(Handle(generation))
    }
    fn program_baud(&self, baud: Baud) {
        self.h(8, self.rh(8) | 0x80);
        let divisor = 1_500_000 / baud.0;
        self.h(0x12, (divisor & 0xff) as u16);
        self.h(0x14, (divisor >> 8) as u16);
        self.h(8, self.rh(8) & !0x80);
        self.h(6, 0x81);
    }
    pub fn baud(&mut self, handle: Handle, baud: Baud) -> Result<(), Error> {
        self.valid(handle)?;
        if self.pending_write.is_some() { return Err(Error::WriteBusy); }
        let saved = self.io.mask_interrupts();
        self.program_baud(baud);
        self.io.restore_interrupts(saved);
        Ok(())
    }
    pub fn start(&mut self, handle: Handle) -> Result<RxEpoch, Error> {
        self.valid(handle)?;
        if self.state == State::Receiving { return Err(Error::AlreadyReceiving); }
        let epoch = self.epoch.checked_add(1).ok_or(Error::Exhausted)?;
        let saved = self.io.mask_interrupts();
        self.binding.enable(&self.io);
        self.epoch = epoch;
        self.state = State::Receiving;
        self.h(2, 5);
        self.io.restore_interrupts(saved);
        Ok(RxEpoch(epoch))
    }
    pub fn stop(&mut self, handle: Handle) -> Result<(), Error> {
        self.valid(handle)?;
        if self.state != State::Receiving { return Err(Error::NotReceiving); }
        let saved = self.io.mask_interrupts();
        self.h(2, 0);
        self.binding.disable(&self.io);
        self.state = State::Open;
        self.read = 0;
        self.write = 0;
        self.io.restore_interrupts(saved);
        Ok(())
    }
    pub fn close(&mut self, handle: Handle) -> Result<(), Error> {
        self.valid(handle)?;
        if self.state == State::Receiving { self.stop(handle)?; }
        let saved = self.io.mask_interrupts();
        self.h(2, 0);
        self.h(6, 0);
        self.modify_word(0xfff8_802c, 1, 0);
        self.pending_write = None;
        self.state = State::Closed;
        self.io.restore_interrupts(saved);
        Ok(())
    }
    pub fn reset_rx(&mut self, handle: Handle) -> Result<(), Error> {
        self.valid(handle)?;
        let saved = self.io.mask_interrupts();
        self.read = 0;
        self.write = 0;
        self.io.restore_interrupts(saved);
        Ok(())
    }
    pub fn clear_rx(&mut self, handle: Handle, fill: u8) -> Result<(), Error> {
        self.valid(handle)?;
        let saved = self.io.mask_interrupts();
        self.buffer.fill(fill);
        self.read = 0;
        self.write = 0;
        self.io.restore_interrupts(saved);
        Ok(())
    }
    pub fn rx_buffer(&self, handle: Handle) -> Result<&[u8; RX_CAPACITY], Error> { self.valid(handle)?; Ok(&self.buffer) }
    pub fn read_position(&self, handle: Handle) -> Result<ReadPosition, Error> {
        self.valid(handle)?;
        Ok(ReadPosition(self.read))
    }
    pub fn read_received(&mut self, handle: Handle, output: &mut [u8]) -> Result<usize, Error> {
        self.valid(handle)?;
        let saved = self.io.mask_interrupts();
        let mut count = 0;
        while count < output.len() && self.read != self.write {
            output[count] = self.buffer[self.read];
            self.read = (self.read + 1) % RX_CAPACITY;
            count += 1;
        }
        self.io.restore_interrupts(saved);
        Ok(count)
    }
    pub fn receive_errors(&self) -> ReceiveErrors { self.errors }
    pub fn service_receive(&mut self) -> Result<(), Error> {
        self.receive_interrupt(RxEpoch(self.epoch))
    }
    pub fn receive_interrupt(&mut self, epoch: RxEpoch) -> Result<(), Error> {
        if self.state != State::Receiving || self.epoch != epoch.0 { return Err(Error::Cancelled); }
        match self.rh(4) & 15 {
            6 => {
                let status = self.rh(0xc);
                self.errors.overrun = self.errors.overrun.wrapping_add(((status >> 1) & 1) as u32);
                self.errors.parity = self.errors.parity.wrapping_add(((status >> 2) & 1) as u32);
                self.errors.framing = self.errors.framing.wrapping_add(((status >> 3) & 1) as u32);
                self.errors.break_signal = self.errors.break_signal.wrapping_add(((status >> 4) & 1) as u32);
            }
            4 | 12 => {
                let saved = self.io.mask_interrupts();
                while self.rh(0xc) & 1 != 0 {
                    self.buffer[self.write] = self.io.read8(PhysicalAddress(UART));
                    self.write = (self.write + 1) % RX_CAPACITY;
                    if self.write == self.read { self.read = (self.read + 1) % RX_CAPACITY; }
                }
                self.io.restore_interrupts(saved);
            }
            0 => { let _ = self.rh(0xe); }
            _ => {}
        }
        Ok(())
    }
    pub fn write_begin<'a>(&mut self, handle: Handle, bytes: &'a [u8]) -> Result<Write<'a>, Error> {
        self.valid(handle)?;
        if self.pending_write.is_some() { return Err(Error::WriteBusy); }
        self.write_sequence = self.write_sequence.checked_add(1).ok_or(Error::Exhausted)?;
        self.pending_write = Some(self.write_sequence);
        Ok(Write { bytes, handle, identity: self.write_sequence, sent: 0, awaiting_wire: false })
    }
    pub fn write_poll(&mut self, write: &mut Write<'_>) -> Poll<Result<(), Error>> {
        if self.valid(write.handle).is_err() || self.pending_write != Some(write.identity) { return Poll::Ready(Err(Error::Cancelled)); }
        if self.wire_pending {
            if self.rh(0xc) & 0x60 != 0x60 { return Poll::Pending; }
            self.wire_pending = false;
            if write.awaiting_wire {
                write.awaiting_wire = false;
                write.sent += 1;
            }
        }
        if write.sent == write.bytes.len() {
            self.pending_write = None;
            return Poll::Ready(Ok(()));
        }
        self.io.write8(PhysicalAddress(UART), write.bytes[write.sent]);
        write.awaiting_wire = true;
        self.wire_pending = true;
        Poll::Pending
    }
    /// A byte already handed to the UART can still leave the physical transmitter.
    pub fn cancel_write(&mut self, write: &Write<'_>) -> Result<(), Error> {
        if self.pending_write != Some(write.identity) { return Err(Error::Cancelled); }
        self.pending_write = None;
        Ok(())
    }
    pub fn write(&mut self, handle: Handle, bytes: &[u8]) -> Result<(), Error> {
        let mut write = self.write_begin(handle, bytes)?;
        loop {
            match self.write_poll(&mut write) {
                Poll::Ready(result) => return result,
                Poll::Pending => core::hint::spin_loop(),
            }
        }
    }
}

#[cfg(target_arch = "arm")]
pub struct NativeIo { _private: () }
#[cfg(target_arch = "arm")]
impl NativeIo {
    /// # Safety
    /// The caller owns the UART, GPIO C0/C1, clock bit zero and IRQ107 controller entry.
    pub unsafe fn new() -> Self { Self { _private: () } }
}
#[cfg(target_arch = "arm")]
impl Io for NativeIo {
    fn read8(&self, address: PhysicalAddress) -> u8 { unsafe { core::ptr::read_volatile(address.0 as *const u8) } }
    fn write8(&self, address: PhysicalAddress, value: u8) { unsafe { core::ptr::write_volatile(address.0 as *mut u8, value) } }
    fn read16(&self, address: PhysicalAddress) -> u16 { unsafe { core::ptr::read_volatile(address.0 as *const u16) } }
    fn write16(&self, address: PhysicalAddress, value: u16) { unsafe { core::ptr::write_volatile(address.0 as *mut u16, value) } }
    fn read32(&self, address: PhysicalAddress) -> u32 { unsafe { core::ptr::read_volatile(address.0 as *const u32) } }
    fn write32(&self, address: PhysicalAddress, value: u32) { unsafe { core::ptr::write_volatile(address.0 as *mut u32, value) } }
    fn mask_interrupts(&self) -> SavedInterrupts {
        let saved: u32;
        unsafe { core::arch::asm!("mrs {saved}, cpsr", "cpsid if", saved = out(reg) saved, options(nostack)); }
        SavedInterrupts(saved)
    }
    fn restore_interrupts(&self, saved: SavedInterrupts) {
        let current: u32;
        unsafe { core::arch::asm!("mrs {current}, cpsr", current = out(reg) current, options(nostack)); }
        let restored = (current & !0xc0) | (saved.0 & 0xc0);
        unsafe { core::arch::asm!("msr cpsr_c, {restored}", restored = in(reg) restored, options(nostack)); }
    }
}

#[cfg(test)]
#[path = "updater_serial_tests.rs"]
mod tests;
