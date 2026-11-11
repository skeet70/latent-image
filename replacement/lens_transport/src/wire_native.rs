#![cfg(target_arch = "arm")]

use core::arch::asm;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicUsize, Ordering};
use super::wire::{Address, CpuBackend, InterruptController, LensEndpoints, Mmio, Platform,
                  RequestedOutput, TimerInterrupt, TimerToken, TransferToken};

pub type NativeBackend = CpuBackend<NativeMmio, NativePlatform, NativeInterrupts>;

#[derive(Clone, Copy)]
pub struct SavedInterrupts(u32);

extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_resource_route_request(selector: u32, slot: u32, level: u32) -> i32;
}

pub fn enter_critical() -> SavedInterrupts {
    SavedInterrupts(unsafe { xm1_power_interrupt_lock() })
}

pub fn leave_critical(saved: SavedInterrupts) {
    unsafe { xm1_power_interrupt_unlock(saved.0) };
}

fn load(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn store(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

pub struct NativeMmio { _exclusive: () }
impl Mmio for NativeMmio {
    type SavedInterrupts = SavedInterrupts;
    fn read_word(&self, address: Address) -> u32 { load(address.get()) }
    fn write_word(&self, address: Address, value: u32) { store(address.get(), value); }
    fn read_byte(&self, address: Address) -> u8 {
        unsafe { read_volatile(address.get() as *const u8) }
    }
    fn write_byte(&self, address: Address, value: u8) {
        unsafe { write_volatile(address.get() as *mut u8, value) }
    }
    fn mask_interrupts(&self) -> SavedInterrupts { enter_critical() }
    fn restore_interrupts(&self, saved: SavedInterrupts) { leave_critical(saved); }
    fn publish_dma(&self) {
        let saved = enter_critical();
        clean_invalidate_data_cache();
        leave_critical(saved);
    }
}

// The DMA start primitive at 0x00042634 selects whole-cache clean/invalidate.
fn clean_invalidate_data_cache() {
    let clidr: u32;
    let original_selector: u32;
    unsafe {
        asm!("dsb sy", "mrc p15, 1, {clidr}, c0, c0, 1",
             "mrc p15, 2, {selector}, c0, c0, 0",
             clidr = out(reg) clidr, selector = out(reg) original_selector,
             options(nostack, preserves_flags));
    }
    for level in 0..((clidr >> 24) & 7) {
        let cache_type = (clidr >> (level * 3)) & 7;
        if cache_type < 2 { continue; }
        let ccsidr: u32;
        unsafe {
            asm!("mcr p15, 2, {selector}, c0, c0, 0", "isb sy",
                 "mrc p15, 1, {geometry}, c0, c0, 0",
                 selector = in(reg) (level << 1), geometry = out(reg) ccsidr,
                 options(nostack, preserves_flags));
        }
        let line_shift = (ccsidr & 7) + 4;
        let last_way = (ccsidr >> 3) & 0x3ff;
        let last_set = (ccsidr >> 13) & 0x7fff;
        let way_shift = last_way.leading_zeros();
        for way in (0..=last_way).rev() {
            for set in (0..=last_set).rev() {
                let way_bits = if last_way == 0 { 0 } else { way << way_shift };
                let operand = (level << 1) | way_bits | (set << line_shift);
                unsafe {
                    asm!("mcr p15, 0, {operand}, c7, c14, 2", operand = in(reg) operand,
                         options(nostack, preserves_flags));
                }
            }
        }
    }
    unsafe {
        asm!("dsb sy", "mcr p15, 2, {selector}, c0, c0, 0", "isb sy",
             selector = in(reg) original_selector, options(nostack, preserves_flags));
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum PlatformError { AlreadyLeased, NotLeased }

static mut LEASED: bool = false;

pub struct NativePlatform { _exclusive: () }
impl NativePlatform {
    /// # Safety
    /// Reserve serial 2, DMA 2/3, timer 11, IRQ 31/94 and 0x0096d000..0x0096e040 against every other core and allocator. Initialize the shared output state at 0x409c1e30 and the diagnostic downcounter at 0xfff91000/004 before use.
    pub const unsafe fn new_exclusive() -> Self { Self { _exclusive: () } }
}

impl Platform for NativePlatform {
    type Error = PlatformError;
    fn acquire(&mut self, _: LensEndpoints) -> Result<(), PlatformError> {
        let saved = enter_critical();
        let occupied = unsafe { read_volatile(addr_of!(LEASED)) };
        if !occupied { unsafe { write_volatile(addr_of_mut!(LEASED), true) }; }
        leave_critical(saved);
        if occupied { Err(PlatformError::AlreadyLeased) } else { Ok(()) }
    }
    fn release(&mut self, _: LensEndpoints) -> Result<(), PlatformError> {
        let saved = enter_critical();
        let occupied = unsafe { read_volatile(addr_of!(LEASED)) };
        if occupied { unsafe { write_volatile(addr_of_mut!(LEASED), false) }; }
        leave_critical(saved);
        if occupied { Ok(()) } else { Err(PlatformError::NotLeased) }
    }
    fn request_output(&mut self, level: RequestedOutput) -> Result<(), PlatformError> {
        let saved = enter_critical();
        let result = request_lens_output(level);
        leave_critical(saved);
        result
    }
}

fn request_lens_output(level: RequestedOutput) -> Result<(), PlatformError> {
    const STATE: u32 = 0x409c_1e30;
    store(STATE + 4 * (17 + 2), match level { RequestedOutput::Inactive => 0, RequestedOutput::Active => 8 });
    let mut selected = load(STATE + 4);
    for requester in 0..19 { selected = selected.max(load(STATE + 4 * (requester + 2))); }
    selected = selected.min(load(STATE));
    unsafe {
        xm1_power_resource_route_request(0, 0, selected);
        xm1_power_resource_route_request(0x101, 0, selected);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqStatus(i32);
#[derive(Debug, Eq, PartialEq)]
pub enum InterruptError { Define(IrqStatus), Enable(IrqStatus), Disable(IrqStatus), DmaBusy, Busy, Unavailable }

pub type IrqHandler = unsafe extern "C" fn(token: u32);
static WIRE_HANDLER: AtomicUsize = AtomicUsize::new(0);
static TIMER_HANDLER: AtomicUsize = AtomicUsize::new(0);

/// # Safety
/// Both handlers own the queue98 transport runtime and run in IRQ context after the lens endpoints are leased.
pub unsafe fn install_irq_handlers(wire: IrqHandler, timer: IrqHandler) -> Result<(), InterruptError> {
    if WIRE_HANDLER.compare_exchange(0, wire as usize, Ordering::AcqRel, Ordering::Acquire).is_err() { return Err(InterruptError::Busy); }
    if TIMER_HANDLER.compare_exchange(0, timer as usize, Ordering::AcqRel, Ordering::Acquire).is_err() {
        WIRE_HANDLER.store(0, Ordering::Release);
        return Err(InterruptError::Busy);
    }
    Ok(())
}
pub fn uninstall_irq_handlers(wire: IrqHandler, timer: IrqHandler) {
    let _ = WIRE_HANDLER.compare_exchange(wire as usize, 0, Ordering::AcqRel, Ordering::Acquire);
    let _ = TIMER_HANDLER.compare_exchange(timer as usize, 0, Ordering::AcqRel, Ordering::Acquire);
}
fn handler(slot: &AtomicUsize) -> Option<IrqHandler> {
    let value = slot.load(Ordering::Acquire);
    if value == 0 { None } else { Some(unsafe { core::mem::transmute::<usize, IrqHandler>(value) }) }
}

#[repr(C)]
struct Definition {
    reserved: u32,
    handler: unsafe extern "C" fn(),
    priority: u32,
    routed: u32,
    mode: u32,
}

extern "C" {
    fn xm1_rtos_irq_define(line: u32, packet: *const Definition) -> i32;
    fn xm1_rtos_irq_enable(line: u32) -> i32;
    fn xm1_rtos_irq_disable(line: u32) -> i32;
}

static mut WIRE_TOKEN: u32 = 0;
static mut TIMER_TOKEN: u32 = 0;

#[no_mangle]
pub unsafe extern "C" fn xm1_lens_wire_vector() {
    let token = unsafe { read_volatile(addr_of!(WIRE_TOKEN)) };
    if token != 0 { if let Some(handler) = handler(&WIRE_HANDLER) { unsafe { handler(token) }; } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_lens_timer_vector() {
    let token = unsafe { read_volatile(addr_of!(TIMER_TOKEN)) };
    if token != 0 { if let Some(handler) = handler(&TIMER_HANDLER) { unsafe { handler(token) }; } }
}

pub struct NativeInterrupts { _exclusive: () }
impl NativeInterrupts {
    fn register(&self, line: u32, handler: unsafe extern "C" fn(), mode: u32) -> Result<(), InterruptError> {
        let definition = Definition { reserved: 0, handler, priority: 7, routed: 1, mode };
        let status = unsafe { xm1_rtos_irq_define(line, &definition) };
        if status != 0 { return Err(InterruptError::Define(IrqStatus(status))); }
        let status = unsafe { xm1_rtos_irq_enable(line) };
        if status != 0 {
            unsafe { xm1_rtos_irq_define(line, core::ptr::null()) };
            return Err(InterruptError::Enable(IrqStatus(status)));
        }
        Ok(())
    }
    fn unregister(&self, line: u32) -> Result<(), InterruptError> {
        unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
        let status = unsafe { xm1_rtos_irq_disable(line) };
        if status != 0 { return Err(InterruptError::Disable(IrqStatus(status))); }
        let status = unsafe { xm1_rtos_irq_define(line, core::ptr::null()) };
        if status != 0 { return Err(InterruptError::Define(IrqStatus(status))); }
        Ok(())
    }
}

impl InterruptController for NativeInterrupts {
    type Error = InterruptError;
    fn register_wire(&mut self, token: TransferToken) -> Result<(), InterruptError> {
        if handler(&WIRE_HANDLER).is_none() { return Err(InterruptError::Unavailable); }
        let saved = enter_critical();
        let result = if load(0xfff0_20a4) & 5 != 0 || load(0xfff0_20e4) & 5 != 0 {
            Err(InterruptError::DmaBusy)
        } else {
            store(0xfff0_20a8, 8);
            store(0xfff0_20e8, 8);
            unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
            self.unregister(94).and_then(|()| {
                unsafe { write_volatile(addr_of_mut!(WIRE_TOKEN), token.get()) };
                self.register(94, xm1_lens_wire_vector, 2)
            })
        };
        if result.is_err() { unsafe { write_volatile(addr_of_mut!(WIRE_TOKEN), 0) }; }
        leave_critical(saved);
        result
    }
    fn unregister_wire(&mut self) -> Result<(), InterruptError> {
        let saved = enter_critical();
        let result = self.unregister(94);
        if result.is_ok() { unsafe { write_volatile(addr_of_mut!(WIRE_TOKEN), 0) }; }
        leave_critical(saved);
        result
    }
    fn register_timer(&mut self, _: TimerInterrupt, token: TimerToken) -> Result<(), InterruptError> {
        if handler(&TIMER_HANDLER).is_none() { return Err(InterruptError::Unavailable); }
        let saved = enter_critical();
        let result = self.unregister(31).and_then(|()| {
            unsafe { write_volatile(addr_of_mut!(TIMER_TOKEN), token.get()) };
            self.register(31, xm1_lens_timer_vector, 1)
        });
        if result.is_err() { unsafe { write_volatile(addr_of_mut!(TIMER_TOKEN), 0) }; }
        leave_critical(saved);
        result
    }
    fn unregister_timer(&mut self, _: TimerInterrupt) -> Result<(), InterruptError> {
        let saved = enter_critical();
        let result = self.unregister(31);
        if result.is_ok() { unsafe { write_volatile(addr_of_mut!(TIMER_TOKEN), 0) }; }
        leave_critical(saved);
        result
    }
}

/// # Safety
/// The caller must satisfy `NativePlatform::new_exclusive` and link the owned IRQ ABI for its loader domain. Both lens IRQ callbacks must retire their captured token through this backend before dispatching the runtime event.
pub unsafe fn backend_exclusive() -> NativeBackend {
    CpuBackend::new(NativeMmio { _exclusive: () }, unsafe { NativePlatform::new_exclusive() }, NativeInterrupts { _exclusive: () })
}
