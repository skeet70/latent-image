//! Stage adapter: the semaphore entries at their stock extents over the retained helpers.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod semaphore;

use core::arch::asm;
use semaphore::layout::*;
use semaphore::*;

#[cfg(all(target_arch = "arm", not(test), not(feature = "component")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

extern "C" {
    fn xm1_semaphore_stock_acquire(lock: LockRecord) -> u32;
    fn xm1_semaphore_stock_release(lock: LockRecord) -> LockRecord;
    fn xm1_semaphore_stock_notify(id: u32) -> u32;
    fn xm1_semaphore_stock_dispatch(code: i32);
    fn xm1_semaphore_stock_mask() -> u32;
    fn xm1_semaphore_stock_restore(saved: u32);
    fn xm1_semaphore_stock_wait_release(owner: KernelContext, task: Task) -> u32;
    fn xm1_semaphore_stock_wait_cancel(owner: KernelContext, task: Task) -> u32;
    fn xm1_semaphore_stock_wait_enqueue(owner: u32, queue: WaitQueue, wait: WaitBlock);
    fn xm1_semaphore_stock_wait_enqueue_timed(
        owner: u32,
        queue: WaitQueue,
        wait: WaitBlock,
        timer: GuestAddress,
        timeout: i32,
    );
}

const PLAIN: i32 = i32::MIN;

struct Cpu;

#[inline(always)]
fn load(address: GuestAddress) -> u32 {
    unsafe { core::ptr::read_volatile(address.0 as *const u32) }
}

#[inline(always)]
fn store(address: GuestAddress, value: u32) {
    unsafe { core::ptr::write_volatile(address.0 as *mut u32, value) }
}

#[inline(always)]
unsafe fn cpsr_masked() -> bool {
    let cpsr: u32;
    unsafe { asm!("mrs {cpsr}, cpsr", cpsr = out(reg) cpsr, options(nostack)) };
    cpsr & 0x80 != 0
}

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, address: GuestAddress) -> u32 {
        load(address)
    }

    #[inline(always)]
    fn load_byte(&self, address: GuestAddress) -> u8 {
        unsafe { core::ptr::read_volatile(address.0 as *const u8) }
    }

    #[inline(always)]
    fn store_word(&self, address: GuestAddress, value: u32) {
        store(address, value);
    }

    #[inline(always)]
    fn store_byte(&self, address: GuestAddress, value: u8) {
        unsafe { core::ptr::write_volatile(address.0 as *mut u8, value) }
    }

    #[inline(always)]
    fn mask_interrupts(&self) -> SavedInterrupts {
        SavedInterrupts(unsafe { xm1_semaphore_stock_mask() })
    }

    #[inline(always)]
    fn restore_interrupts(&self, saved: SavedInterrupts) {
        unsafe { xm1_semaphore_stock_restore(saved.0) }
    }

    #[inline(always)]
    fn restore_f(&self, saved: SavedInterrupts) {
        let f = saved.0 & 0x40;
        unsafe {
            asm!(
                "mrs {c}, cpsr",
                "bic {c}, {c}, #0xc0",
                "orr {c}, {c}, {f}",
                "orr {c}, {c}, #0x80",
                "msr cpsr_c, {c}",
                c = out(reg) _,
                f = in(reg) f,
                options(nostack),
            );
        }
    }


    #[inline(always)]
    fn dmb(&self) {
        unsafe { asm!("dmb", options(nostack)) }
    }
}

impl IrqScope for Cpu {
    #[inline(always)]
    fn disable_irq(&self) {
        unsafe {
            asm!("mrs {0}, cpsr", "orr {0}, {0}, #0x80", "msr cpsr_c, {0}", out(reg) _, options(nostack));
        }
    }

    #[inline(always)]
    fn enable_irq(&self) {
        unsafe {
            asm!("mrs {0}, cpsr", "bic {0}, {0}, #0x80", "msr cpsr_c, {0}", out(reg) _, options(nostack));
        }
    }
}

impl Locks for Cpu {
    #[inline(always)]
    fn lock(&self, record: LockRecord) {
        unsafe { xm1_semaphore_stock_acquire(record) };
    }

    #[inline(always)]
    fn unlock(&self, record: LockRecord) {
        unsafe { xm1_semaphore_stock_release(record) };
    }
}

impl Release for Cpu {
    #[inline(always)]
    fn context(&self, owner: Domain) -> KernelContext {
        ENTRY.words().context(owner)
    }

    #[inline(always)]
    fn release(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_semaphore_stock_wait_release(owner, task) } {
            0 => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    #[inline(always)]
    fn cancel(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_semaphore_stock_wait_cancel(owner, task) } {
            0 => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }
}

impl Wake for Cpu {
    #[inline(always)]
    fn in_handler(&self) -> bool {
        load(ENTRY.words().handler_context) != 0
    }

    #[inline(always)]
    fn waiter_domain(&self, task: Task) -> Domain {
        match DomainId::from_word(load(task.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => loop {},
        }
    }

    #[inline(always)]
    fn record_id(&self, owner: KernelContext) -> u32 {
        load(owner.id())
    }

    #[inline(always)]
    fn notify(&self, id: u32) -> DispatchTarget {
        if unsafe { xm1_semaphore_stock_notify(id) } != 0 {
            DispatchTarget::Local
        } else {
            DispatchTarget::Remote
        }
    }

    #[inline(always)]
    fn dispatch(&self, code: i32) {
        unsafe { xm1_semaphore_stock_dispatch(code) };
    }

    #[inline(always)]
    fn request_delayed_dispatch(&self) {
        store(ENTRY.words().reschedule, 1);
    }

    #[inline(always)]
    fn finish_dispatch(&self, code: i32) {
        unsafe { xm1_semaphore_stage_dispatch(code) };
    }
}

#[inline(never)]
#[link_section = ".text.sem_signal_body"]
extern "C" fn xm1_semaphore_stage_dispatch(code: i32) {
    if load(ENTRY.words().handler_context) != 0 {
        store(ENTRY.words().reschedule, 1);
    } else {
        unsafe { xm1_semaphore_stock_dispatch(code) };
    }
}

impl Block for Cpu {
    #[inline(always)]
    fn enqueue(
        &self,
        _entry: Domain,
        owner: KernelContext,
        queue: WaitQueue,
        wait: WaitBlock,
        how: Enqueue,
    ) {
        let owner = load(owner.owner());
        match how {
            Enqueue::Plain => unsafe { xm1_semaphore_stock_wait_enqueue(owner, queue, wait) },
            Enqueue::Timed { timer, timeout } => unsafe {
                xm1_semaphore_stock_wait_enqueue_timed(owner, queue, wait, timer.0, timeout)
            },
        }
    }
}

#[inline(always)]
fn admitted(id: i32, service: Service) -> Result<SemaphoreId, SemaphoreStatus> {
    let words = ENTRY.words();
    let entry = if service.owns_its_table() {
        None
    } else {
        Some(Entry {
            handler_context: load(words.handler_context),
            irq_masked: unsafe { cpsr_masked() },
            dispatch_enabled: if service.blocks() {
                load(words.dispatch_enabled)
            } else {
                1
            },
        })
    };
    let request = Request {
        service,
        entry,
        entry_domain: EntryDomain::new(ENTRY),
        id: EncodedId(id),
    };
    admit(&request, |domain| {
        load(words.context(domain).semaphore_count())
    })
}

#[inline(always)]
fn created(semaphore: SemaphoreId) -> Result<(), SemaphoreStatus> {
    require_created(load(
        semaphore.domain.words().semaphore_config(semaphore.slot).exists(),
    ))
}

#[inline(always)]
fn resumed(pending: SemaphorePending) -> i32 {
    match resume(&Cpu, pending) {
        SemaphoreResume::Completed(outcome) => outcome.code(),
        SemaphoreResume::Corrupt(pending) => load(pending.wait.result()) as i32,
        SemaphoreResume::Pending(_) => loop {},
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_admit"]
extern "C" fn xm1_rtos_semaphore_admit(
    id: i32,
    service: Service,
) -> Result<SemaphoreId, SemaphoreStatus> {
    match admitted(id, service) {
        Ok(semaphore) if service.owns_its_table() => Ok(semaphore),
        Ok(semaphore) => match created(semaphore) {
            Ok(()) => Ok(semaphore),
            Err(status) => Err(status),
        },
        Err(status) => Err(status),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_signal_body"]
extern "C" fn xm1_rtos_semaphore_signal_body(semaphore: SemaphoreId, mode: WakeMode) -> i32 {
    match signal(&Cpu, semaphore, mode) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_block_body"]
extern "C" fn xm1_rtos_semaphore_block_body(semaphore: SemaphoreId, timeout: i32) -> i32 {
    let plain = timeout == PLAIN;
    match block(&Cpu, EntryDomain::new(ENTRY), semaphore, timeout, !plain) {
        Ok(SemaphoreBlocking::Ready(outcome)) => outcome.code(),
        Ok(SemaphoreBlocking::Pending(pending)) => resumed(pending),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_create_body"]
extern "C" fn xm1_rtos_semaphore_create_body(semaphore: SemaphoreId, request: u32) -> i32 {
    let create = || SemaphoreCreate {
        attributes: SemaphoreAttributes(load(GuestAddress(request))),
        initial: Count::from_word(load(GuestAddress(request + 4))),
        maximum: Maximum::from_word(load(GuestAddress(request + 8))),
    };
    match create_semaphore(&Cpu, semaphore, create) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_delete_body"]
extern "C" fn xm1_rtos_semaphore_delete_body(semaphore: SemaphoreId) -> i32 {
    match delete_semaphore(&Cpu, semaphore) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_signal"]
pub extern "C" fn xm1_rtos_semaphore_signal(id: i32) -> i32 {
    match xm1_rtos_semaphore_admit(id, Service::Signal) {
        Ok(semaphore) => xm1_rtos_semaphore_signal_body(semaphore, WakeMode::Task),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_isignal"]
pub extern "C" fn xm1_rtos_semaphore_interrupt_signal(id: i32) -> i32 {
    match xm1_rtos_semaphore_admit(id, Service::InterruptSignal) {
        Ok(semaphore) => xm1_rtos_semaphore_signal_body(semaphore, WakeMode::Handler),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_wait"]
pub extern "C" fn xm1_rtos_semaphore_wait(id: i32) -> i32 {
    match xm1_rtos_semaphore_admit(id, Service::Wait) {
        Ok(semaphore) => xm1_rtos_semaphore_block_body(semaphore, PLAIN),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_timed_wait"]
pub extern "C" fn xm1_rtos_semaphore_timed_wait(id: i32, timeout: i32) -> i32 {
    match xm1_rtos_semaphore_admit(id, Service::TimedWait) {
        Ok(semaphore) => xm1_rtos_semaphore_block_body(semaphore, timeout),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_poll"]
pub extern "C" fn xm1_rtos_semaphore_poll(id: i32) -> i32 {
    match xm1_rtos_semaphore_admit(id, Service::Poll) {
        Ok(semaphore) => match poll(&Cpu, semaphore) {
            Ok(outcome) => outcome.code(),
            Err(status) => status.code(),
        },
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_create"]
pub extern "C" fn xm1_rtos_semaphore_create(id: u32, request: u32) -> i32 {
    match xm1_rtos_semaphore_admit(id as i32, Service::Create) {
        Ok(semaphore) => xm1_rtos_semaphore_create_body(semaphore, request),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.sem_delete"]
pub extern "C" fn xm1_rtos_semaphore_delete(id: u32) -> i32 {
    match xm1_rtos_semaphore_admit(id as i32, Service::Delete) {
        Ok(semaphore) => xm1_rtos_semaphore_delete_body(semaphore),
        Err(status) => status.code(),
    }
}
