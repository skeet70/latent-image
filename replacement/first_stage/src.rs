//! Owned X-M1 first-record startup and media staging.

#![no_std]

mod format;
mod media;
mod images;
mod engine;

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
    /// Labelled assembly handoff at the final `mov pc`.
    fn xm1_first_stage_handoff() -> !;
    /// Stores the kernel timer counter at `slot` through the labelled `xm1_first_stage_log_read`/`_store` pair.
    fn xm1_first_stage_log_timestamp(slot: *mut u32);
    /// Zeroes 0x400a00c4 through a literal-loaded base and a zero-offset store.
    fn xm1_first_stage_completion_zero();
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
    xm1_first_stage_log_timestamp(base.add(slot as usize));
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
    asm!("dsb sy", options(nostack));
    release_peer();
}

enum BootError {
    Media(media::MediaError),
    Image(images::ImageError),
    Publication,
}

/// Record-1 failure GPIO sequence at 0x1de8..0x1e3c.
unsafe fn fail(error: BootError) -> ! {
    match error {
        BootError::Media(_) | BootError::Image(_) | BootError::Publication => {}
    }
    let mut port = 0xfff8_9018usize;
    while port != 0xfff8_9418 {
        write(port as *mut u32, 0);
        for offset in [0x0c, 0x08, 0x10, 0x14, 0x18] {
            write((port - offset) as *mut u32, 0xffff_0000);
        }
        port += 0x40;
    }
    let address = 0xfff8_8084 as *mut u32;
    write(address, read(address) & !1);
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn skip_publication() -> Result<*mut u32, BootError> {
    let pointer = read(SEGMENT_TABLE_PUBLICATION);
    let physical = pointer & !0x4000_0000;
    if pointer & 3 != 0 || physical < 0x0061_c000 || physical > 0x0061_c3f8 {
        return Err(BootError::Publication);
    }
    Ok(pointer as *mut u32)
}

unsafe fn stage_next(core: Core) -> Result<(), BootError> {
    core0_clock(core);
    media::xm1_first_stage_storage_init().map_err(BootError::Media)?;
    if core == Core::Zero {
        log_event(0x0100_0100, LogChannel::Zero);
        let scan = media::scan();
        log_event(0x1100_0100, LogChannel::Zero);
        scan.map_err(BootError::Media)?;
        images::segment_table(&mut images::VolatileMedia).map_err(BootError::Image)?;
        write(skip_publication()?.add(1), read(DATA_SKIP_POINTER));
        images::xm1_first_stage_trailer_check().map_err(BootError::Image)?;
    } else {
        write(DATA_SKIP_POINTER as *mut u32, read(skip_publication()?.add(1)));
    }
    log_event(0x1100_0000, core.log_channel());
    images::xm1_first_stage_stage_images(core == Core::Zero).map_err(BootError::Image)?;
    media::storage_fini().map_err(BootError::Media)?;
    xm1_first_stage_completion_zero();
    write(CELL_PEER0, 0);
    asm!("dsb sy");
    while read(CELL_PEER1) == PEER_READY {
        core::hint::spin_loop();
    }
    asm!("dsb sy", "isb sy");
    Ok(())
}

#[no_mangle]
pub unsafe extern "C" fn xm1_first_stage_main() -> ! {
    let core = core_discriminator();
    if core == Core::Zero {
        media::core_init();
    }
    configure_mpu(core);
    peer_rendezvous(core);
    if let Err(error) = stage_next(core) {
        fail(error);
    }
    #[cfg(first_stage_handler_tail_fault)]
    if core == Core::Zero {
        xm1_first_stage_handler_tail_fault();
    }
    inject_fault();
    xm1_first_stage_handoff()
}

#[cfg(first_stage_fault)]
unsafe fn inject_fault() {
    write(0x4300 as *mut u32, 0xdead_be00);
}

#[cfg(not(first_stage_fault))]
unsafe fn inject_fault() {}

// Synthetic handler-tail fault: a core-0 write outside the seven-store protocol.
#[cfg(first_stage_handler_tail_fault)]
#[no_mangle]
#[inline(never)]
unsafe extern "C" fn xm1_first_stage_handler_tail_fault() {
    write(0x24 as *mut u32, 0xdead_be00);
}
