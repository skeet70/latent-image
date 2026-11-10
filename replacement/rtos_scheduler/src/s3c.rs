#![no_std]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;
use layout::{Domain, GuestAddress, KernelContext, Task};

#[cfg(feature = "domain-a")]
const DOMAIN: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const DOMAIN: Domain = Domain::B;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TaskId(i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ExceptionPattern(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ServiceResult(i32);
#[derive(Clone, Copy)]
#[repr(i32)]
enum SwitchReason { Sleep = -17, Suspend = -22, Resume = -23 }

extern "C" {
    fn s3c_remove(record: KernelContext, task: Task) -> u32;
    fn s3c_insert(record: KernelContext, task: Task) -> u32;
    fn s3c_notify(id: u32) -> u32;
    fn s3c_switch(reason: SwitchReason);
}

#[inline(always)]
fn word(at: GuestAddress) -> u32 { unsafe { read_volatile(at.0 as *const u32) } }
#[inline(always)]
fn put(at: GuestAddress, value: u32) { unsafe { write_volatile(at.0 as *mut u32, value) } }
#[inline(always)]
fn byte(at: GuestAddress) -> u8 { unsafe { read_volatile(at.0 as *const u8) } }
#[inline(always)]
fn put_byte(at: GuestAddress, value: u8) { unsafe { write_volatile(at.0 as *mut u8, value) } }
#[inline(always)]
fn flags(task: Task, clear: u8, set: u8) { put_byte(task.0.offset(14), byte(task.0.offset(14)) & !clear | set); }
#[inline(always)]
fn cpsr() -> u32 { let value; unsafe { asm!("mrs {}, cpsr", out(reg) value, options(nostack, preserves_flags)); } value }
#[inline(always)]
fn mask() { unsafe { asm!("cpsid i", options(nostack, preserves_flags)); } }
#[inline(always)]
fn unmask() { unsafe { asm!("cpsie i", options(nostack, preserves_flags)); } }
#[inline(always)]
fn running() -> Task { Task(GuestAddress(word(DOMAIN.words().running))) }
#[inline(always)]
fn current() -> KernelContext { KernelContext(GuestAddress(word(DOMAIN.words().current_context))) }
#[inline(always)]
fn config(task: Task) -> GuestAddress { GuestAddress(word(task.0.offset(8))) }
#[cfg_attr(feature = "component", inline(never))]
#[cfg_attr(not(feature = "component"), inline(always))]
fn invalid_context(interrupt: bool) -> bool { (word(DOMAIN.words().handler_context) != 0) != interrupt || cpsr() & 128 != 0 }

#[cfg_attr(feature = "component", inline(never))]
#[cfg_attr(not(feature = "component"), inline(always))]
fn resolve(id: TaskId, zero: bool, interrupt: bool) -> Result<(KernelContext, Task), ServiceResult> {
    let class = id.0 >> 16;
    let record = match class {
        0 => current(),
        1 => DOMAIN.words().context(Domain::A),
        2 => DOMAIN.words().context(Domain::B),
        _ => return Err(ServiceResult(-18)),
    };
    let number = id.0 as i16 as i32;
    if number < 0 || number > word(record.0.offset(0x48)) as i32 || (!zero && number == 0) {
        return Err(ServiceResult(-18));
    }
    let task = if number == 0 && !interrupt { running() } else {
        Task(GuestAddress(word(record.0.offset(0x44)).wrapping_add((number.wrapping_sub(1) as u32).wrapping_mul(56))))
    };
    Ok((record, task))
}

struct SchedulerGuard(KernelContext);
impl SchedulerGuard {
    #[inline(always)]
    fn acquire(record: KernelContext) -> Option<Self> {
        let acquire: unsafe extern "C" fn(GuestAddress) -> u32 = unsafe { core::mem::transmute(word(record.0.offset(12))) };
        if unsafe { acquire(record.0.offset(8)) } == 0 { Some(Self(record)) } else { None }
    }
    #[inline(always)]
    fn release(self) {
        let release: unsafe extern "C" fn(GuestAddress) = unsafe { core::mem::transmute(word(self.0.0.offset(16))) };
        unsafe { release(self.0.0.offset(8)); }
    }
}
#[cfg_attr(feature = "component", inline(never))]
#[cfg_attr(not(feature = "component"), inline(always))]
fn lock(record: KernelContext) -> SchedulerGuard {
    loop { mask(); if let Some(guard) = SchedulerGuard::acquire(record) { return guard; } unmask(); }
}
#[inline(always)]
fn lock_running() -> SchedulerGuard {
    loop {
        let guard = lock(current());
        if byte(running().0.offset(12)) & 1 != 0 { return guard; }
        guard.release(); unmask();
    }
}
#[inline(always)]
fn finish(guard: SchedulerGuard, result: i32) -> ServiceResult { guard.release(); unmask(); ServiceResult(result) }

#[no_mangle]
#[link_section = ".text.s3c_get_priority"]
pub extern "C" fn s3c_get_priority(id: TaskId, output: GuestAddress) -> ServiceResult {
    if invalid_context(false) { return ServiceResult(-25); }
    let (record, task) = match resolve(id, true, false) { Ok(pair) => pair, Err(error) => return error };
    if word(config(task).offset(0x24)) == 0 { return ServiceResult(-42); }
    let guard = lock(record);
    let result = if byte(task.0.offset(12)) == 0 || (task == running() && word(task.0.offset(48)) != 0) { -41 } else {
        put(output, byte(task.0.offset(13)) as u32 + 1); 0
    };
    finish(guard, result)
}

#[no_mangle]
#[link_section = ".text.s3c_sleep"]
pub extern "C" fn s3c_sleep() -> ServiceResult {
    if invalid_context(false) || word(DOMAIN.words().dispatch_enabled) == 0 { return ServiceResult(-25); }
    let slot = word(config(running()).offset(0x20));
    let guard = lock_running();
    let task = running();
    if byte(task.0.offset(14)) & 2 != 0 { flags(task, 2, 0); return finish(guard, 0); }
    let wait = DOMAIN.words().dispatch_enabled.offset(24 + slot.wrapping_sub(1) * 4);
    put_byte(task.0.offset(12), 10);
    unsafe { s3c_remove(current(), task); }
    put(running().0.offset(20), wait.0);
    put(wait, 0);
    guard.release();
    unsafe { s3c_switch(SwitchReason::Sleep); }
    let result = word(wait) as i32;
    unmask(); ServiceResult(result)
}

#[no_mangle]
#[link_section = ".text.s3c_suspend"]
pub extern "C" fn s3c_suspend(id: TaskId) -> ServiceResult {
    if invalid_context(false) { return ServiceResult(-25); }
    let (record, task) = match resolve(id, true, false) { Ok(pair) => pair, Err(error) => return error };
    if word(config(task).offset(0x24)) == 0 { return ServiceResult(-42); }
    loop {
        mask();
        let self_task = task == running();
        let acquisition = if self_task { current() } else { record };
        if let Some(guard) = SchedulerGuard::acquire(acquisition) {
            if !self_task || byte(running().0.offset(12)) & 1 != 0 { core::mem::forget(guard); break; }
            guard.release();
        }
        unmask();
    }
    // Stock releases the id-selected record even when the self path acquired the current record.
    let guard = SchedulerGuard(record);
    if task == running() && word(DOMAIN.words().dispatch_enabled) == 0 { return finish(guard, -25); }
    let state = byte(task.0.offset(12));
    if state == 0 { return finish(guard, -41); }
    if state & 1 == 0 {
        if state & 4 != 0 { return finish(guard, -43); }
        put_byte(task.0.offset(12), state | 4);
        return finish(guard, 0);
    }
    put_byte(task.0.offset(12), 4);
    let dispatch = unsafe { s3c_remove(record, task) != 0 && s3c_notify(word(record.0)) != 0 };
    guard.release();
    if dispatch { unsafe { s3c_switch(SwitchReason::Suspend); } }
    unmask(); ServiceResult(0)
}

#[no_mangle]
#[link_section = ".text.s3c_resume"]
pub extern "C" fn s3c_resume(id: TaskId) -> ServiceResult {
    if invalid_context(false) { return ServiceResult(-25); }
    let (record, task) = match resolve(id, false, false) { Ok(pair) => pair, Err(error) => return error };
    if word(config(task).offset(0x24)) == 0 { return ServiceResult(-42); }
    let guard = lock(record);
    let state = byte(task.0.offset(12));
    if state & 4 == 0 || (task == running() && word(task.0.offset(48)) != 0) { return finish(guard, -41); }
    if state & 2 != 0 { put_byte(task.0.offset(12), state & !4); return finish(guard, 0); }
    let dispatch = unsafe { s3c_insert(record, task) != 0 && s3c_notify(word(record.0)) != 0 };
    guard.release();
    if dispatch { unsafe { s3c_switch(SwitchReason::Resume); } }
    unmask(); ServiceResult(0)
}

#[no_mangle]
#[link_section = ".text.s3c_force_resume"]
pub extern "C" fn s3c_force_resume(id: TaskId) -> ServiceResult { s3c_resume(id) }

#[inline(always)]
fn raise(id: TaskId, pattern: ExceptionPattern, interrupt: bool) -> ServiceResult {
    if invalid_context(interrupt) { return ServiceResult(-25); }
    let (record, task) = match resolve(id, true, interrupt) { Ok(pair) => pair, Err(error) => return error };
    if pattern.0 == 0 { return ServiceResult(-17); }
    let guard = if interrupt {
        loop { mask(); if let Some(guard) = SchedulerGuard::acquire(record) { break guard; } }
    } else { lock(record) };
    if byte(task.0.offset(12)) == 0 || word(config(task).offset(0x1c)) == 0 || (!interrupt && task == running() && word(task.0.offset(48)) != 0) {
        return finish(guard, -41);
    }
    put(task.0.offset(16), word(task.0.offset(16)) | pattern.0);
    if unsafe { s3c_notify(word(record.0)) } != 0 && task == running() && byte(task.0.offset(14)) & 4 != 0 {
        if interrupt { put(DOMAIN.words().reschedule, 1); } else { s3c_exception_loop(); }
    }
    finish(guard, 0)
}
#[no_mangle]
#[link_section = ".text.s3c_raise"]
pub extern "C" fn s3c_raise(id: TaskId, pattern: ExceptionPattern) -> ServiceResult { raise(id, pattern, false) }
#[no_mangle]
#[link_section = ".text.s3c_interrupt_raise"]
pub extern "C" fn s3c_interrupt_raise(id: TaskId, pattern: ExceptionPattern) -> ServiceResult { raise(id, pattern, true) }

#[no_mangle]
#[link_section = ".text.s3c_disable_exception"]
pub extern "C" fn s3c_disable_exception() -> ServiceResult {
    if invalid_context(false) { return ServiceResult(-25); }
    let guard = lock(current());
    let task = running();
    if word(config(task).offset(0x1c)) == 0 || word(task.0.offset(48)) != 0 { return finish(guard, -41); }
    flags(task, 4, 0); finish(guard, 0)
}
#[no_mangle]
#[link_section = ".text.s3c_enable_exception"]
pub extern "C" fn s3c_enable_exception() -> ServiceResult {
    if invalid_context(false) { return ServiceResult(-25); }
    let guard = lock_running();
    let task = running();
    if word(config(task).offset(0x1c)) == 0 || word(task.0.offset(48)) != 0 { return finish(guard, -41); }
    flags(task, 0, 4);
    if word(task.0.offset(16)) != 0 { s3c_exception_loop(); }
    finish(guard, 0)
}

#[no_mangle]
#[link_section = ".text.s3c_exception_loop"]
pub extern "C" fn s3c_exception_loop() {
    loop {
        let task = running();
        let pattern = ExceptionPattern(word(task.0.offset(16)));
        flags(task, 4, 0); put(task.0.offset(16), 0);
        SchedulerGuard(current()).release(); unmask();
        let definition = config(running());
        let callback: unsafe extern "C" fn(ExceptionPattern, u32) = unsafe { core::mem::transmute(word(definition.offset(0x1c))) };
        unsafe { callback(pattern, word(definition.offset(4))); }
        if cpsr() & 128 != 0 { unmask(); }
        core::mem::forget(lock(current()));
        if word(running().0.offset(16)) == 0 { break; }
    }
    flags(running(), 0, 4);
}
#[no_mangle]
#[link_section = ".text.s3c_exception_delivery"]
pub extern "C" fn s3c_exception_delivery() {
    let guard = loop {
        if let Some(guard) = SchedulerGuard::acquire(current()) { break guard; }
        unmask(); mask();
    };
    let task = running();
    if byte(task.0.offset(14)) & 4 != 0 && word(task.0.offset(16)) != 0 { s3c_exception_loop(); }
    guard.release();
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop {} }
