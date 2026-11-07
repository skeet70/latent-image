use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

use crate::layout::{DomainId, DomainWords, GuestAddress, KernelContext, LockRecord, Priority, Task, TaskFlags, TaskState};
use crate::{Acquired, RecordId, Reschedule, Notified, DOMAIN};

const WORDS: &DomainWords = DOMAIN.words();

extern "C" {
    fn s3a_stock_remove(record: KernelContext, task: Task) -> Reschedule;
    fn s3a_stock_reset(task: Task) -> Task;
    fn s3a_stock_prepare(record: KernelContext, task: Task) -> Reschedule;
    fn s3a_stock_rotate(record: KernelContext, priority: u32) -> Reschedule;
    fn s3a_stock_notify(id: RecordId) -> Notified;
    fn s3a_stock_save(reason: DispatchReason);
    fn s3a_stock_discard(reason: DispatchReason) -> !;
}

#[derive(Clone, Copy)]
#[repr(i32)]
enum DispatchReason { Exit = -10, Enable = -92, Rotate = -85, CoreRotate = -4 }

#[derive(Clone, Copy)]
#[repr(i32)]
pub(crate) enum Status { Ok = 0, Context = -25, Priority = -17 }

#[inline(always)]
fn load(at: GuestAddress) -> u32 { unsafe { read_volatile(at.0 as *const u32) } }
#[inline(always)]
fn store(at: GuestAddress, value: u32) { unsafe { write_volatile(at.0 as *mut u32, value) } }
#[inline(always)]
fn byte(at: GuestAddress) -> u8 { unsafe { read_volatile(at.0 as *const u8) } }
#[inline(always)]
fn store_byte(at: GuestAddress, value: u8) { unsafe { write_volatile(at.0 as *mut u8, value) } }
#[inline(always)]
fn masked() -> bool {
    let cpsr: u32;
    unsafe { asm!("mrs {}, cpsr", out(reg) cpsr, options(nostack, preserves_flags)) };
    cpsr & 0x80 != 0
}
#[inline(always)]
fn mask() { unsafe { asm!("cpsid i", options(nostack, preserves_flags)) }; }
#[inline(always)]
fn unmask() { unsafe { asm!("cpsie i", options(nostack, preserves_flags)) }; }
#[inline(always)]
fn current() -> KernelContext { KernelContext(GuestAddress(load(WORDS.current_context))) }
#[inline(always)]
fn running() -> Task { Task(GuestAddress(load(WORDS.running))) }
#[inline(always)]
fn context_ok(interrupt: bool) -> bool { (load(WORDS.handler_context) != 0) == interrupt && !masked() }

struct Scheduler;
struct Guard<'a> { record: KernelContext, _owner: &'a mut Scheduler }

impl Scheduler {
    #[inline(always)]
    fn lock(&mut self, record: KernelContext, interrupt_retry: bool) -> Guard<'_> {
        loop {
            mask();
            let lock = record.scheduler_lock();
            let acquire: unsafe extern "C" fn(LockRecord) -> Acquired = unsafe { core::mem::transmute(load(lock.acquire())) };
            if unsafe { acquire(lock) }.0 == 0 { break; }
            if !interrupt_retry { unmask(); }
        }
        Guard { record, _owner: self }
    }
}
impl Guard<'_> {
    #[inline(always)]
    fn release(self) {
        let lock = self.record.scheduler_lock();
        let release: unsafe extern "C" fn(LockRecord) = unsafe { core::mem::transmute(load(lock.release())) };
        unsafe { release(lock) };
    }
    #[inline(always)]
    fn rotate(&self, priority: Priority) -> bool { unsafe { s3a_stock_rotate(self.record, priority.get()).0 != 0 } }
    #[inline(always)]
    fn notify(&self) -> bool { unsafe { s3a_stock_notify(RecordId(load(self.record.id()))) == Notified::Local } }
}

#[no_mangle]
#[link_section = ".text.s3a_disable"]
pub extern "C" fn xm1_s3a_disable_dispatch() -> Status {
    if !context_ok(false) { return Status::Context; }
    let mut scheduler = Scheduler;
    loop {
        let guard = scheduler.lock(current(), false);
        if byte(running().state()) & TaskState::READY.bits() != 0 {
            store(WORDS.dispatch_enabled, 0);
            guard.release();
            unmask();
            return Status::Ok;
        }
        guard.release();
        unmask();
    }
}

#[no_mangle]
#[link_section = ".text.s3a_enable"]
pub extern "C" fn xm1_s3a_enable_dispatch() -> Status {
    if !context_ok(false) { return Status::Context; }
    let mut scheduler = Scheduler;
    let guard = scheduler.lock(current(), false);
    store(WORDS.dispatch_enabled, 1);
    if load(running().context_discarded()) != 0 {
        guard.release();
        unmask();
        unsafe { s3a_stock_discard(DispatchReason::Enable) }
    }
    let needs_dispatch = load(WORDS.running) != load(WORDS.selected);
    guard.release();
    if needs_dispatch { unsafe { s3a_stock_save(DispatchReason::Enable) }; }
    unmask();
    Status::Ok
}

#[no_mangle]
#[link_section = ".text.s3a_exit"]
pub extern "C" fn xm1_s3a_exit_task() -> ! {
    let mut scheduler = Scheduler;
    let guard = scheduler.lock(current(), false);
    if load(WORDS.dispatch_enabled) == 0 { store(WORDS.dispatch_enabled, 1); }
    if load(running().context_discarded()) == 0 {
        unsafe {
            s3a_stock_remove(current(), running());
            s3a_stock_reset(running());
        }
        let task = running();
        let flags = TaskFlags(byte(task.flags()));
        if flags.contains(TaskFlags::QUEUED_ACTIVATION) {
            store_byte(task.flags(), flags.0 & !TaskFlags::QUEUED_ACTIVATION.0);
            unsafe { s3a_stock_prepare(current(), task); }
        }
    }
    guard.release();
    unsafe { s3a_stock_discard(DispatchReason::Exit) }
}

#[no_mangle]
#[link_section = ".text.s3a_rotate"]
pub extern "C" fn xm1_s3a_rotate_ready(priority: u32) -> Status {
    if !context_ok(false) { return Status::Context; }
    if priority > 16 { return Status::Priority; }
    let mut scheduler = Scheduler;
    let guard = scheduler.lock(current(), false);
    let stored = if priority == 0 { byte(running().priority()) as u32 } else { priority - 1 };
    // Running task priorities are stored in 0..=15.
    let priority = unsafe { Priority::new(stored).unwrap_unchecked() };
    let dispatch = guard.rotate(priority);
    guard.release();
    if dispatch { unsafe { s3a_stock_save(DispatchReason::Rotate) }; }
    unmask();
    Status::Ok
}

#[no_mangle]
#[link_section = ".text.s3a_irotate"]
pub extern "C" fn xm1_s3a_interrupt_rotate_ready(priority: u32) -> Status {
    if !context_ok(true) { return Status::Context; }
    let Some(priority) = Priority::new(priority.wrapping_sub(1)) else { return Status::Priority; };
    let mut scheduler = Scheduler;
    let guard = scheduler.lock(current(), false);
    if guard.rotate(priority) { store(WORDS.reschedule, 1); }
    guard.release();
    unmask();
    Status::Ok
}

#[inline(always)]
fn selected_context(selector: DomainId) -> KernelContext {
    if selector == DomainId::Own { current() } else { KernelContext(WORDS.contexts.offset((selector.word() - 1) * KernelContext::SIZE)) }
}

#[no_mangle]
#[link_section = ".text.s3a_crotate"]
pub extern "C" fn xm1_s3a_core_rotate_ready(selector: u32, priority: u32) -> Status {
    if !context_ok(false) { return Status::Ok; }
    let Some(priority) = Priority::new(priority.wrapping_sub(1)) else { return Status::Ok; };
    let Some(selector) = DomainId::from_word(selector) else { return Status::Ok; };
    let mut scheduler = Scheduler;
    let guard = scheduler.lock(selected_context(selector), false);
    let dispatch = guard.rotate(priority) && guard.notify();
    guard.release();
    if dispatch { unsafe { s3a_stock_save(DispatchReason::CoreRotate) }; }
    unmask();
    Status::Ok
}

#[no_mangle]
#[link_section = ".text.s3a_icrotate"]
pub extern "C" fn xm1_s3a_interrupt_core_rotate_ready(selector: u32, priority: u32) -> Status {
    if !context_ok(true) { return Status::Context; }
    let Some(priority) = Priority::new(priority.wrapping_sub(1)) else { return Status::Priority; };
    // The interrupt service's stock ABI requires a selector in 0..=2.
    let selector = unsafe { DomainId::from_word(selector).unwrap_unchecked() };
    let mut scheduler = Scheduler;
    let guard = scheduler.lock(selected_context(selector), true);
    if guard.rotate(priority) {
        let reschedule = if guard.notify() { 1 } else { load(WORDS.reschedule) };
        store(WORDS.reschedule, reschedule);
    }
    guard.release();
    unmask();
    Status::Ok
}
