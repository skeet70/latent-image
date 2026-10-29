//! Owned X-M1 first-stage startup control. The stock media/decompression body
//! and pin table stay at their reviewed addresses.

#![no_std]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

const CELL_PEER0: *mut u32 = 0x4002_0000 as *mut u32;
const CELL_PEER1: *mut u32 = 0x4002_0004 as *mut u32;
const PEER_READY: u32 = 0x22b;

const LOG_POINTER: *mut u32 = 0x4000_42e4 as *mut u32;
const LOG_STORE: u32 = 0x400a_0040;
const LOG_INDEX_0: *mut u32 = 0x4000_42e8 as *mut u32;
const LOG_INDEX_1: *mut u32 = 0x4000_42ec as *mut u32;
const LOG_END: u32 = 0x5354_4544;

const TIMER_COUNTER: *const u32 = 0xfff9_1000 as *const u32;

const DATA_SKIP_POINTER: *const u32 = 0x0000_4804 as *const u32;
const SEGMENT_TABLE_PUBLICATION: *const u32 = 0x0061_c00c as *const u32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Core {
    Zero,
    One,
}

impl Core {
    fn from_discriminator(value: u32) -> Self {
        if value == 0x4003 {
            Core::Zero
        } else {
            Core::One
        }
    }

    fn c_abi(self) -> i32 {
        match self {
            Core::Zero => 0,
            Core::One => 1,
        }
    }

    fn log_channel(self) -> LogChannel {
        match self {
            Core::Zero => LogChannel::Zero,
            Core::One => LogChannel::One,
        }
    }
}

#[derive(Clone, Copy)]
enum LogChannel {
    Zero,
    One,
}

extern "C" {
    /// Stock record 1 core-0 peripheral and pin-table init (`0x25d4`).
    fn xm1_retained_core_init();
    /// Stock record 1 storage controller init (`0x2034`).
    fn xm1_retained_storage_init() -> i32;
    /// Stock record 1 bad-block scan (`0x2058`).
    fn xm1_retained_skip_scan() -> i32;
    /// Stock record 1 segment-table read (`0x1dc4`).
    fn xm1_retained_segment_table();
    /// Stock record 1 trailer gate (`0x1cc0`).
    fn xm1_retained_trailer_check() -> i32;
    /// Stock record 1 loader copy, image-4 decompression and bss fill (`0x1d08`).
    fn xm1_retained_stage_images(skip: i32, core: i32) -> i32;
    /// Stock record 1 storage teardown (`0x2044`).
    fn xm1_retained_storage_fini() -> i32;
    /// Labelled assembly handoff at the final `mov pc`.
    fn xm1_first_stage_handoff() -> !;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    halt()
}

fn halt() -> ! {
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) }
    }
}

#[inline(always)]
unsafe fn write(address: *mut u32, value: u32) {
    write_volatile(address, value);
}

#[inline(always)]
unsafe fn read(address: *const u32) -> u32 {
    read_volatile(address)
}

unsafe fn log_event(value: u32, channel: LogChannel) {
    let pointer = read(LOG_POINTER);
    if pointer == 0x3333_3333 {
        return;
    }
    let (index, limit) = match channel {
        LogChannel::Zero => (LOG_INDEX_0, 0x10),
        LogChannel::One => (LOG_INDEX_1, 0x20),
    };
    let slot = read(index);
    if slot >= limit {
        return;
    }
    let base = pointer as *mut u32;
    write(base.add(slot as usize), read(TIMER_COUNTER));
    write(base.add(slot as usize + 1), value);
    write(index, slot + 2);
    write(base.add(slot as usize + 2), LOG_END);
}

unsafe fn core_discriminator() -> Core {
    let value: u32;
    asm!("mrc p14, 0, {0}, c2, c0, 0", out(reg) value);
    Core::from_discriminator(value)
}

unsafe fn mpu_region(region: u32, base: u32, size: u32, access: u32) {
    asm!(
        "mcr p15, 0, {region}, c6, c2, 0",
        "mcr p15, 0, {size}, c6, c1, 4",
        "mcr p15, 0, {base}, c6, c1, 0",
        "mcr p15, 0, {access}, c6, c1, 2",
        region = in(reg) region,
        size = in(reg) size,
        base = in(reg) base,
        access = in(reg) access,
    );
}

/// Stock record MPU regions 0-4 at 0x19e4-0x1a9c.
unsafe fn configure_mpu(core: Core) {
    mpu_region(0, 0x0000_0000, 0x303, 0x3b);
    mpu_region(1, 0x4000_0000, 0x30c, 0x3b);
    mpu_region(2, 0x8000_0000, 0x305, 0x3d);
    mpu_region(3, 0xfff0_0000, 0x30c, 0x17);
    if core != Core::Zero {
        mpu_region(4, 0xffff_0000, 0x303, 0x1f);
    }
    let control: u32;
    asm!("mrc p15, 0, {0}, c1, c0, 0", out(reg) control);
    let enabled = control | 0x1005;
    asm!(
        "dsb sy",
        "mcr p15, 0, {zero}, c15, c5, 0",
        "mcr p15, 0, {zero}, c7, c5, 0",
        "mcr p15, 0, {value}, c1, c0, 0",
        "isb sy",
        zero = in(reg) 0u32,
        value = in(reg) enabled,
    );
}

/// The second context starts from RAM 0x10000 and reads its vector table from
/// physical 0x3fff0000 through the 0x7fff0000 alias.
unsafe fn copy_peer_vectors() {
    let source = 0x0001_0000 as *const u32;
    let destination = 0x7fff_0000 as *mut u32;
    let mut offset = 0usize;
    while offset < 0x3e8 / 4 {
        write(destination.add(offset), read(source.add(offset)));
        offset += 1;
    }
}

unsafe fn release_peer() {
    write(0xfffb_408c as *mut u32, 0x0100_0000);
    write(0xfffb_40cc as *mut u32, 0x0100_0000);
    write(0xfffb_40ac as *mut u32, 0x0100_0000);
    write(0xfffb_40ec as *mut u32, 0x0100_0000);
}

unsafe fn core0_clock(core: Core) {
    if core == Core::Zero && read(0xfff8_9314 as *const u32) & 0x80 == 0 {
        let value = read(0xfff8_8028 as *const u32) & !0x000f_0000;
        write(0xfff8_8028 as *mut u32, value);
        let value = read(0xfff8_8030 as *const u32) | 0x200;
        write(0xfff8_8030 as *mut u32, value);
    }
}

unsafe fn peer_rendezvous(core: Core) {
    write(CELL_PEER0, PEER_READY);
    write(CELL_PEER1, PEER_READY);
    write(LOG_POINTER, LOG_STORE);
    log_event(0x0100_0000, core.log_channel());
    copy_peer_vectors();
    release_peer();
}

/// Stock record 1's init-body control sequence (0x1e4c); media and
/// decompression are retained.
unsafe fn stage_retained_next(core: Core) {
    core0_clock(core);

    if xm1_retained_storage_init() != 0 {
        halt();
    }

    let mut status = 0i32;
    if core == Core::Zero {
        log_event(0x0100_0100, LogChannel::Zero);
        let scan = xm1_retained_skip_scan();
        log_event(0x1100_0100, LogChannel::Zero);
        if scan != 0 {
            halt();
        }
        xm1_retained_segment_table();
        let publication = read(SEGMENT_TABLE_PUBLICATION) as *mut u32;
        write(publication.add(1), read(DATA_SKIP_POINTER));
        status = xm1_retained_trailer_check();
        if status != 0 {
            halt();
        }
    } else {
        let publication = read(SEGMENT_TABLE_PUBLICATION) as *const u32;
        write(DATA_SKIP_POINTER as *mut u32, read(publication.add(1)));
    }

    log_event(0x1100_0000, core.log_channel());
    if xm1_retained_stage_images(status, core.c_abi()) != 0 {
        halt();
    }
    let _ = xm1_retained_storage_fini();

    write(0x400a_00c4 as *mut u32, 0);
    write(CELL_PEER0, 0);
    asm!("dsb sy");
    while read(CELL_PEER1) == PEER_READY {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_first_stage_main() -> ! {
    let core = core_discriminator();
    if core == Core::Zero {
        xm1_retained_core_init();
    }
    configure_mpu(core);
    peer_rendezvous(core);
    stage_retained_next(core);
    xm1_first_stage_handoff()
}
