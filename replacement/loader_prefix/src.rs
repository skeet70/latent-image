//! Owned X-M1 loader-B core-1 prefix: stock entry `0xd0020`-`0xd00c8`, the
//! core decision `0xd09ac`-`0xd09d0` and the completion wait
//! `0xd0a44`-`0xd0a64`, linked with the owned record-2 startup. `entry.S`
//! holds the stack switch and the loader-B frame.

#![no_std]

#[allow(unused_attributes)]
#[path = "../secondary_stage/src.rs"]
mod secondary_stage;

mod adapter;

use adapter::*;
use core::arch::asm;
use core::ptr::{addr_of, read_volatile, write_volatile};

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

/// Loader A's completion word; any nonzero value ends the wait.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct CompletionWord(u32);

impl CompletionWord {
    #[cfg(not(loader_prefix_predicate_fault))]
    fn is_complete(self) -> bool {
        self.0 != 0
    }

    #[cfg(loader_prefix_predicate_fault)]
    fn is_complete(self) -> bool {
        self.0 == 1
    }
}

#[cfg(not(loader_prefix_fill_fault))]
const WORKSPACE_SHORTFALL: u32 = 0;
#[cfg(loader_prefix_fill_fault)]
const WORKSPACE_SHORTFALL: u32 = 4;

unsafe fn clear_words(start: u32, end: u32) {
    (0..(end - start) / 4).for_each(|word| write_volatile((start + 4 * word) as *mut u32, 0));
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
    clear_words(STACK_WINDOW_START, stack_top());
    base
}

/// Stock `0xd00b0`-`0xd00c0`, on the loader-B stack.
#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_loader_prefix_clear_workspace() {
    clear_words(WORKSPACE_START, STACK_WINDOW_START - WORKSPACE_SHORTFALL);
}

#[no_mangle]
#[inline(never)]
pub(crate) unsafe extern "C" fn xm1_loader_prefix_completion_cell() -> CompletionWord {
    CompletionWord(read_volatile(COMPLETION))
}

/// Stock `0xd0a44`-`0xd0a60`.
unsafe fn completion_wait() {
    while !xm1_loader_prefix_completion_cell().is_complete() {
        (0..COMPLETION_DELAY_ITERATIONS).for_each(|_| asm!("nop", options(nomem, nostack)));
    }
}

/// Stock `0xd09b8`-`0xd09cc` and the wait; called with the loader-B frame
/// formed. The entry assembly branches to the result.
#[no_mangle]
pub(crate) unsafe extern "C" fn xm1_loader_prefix_dispatch() -> Continuation {
    match Core::current() {
        Core::Zero => Continuation::of(xm1_loader_b_core0_continuation),
        Core::One => {
            completion_wait();
            Continuation::of(xm1_loader_b_core1_continuation)
        }
    }
}
