//! Owned X-M1 record-2 startup: entry decisions, companion startup, peer
//! rendezvous and loader-B publication over the retained record-2 services
//! named in [`adapter`].

#![no_std]

mod adapter;

use adapter::*;
use core::arch::asm;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};

/// The CP14 `(0,c2,c0,0)` discriminator: `Zero` for `0x4003`, `One` otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Core {
    Zero,
    One,
}

impl Core {
    fn from_discriminator(value: u32) -> Self {
        if value == DISCRIMINATOR_PRIMARY {
            Core::Zero
        } else {
            Core::One
        }
    }

    fn log_slots(self) -> (*mut u32, u32) {
        match self {
            Core::Zero => (LOG_INDEX_0, LOG_LAST_SLOT_0),
            Core::One => (LOG_INDEX_1, LOG_LAST_SLOT_1),
        }
    }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum LogEvent {
    StartupBegin = 0x0100_0300,
    CompanionPowerDecided = 0x0100_0400,
    StructureReady = 0x1100_0400,
    StartupEnd = 0x1100_0300,
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

#[inline(always)]
unsafe fn modify(address: *mut u32, clear: u32, set: u32) {
    write(address, (read(address) & !clear) | set);
}

unsafe fn core_discriminator() -> Core {
    let value: u32;
    asm!("mrc p14, 0, {0}, c2, c0, 0", out(reg) value, options(nomem, nostack));
    Core::from_discriminator(value)
}

unsafe fn clear_publication_globals() {
    (0..PUBLICATION_GLOBALS_WORDS).for_each(|word| write(PUBLICATION_GLOBALS_START.add(word), 0));
}

unsafe fn mpu_region(region: &MpuRegion) {
    asm!(
        "mcr p15, 0, {number}, c6, c2, 0",
        "mcr p15, 0, {access}, c6, c1, 4",
        "mcr p15, 0, {base}, c6, c1, 0",
        "mcr p15, 0, {size}, c6, c1, 2",
        number = in(reg) region.number,
        access = in(reg) region.access,
        base = in(reg) region.base,
        size = in(reg) region.size_enable,
        options(nostack),
    );
}

unsafe fn configure_mpu(core: Core) {
    MPU_SHARED_REGIONS.iter().for_each(|region| mpu_region(region));
    if core != Core::Zero {
        mpu_region(&MPU_NON_PRIMARY_REGION);
    }
    let control: u32;
    asm!("mrc p15, 0, {0}, c1, c0, 0", out(reg) control, options(nostack));
    asm!(
        "dsb sy",
        "mcr p15, 0, {zero}, c15, c5, 0",
        "mcr p15, 0, {zero}, c7, c5, 0",
        "mcr p15, 0, {value}, c1, c0, 0",
        "isb sy",
        zero = in(reg) 0u32,
        value = in(reg) control | SCTLR_ENABLE,
        options(nostack),
    );
}

unsafe fn log_init() {
    write(LOG_POINTER, LOG_STORE);
}

unsafe fn log_event(event: LogEvent, core: Core) {
    let pointer = read(LOG_POINTER);
    if pointer == LOG_DISABLED {
        return;
    }
    let (index, last) = core.log_slots();
    let slot = read(index);
    if slot > last {
        return;
    }
    let base = pointer as *mut u32;
    write(base.add(slot as usize), read(TIMER_COUNTER));
    write(base.add(slot as usize + 1), event as u32);
    write(base.add(slot as usize + 2), LOG_END);
    write(index, slot + 2);
}

/// Stock `0x1386c`: 1 only for validity 1, flags 1 and record class 7.
unsafe fn companion_selected(flags: u32, record: *const u8, validity: u32) -> u32 {
    let selected = validity == 1
        && flags == 1
        && read_volatile(record.add(CONFIG_RECORD_CLASS)) & 0xf == 7;
    u32::from(selected)
}

unsafe fn companion_eligible(sample: u32) -> bool {
    (read(PERIPHERAL_F55104) as i32) >= 0
        && sample & 0x90 == 0x10
        && read(PERIPHERAL_F89014) & 0x10 == 0
}

unsafe fn companion_ready() -> bool {
    read(PERIPHERAL_F89014) & 0x10 == 0
        && read(PERIPHERAL_F89014) & 0x80 != 0
        && read(PERIPHERAL_F89094) & 0x1000 != 0
        && read(PERIPHERAL_F89354) & 0x100 != 0
        && read(PERIPHERAL_F89354) & 0x4 == 0
        && read(PERIPHERAL_F89354) & 0x20 != 0
}

/// Stock `0x11d98`-`0x11eec` and the `0x11fbc` refusal.
unsafe fn companion_power() {
    write(PERIPHERAL_F89080, 0x4000_4000);
    modify(PERIPHERAL_F89358, 0xc00, 0x400);
    modify(PERIPHERAL_F89358, 0x3_0000, 0x1_0000);
    xm1_retained_delay(POWER_SETTLE_DELAY);
    if !companion_ready() {
        write(PERIPHERAL_F89080, 0x4000_0000);
        modify(PERIPHERAL_F89358, 0xc00, 0);
        modify(PERIPHERAL_F89358, 0x3_0000, 0);
        return;
    }
    modify(PERIPHERAL_FB3060, 0x8000, 0);
    modify(PERIPHERAL_FB3100, 0, 0x8000);
    modify(PERIPHERAL_FB3180, 0xc000_0000, 0);
    modify(PERIPHERAL_FB3140, 0xc000_0000, 0x8000_0000);
    write(PERIPHERAL_FB343C, 0);
    write(PERIPHERAL_FB383C, 12);
    write(PERIPHERAL_FB3120, 0x8000);
    let _ = read(PERIPHERAL_FB3040);
    write(PERIPHERAL_F89080, 0x8000_8000);
    write(PERIPHERAL_F8900C, 0xc000_4000);
    write(PERIPHERAL_F89080, 0x1000_1000);
    write(PERIPHERAL_F89090, 0x0300_0000);
    write(PERIPHERAL_F89084, 0x1000_1000);
    write(PERIPHERAL_F89350, 0x0003_0001);
    write(PERIPHERAL_F89344, 0x0100_0100);
    write(PERIPHERAL_F8934C, 0x0030_0010);
    write(PERIPHERAL_F89344, 0x0004_0004);
    write(PERIPHERAL_F8934C, 0x0c00_0400);
    write(PERIPHERAL_F89344, 0x0020_0000);
    write(COMPANION_POWERED, 1);
}

/// Stock `0x11cc0` through the rendezvous tail `0x11f40`. The empty stock
/// bodies `0x12abc` and `0x12960` are not called.
unsafe fn companion_startup(core: Core) {
    log_init();
    log_event(LogEvent::StartupBegin, core);
    let word = |index: usize| read(COMPANION_DESCRIPTOR.add(index));
    let mut descriptor: [u32; COMPANION_DESCRIPTOR_WORDS] =
        [word(0), word(1), word(2), word(3), word(4), word(5), word(6), word(7), word(8)];
    write(COMPANION_SAMPLE, xm1_retained_companion_transfer(&mut descriptor, COMPANION_COMMAND));
    let _ = xm1_retained_config_record();
    xm1_retained_conversion(CONVERSION);
    let flags = xm1_retained_companion_flags(read(COMPANION_SAMPLE), CONVERSION);
    let selected = companion_selected(flags, CONFIG_RECORD, read(CONFIG_VALID));
    let sample = read(COMPANION_SAMPLE);
    write(COMPANION_POWERED, 0);
    if companion_eligible(sample) {
        companion_power();
    }
    log_event(LogEvent::CompanionPowerDecided, core);
    write(STRUCTURE_STATUS, xm1_retained_structure_init(selected, STRUCTURE, STRUCTURE_LENGTH));
    log_event(LogEvent::StructureReady, core);
    log_event(LogEvent::StartupEnd, core);
    let _ = xm1_retained_peripheral_final();
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_secondary_peer_cell() -> u32 {
    read(CELL_PRIMARY)
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_secondary_rendezvous() {
    write(CELL_SECONDARY, 0);
    asm!("dsb sy", options(nostack));
    while xm1_secondary_peer_cell() == PEER_SENTINEL {
        (0..PEER_DELAY_ITERATIONS).for_each(|_| asm!("nop", options(nomem, nostack)));
    }
}

/// Publishes the loader-B entry and the image-4 argument. Stock `0x11f90`
/// dereferences a missing segment; this path halts before publishing.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_secondary_publish() {
    let record = xm1_retained_segment_lookup(LOADER_B_SEGMENT_ID);
    if record.is_null() {
        halt();
    }
    let header = read_volatile(addr_of_mut!((*(record as *mut SegmentRecord)).load));
    write(addr_of_mut!(xm1_publication_target), read_volatile(addr_of_mut!((*(header as *mut SegmentHeader)).entry)));
    write(addr_of_mut!(xm1_publication_argument), read(IMAGE4_ENTRY));
    inject_fault();
}

/// Called by the entry after the retained state and stack services; the
/// assembly handoff follows its return.
#[no_mangle]
pub unsafe extern "C" fn xm1_secondary_main() {
    clear_publication_globals();
    let core = core_discriminator();
    if core == Core::Zero {
        xm1_retained_peripheral_init();
    }
    configure_mpu(core);
    companion_startup(core);
    xm1_secondary_rendezvous();
    xm1_secondary_publish();
}

#[cfg(secondary_stage_fault)]
unsafe fn inject_fault() {
    write(addr_of_mut!(xm1_publication_target), 0xdead_be00);
}

#[cfg(not(secondary_stage_fault))]
unsafe fn inject_fault() {}
