//! Stage adapter: the fixed-pool entries at their stock extents over the retained helpers.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod fixed_pool;

use core::arch::asm;
use fixed_pool::layout::*;
use fixed_pool::*;

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

extern "C" {
    fn xm1_fixed_pool_stock_acquire(lock: LockRecord) -> u32;
    fn xm1_fixed_pool_stock_release(lock: LockRecord) -> LockRecord;
    fn xm1_fixed_pool_stock_notify(id: u32) -> u32;
    fn xm1_fixed_pool_stock_dispatch(code: i32);
    fn xm1_fixed_pool_stock_mask() -> u32;
    fn xm1_fixed_pool_stock_restore(saved: u32);
    fn xm1_fixed_pool_stock_save_irq() -> u32;
    fn xm1_fixed_pool_stock_restore_irq(saved: u32);
    fn xm1_fixed_pool_stock_wait_release(owner: KernelContext, task: Task) -> u32;
    fn xm1_fixed_pool_stock_wait_cancel(owner: KernelContext, task: Task) -> u32;
    fn xm1_fixed_pool_stock_wait_enqueue(owner: u32, queue: WaitQueue, wait: WaitBlock);
    fn xm1_fixed_pool_stock_wait_enqueue_timed(
        owner: u32,
        queue: WaitQueue,
        wait: WaitBlock,
        timer: GuestAddress,
        timeout: i32,
    );
    fn xm1_fixed_pool_stock_uidivmod(numerator: u32, denominator: u32) -> u64;
}

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
fn cpsr_masked() -> bool {
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
        SavedInterrupts(unsafe { xm1_fixed_pool_stock_mask() })
    }

    #[inline(always)]
    fn restore_interrupts(&self, saved: SavedInterrupts) {
        unsafe { xm1_fixed_pool_stock_restore(saved.0) };
    }

    #[inline(always)]
    fn prepare_dispatch(&self, saved: SavedInterrupts) {
        let cpsr: u32;
        unsafe { asm!("mrs {cpsr}, cpsr", cpsr = out(reg) cpsr, options(nostack)) };
        let value = (cpsr & !0xc0) | (saved.0 & 0x40) | 0x80;
        unsafe { asm!("msr cpsr_c, {value}", value = in(reg) value, options(nostack)) };
    }

    #[inline(always)]
    fn dmb(&self) {
        unsafe { asm!("dmb", options(nostack)) };
    }
}

impl Locks for Cpu {
    #[inline(always)]
    fn lock(&self, record: LockRecord) {
        unsafe { xm1_fixed_pool_stock_acquire(record) };
    }

    #[inline(always)]
    fn unlock(&self, record: LockRecord) {
        unsafe { xm1_fixed_pool_stock_release(record) };
    }
}

impl Irq for Cpu {
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

impl SaveIrq for Cpu {
    #[inline(always)]
    fn save_irq(&self) -> u32 {
        unsafe { xm1_fixed_pool_stock_save_irq() }
    }

    #[inline(always)]
    fn restore_irq(&self, saved: u32) {
        unsafe { xm1_fixed_pool_stock_restore_irq(saved) };
    }
}

impl DivMod for Cpu {
    #[inline(always)]
    fn divmod(&self, numerator: u32, denominator: u32) -> (u32, u32) {
        let result = unsafe { xm1_fixed_pool_stock_uidivmod(numerator, denominator) };
        (result as u32, (result >> 32) as u32)
    }
}

impl Wake for Cpu {
    #[inline(always)]
    fn context(&self, domain: Domain) -> KernelContext {
        ENTRY.words().context(domain)
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
        if unsafe { xm1_fixed_pool_stock_notify(id) } != 0 {
            DispatchTarget::Local
        } else {
            DispatchTarget::Remote
        }
    }

    #[inline(always)]
    fn dispatch(&self, code: i32) {
        unsafe { xm1_fixed_pool_stock_dispatch(code) };
    }

    fn request_delayed_dispatch(&self) {
        store(ENTRY.words().reschedule, 1);
    }

    #[inline(always)]
    fn release(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_fixed_pool_stock_wait_release(owner, task) } {
            0 => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }

    #[inline(always)]
    fn cancel(&self, owner: KernelContext, task: Task) -> Selection {
        match unsafe { xm1_fixed_pool_stock_wait_cancel(owner, task) } {
            0 => Selection::Unchanged,
            _ => Selection::Changed,
        }
    }
}

impl Block for Cpu {
    #[inline(always)]
    fn enqueue(
        &self,
        _entry: Domain,
        owner: u32,
        queue: WaitQueue,
        wait: FixedPoolWait,
        how: Enqueue,
    ) {
        match how {
            Enqueue::Plain => unsafe {
                xm1_fixed_pool_stock_wait_enqueue(owner, queue, wait.base())
            },
            Enqueue::Timed { timer, timeout } => unsafe {
                xm1_fixed_pool_stock_wait_enqueue_timed(owner, queue, wait.base(), timer, timeout)
            },
        }
    }
}

#[inline(always)]
fn admitted(id: i32, service: Service, timeout: i32) -> Result<Admitted, PoolStatus> {
    let words = ENTRY.words();
    let owns = matches!(service, Service::Create | Service::Delete);
    let unchecked = service == Service::PollUnchecked;
    let blocks = matches!(service, Service::Get | Service::TimedGet);
    let entry = if owns || unchecked {
        None
    } else {
        Some(Entry {
            handler_context: load(words.handler_context),
            irq_masked: cpsr_masked(),
            dispatch_enabled: if blocks {
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
        counts: Counts {
            a: load(words.context(Domain::A).fixed_pool_count()),
            b: load(words.context(Domain::B).fixed_pool_count()),
        },
        timeout,
    };
    admit(&request, |pool| {
        load(pool.domain.words().fixed_pool_config(pool.slot).exists()) != 0
    })
}

#[inline(always)]
fn resumed(pending: Pending) -> i32 {
    match resume(&Cpu, pending) {
        PoolResume::Completed(outcome) => outcome.code(),
        PoolResume::Corrupt(pending) => load(pending.wait.base().result()) as i32,
        PoolResume::Pending(_) => loop {},
    }
}

#[inline(never)]
#[link_section = ".text.fp_bodies"]
fn create_body(pool: PoolId, request: u32) -> i32 {
    let create = PoolCreate {
        attributes: PoolAttributes(load(GuestAddress(request))),
        block_count: load(GuestAddress(request + 4)),
        block_size: load(GuestAddress(request + 8)),
        buffer: GuestAddress(load(GuestAddress(request + 0x0c))),
    };
    match create_pool(&Cpu, pool, create) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

#[inline(never)]
#[link_section = ".text.fp_bodies"]
fn delete_body(pool: PoolId, handler: bool) -> i32 {
    match delete_pool(&Cpu, pool, handler) {
        Ok(()) => 0,
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_create_body(id: u32, request: u32) -> i32 {
    match admitted(id as i32, Service::Create, 0) {
        Ok(admitted) => create_body(admitted.pool, request),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_delete_body(id: u32) -> i32 {
    match admitted(id as i32, Service::Delete, 0) {
        Ok(admitted) => delete_body(admitted.pool, load(ENTRY.words().handler_context) != 0),
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_get_body(id: i32, out: u32) -> i32 {
    match admitted(id, Service::Get, 0) {
        Ok(admitted) => match get(
            &Cpu,
            EntryDomain::new(ENTRY),
            admitted.pool,
            OutWord(GuestAddress(out)),
            Timeout::Forever,
        ) {
            Ok(PoolBlocking::Ready(outcome)) => outcome.code(),
            Ok(PoolBlocking::Pending(pending)) => resumed(pending),
            Err(status) => status.code(),
        },
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_poll_body(id: i32, out: u32) -> i32 {
    match admitted(id, Service::Poll, 0) {
        Ok(admitted) => match poll(&Cpu, admitted.pool, OutWord(GuestAddress(out))) {
            Ok(outcome) => outcome.code(),
            Err(status) => status.code(),
        },
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_timed_get_body(id: i32, out: u32, timeout: i32) -> i32 {
    match admitted(id, Service::TimedGet, timeout) {
        Ok(admitted) => match get(
            &Cpu,
            EntryDomain::new(ENTRY),
            admitted.pool,
            OutWord(GuestAddress(out)),
            admitted.timeout.unwrap_or(Timeout::Forever),
        ) {
            Ok(PoolBlocking::Ready(outcome)) => outcome.code(),
            Ok(PoolBlocking::Pending(pending)) => resumed(pending),
            Err(status) => status.code(),
        },
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_release_body(id: i32, block: u32) -> i32 {
    match admitted(id, Service::Release, 0) {
        Ok(admitted) => match release(
            &Cpu,
            admitted.pool,
            BlockAddress(GuestAddress(block)),
        ) {
            Ok(()) => 0,
            Err(status) => status.code(),
        },
        Err(status) => status.code(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.fp_bodies"]
pub extern "C" fn xm1_rtos_fixed_pool_poll_unchecked_body(id: i32, out: u32) -> i32 {
    match admitted(id, Service::PollUnchecked, 0) {
        Ok(admitted) => match poll_unchecked(&Cpu, admitted.pool, OutWord(GuestAddress(out))) {
            Ok(outcome) => outcome.code(),
            Err(status) => status.code(),
        },
        Err(status) => status.code(),
    }
}
