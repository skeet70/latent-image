#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;

#[path = "../rtos_reservation/src/lib.rs"]
mod reservation;

use reservation::TASK_POOL;

const POOL_ALLOCATABLE: u32 = TASK_POOL.allocatable_bytes();

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TaskIndex(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct RequestedBytes(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct GuestAddress(u32);

extern "C" {
    static xm1_task_storage_pool_base_literal: GuestAddress;
}

#[repr(i32)]
pub enum ReservationResult {
    Reserved = 0,
    SizeOrCapacity = -33,
    AlreadyReserved = -18,
}

impl GuestAddress {
    #[inline(always)]
    fn offset(self, bytes: u32) -> Self {
        Self(self.0.wrapping_add(bytes))
    }

    #[inline(always)]
    unsafe fn load(self) -> u32 {
        let value;
        unsafe {
            asm!("ldr {value}, [{address}]", address = in(reg) self.0,
                value = out(reg) value, options(nostack, preserves_flags));
        }
        value
    }

    #[inline(always)]
    unsafe fn store(self, value: u32) {
        unsafe {
            asm!("str {value}, [{address}]", address = in(reg) self.0,
                value = in(reg) value, options(nostack, preserves_flags));
        }
    }
}

/// # Safety
/// The retained helper prelude must have completed. Each reached guest access must be mapped with the target's word-access semantics. Guest writes must not overlap executing code or the adapter/Rust stack. No Rust references may cover the guest storage. Interrupt, synchronization, mapping and storage-lifetime premises remain those of the retained caller.
#[no_mangle]
pub unsafe extern "C" fn xm1_task_storage_transition(
    index: TaskIndex,
    requested: RequestedBytes,
    output: GuestAddress,
) -> ReservationResult {
    let record = GuestAddress(0x40a0_fc44).offset(index.0.wrapping_mul(12));
    let previous = unsafe { record.load() };
    if previous != 0 {
        if unsafe { record.offset(4).load() } != requested.0 {
            return ReservationResult::SizeOrCapacity;
        }
        if unsafe { record.offset(8).load() } != 0 {
            return ReservationResult::AlreadyReserved;
        }
        unsafe {
            output.store(previous);
            record.offset(8).store(1);
        }
        return ReservationResult::Reserved;
    }

    let cursor = GuestAddress(0x4089_ea98);
    let current = unsafe { cursor.load() };
    let pool_base = unsafe {
        let value;
        asm!("ldr {value}, {literal}", value = out(reg) value,
            literal = sym xm1_task_storage_pool_base_literal,
            options(nostack, preserves_flags));
        GuestAddress(value)
    };
    // The cursor-domain invariant is pool_base <= current <= pool_base + POOL_ALLOCATABLE.
    let remaining = pool_base.0.wrapping_add(POOL_ALLOCATABLE).wrapping_sub(current);
    if requested.0 > remaining {
        return ReservationResult::SizeOrCapacity;
    }
    unsafe {
        output.store(current & !0x4000_0000);
        let current = cursor.load();
        cursor.store(current.wrapping_add(requested.0 & !3));
        let published = output.load();
        record.store(published);
        record.offset(4).store(requested.0);
        record.offset(8).store(1);
    }
    ReservationResult::Reserved
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
