//! X-M1 overlay47 SDIO host at 0xfff51000, not the filesystem NAND device.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3. The NONMATCHING command,
//! FIFO, transfer and IRQ reconstructions are grounded in authenticated A32 bytes
//! at 0128fb48, 01290384, 0129075c, 012908e0, 01290da0, 01290e58,
//! 012912c0, 0129147c, 0128f470 and 0128f5a4. Register accessors are exact.

use core::task::Poll;
use crate::transport::cpu::{CacheInstructions, clean_invalidate_all};
#[cfg(target_arch = "arm")]
use xm1_power_services::callback_registry::{registry as callbacks, CallbackWord};

pub const BASE: u32 = 0xfff5_1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum HalfRegister {
    Length = 0x004,
    Remaining = 0x006,
    ArgumentLow = 0x008,
    ArgumentHigh = 0x00a,
    Config = 0x028,
    Clock = 0x02c,
    Mode = 0x02e,
    Status = 0x030,
    Errors = 0x032,
    StatusMask = 0x034,
    ErrorMask = 0x036,
    StatusSignal = 0x038,
    ErrorSignal = 0x03a,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WordRegister { DmaAddress = 0, Command = 0x00c, Fifo = 0x020, Present = 0x024 }

pub trait Mmio {
    fn read_half(&mut self, register: HalfRegister) -> u16;
    fn write_half(&mut self, register: HalfRegister, value: u16);
    fn read_word(&mut self, register: WordRegister) -> u32;
    fn write_word(&mut self, register: WordRegister, value: u32);
    fn response_half(&mut self, index: u8) -> u16;
}

pub struct NativeMmio { _exclusive: () }
impl NativeMmio {
    /// # Safety
    /// The caller exclusively owns the host registers and its interrupt route.
    pub unsafe fn acquire() -> Self { Self { _exclusive: () } }
}
impl Mmio for NativeMmio {
    fn read_half(&mut self, register: HalfRegister) -> u16 {
        unsafe { core::ptr::read_volatile((BASE + register as u32) as *const u16) }
    }
    fn write_half(&mut self, register: HalfRegister, value: u16) {
        unsafe { core::ptr::write_volatile((BASE + register as u32) as *mut u16, value) }
    }
    fn read_word(&mut self, register: WordRegister) -> u32 {
        unsafe { core::ptr::read_volatile((BASE + register as u32) as *const u32) }
    }
    fn write_word(&mut self, register: WordRegister, value: u32) {
        unsafe { core::ptr::write_volatile((BASE + register as u32) as *mut u32, value) }
    }
    fn response_half(&mut self, index: u8) -> u16 {
        if index >= 8 { return 0; }
        unsafe { core::ptr::read_volatile((BASE + 0x10 + u32::from(index) * 2) as *const u16) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(u32);
impl Milliseconds {
    pub const fn new(value: u32) -> Self { Self(value) }
    fn elapsed_since(self, start: Self) -> u32 { self.0.wrapping_sub(start.0) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferUnit(u16);
impl TransferUnit {
    pub const BLOCK_512: Self = Self(512);
    pub const fn new(value: u16) -> Option<Self> {
        if value > 0 && value <= 512 { Some(Self(value)) } else { None }
    }
    pub const fn get(self) -> u16 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Execution { Interrupt, Polling }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { Direct52, Extended53 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Argument(u32);
impl Argument {
    pub const fn from_wire(value: u32) -> Self { Self(value) }
    pub const fn wire(self) -> u32 { self.0 }
    fn write(self) -> bool { self.0 & 0x8000_0000 != 0 }
    fn block(self) -> bool { self.0 & 0x0800_0000 != 0 }
    fn count(self) -> usize { (self.0 & 511) as usize }
}

pub struct DmaBuffer { address: u32, bytes: &'static mut [u8] }
impl DmaBuffer {
    /// # Safety
    /// `address` maps `bytes` in the host DMA address space. The mapping remains
    /// valid permanently, including after a transaction is dropped.
    pub unsafe fn from_mapping(address: u32, bytes: &'static mut [u8]) -> Result<Self, AdmissionError> {
        if address & 31 != 0 { return Err(AdmissionError::DmaAlignment); }
        if u32::try_from(bytes.len()).ok().and_then(|len| address.checked_add(len)).is_none() {
            return Err(AdmissionError::BufferLength);
        }
        Ok(Self { address, bytes })
    }
    pub fn bytes(&self) -> &[u8] { self.bytes }
}

pub enum Buffer<'a> { None, Fifo(&'a mut [u8]), Dma(DmaBuffer) }
pub struct Request<'a> {
    pub operation: Operation,
    pub argument: Argument,
    pub execution: Execution,
    pub buffer: Buffer<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError { BufferLength, DmaAlignment, Busy }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VendorCode { Success = 0, Invalid = 3, Timeout = 5, HardwareError = 7, CommandError = 8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalResult { Completed, TimedOut, Error, Unconfirmed, Cancelled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    pub code: VendorCode,
    pub physical: PhysicalResult,
    pub transferred: u16,
    pub status: u32,
    pub response: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode { Direct, DmaRead, DmaWrite, FifoRead, FifoWrite }
impl Mode {
    fn register_options(self) -> u16 {
        match self { Self::DmaRead | Self::DmaWrite => 10, _ => 1 }
    }
    fn first_done_mask(self) -> u16 {
        match self { Self::Direct | Self::FifoRead | Self::FifoWrite => 1, _ => 2 }
    }
    fn dma(self) -> bool { matches!(self, Self::DmaRead | Self::DmaWrite) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, SecondDma, FirstInterrupt, FirstPolling, Fifo, Ready, Cancelling, Finished }

pub struct Host {
    unit: TransferUnit,
    enabled: bool,
    notification_mask: u16,
    phase: Phase,
    mode: Mode,
    execution: Execution,
    command: u32,
    argument: Argument,
    total: usize,
    offset: usize,
    first_mask: u16,
    error_mask: u16,
    irq_status: u16,
    irq_errors: u16,
    notified: bool,
    started: Milliseconds,
    completion: Completion,
}
impl Host {
    pub const fn new(unit: TransferUnit) -> Self {
        Self { unit, enabled: false, notification_mask: 0, phase: Phase::Finished,
            mode: Mode::Direct, execution: Execution::Polling, command: 0,
            argument: Argument(0), total: 0, offset: 0, first_mask: 0,
            error_mask: 0, irq_status: 0, irq_errors: 0, notified: false,
            started: Milliseconds(0), completion: Completion { code: VendorCode::Success,
                physical: PhysicalResult::Unconfirmed, transferred: 0, status: 0, response: 0 } }
    }
    pub fn set_unit(&mut self, unit: TransferUnit) -> Result<(), AdmissionError> {
        if self.phase != Phase::Finished { return Err(AdmissionError::Busy); }
        self.unit = unit;
        Ok(())
    }
    pub fn reap_cancelled(&mut self, mmio: &mut impl Mmio) -> Poll<Completion> {
        if self.phase != Phase::Cancelling { return Poll::Pending; }
        if mmio.read_word(WordRegister::Present) & 3 != 0
            || mmio.read_half(HalfRegister::Remaining) != 0 { return Poll::Pending; }
        self.end(mmio);
        self.completion.physical = PhysicalResult::Cancelled;
        self.phase = Phase::Finished;
        Poll::Ready(self.completion)
    }
    pub fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
    pub fn set_notifications(&mut self, insertion: bool, removal: bool, card_interrupt: bool) {
        self.notification_mask = (if insertion { 0x40 } else { 0 })
            | (if removal { 0x80 } else { 0 }) | (if card_interrupt { 0x100 } else { 0 });
    }
    pub fn begin<'a, M: Mmio>(
        &'a mut self, mmio: &'a mut M, cache: &mut impl CacheInstructions,
        request: Request<'a>, now: Milliseconds,
    ) -> Result<Transaction<'a, M>, AdmissionError> {
        if self.phase != Phase::Finished { return Err(AdmissionError::Busy); }
        let count = request.argument.count();
        let total = match request.operation {
            Operation::Direct52 => 0,
            Operation::Extended53 if request.argument.block() => usize::from(self.unit.0) * count,
            Operation::Extended53 => if count == 0 { 512 } else { count },
        };
        let (length, dma) = match &request.buffer {
            Buffer::None => (0, false), Buffer::Fifo(bytes) => (bytes.len(), false),
            Buffer::Dma(buffer) => (buffer.bytes.len(), true),
        };
        if length < total { return Err(AdmissionError::BufferLength); }
        self.mode = match request.operation {
            Operation::Direct52 => Mode::Direct,
            Operation::Extended53 => match (dma, request.argument.write()) {
                (true, false) => Mode::DmaRead, (true, true) => Mode::DmaWrite,
                (false, false) => Mode::FifoRead, (false, true) => Mode::FifoWrite,
            },
        };
        self.command = match request.operation {
            Operation::Direct52 => if request.argument.write() { 0x341a_0000 } else { 0x341a_0010 },
            Operation::Extended53 => 0x353a_0000 | (if request.argument.block() { 0x22 } else { 0 })
                | (if dma { 1 } else { 0 }) | (if request.argument.write() { 0 } else { 0x10 }),
        };
        self.argument = request.argument;
        self.total = total;
        self.offset = 0;
        self.execution = request.execution;
        self.started = now;
        self.notified = false;
        self.irq_status = 0;
        self.irq_errors = 0;
        self.completion = Completion { code: VendorCode::Success, physical: PhysicalResult::Unconfirmed,
            transferred: 0, status: 0, response: 0 };
        if request.operation == Operation::Extended53 {
            mmio.write_half(HalfRegister::Remaining, if request.argument.block() { count as u16 } else { 0 });
            let unit = if request.argument.block() { self.unit.0 } else { total as u16 };
            let length = mmio.read_half(HalfRegister::Length);
            mmio.write_half(HalfRegister::Length, (length & !0xfff) | (unit & 0xfff));
            let config = mmio.read_half(HalfRegister::Config);
            mmio.write_half(HalfRegister::Config, (config & !2) | if self.enabled { 2 } else { 0 });
        }
        if let Buffer::Dma(buffer) = &request.buffer {
            if request.operation == Operation::Extended53 {
                let config = mmio.read_half(HalfRegister::Config);
                mmio.write_half(HalfRegister::Config, config & !0x18);
                mmio.write_word(WordRegister::DmaAddress, buffer.address);
                clean_invalidate_all(cache);
            }
        }
        self.prepare(mmio);
        self.phase = Phase::Idle;
        Ok(Transaction { host: self, mmio, buffer: request.buffer })
    }
    fn prepare(&mut self, mmio: &mut impl Mmio) {
        mmio.write_half(HalfRegister::Status, 0xffff);
        let mask = match self.execution {
            Execution::Interrupt => 0xee00 | self.mode.register_options(),
            Execution::Polling => 0xffff,
        };
        mmio.write_half(HalfRegister::StatusMask, mask);
        match self.execution {
            Execution::Interrupt => {
                mmio.write_half(HalfRegister::StatusSignal, mask);
                mmio.write_half(HalfRegister::ErrorSignal, 0x17ff);
            }
            Execution::Polling => (),
        }
        mmio.write_half(HalfRegister::Errors, 0xffff);
        mmio.write_half(HalfRegister::ErrorMask, 0xffff);
        self.first_mask = mmio.read_half(HalfRegister::StatusMask);
        self.error_mask = mmio.read_half(HalfRegister::ErrorMask);
    }
    fn end(&mut self, mmio: &mut impl Mmio) {
        mmio.write_half(HalfRegister::Status, 0xffff);
        mmio.write_half(HalfRegister::StatusMask, self.notification_mask);
        mmio.write_half(HalfRegister::StatusSignal, self.notification_mask);
        mmio.write_half(HalfRegister::Errors, 0xffff);
        mmio.write_half(HalfRegister::ErrorMask, 0);
        mmio.write_half(HalfRegister::ErrorSignal, 0);
        self.notified = false;
    }
    fn pending_result(&mut self, pending: u16) -> VendorCode {
        if pending & self.error_mask & 0x17ff == 0 {
            self.completion.transferred = 0;
            VendorCode::Success
        } else {
            self.completion.transferred = pending;
            if pending & 1 != 0 { VendorCode::Timeout } else { VendorCode::HardwareError }
        }
    }
    fn command_finished(&mut self, mmio: &mut impl Mmio, code: VendorCode, physical: PhysicalResult, now: Milliseconds) {
        self.end(mmio);
        self.completion.code = code;
        self.completion.physical = physical;
        if !self.mode.dma() && self.total != 0 && code == VendorCode::Success {
            mmio.write_half(HalfRegister::Status, 0xffff);
            mmio.write_half(HalfRegister::StatusMask, 0xffff);
            mmio.write_half(HalfRegister::StatusSignal, 0);
            self.started = now;
            self.phase = Phase::Fifo;
        } else { self.finish(mmio); }
    }
    fn finish(&mut self, mmio: &mut impl Mmio) {
        if self.completion.code == VendorCode::Success {
            let packed = u32::from(mmio.response_half(0)) | (u32::from(mmio.response_half(1)) << 16);
            self.completion.response = packed as u8;
            self.completion.status = ((packed & 0xc800) << 8)
                | if packed & 0x300 != 0 { 0x8000_0000 } else { 0 };
            if self.completion.status != 0 {
                self.completion.code = VendorCode::CommandError;
                self.completion.physical = PhysicalResult::Error;
            }
        }
        self.phase = Phase::Finished;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Notifications { pub inserted: bool, pub removed: bool, pub card_interrupt: bool }

impl Host {
    pub fn interrupt(&mut self, mmio: &mut impl Mmio) -> Notifications {
        let status = mmio.read_half(HalfRegister::Status);
        let errors = mmio.read_half(HalfRegister::Errors);
        self.irq_status = status;
        self.irq_errors = errors;
        let mut mask = mmio.read_half(HalfRegister::StatusMask);
        let old_mask = mask;
        mask &= !0x1c0;
        mmio.write_half(HalfRegister::StatusMask, mask);
        let old_errors = mmio.read_half(HalfRegister::ErrorMask);
        mmio.write_half(HalfRegister::ErrorMask, 0);
        mmio.write_half(HalfRegister::Status, status & 0xc0);
        let signalled = old_errors & errors & 0x17ff != 0 || old_mask & status & 0x3b != 0;
        let mut restored_errors = old_errors;
        if signalled {
            if self.phase == Phase::FirstInterrupt {
                let command = mmio.read_word(WordRegister::Command);
                if command & 0x3f00_0001 == 0x3500_0001
                    && (mmio.read_half(HalfRegister::Remaining) != 0 || status & 2 == 0)
                {
                    let status = mmio.read_half(HalfRegister::Status);
                    mmio.write_half(HalfRegister::Status, status | 8);
                    let address = mmio.read_word(WordRegister::DmaAddress);
                    mmio.write_word(WordRegister::DmaAddress, address);
                } else {
                    self.notified = true;
                    mask = mmio.read_half(HalfRegister::StatusMask) & !0x3b;
                    mmio.write_half(HalfRegister::StatusMask, mask);
                    mmio.write_half(HalfRegister::ErrorMask, 0);
                }
                mask = mmio.read_half(HalfRegister::StatusMask);
                restored_errors = mmio.read_half(HalfRegister::ErrorMask);
            } else {
                mask &= !0x3b;
                restored_errors = 0;
            }
        }
        mmio.write_half(HalfRegister::StatusMask, mask | self.notification_mask);
        mmio.write_half(HalfRegister::ErrorMask, restored_errors);
        Notifications { inserted: status & 0x40 != 0, removed: status & 0x80 != 0,
            card_interrupt: status & 0x100 != 0 }
    }
}

pub struct Transaction<'a, M: Mmio> { host: &'a mut Host, mmio: &'a mut M, buffer: Buffer<'a> }
impl<M: Mmio> Transaction<'_, M> {
    pub fn interrupt(&mut self) -> Notifications { self.host.interrupt(self.mmio) }
    pub fn poll(&mut self, now: Milliseconds) -> Poll<Completion> {
        let host = &mut *self.host;
        match host.phase {
            Phase::Idle => {
                if self.mmio.read_word(WordRegister::Present) & 3 != 0 { return Poll::Pending; }
                self.mmio.write_half(HalfRegister::ArgumentLow, host.argument.0 as u16);
                self.mmio.write_half(HalfRegister::ArgumentHigh, (host.argument.0 >> 16) as u16);
                self.mmio.write_word(WordRegister::Command, host.command);
                host.started = now;
                host.phase = match host.execution {
                    Execution::Interrupt => Phase::FirstInterrupt,
                    Execution::Polling if host.mode.dma() => Phase::SecondDma,
                    Execution::Polling => Phase::FirstPolling,
                };
            }
            Phase::SecondDma => {
                let status = self.mmio.read_half(HalfRegister::Status);
                if self.mmio.read_half(HalfRegister::Remaining) == 0 {
                    host.phase = Phase::FirstPolling;
                    host.started = now;
                } else {
                    if status & 8 != 0 {
                        self.mmio.write_half(HalfRegister::Status, status | 8);
                        let address = self.mmio.read_word(WordRegister::DmaAddress);
                        self.mmio.write_word(WordRegister::DmaAddress, address);
                    }
                    if now.elapsed_since(host.started) >= 500 {
                        host.command_finished(self.mmio, VendorCode::Timeout, PhysicalResult::TimedOut, now);
                    }
                }
            }
            Phase::FirstPolling => {
                let status = self.mmio.read_half(HalfRegister::Status);
                let errors = self.mmio.read_half(HalfRegister::Errors);
                let done = status & host.mode.first_done_mask() != 0;
                if done || errors & 0x17ff != 0 {
                    host.completion.transferred = errors;
                    let pending = self.mmio.read_half(HalfRegister::Errors);
                    let code = host.pending_result(pending);
                    host.command_finished(self.mmio, code,
                        if errors & 0x17ff != 0 { PhysicalResult::Error } else { PhysicalResult::Completed }, now);
                } else if now.elapsed_since(host.started) >= 500 {
                    host.completion.transferred = errors;
                    host.command_finished(self.mmio, VendorCode::Timeout, PhysicalResult::TimedOut, now);
                }
            }
            Phase::FirstInterrupt => {
                if host.notified {
                    let physical = if host.irq_errors & 0x17ff != 0 { PhysicalResult::Error }
                        else if host.first_mask & host.irq_status & 0x3b != 0 { PhysicalResult::Completed }
                        else { PhysicalResult::Unconfirmed };
                    let wait_code = if host.first_mask & host.irq_status & 0x3b != 0 {
                        VendorCode::Success
                    } else if host.irq_errors & 1 != 0 { VendorCode::Timeout }
                    else { VendorCode::CommandError };
                    let code = if wait_code == VendorCode::Timeout { wait_code }
                        else { host.pending_result(host.irq_errors) };
                    host.command_finished(self.mmio, code, physical, now);
                } else if now.elapsed_since(host.started) >= 500 {
                    host.command_finished(self.mmio, VendorCode::Timeout, PhysicalResult::TimedOut, now);
                }
            }
            Phase::Fifo => {
                let ready_mask = if host.mode == Mode::FifoRead { 0x800 } else { 0x400 };
                if self.mmio.read_word(WordRegister::Present) & ready_mask != 0 {
                    if let Buffer::Fifo(bytes) = &mut self.buffer {
                        let unit = usize::from(host.unit.0);
                        if host.mode == Mode::FifoRead {
                            let mut consumed = 0;
                            while consumed < unit {
                                let word = self.mmio.read_word(WordRegister::Fifo).to_le_bytes();
                                let count = (unit - consumed).min(4);
                                for byte in &word[..count] {
                                    if host.offset < host.total { bytes[host.offset] = *byte; host.offset += 1; }
                                }
                                consumed += count;
                            }
                        } else {
                            let end = host.total.min(host.offset + unit);
                            let mut word = [0u8; 4];
                            while host.offset < end {
                                let count = (end - host.offset).min(4);
                                word[..count].copy_from_slice(&bytes[host.offset..host.offset + count]);
                                host.offset += count;
                                self.mmio.write_word(WordRegister::Fifo, u32::from_le_bytes(word));
                            }
                        }
                    }
                    host.started = now;
                    if host.offset == host.total { host.phase = Phase::Ready; }
                } else if now.elapsed_since(host.started) >= if host.mode == Mode::FifoRead { 1000 } else { 500 } {
                    host.completion.code = VendorCode::Timeout;
                    host.completion.physical = PhysicalResult::TimedOut;
                    host.end(self.mmio);
                    host.finish(self.mmio);
                }
            }
            Phase::Ready => {
                if self.mmio.read_half(HalfRegister::Status) & 2 != 0 {
                    host.end(self.mmio);
                    host.finish(self.mmio);
                } else if now.elapsed_since(host.started) >= 500 {
                    host.completion.code = VendorCode::Timeout;
                    host.completion.physical = PhysicalResult::TimedOut;
                    host.end(self.mmio);
                    host.finish(self.mmio);
                }
            }
            Phase::Cancelling => {
                if self.mmio.read_word(WordRegister::Present) & 3 == 0
                    && self.mmio.read_half(HalfRegister::Remaining) == 0 {
                    host.phase = Phase::Finished;
                }
            }
            Phase::Finished => (),
        }
        if host.phase == Phase::Finished { Poll::Ready(host.completion) } else { Poll::Pending }
    }
    pub fn cancel(&mut self) {
        if self.host.phase == Phase::Finished { return; }
        self.host.end(self.mmio);
        self.host.completion.physical = PhysicalResult::Cancelled;
        self.host.phase = Phase::Cancelling;
    }
    pub fn buffer(&self) -> &[u8] {
        match &self.buffer { Buffer::None => &[], Buffer::Fifo(bytes) => bytes, Buffer::Dma(buffer) => buffer.bytes() }
    }
}
impl<M: Mmio> Drop for Transaction<'_, M> {
    fn drop(&mut self) {
        if self.host.phase != Phase::Finished {
            self.host.end(self.mmio);
            self.host.phase = Phase::Cancelling;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockSelection { Stopped, Mode1, Mode2, Mode3, Mode4, Mode5, Mode6, Mode7, Mode8, Mode9, Mode10, Mode11 }
impl ClockSelection {
    pub const fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Stopped), 1 => Some(Self::Mode1), 2 => Some(Self::Mode2),
            3 => Some(Self::Mode3), 4 => Some(Self::Mode4), 5 => Some(Self::Mode5),
            6 => Some(Self::Mode6), 7 => Some(Self::Mode7), 8 => Some(Self::Mode8),
            9 => Some(Self::Mode9), 10 => Some(Self::Mode10), 11 => Some(Self::Mode11),
            _ => None,
        }
    }
    const fn divider(self) -> u16 {
        match self {
            Self::Stopped | Self::Mode10 | Self::Mode11 => 0, Self::Mode1 => 0x40,
            Self::Mode2 => 0x8000, Self::Mode3 => 0x4000, Self::Mode4 => 0x2000,
            Self::Mode5 => 0x1000, Self::Mode6 => 0x800, Self::Mode7 => 0x400,
            Self::Mode8 => 0x200, Self::Mode9 => 0x100,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockRegistration(pub u32);
pub trait ClockPower {
    fn unregister(&mut self, registration: ClockRegistration) -> bool;
    fn set_gate(&mut self, enabled: bool);
    fn register(&mut self) -> Option<ClockRegistration>;
    fn select_source(&mut self, high: bool);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockProgress { WaitingForStable, WaitingForEnabled, Complete }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockDelayTicks(u32);
impl ClockDelayTicks { pub const fn get(self) -> u32 { self.0 } }
impl ClockProgress {
    pub const fn delay_ticks(self) -> Option<ClockDelayTicks> {
        match self { Self::WaitingForEnabled => Some(ClockDelayTicks(10)), _ => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardInterruptRoute { Removed, Wireless }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevicePort { Zero }
impl DevicePort {
    pub const fn parse(value: u8) -> Result<Self, ControlError> {
        if value == 0 { Ok(Self::Zero) } else { Err(ControlError::Invalid) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ControlError { Unavailable = 1, Disconnected = 2, Invalid = 3, Os = 6, Busy = 9 }
pub trait InterruptRouter {
    fn remove(&mut self) -> Result<(), ControlError>;
    fn install(&mut self) -> Result<(), ControlError>;
}
pub trait OperationLock {
    fn acquire(&mut self) -> Result<(), ControlError>;
    fn release(&mut self) -> Result<(), ControlError>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeginError { Control(ControlError), Transfer(AdmissionError) }
pub struct ControlledTransaction<'a, M: Mmio, L: OperationLock> {
    transfer: Option<Transaction<'a, M>>,
    lock: &'a mut L,
}
impl<M: Mmio, L: OperationLock> ControlledTransaction<'_, M, L> {
    pub fn poll(&mut self, now: Milliseconds) -> Poll<Completion> {
        self.transfer.as_mut().unwrap().poll(now)
    }
    pub fn interrupt(&mut self) -> Notifications { self.transfer.as_mut().unwrap().interrupt() }
    pub fn cancel(&mut self) { self.transfer.as_mut().unwrap().cancel(); }
    pub fn buffer(&self) -> &[u8] { self.transfer.as_ref().unwrap().buffer() }
    pub fn finish(mut self) -> Result<(), ControlError> {
        drop(self.transfer.take());
        self.lock.release()
    }
}
impl<M: Mmio, L: OperationLock> Drop for ControlledTransaction<'_, M, L> {
    fn drop(&mut self) {
        if let Some(transfer) = self.transfer.take() {
            drop(transfer);
            let _ = self.lock.release();
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClockPhase { Idle, Stable, Enabled }

pub struct ControlPlane {
    initialized: bool,
    available: bool,
    connected: bool,
    route: CardInterruptRoute,
    clock: ClockSelection,
    clock_phase: ClockPhase,
    registration: Option<ClockRegistration>,
}
impl Default for ControlPlane { fn default() -> Self { Self::new() } }
impl ControlPlane {
    pub const fn new() -> Self {
        Self { initialized: false, available: false, connected: false,
            route: CardInterruptRoute::Removed, clock: ClockSelection::Stopped,
            clock_phase: ClockPhase::Idle, registration: None }
    }
    pub fn initialize(&mut self) { self.initialized = true; }
    pub fn publish_available(&mut self, available: bool) { self.available = available; }
    pub fn publish_connected(&mut self, connected: bool) { self.connected = connected; }
    pub fn admit(&self, _: DevicePort, _: Operation) -> Result<(), ControlError> {
        if !self.initialized { return Err(ControlError::Disconnected); }
        if !self.available { return Err(ControlError::Unavailable); }
        if !self.connected { return Err(ControlError::Disconnected); }
        Ok(())
    }
    pub fn begin<'a, M: Mmio, L: OperationLock>(&self, port: DevicePort,
        host: &'a mut Host, mmio: &'a mut M, cache: &mut impl CacheInstructions,
        lock: &'a mut L, request: Request<'a>, now: Milliseconds,
    ) -> Result<ControlledTransaction<'a, M, L>, BeginError> {
        if !self.initialized { return Err(BeginError::Control(ControlError::Disconnected)); }
        lock.acquire().map_err(BeginError::Control)?;
        if let Err(error) = self.admit(port, request.operation) {
            let _ = lock.release();
            return Err(BeginError::Control(error));
        }
        match host.begin(mmio, cache, request, now) {
            Ok(transfer) => Ok(ControlledTransaction { transfer: Some(transfer), lock }),
            Err(error) => {
                let _ = lock.release();
                Err(BeginError::Transfer(error))
            }
        }
    }
    pub fn set_transfer_enabled(&mut self, host: &mut Host, mmio: &mut impl Mmio, enabled: bool) {
        let config = mmio.read_half(HalfRegister::Config);
        mmio.write_half(HalfRegister::Config, (config & !2) | if enabled { 2 } else { 0 });
        host.set_enabled(enabled);
    }
    pub fn set_transfer_unit(&mut self, host: &mut Host, mmio: &mut impl Mmio, unit: TransferUnit) -> Result<(), AdmissionError> {
        host.set_unit(unit)?;
        let length = mmio.read_half(HalfRegister::Length);
        mmio.write_half(HalfRegister::Length, (length & !0xfff) | unit.get());
        Ok(())
    }
    pub fn set_card_interrupt(&mut self, host: &mut Host, mmio: &mut impl Mmio,
        router: &mut impl InterruptRouter, route: CardInterruptRoute) -> Result<(), ControlError> {
        router.remove()?;
        let signal = mmio.read_half(HalfRegister::StatusSignal) & !0x100;
        mmio.write_half(HalfRegister::StatusSignal, signal);
        self.route = route;
        host.notification_mask = (host.notification_mask & !0x100)
            | if route == CardInterruptRoute::Wireless { 0x100 } else { 0 };
        if route == CardInterruptRoute::Wireless {
            mmio.write_half(HalfRegister::Status, signal | 0x100);
            mmio.write_half(HalfRegister::StatusSignal, signal | 0x100);
        }
        router.install()
    }
    pub fn card_interrupt_route(&self) -> CardInterruptRoute { self.route }
    pub fn start_clock(&mut self, mmio: &mut impl Mmio, power: &mut impl ClockPower,
        selection: ClockSelection) -> Result<ClockProgress, ControlError> {
        if self.clock_phase != ClockPhase::Idle { return Err(ControlError::Busy); }
        if selection == ClockSelection::Stopped || self.registration.is_some() {
            mmio.write_half(HalfRegister::Clock, 1);
            if let Some(registration) = self.registration {
                if power.unregister(registration) { self.registration = None; }
            }
            power.set_gate(false);
        }
        self.clock = selection;
        if selection == ClockSelection::Stopped { return Ok(ClockProgress::Complete); }
        power.select_source(selection != ClockSelection::Mode11);
        power.set_gate(true);
        if let Some(registration) = power.register() { self.registration = Some(registration); }
        mmio.write_half(HalfRegister::Clock, selection.divider() | 1);
        self.clock_phase = ClockPhase::Stable;
        Ok(self.poll_clock(mmio))
    }
    pub fn poll_clock(&mut self, mmio: &mut impl Mmio) -> ClockProgress {
        let clock = mmio.read_half(HalfRegister::Clock);
        match self.clock_phase {
            ClockPhase::Idle => ClockProgress::Complete,
            ClockPhase::Stable if clock & 2 == 0 => ClockProgress::WaitingForStable,
            ClockPhase::Stable | ClockPhase::Enabled => {
                mmio.write_half(HalfRegister::Clock, clock | 4);
                if mmio.read_half(HalfRegister::Clock) & 4 == 0 {
                    self.clock_phase = ClockPhase::Enabled;
                    ClockProgress::WaitingForEnabled
                } else {
                    self.clock_phase = ClockPhase::Idle;
                    ClockProgress::Complete
                }
            }
        }
    }
}

#[cfg(target_arch = "arm")]
pub struct NativeClockPower { _exclusive: () }
#[cfg(target_arch = "arm")]
impl NativeClockPower {
    /// # Safety
    /// The caller owns the SD clock source and initialized power resource registry.
    pub unsafe fn acquire() -> Self { Self { _exclusive: () } }
}
#[cfg(target_arch = "arm")]
impl ClockPower for NativeClockPower {
    fn unregister(&mut self, registration: ClockRegistration) -> bool {
        unsafe { xm1_power_resource_release(registration.0) == 0 }
    }
    fn set_gate(&mut self, enabled: bool) {
        unsafe { xm1_power_resource_route_request(0x70a, 28, if enabled { 11 } else { 0 }); }
    }
    fn register(&mut self) -> Option<ClockRegistration> {
        let mut handle = 0;
        if unsafe { xm1_power_resource_acquire(33, &mut handle) } == 0 {
            Some(ClockRegistration(handle))
        } else { None }
    }
    fn select_source(&mut self, high: bool) {
        unsafe {
            let saved = xm1_power_interrupt_lock();
            let register = 0xfff8_8028 as *mut u32;
            let source = core::ptr::read_volatile(register);
            core::ptr::write_volatile(register, (source & !(1 << 20)) | (u32::from(high) << 20));
            xm1_power_interrupt_unlock(saved);
        }
    }
}
#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_resource_release(handle: u32) -> i32;
    fn xm1_power_resource_acquire(resource: u32, output: *mut u32) -> i32;
    fn xm1_power_resource_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_rtos_own_first_slot_1618() -> u32;
    fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(id: i32) -> i32;
}

#[cfg(target_arch = "arm")]
pub struct NativeOperationLock { semaphore: i32 }
#[cfg(target_arch = "arm")]
impl NativeOperationLock {
    /// # Safety
    /// Overlay47's semaphore publication at 0x0128f46c is initialized.
    pub unsafe fn adopt() -> Self {
        Self { semaphore: unsafe { core::ptr::read_volatile(0x0128_f46c as *const i32) } }
    }
}
#[cfg(target_arch = "arm")]
impl OperationLock for NativeOperationLock {
    fn acquire(&mut self) -> Result<(), ControlError> {
        if unsafe { xm1_power_semaphore_wait(self.semaphore, -1) } < 0 { Err(ControlError::Os) } else { Ok(()) }
    }
    fn release(&mut self) -> Result<(), ControlError> {
        if unsafe { xm1_power_semaphore_signal(self.semaphore) } != 0 { Err(ControlError::Os) } else { Ok(()) }
    }
}
#[cfg(target_arch = "arm")]
pub struct NativeInterruptRouter { handler: unsafe extern "C" fn() }
#[cfg(target_arch = "arm")]
impl NativeInterruptRouter {
    /// # Safety
    /// `handler` routes IRQ79 into the exclusively owned Host and wireless notification consumer.
    pub unsafe fn acquire(handler: unsafe extern "C" fn()) -> Self { Self { handler } }
    fn configure(&mut self, handler: u32, priority: u32, polarity: u32) -> Result<(), ControlError> {
        let interface = unsafe {
            let index = xm1_rtos_own_first_slot_1618();
            core::ptr::read_volatile((0x0096_bd20 + index * 4) as *const u32)
        };
        let configure: unsafe extern "C" fn(u32, *const u32) -> i32 = callbacks()
            .resolve(CallbackWord(unsafe { core::ptr::read_volatile((interface + 0x160) as *const u32) }))
            .map_err(|_| ControlError::Os)?;
        let enable: unsafe extern "C" fn(u32) -> i32 = callbacks()
            .resolve(CallbackWord(unsafe { core::ptr::read_volatile((interface + 0x158) as *const u32) }))
            .map_err(|_| ControlError::Os)?;
        let record = [0, handler, priority, polarity, 2];
        let saved = unsafe { xm1_power_interrupt_lock() };
        unsafe {
            configure(79, record.as_ptr());
            enable(79);
            xm1_power_interrupt_unlock(saved);
        }
        Ok(())
    }
}
#[cfg(target_arch = "arm")]
impl InterruptRouter for NativeInterruptRouter {
    fn remove(&mut self) -> Result<(), ControlError> { self.configure(0, 0, 1) }
    fn install(&mut self) -> Result<(), ControlError> { self.configure(self.handler as usize as u32, 3, 0) }
}

#[cfg(target_arch = "arm")]
pub struct NativeSdController {
    pub control: ControlPlane,
    pub host: Host,
    pub mmio: NativeMmio,
    pub clock: NativeClockPower,
    pub interrupts: NativeInterruptRouter,
    pub lock: NativeOperationLock,
}
#[cfg(target_arch = "arm")]
impl NativeSdController {
    /// # Safety
    /// The caller exclusively owns overlay47's initialized SD publications, MMIO, IRQ79 and DMA mappings.
    /// `handler` enters Host::interrupt and forwards its notifications to the owned wireless consumer.
    pub unsafe fn adopt(handler: unsafe extern "C" fn()) -> Result<Self, ControlError> {
        unsafe {
            let read = |address: u32| core::ptr::read_volatile(address as *const u32);
            if core::ptr::read_volatile(0x0146_b854 as *const u8) != 0 { return Err(ControlError::Invalid); }
            let mut control = ControlPlane::new();
            control.initialized = read(0x0146_b858) == 1;
            control.connected = read(0x0146_b890) == 1;
            control.clock = ClockSelection::from_wire(read(0x0146_b878)).ok_or(ControlError::Invalid)?;
            let registration = read(0x0146_b880);
            control.registration = if registration == u32::MAX { None } else { Some(ClockRegistration(registration)) };
            let unit = u16::try_from(read(0x0146_b8a4)).ok().and_then(TransferUnit::new).ok_or(ControlError::Invalid)?;
            let mut host = Host::new(unit);
            host.set_enabled(read(0x0146_b8a8) == 1);
            let card_interrupt = read(0x0146_b868) != 0;
            host.set_notifications(read(0x0146_b85c) != 0, read(0x0146_b860) != 0, card_interrupt);
            control.route = if card_interrupt { CardInterruptRoute::Wireless } else { CardInterruptRoute::Removed };
            let mut mmio = NativeMmio::acquire();
            control.available = mmio.read_word(WordRegister::Present) & 0x10000 != 0;
            Ok(Self { control, host, mmio, clock: NativeClockPower::acquire(),
                interrupts: NativeInterruptRouter::acquire(handler), lock: NativeOperationLock::adopt() })
        }
    }
    pub fn refresh_card_presence(&mut self) {
        self.control.available = self.mmio.read_word(WordRegister::Present) & 0x10000 != 0;
    }
    pub fn begin<'a>(&'a mut self, cache: &mut impl CacheInstructions, request: Request<'a>,
        now: Milliseconds) -> Result<ControlledTransaction<'a, NativeMmio, NativeOperationLock>, BeginError> {
        self.refresh_card_presence();
        self.control.begin(DevicePort::Zero, &mut self.host, &mut self.mmio,
            cache, &mut self.lock, request, now)
    }
    pub fn interrupt(&mut self) -> Notifications { self.host.interrupt(&mut self.mmio) }
}

#[cfg(test)]
#[path = "sd_controller_tests.rs"]
mod tests;
