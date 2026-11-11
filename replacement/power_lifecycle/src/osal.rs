#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuCore { First, Second }
impl CpuCore {
    pub const fn index(self) -> u32 { match self { Self::First => 0, Self::Second => 1 } }
    pub fn from_abi(value: u32) -> Option<Self> { match value { 0 => Some(Self::First), 1 => Some(Self::Second), _ => None } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedInterrupts(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockStamp(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Microseconds(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerReload(pub u32);
pub trait CriticalKernel {
    fn core(&self) -> CpuCore;
    fn mask_interrupts(&mut self) -> SavedInterrupts;
    fn restore_interrupts(&mut self, saved: SavedInterrupts);
    fn observe_global_lock(&mut self) -> LockStamp;
    fn try_global_lock(&mut self, stamp: LockStamp) -> bool;
    fn release_global_lock(&mut self);
    fn nesting(&self) -> u32;
    fn set_nesting(&mut self, depth: u32);
}
pub fn interrupt_lock(kernel: &mut impl CriticalKernel) -> SavedInterrupts {
    let core = kernel.core().index() + 1;
    let saved = kernel.mask_interrupts();
    if kernel.observe_global_lock() != LockStamp(core | 0x10) {
        if kernel.observe_global_lock() != LockStamp(core | 0x20) {
            while !kernel.try_global_lock(LockStamp(core | 0x20)) { core::hint::spin_loop(); }
        }
        kernel.set_nesting(kernel.nesting().wrapping_add(1));
    }
    saved
}
pub fn interrupt_unlock(kernel: &mut impl CriticalKernel, saved: SavedInterrupts) {
    let irq_owner = LockStamp((kernel.core().index() + 1) | 0x10);
    if kernel.observe_global_lock() != irq_owner && kernel.nesting() != 0 {
        let depth = kernel.nesting().wrapping_sub(1);
        kernel.set_nesting(depth);
        if depth == 0 { kernel.release_global_lock(); }
    }
    kernel.restore_interrupts(saved);
}
pub trait DelayTimer {
    fn reload(&mut self) -> u32;
    fn current(&mut self) -> u32;
    fn division_fault(&mut self, _: u32) -> ! { panic!("unsigned division fault") }
}
pub fn delay_us(timer: &mut impl DelayTimer, duration: Microseconds) {
    let _ = delay_us_with_residue(timer, duration);
}
pub fn delay_us_with_residue(timer: &mut impl DelayTimer, duration: Microseconds) -> TimerReload {
    let reload = timer.reload();
    let divisor = divide(timer, 10_000_000, reload);
    let target = divide(timer, duration.0.wrapping_mul(1000), divisor);
    let mut previous = timer.reload().wrapping_sub(timer.current());
    let mut elapsed = 0u32;
    loop {
        let current = timer.reload().wrapping_sub(timer.current());
        let adjusted = if previous >= current { previous } else { reload.wrapping_add(previous).wrapping_add(1) };
        elapsed = adjusted.wrapping_add(elapsed).wrapping_sub(current);
        if elapsed >= target { break; }
        previous = current;
    }
    TimerReload(reload)
}

fn divide(timer: &mut impl DelayTimer, dividend: u32, divisor: u32) -> u32 {
    let Some(divisor) = core::num::NonZeroU32::new(divisor) else { timer.division_fault(dividend) };
    dividend / divisor.get()
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::callback_registry::{kernel_slot, refused, CallbackError};
    use core::ptr::{read_volatile, write_volatile};
    extern "C" {
        fn xm1_rtos_own_first_slot_1618() -> u32;
        fn xm1_rtos_own_first_slot_164() -> u32;
        fn xm1_rtos_own_first_slot_168(saved: u32) -> u32;
        fn xm1_rtos_global_lock_observe(word: u32) -> u32;
        fn xm1_rtos_global_lock_try(word: u32, stamp: u32) -> i32;
        fn xm1_rtos_global_lock_release(word: u32) -> i32;
        fn xm1_power_service_error(operation: u32, id: i32, status: i32);
    }
    unsafe fn root(core: CpuCore) -> u32 { read_volatile((0x96bd20 + core.index() * 4) as *const u32) }
    fn core() -> Result<CpuCore, CallbackError> { CpuCore::from_abi(unsafe { xm1_rtos_own_first_slot_1618() }).ok_or(CallbackError::Invalid) }
    struct NativeKernel;
    impl CriticalKernel for NativeKernel {
        fn core(&self) -> CpuCore {
            match core() { Ok(core) => core, Err(error) => { refused(error); CpuCore::First } }
        }
        fn mask_interrupts(&mut self) -> SavedInterrupts { SavedInterrupts(unsafe { xm1_rtos_own_first_slot_164() }) }
        fn restore_interrupts(&mut self, saved: SavedInterrupts) { unsafe { let _ = xm1_rtos_own_first_slot_168(saved.0); } }
        fn observe_global_lock(&mut self) -> LockStamp { LockStamp(unsafe { xm1_rtos_global_lock_observe(0) }) }
        fn try_global_lock(&mut self, stamp: LockStamp) -> bool { unsafe { xm1_rtos_global_lock_try(0, stamp.0) == 0 } }
        fn release_global_lock(&mut self) { unsafe { let _ = xm1_rtos_global_lock_release(0); } }
        fn nesting(&self) -> u32 { unsafe { read_volatile(0x40a0f824 as *const u32) } }
        fn set_nesting(&mut self, depth: u32) { unsafe { write_volatile(0x40a0f824 as *mut u32, depth); } }
    }
    struct NativeTimer;
    impl DelayTimer for NativeTimer {
        fn reload(&mut self) -> u32 { unsafe { read_volatile(0xfff91004 as *const u32) } }
        fn current(&mut self) -> u32 { unsafe { read_volatile(0xfff91000 as *const u32) } }
        fn division_fault(&mut self, dividend: u32) -> ! {
            let operand = if dividend == 0 { 0 } else { u32::MAX };
            unsafe { core::arch::asm!("svc #0x1010", "2: b 2b", in("r0") operand, in("r1") 0u32, options(noreturn)); }
        }
    }
    unsafe fn encoded_semaphore(id: i32) -> i32 {
        let slot = id as u32 & 0xffff;
        let class = if id > 169 { 1 } else { read_volatile((0x93e6f4 + slot.wrapping_sub(1) * 4) as *const u32) };
        (slot | class << 16) as i32
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32 {
        let core = match core() { Ok(core) => core, Err(error) => return refused(error) };
        let wait: unsafe extern "C" fn(i32, i32) -> i32 = unsafe { kernel_slot(root(core), 0x7c) };
        let status = wait(encoded_semaphore(id), timeout);
        if status != 0 && status != -50 { xm1_power_service_error(24, id, status); }
        status
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_semaphore_signal(id: i32) -> i32 {
        let core = match core() { Ok(core) => core, Err(error) => return refused(error) };
        let context: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(root(core), 0x148) };
        let offset = if context() == 1 { 0x70 } else { 0x6c };
        let signal: unsafe extern "C" fn(i32) -> i32 = unsafe { kernel_slot(root(core), offset) };
        let status = signal(encoded_semaphore(id));
        if status != 0 { xm1_power_service_error(25, id, status); }
        status
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_current_task() -> u32 {
        let core = match core() { Ok(core) => core, Err(error) => { refused(error); return 0; } };
        let current: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(root(core), 0x16c) };
        current()
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_interrupt_lock() -> u32 { interrupt_lock(&mut NativeKernel).0 }
    #[no_mangle]
    pub extern "C" fn xm1_power_interrupt_unlock(saved: u32) { interrupt_unlock(&mut NativeKernel, SavedInterrupts(saved)); }
    #[no_mangle]
    pub extern "C" fn xm1_power_delay_us(duration: u32) { delay_us(&mut NativeTimer, Microseconds(duration)); }
    #[no_mangle]
    pub extern "C" fn xm1_power_delay_us_with_residue(duration: u32) -> u32 {
        delay_us_with_residue(&mut NativeTimer, Microseconds(duration)).0
    }
}
