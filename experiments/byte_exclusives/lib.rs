//! Byte and halfword exclusives (#157). On the first entry into the stock modal
//! dispatcher `FUN_00bfb028` it runs each width once with a store that succeeds
//! and once with a store after CLREX that must fail, then a Rust `AtomicU8` and
//! `AtomicU16` read-modify-write, and records what it saw in `BYTE_EXCLUSIVES`.
#![no_std]

use core::arch::asm;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicU16, AtomicU8, Ordering};

#[repr(C)]
pub struct Regs {
    pub r: [u32; 13],
    pub lr: u32,
}

#[repr(C)]
pub struct Pair {
    pub loaded: u32,
    pub status: u32,
    pub after: u32,
}

#[repr(C)]
pub struct Record {
    pub magic: u32,
    pub entries: u32,
    pub byte_stored: Pair,
    pub byte_failed: Pair,
    pub half_stored: Pair,
    pub half_failed: Pair,
    pub atomic_byte: u32,
    pub atomic_half: u32,
    pub done: u32,
}

const EMPTY: Pair = Pair { loaded: 0, status: 0xffff_ffff, after: 0 };

/// Read it with `--read BYTE_EXCLUSIVES:68`. `done` is 1 once every field is written.
#[no_mangle]
pub static mut BYTE_EXCLUSIVES: Record = Record {
    magic: u32::from_le_bytes(*b"BXCL"),
    entries: 0,
    byte_stored: EMPTY,
    byte_failed: EMPTY,
    half_stored: EMPTY,
    half_failed: EMPTY,
    atomic_byte: 0,
    atomic_half: 0,
    done: 0,
};

#[no_mangle]
pub static mut BYTE_CELL: u8 = 0x5a;
#[no_mangle]
pub static mut HALF_CELL: u16 = 0x1234;
static ATOMIC_BYTE: AtomicU8 = AtomicU8::new(7);
static ATOMIC_HALF: AtomicU16 = AtomicU16::new(0x0100);

fn uncached<T>(pointer: *mut T) -> *mut T {
    ((pointer as usize) | 0x4000_0000) as *mut T
}

unsafe fn byte_pair(cell: *mut u8, value: u32, clear: bool) -> Pair {
    let (loaded, status): (u32, u32);
    if clear {
        asm!("mrs {c}, cpsr", "cpsid i", "ldrexb {l}, [{p}]", "clrex", "strexb {s}, {v}, [{p}]", "msr cpsr_c, {c}",
            p = in(reg) cell, v = in(reg) value, l = out(reg) loaded, s = out(reg) status, c = out(reg) _);
    } else {
        asm!("mrs {c}, cpsr", "cpsid i", "ldrexb {l}, [{p}]", "strexb {s}, {v}, [{p}]", "msr cpsr_c, {c}",
            p = in(reg) cell, v = in(reg) value, l = out(reg) loaded, s = out(reg) status, c = out(reg) _);
    }
    Pair { loaded, status, after: u32::from(read_volatile(cell)) }
}

unsafe fn half_pair(cell: *mut u16, value: u32, clear: bool) -> Pair {
    let (loaded, status): (u32, u32);
    if clear {
        asm!("mrs {c}, cpsr", "cpsid i", "ldrexh {l}, [{p}]", "clrex", "strexh {s}, {v}, [{p}]", "msr cpsr_c, {c}",
            p = in(reg) cell, v = in(reg) value, l = out(reg) loaded, s = out(reg) status, c = out(reg) _);
    } else {
        asm!("mrs {c}, cpsr", "cpsid i", "ldrexh {l}, [{p}]", "strexh {s}, {v}, [{p}]", "msr cpsr_c, {c}",
            p = in(reg) cell, v = in(reg) value, l = out(reg) loaded, s = out(reg) status, c = out(reg) _);
    }
    Pair { loaded, status, after: u32::from(read_volatile(cell)) }
}

#[no_mangle]
pub extern "C" fn byte_exclusives_on_dispatch(_: &mut Regs) {
    let record = uncached(addr_of_mut!(BYTE_EXCLUSIVES));
    unsafe {
        let entries = read_volatile(addr_of_mut!((*record).entries));
        write_volatile(addr_of_mut!((*record).entries), entries.wrapping_add(1));
        if entries != 0 {
            return;
        }
        let byte = addr_of_mut!(BYTE_CELL);
        let half = addr_of_mut!(HALF_CELL);
        write_volatile(addr_of_mut!((*record).byte_stored), byte_pair(byte, 0x1a5, false));
        write_volatile(addr_of_mut!((*record).byte_failed), byte_pair(byte, 0x33, true));
        write_volatile(addr_of_mut!((*record).half_stored), half_pair(half, 0x1_abcd, false));
        write_volatile(addr_of_mut!((*record).half_failed), half_pair(half, 0x4444, true));
        let previous = ATOMIC_BYTE.fetch_add(3, Ordering::SeqCst);
        write_volatile(addr_of_mut!((*record).atomic_byte), u32::from(previous) << 8 | u32::from(ATOMIC_BYTE.load(Ordering::SeqCst)));
        let previous = ATOMIC_HALF
            .compare_exchange(0x0100, 0x0302, Ordering::SeqCst, Ordering::SeqCst)
            .unwrap_or(0xffff);
        write_volatile(addr_of_mut!((*record).atomic_half), u32::from(previous) << 16 | u32::from(ATOMIC_HALF.load(Ordering::SeqCst)));
        write_volatile(addr_of_mut!((*record).done), 1);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
