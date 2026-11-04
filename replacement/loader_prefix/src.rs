//! Owned X-M1 loader-B core-1 entry: stock `0xd0020`-`0xd00c8`, linked with
//! the owned record-2 startup. `entry.S` holds the stack switch, the exit to
//! loader-B startup `0xd09ac` and the clear loop.

#![no_std]

#[allow(unused_attributes)]
#[path = "../secondary_stage/src.rs"]
mod secondary_stage;

mod adapter;

use adapter::*;
use core::arch::asm;
use core::ptr::{addr_of, write_volatile};

/// The CP14 `(0,c2,c0,0)` discriminator: `Zero` for `0x4003`, `One` otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Core {
    Zero,
    One,
}

impl Core {
    unsafe fn current() -> Self {
        let value: u32;
        asm!("mrc p14, 0, {0}, c2, c0, 0", out(reg) value, options(nomem, nostack, preserves_flags));
        if value == DISCRIMINATOR_PRIMARY {
            Core::Zero
        } else {
            Core::One
        }
    }

    fn vector_base(self) -> VectorBase {
        match self {
            Core::Zero => VectorBase(VECTOR_BASE_PRIMARY),
            Core::One => VectorBase(VECTOR_BASE_SECONDARY),
        }
    }
}

unsafe fn install_vectors(base: VectorBase) {
    VECTOR_HANDLERS
        .iter()
        .enumerate()
        .for_each(|(slot, &handler)| write_volatile((base.0 + VECTOR_SLOTS_OFFSET + 4 * slot as u32) as *mut u32, handler));
}

/// Stock `0xd0090`, CP15 `c7,c10,4`.
unsafe fn vector_barrier() {
    asm!("mcr p15, 0, {0}, c7, c10, 4", in(reg) 0u32, options(nostack, preserves_flags));
}

unsafe fn stack_top() -> u32 {
    addr_of!(xm1_loader_b_stack_top) as u32
}

// Synthetic core-refusal fault: reject the qualified core-1 discriminator.
#[cfg(loader_prefix_core_refusal)]
#[no_mangle]
#[inline(never)]
pub(crate) extern "C" fn xm1_loader_prefix_refusal() -> ! {
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) }
    }
}

/// Stock `0xd0020`-`0xd00a4`: vectors, barrier and the stack-window clear,
/// on the entry stack.
#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_loader_prefix_install() -> VectorBase {
    #[cfg(loader_prefix_core_refusal)]
    if Core::current() != Core::Zero {
        xm1_loader_prefix_refusal()
    }
    let base = Core::current().vector_base();
    install_vectors(base);
    vector_barrier();
    xm1_loader_prefix_clear(STACK_WINDOW_START, stack_top());
    base
}

/// Stock `0xd00b0`-`0xd00c0`, on the loader-B stack.
#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_loader_prefix_clear_workspace() {
    xm1_loader_prefix_clear(WORKSPACE_START, STACK_WINDOW_START);
    // Synthetic fill fault: leave the last workspace word nonzero.
    #[cfg(loader_prefix_fill_fault)]
    xm1_loader_prefix_fill_fault();
}

#[cfg(loader_prefix_fill_fault)]
#[no_mangle]
#[inline(never)]
unsafe extern "C" fn xm1_loader_prefix_fill_fault() {
    write_volatile((STACK_WINDOW_START - 4) as *mut u32, 1);
}
