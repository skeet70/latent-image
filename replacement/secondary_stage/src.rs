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

/// Stock `0x1386c`.
unsafe fn companion_selection(flags: CompanionFlags, validity: ConfigValidity) -> Selection {
    if validity == ConfigValidity::Valid
        && flags == COMPANION_FLAGS_SELECTABLE
        && read_volatile(CONFIG_RECORD.add(CONFIG_RECORD_CLASS)) & 0xf == CONFIG_CLASS_SELECTABLE
    {
        Selection::Selected
    } else {
        Selection::NotSelected
    }
}

unsafe fn companion_eligible(sample: CompanionSample) -> bool {
    (read(PERIPHERAL_F55104) as i32) >= 0
        && sample.bits(0x90) == 0x10
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
    write_volatile(COMPANION_POWER, CompanionPower::On);
}

/// Stock `0x11cc0` through the rendezvous tail `0x11f40`. The empty stock
/// bodies `0x12abc` and `0x12960` are not called.
unsafe fn companion_startup(core: Core) {
    log_init();
    log_event(LogEvent::StartupBegin, core);
    let descriptor = read_volatile(COMPANION_DESCRIPTOR);
    write_volatile(COMPANION_SAMPLE, xm1_retained_companion_transfer(&descriptor, COMPANION_READ_REGISTER_60));
    xm1_retained_config_record();
    xm1_retained_conversion(CONVERSION);
    let flags = xm1_retained_companion_flags(read_volatile(COMPANION_SAMPLE), CONVERSION);
    let selection = companion_selection(flags, ConfigValidity::from_word(read(CONFIG_VALID)));
    let sample = read_volatile(COMPANION_SAMPLE);
    write_volatile(COMPANION_POWER, CompanionPower::Off);
    if companion_eligible(sample) {
        companion_power();
    }
    log_event(LogEvent::CompanionPowerDecided, core);
    write_volatile(STRUCTURE_STATUS, xm1_retained_structure_init(selection, STRUCTURE, STRUCTURE_LENGTH));
    log_event(LogEvent::StructureReady, core);
    log_event(LogEvent::StartupEnd, core);
    xm1_retained_peripheral_final();
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

/// Publishes id 1's header word `+0xc` and the image-4 argument. As stock
/// `0x11f90`, a zero lookup result is dereferenced unchecked.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_secondary_publish() {
    let record = xm1_retained_segment_lookup(LOADER_B_SEGMENT);
    let header = GuestAddress::from_word(load_guest_word(record.offset(SEGMENT_RECORD_HEADER)));
    write(addr_of_mut!(xm1_publication_target), load_guest_word(header.offset(SEGMENT_HEADER_ENTRY)));
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

#[cfg(secondary_stage_publication_fault)]
unsafe fn inject_fault() {
    write(addr_of_mut!(xm1_publication_target), 0xdead_be00);
}

#[cfg(not(secondary_stage_publication_fault))]
unsafe fn inject_fault() {}
