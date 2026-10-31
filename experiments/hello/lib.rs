//! Counts entries into the stock modal dispatcher `FUN_00bfb028`, which every
//! MENU open and key press reaches, and keeps the arguments of the last one.
#![no_std]

use core::ptr::{addr_of_mut, read_volatile, write_volatile};

/// The hooked function's r0-r12 and lr at entry, as the trampoline saved them.
#[repr(C)]
pub struct Regs {
    pub r: [u32; 13],
    pub lr: u32,
}

#[repr(C)]
pub struct Hello {
    pub magic: u32,
    pub calls: u32,
    pub last_args: [u32; 4],
    pub last_lr: u32,
}

/// Read it with `read-ram` at the `HELLO` address in experiment.json, or at
/// that address plus 0x40000000.
#[no_mangle]
pub static mut HELLO: Hello = Hello { magic: u32::from_le_bytes(*b"HELO"), calls: 0, last_args: [0; 4], last_lr: 0 };

/// The same DRAM through the stock MPU's non-cacheable window, so a store is
/// visible to the other core and over USB at once.
fn uncached<T>(pointer: *mut T) -> *mut T {
    ((pointer as usize) | 0x4000_0000) as *mut T
}

#[no_mangle]
pub extern "C" fn hello_on_dispatch(regs: &mut Regs) {
    let hello = uncached(addr_of_mut!(HELLO));
    unsafe {
        write_volatile(addr_of_mut!((*hello).calls), read_volatile(addr_of_mut!((*hello).calls)).wrapping_add(1));
        for (index, value) in regs.r[..4].iter().enumerate() {
            write_volatile(addr_of_mut!((*hello).last_args[index]), *value);
        }
        write_volatile(addr_of_mut!((*hello).last_lr), regs.lr);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
