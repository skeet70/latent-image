use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

use crate::*;

struct Cpu;

extern "C" {
    /// Defined by the linker script at the loader's stock raise-IRQ entry.
    fn xm1_rtos_scheduler_stock_raise_irq(core: IrqCore, irq: Irq);
}

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        unsafe { read_volatile(at.0 as *const u32) }
    }

    #[inline(always)]
    fn load_byte(&self, at: GuestAddress) -> u8 {
        unsafe { read_volatile(at.0 as *const u8) }
    }

    #[inline(always)]
    fn store_word(&self, at: GuestAddress, value: u32) {
        unsafe { write_volatile(at.0 as *mut u32, value) }
    }

    #[inline(always)]
    fn store_byte(&self, at: GuestAddress, value: u8) {
        unsafe { write_volatile(at.0 as *mut u8, value) }
    }

    #[inline(always)]
    fn try_claim(&self, word: GuestAddress) -> bool {
        let status: u32;
        unsafe {
            asm!(
                "ldrex {status}, [{word}]",
                "cmp {status}, #0",
                "strexeq {status}, {one}, [{word}]",
                status = out(reg) status,
                word = in(reg) word.0,
                one = in(reg) 1u32,
                options(nostack),
            )
        };
        status == 0
    }

    #[inline(always)]
    fn dmb(&self) {
        unsafe { asm!("dmb sy", options(nostack, preserves_flags)) };
    }

    #[inline(always)]
    fn dsb(&self) {
        unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
    }

    #[inline(always)]
    fn raise_irq(&self, core: IrqCore, irq: Irq) {
        unsafe { xm1_rtos_scheduler_stock_raise_irq(core, irq) }
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.acquire"]
pub extern "C" fn xm1_rtos_scheduler_acquire(lock: LockRecord) -> Acquired {
    acquire(&Cpu, lock)
}

/// r0 at return is `lock`, as stock.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.release"]
pub extern "C" fn xm1_rtos_scheduler_release(lock: LockRecord) -> LockRecord {
    release(&Cpu, lock);
    lock
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.find"]
pub extern "C" fn xm1_rtos_scheduler_find(record: KernelContext) -> Task {
    find(&Cpu, record)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.insert"]
pub extern "C" fn xm1_rtos_scheduler_insert(record: KernelContext, task: Task) -> Reschedule {
    insert(&Cpu, record, task)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.remove"]
pub extern "C" fn xm1_rtos_scheduler_remove(record: KernelContext, task: Task) -> Reschedule {
    remove(&Cpu, record, task, || xm1_rtos_scheduler_find(record))
}

/// r0 at return is `task`, as stock.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.reset"]
pub extern "C" fn xm1_rtos_scheduler_reset(task: Task) -> Task {
    reset(&Cpu, task);
    task
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.prepare_body"]
pub extern "C" fn xm1_rtos_scheduler_prepare_body(record: KernelContext, task: Task) -> Reschedule {
    prepare(&Cpu, record, task, || xm1_rtos_scheduler_insert(record, task))
}

// Shim: the body takes 48 bytes and the prepare extent 44, so the body sits at the end of the find
// extent. Removable once the stock layout no longer constrains the extents.
global_asm!(
    ".pushsection .text.prepare, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_scheduler_prepare",
    ".type xm1_rtos_scheduler_prepare, %function",
    "xm1_rtos_scheduler_prepare:",
    "b xm1_rtos_scheduler_prepare_body",
    ".size xm1_rtos_scheduler_prepare, . - xm1_rtos_scheduler_prepare",
    ".popsection",
);

#[no_mangle]
#[inline(never)]
#[link_section = ".text.notify"]
pub extern "C" fn xm1_rtos_scheduler_notify(id: RecordId) -> Notified {
    notify(&Cpu, DOMAIN, id)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.requeue"]
pub extern "C" fn xm1_rtos_scheduler_requeue(record: KernelContext, task: Task, to: u32) -> Reschedule {
    // SAFETY: the stock caller passes its validated argument less one or the definition's priority word.
    requeue(&Cpu, record, task, unsafe { stored_priority(to) }, || xm1_rtos_scheduler_find(record))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.rotate"]
pub extern "C" fn xm1_rtos_scheduler_rotate(record: KernelContext, p: u32) -> Reschedule {
    // SAFETY: the stock callers pass a priority.
    rotate(&Cpu, record, unsafe { stored_priority(p) })
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {}
}
