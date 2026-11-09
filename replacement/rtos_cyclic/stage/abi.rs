//! CYCLIC A32 stage adapter: the stock entries over the owned lifecycle core.

use core::arch::{asm, global_asm};
use crate::layout::{Domain, GuestAddress, KernelContext};
use crate::{
    configure, create_from_request, delete, expire, initialize, schedule, start, stop,
    CyclicArgument, CyclicControl, CyclicHandler, EncodedId, Expiry, Machine, SavedInterrupts,
};

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

#[cfg(feature = "domain-a")]
const CONFIGURATION: GuestAddress = GuestAddress(0x400c_0818);
#[cfg(feature = "domain-b")]
const CONFIGURATION: GuestAddress = GuestAddress(0x400c_0888);

#[cfg(feature = "domain-a")]
const TIMER_INSERT: u32 = 0x44c60;
#[cfg(feature = "domain-b")]
const TIMER_INSERT: u32 = 0xd4c20;
#[cfg(feature = "domain-a")]
const TIMER_REMOVE: u32 = 0x44ca8;
#[cfg(feature = "domain-b")]
const TIMER_REMOVE: u32 = 0xd4c68;

#[cfg(feature = "domain-a")]
const EXPIRE_ENTRY: u32 = 0x4beb0;
#[cfg(feature = "domain-b")]
const EXPIRE_ENTRY: u32 = 0xdbe70;

#[cfg(feature = "domain-a")]
const CYCLIC_COUNT_WORD: u32 = 0x0004_dacc;
#[cfg(feature = "domain-b")]
const CYCLIC_COUNT_WORD: u32 = 0x000d_da90;

#[inline(always)]
fn read(address: u32) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
fn write(address: u32, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
fn cpsr() -> u32 {
    let value: u32;
    unsafe {
        asm!("mrs {}, cpsr", out(reg) value);
    }
    value
}

#[inline(always)]
fn set_control(value: u32) {
    unsafe {
        asm!("msr cpsr_c, {}", in(reg) value);
    }
}

#[inline(always)]
fn current_context() -> KernelContext {
    KernelContext(GuestAddress(read(ENTRY.words().current_context.0)))
}

struct Cpu;

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        read(at.0)
    }

    #[inline(always)]
    fn store_word(&self, at: GuestAddress, value: u32) {
        write(at.0, value);
    }

    #[inline(always)]
    fn mask_interrupts(&self) -> SavedInterrupts {
        let old = cpsr();
        set_control((old & !0xc0) | 0xc0);
        SavedInterrupts(old & 0xc0)
    }

    #[inline(always)]
    fn restore_interrupts(&self, saved: SavedInterrupts) {
        let current = cpsr();
        set_control((current & !0xc0) | (saved.0 & 0xc0));
    }

    #[inline(always)]
    fn handler_context(&self) -> u32 {
        read(ENTRY.words().handler_context.0)
    }

    #[inline(always)]
    fn irq_masked(&self) -> bool {
        cpsr() & 0x80 != 0
    }

    #[inline(always)]
    fn irq_set(&self) {
        set_control(cpsr() | 0x80);
    }

    #[inline(always)]
    fn irq_clear(&self) {
        set_control(cpsr() & !0x80);
    }

    #[inline(always)]
    fn current_context(&self) -> KernelContext {
        current_context()
    }

    #[inline(always)]
    fn context(&self, domain: Domain) -> KernelContext {
        ENTRY.words().context(domain)
    }

    #[inline(always)]
    fn acquire_lock(&self, context: KernelContext) -> bool {
        let lock = context.scheduler_lock();
        let acquire: extern "C" fn(u32) -> i32 = unsafe { core::mem::transmute(read(lock.acquire().0)) };
        acquire(lock.0 .0) == 0
    }

    #[inline(always)]
    fn release_lock(&self, context: KernelContext) {
        let lock = context.scheduler_lock();
        let release: extern "C" fn(u32) -> i32 = unsafe { core::mem::transmute(read(lock.release().0)) };
        let _ = release(lock.0 .0);
    }

    #[inline(always)]
    fn insert_timer(&self, context: KernelContext, entry: GuestAddress, expiry: u32) -> bool {
        if read(entry.0) != 0 {
            return false;
        }
        let insert: extern "C" fn(u32, u32, u32) = unsafe { core::mem::transmute(TIMER_INSERT) };
        insert(context.0 .0, entry.0, expiry);
        read(entry.0) != 0
    }

    #[inline(always)]
    fn remove_timer(&self, context: KernelContext, entry: GuestAddress) -> bool {
        let remove: extern "C" fn(u32, u32) = unsafe { core::mem::transmute(TIMER_REMOVE) };
        remove(context.0 .0, entry.0);
        read(entry.0) == 0
    }

    #[inline(always)]
    fn invoke(&self, handler: CyclicHandler, argument: CyclicArgument) {
        let invoke: extern "C" fn(u32) = unsafe { core::mem::transmute(handler.0) };
        invoke(argument.0);
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_configure"]
pub extern "C" fn xm1_rtos_cyclic_configure_body() {
    configure(
        &Cpu,
        CONFIGURATION,
        ENTRY.words().cyclic_controls,
        read(CYCLIC_COUNT_WORD),
        EXPIRE_ENTRY,
    );
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_schedule"]
pub extern "C" fn xm1_rtos_cyclic_schedule_body(context: u32, cyclic: u32, expiry: u32) {
    let _ = schedule(
        &Cpu,
        KernelContext(GuestAddress(context)),
        CyclicControl(GuestAddress(cyclic)),
        Expiry(expiry),
    );
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_expire"]
pub extern "C" fn xm1_rtos_cyclic_expire_body(argument: u32) {
    let _ = expire(&Cpu, CyclicControl(GuestAddress(argument)));
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_initialize"]
pub extern "C" fn xm1_rtos_cyclic_initialize_body() {
    let context = current_context();
    initialize(
        &Cpu,
        context,
        ENTRY.words().cyclic_configs,
        ENTRY.words().cyclic_controls,
        read(context.cyclic_count().0),
    );
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_start"]
pub extern "C" fn xm1_rtos_cyclic_start_body(id: i32) -> i32 {
    start(&Cpu, ENTRY, EncodedId(id)).code()
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_stop"]
pub extern "C" fn xm1_rtos_cyclic_stop_body(id: i32) -> i32 {
    stop(&Cpu, ENTRY, EncodedId(id)).code()
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_create"]
pub extern "C" fn xm1_rtos_cyclic_create_body(id: u32, request: u32) -> i32 {
    create_from_request(&Cpu, ENTRY, EncodedId(id as i32), GuestAddress(request)).code()
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.cyclic_body_delete"]
pub extern "C" fn xm1_rtos_cyclic_delete_body(id: u32) -> i32 {
    delete(&Cpu, ENTRY, EncodedId(id as i32)).code()
}

global_asm!(
    ".pushsection .text.cyclic_configure, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_configure",
    ".type xm1_rtos_cyclic_configure, %function",
    "xm1_rtos_cyclic_configure:",
    "b xm1_rtos_cyclic_configure_body",
    ".size xm1_rtos_cyclic_configure, . - xm1_rtos_cyclic_configure",
    ".popsection",
    ".pushsection .text.cyclic_schedule, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_schedule",
    ".type xm1_rtos_cyclic_schedule, %function",
    "xm1_rtos_cyclic_schedule:",
    "b xm1_rtos_cyclic_schedule_body",
    ".size xm1_rtos_cyclic_schedule, . - xm1_rtos_cyclic_schedule",
    ".popsection",
    ".pushsection .text.cyclic_expire, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_expire",
    ".type xm1_rtos_cyclic_expire, %function",
    "xm1_rtos_cyclic_expire:",
    "b xm1_rtos_cyclic_expire_body",
    ".size xm1_rtos_cyclic_expire, . - xm1_rtos_cyclic_expire",
    ".popsection",
    ".pushsection .text.cyclic_initialize, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_initialize",
    ".type xm1_rtos_cyclic_initialize, %function",
    "xm1_rtos_cyclic_initialize:",
    "b xm1_rtos_cyclic_initialize_body",
    ".size xm1_rtos_cyclic_initialize, . - xm1_rtos_cyclic_initialize",
    ".popsection",
    ".pushsection .text.cyclic_start, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_start",
    ".type xm1_rtos_cyclic_start, %function",
    "xm1_rtos_cyclic_start:",
    "b xm1_rtos_cyclic_start_body",
    ".size xm1_rtos_cyclic_start, . - xm1_rtos_cyclic_start",
    ".popsection",
    ".pushsection .text.cyclic_stop, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_stop",
    ".type xm1_rtos_cyclic_stop, %function",
    "xm1_rtos_cyclic_stop:",
    "b xm1_rtos_cyclic_stop_body",
    ".size xm1_rtos_cyclic_stop, . - xm1_rtos_cyclic_stop",
    ".popsection",
    ".pushsection .text.cyclic_create, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_create",
    ".type xm1_rtos_cyclic_create, %function",
    "xm1_rtos_cyclic_create:",
    "b xm1_rtos_cyclic_create_body",
    ".size xm1_rtos_cyclic_create, . - xm1_rtos_cyclic_create",
    ".popsection",
    ".pushsection .text.cyclic_delete, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_cyclic_delete",
    ".type xm1_rtos_cyclic_delete, %function",
    "xm1_rtos_cyclic_delete:",
    "b xm1_rtos_cyclic_delete_body",
    ".size xm1_rtos_cyclic_delete, . - xm1_rtos_cyclic_delete",
    ".popsection",
);

#[cfg(all(target_arch = "arm", not(test), not(feature = "component")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
