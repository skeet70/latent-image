use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

pub const UNIT_BYTES: u32 = 0x20000;
const BLOCKS: u32 = 1024;
const SCAN_BLOCKS: usize = 314;
const NAND: u32 = 0xfff5_2000;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaError {
    NotInitialized,
    UnalignedSource,
    AddressRange,
    Timeout,
    ReadFault,
    SkipBudget,
}

impl MediaError {
    /// The value a discarded read status leaves for a diagnostic.
    pub const fn status(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy)]
struct PhysicalBlock(u32);
#[derive(Clone, Copy)]
struct Sector(u32);
#[derive(Clone, Copy)]
struct Destination(u32);

#[repr(C)]
struct SkipTable {
    count: u32,
    blocks: [u32; SCAN_BLOCKS],
}

// Parent entry clears the owned workspace at 0x4300..0x5280.
const SKIPS: *mut SkipTable = 0x4310 as *mut SkipTable;
const INITIALIZED: u32 = 0x4800;
const SAVED_CONTROLLER: u32 = 0x5258;

#[inline(always)]
unsafe fn read(address: u32) -> u32 {
    read_volatile(address as *const u32)
}

#[inline(always)]
unsafe fn write(address: u32, value: u32) {
    write_volatile(address as *mut u32, value);
}

unsafe fn reset_transfer() {
    write(NAND + 0x10, 0);
    write(NAND + 0x18, 0x8023);
}

unsafe fn set_sector(sector: Sector, spare: bool) {
    write(NAND + 0x60, sector.0);
    write(NAND + 0x20, if spare { 512 } else { 0 });
    write(NAND + 0x24, sector.0 & 0x00ff_ffff);
}

// Boot 0x2db8 and 0x2bf4 use 625 polls per timeout unit.
unsafe fn wait_complete(mut polls: u32) -> Result<(), MediaError> {
    while read(NAND + 4) & 4 != 0 {
        if polls == 0 {
            return Err(MediaError::Timeout);
        }
        polls -= 1;
    }
    Ok(())
}

unsafe fn spare(sector: Sector) -> [u32; 4] {
    write(NAND + 0x1c, 0x0030_0000);
    write(NAND + 0x18, 0x8003);
    set_sector(sector, true);
    write(NAND + 4, 0x108);
    write(NAND + 8, 0x0080_2041);
    // Boot 0x3008 discards the wait result before reading the spare registers.
    let _ = wait_complete(625_000);
    let result = [read(NAND + 0x400), read(NAND + 0x404), read(NAND + 0x408), read(NAND + 0x40c)];
    reset_transfer();
    result
}

fn bad_block(spare: [u32; 4]) -> bool {
    let erased = spare.into_iter().fold(u32::MAX, |all, word| all & word) == u32::MAX;
    !erased && (spare[1] & 0xffff != 0xffff)
}

/// The caller owns the controller until `storage_fini` and excludes the other core.
#[no_mangle]
#[inline(never)]
pub unsafe fn xm1_first_stage_storage_init() -> Result<(), MediaError> {
    write(INITIALIZED, 0);
    write(0xfff8_8028, read(0xfff8_8028) & !0x0300_0000);
    write(0xfff8_8030, read(0xfff8_8030) | 0x800);
    for (index, offset) in [0, 0x50, 0x58, 0x98, 0x78].into_iter().enumerate() {
        write(SAVED_CONTROLLER + index as u32 * 4, read(NAND + offset));
    }
    for (offset, value) in [
        (0xe8, 0x0001_0003), (0, 1), (0x50, 0x2231_2131),
        (0x78, 0x605), (4, 0x108), (8, 0), (0x98, 0x8000_0000), (0x58, 0x330),
    ] {
        write(NAND + offset, value);
    }
    reset_transfer();
    write(NAND + 0x1c, 0xff00_7000);
    write(NAND + 0x18, 0x8003);
    write(NAND + 4, 0x108);
    write(NAND + 8, 0x0080_2007);
    // Boot 0x3070 ignores timeout and tests only status bit 24.
    let _ = wait_complete(625_000);
    let _status = read(NAND + 0x28);
    write(NAND + 8, 0);
    reset_transfer();
    write(INITIALIZED, 1);
    Ok(())
}

pub unsafe fn storage_fini() -> Result<(), MediaError> {
    if read(INITIALIZED) == 1 {
        for (index, offset) in [0, 0x50, 0x58, 0x98, 0x78].into_iter().enumerate() {
            write(NAND + offset, read(SAVED_CONTROLLER + index as u32 * 4));
        }
        write(INITIALIZED, 0);
    }
    Ok(())
}

pub unsafe fn scan() -> Result<(), MediaError> {
    if read(INITIALIZED) != 1 {
        return Err(MediaError::NotInitialized);
    }
    write_volatile(addr_of_mut!((*SKIPS).count), 0);
    write(0x4804, SKIPS as u32);
    let mut count = 0;
    for block in 0..SCAN_BLOCKS {
        if bad_block(spare(Sector(block as u32 * 64))) {
            write_volatile(addr_of_mut!((*SKIPS).blocks).cast::<u32>().add(count), block as u32);
            count += 1;
            write_volatile(addr_of_mut!((*SKIPS).count), count as u32);
        }
    }
    Ok(())
}

fn map_block(logical: u32, bad: &[u32]) -> Result<PhysicalBlock, MediaError> {
    let mut block = logical;
    for &skipped in bad {
        if block >= skipped {
            block = block.checked_add(1).ok_or(MediaError::AddressRange)?;
        }
    }
    if block >= BLOCKS {
        return Err(MediaError::AddressRange);
    }
    Ok(PhysicalBlock(block))
}

unsafe fn physical_block(logical: u32) -> Result<PhysicalBlock, MediaError> {
    let count = read_volatile(addr_of!((*SKIPS).count)) as usize;
    if count > SCAN_BLOCKS {
        return Err(MediaError::SkipBudget);
    }
    let blocks = core::slice::from_raw_parts(addr_of!((*SKIPS).blocks).cast::<u32>(), count);
    map_block(logical, blocks)
}

// Completion is status bit 2 clearing; the device writes the DMA destination.
unsafe fn read_block(block: PhysicalBlock, destination: Destination) -> Result<(), MediaError> {
    let dma = destination.0 & 31 == 0;
    let mut sectors_left = 256;
    let mut sector = Sector(block.0 * 64);
    let mut destination = destination;
    while sectors_left != 0 {
        let sectors = if dma { sectors_left.min(256) } else { 1 };
        if dma {
            write(NAND + 0x8a0, 0x0030_1000);
            write(NAND + 0x884, 0x0001_0000);
            write(NAND + 0x8a4, destination.0);
            write(NAND + 0x8a8, 0x0010_0000);
            write(NAND + 0x884, read(NAND + 0x884) | 1);
        }
        write(NAND + 0x1c, 0x0030_0000 | sectors);
        write(NAND + 0x18, 0x8000_8003);
        set_sector(sector, false);
        write(NAND + 4, 0x108);
        write(NAND + 8, if dma { 0x0080_23f3 } else { 0x0080_20f3 });
        let completion = wait_complete(625_000);
        if dma {
            write(NAND + 0x884, read(NAND + 0x884) & !1);
        }
        reset_transfer();
        if let Err(error) = completion {
            write(NAND + 8, 0);
            return Err(error);
        }
        if !dma {
            for byte in 0..512 {
                let value = read_volatile((NAND + 0x400 + byte) as *const u8);
                write_volatile((destination.0 + byte) as *mut u8, value);
            }
        }
        if read(NAND + 4) & 0x10000 != 0 {
            return Err(MediaError::ReadFault);
        }
        sector.0 += sectors;
        destination.0 += sectors * 512;
        sectors_left -= sectors;
    }
    if read(NAND + 0x44) >> 24 != 0xff {
        return Err(MediaError::ReadFault);
    }
    Ok(())
}

/// `destination` must name writable device-accessible memory for `count` units.
pub unsafe fn read_units(source: u32, destination: u32, count: u32) -> Result<(), MediaError> {
    if read(INITIALIZED) != 1 {
        return Err(MediaError::NotInitialized);
    }
    if source & (UNIT_BYTES - 1) != 0 {
        return Err(MediaError::UnalignedSource);
    }
    let bytes = count.checked_mul(UNIT_BYTES).ok_or(MediaError::AddressRange)?;
    destination.checked_add(bytes).ok_or(MediaError::AddressRange)?;
    let mut block = physical_block(source / UNIT_BYTES)?;
    if block.0.checked_add(count).ok_or(MediaError::AddressRange)? > BLOCKS {
        return Err(MediaError::AddressRange);
    }
    let mut remaining = count;
    let mut destination = Destination(destination);
    let mut budget = read_volatile(addr_of!((*SKIPS).count));
    let unlimited_skips = budget == 0;
    let mut fault = None;
    while remaining != 0 {
        if block.0 >= BLOCKS {
            return Err(MediaError::AddressRange);
        }
        if bad_block(spare(Sector(block.0 * 64))) {
            if !unlimited_skips {
                if budget == 0 {
                    return Err(MediaError::SkipBudget);
                }
                budget -= 1;
            }
        } else {
            match read_block(block, destination) {
                Err(MediaError::Timeout) => return Err(MediaError::Timeout),
                Err(error) => fault = Some(error),
                Ok(()) => (),
            }
            remaining -= 1;
            destination.0 = destination.0.wrapping_add(UNIT_BYTES);
        }
        block.0 += 1;
    }
    match fault {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

// boot-40048.bin SHA-256 c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3,
// bytes 0x1b00..0x1cc0, consumed by 0x2644..0x2684.
const PINS: [[u32; 7]; 16] = [
    [0x00000154, 0xffff1554, 0xffff0055, 0xffff0fff, 0xffff0001, 0xffff0000, 0x00555555],
    [0x00000000, 0xffff5540, 0xffff0000, 0xffff00ff, 0xffff00ff, 0xffff0000, 0x00005555],
    [0x00040000, 0xffffaaaa, 0xffff0000, 0xffffffff, 0xffffe4ff, 0xffff0000, 0xaaaaaaaa],
    [0x03000010, 0xffff0000, 0xffff0000, 0xffffffff, 0xffffa8fb, 0xffff0000, 0xaaaaaaaa],
    [0x00000000, 0xffffa000, 0xffff0aa2, 0xffff3fff, 0xffff023f, 0xffff0002, 0x0aaaaaaa],
    [0x00000000, 0xffff0000, 0xffff0000, 0xffff0fff, 0xffff0ffb, 0xffff0000, 0x00555555],
    [0x00000555, 0xffff0555, 0xffff0000, 0xffff003f, 0xffff003f, 0xffff0000, 0x00000555],
    [0x00000010, 0xffff0000, 0xffff0000, 0xffff003f, 0xffff003b, 0xffff0000, 0x00000000],
    [0x00000000, 0xffff5555, 0xffff0000, 0xffffffff, 0xffffffff, 0xffff0000, 0x00000000],
    [0x00000000, 0xffff0000, 0xffff0000, 0xffff0fff, 0xffff0fff, 0xffff0000, 0x00000000],
    [0x00000000, 0xffff0000, 0xffff0000, 0xffffffff, 0xffffffff, 0xffff0000, 0x55555555],
    [0x00000000, 0xffff0000, 0xffff0000, 0xffff000f, 0xffff000b, 0xffff0008, 0x00000055],
    [0x00000000, 0xffff0000, 0xffff0000, 0xffff00ff, 0xffff001c, 0xffff0008, 0x00005555],
    [0x00000000, 0xffff5145, 0xffff0000, 0xffff03ff, 0xffff02c3, 0xffff0000, 0x00055555],
    [0x00000000, 0xffff0005, 0xffff0000, 0xffff0003, 0xffff0003, 0xffff0000, 0x00000005],
    [0x00000000, 0xffff0000, 0xffff0000, 0xffff000f, 0xffff000f, 0xffff000f, 0x000000ff],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeripheralRegister(u32);

impl PeripheralRegister {
    pub fn address(self) -> u32 {
        self.0
    }
}

pub trait PeripheralBus {
    fn read(&mut self, register: PeripheralRegister) -> u32;
    fn write(&mut self, register: PeripheralRegister, value: u32);
}

pub struct VolatilePeripherals;

impl PeripheralBus for VolatilePeripherals {
    fn read(&mut self, register: PeripheralRegister) -> u32 {
        unsafe { read(register.0) }
    }

    fn write(&mut self, register: PeripheralRegister, value: u32) {
        unsafe { write(register.0, value); }
    }
}


fn interrupt_controller(bus: &mut impl PeripheralBus, base: u32) {
    bus.write(PeripheralRegister(base + 0x1c0), 0xffff);
    for index in 0..4 {
        bus.write(PeripheralRegister(base + 0xa0 + index * 4), u32::MAX);
        bus.write(PeripheralRegister(base + 0x60 + index * 4), 0);
        bus.write(PeripheralRegister(base + 0xe0 + index * 4), u32::MAX);
        bus.write(PeripheralRegister(base + 0x100 + index * 4), 0);
        bus.write(PeripheralRegister(base + 0x120 + index * 4), u32::MAX);
    }
    for index in 0..8 {
        bus.write(PeripheralRegister(base + 0x140 + index * 4), 0);
        bus.write(PeripheralRegister(base + 0x180 + index * 4), 0);
    }
    bus.write(PeripheralRegister(base + 0x1c8), 1);
    for index in 0..128 {
        bus.write(PeripheralRegister(base + 0x400 + index * 4), 0);
        bus.write(PeripheralRegister(base + 0x800 + index * 4), 15);
    }
    bus.write(PeripheralRegister(base + 0xc00), 0);
    bus.write(PeripheralRegister(base + 0x1c4), 0xffff);
}

pub fn initialize_peripherals(bus: &mut impl PeripheralBus) {
    for (offset, value) in [(0, 0xff5f), (4, 1), (8, 17), (12, 0x1111), (16, 0x1111)] {
        bus.write(PeripheralRegister(0xfffb_2004 + offset), value);
    }
    for (address, value) in [(0xfff8_802c, 0x0800_0000), (0xfff8_8030, 0x810), (0xfff8_8024, 0), (0xfff8_8028, 0)] {
        bus.write(PeripheralRegister(address), value);
    }
    for (index, values) in PINS.iter().enumerate() {
        let base = 0xfff8_9000 + index as u32 * 64;
        bus.write(PeripheralRegister(base + 0x18), values[0]);
        bus.write(PeripheralRegister(base + 0x0c), values[1]);
        bus.write(PeripheralRegister(base + 0x10), values[2]);
        bus.write(PeripheralRegister(base + 0x08), values[3]);
        bus.write(PeripheralRegister(base), values[5]);
        bus.write(PeripheralRegister(base + 0x04), values[4]);
        bus.write(PeripheralRegister(base + 0x1c), values[6]);
    }
    for index in 0..16 {
        let sample = bus.read(PeripheralRegister(0xfff8_9014 + index * 64));
        bus.write(PeripheralRegister(0x400a_0000 + index * 4), sample);
    }
    bus.write(PeripheralRegister(0x400a_00c8), 0);
    bus.write(PeripheralRegister(0xfff9_1000), 0);
    bus.write(PeripheralRegister(0xfff9_1004), u32::MAX);
    bus.write(PeripheralRegister(0xfff9_1008), 0x63);
    let hold = PeripheralRegister(0xfff8_8084);
    let value = bus.read(hold);
    bus.write(hold, value | 1);
    interrupt_controller(bus, 0xfffb_3000);
    interrupt_controller(bus, 0xfffb_4000);
}

pub unsafe fn core_init() {
    initialize_peripherals(&mut VolatilePeripherals);
}

/// The EXR II channel-0 memory DMA block stock fills the application bss with.
pub const FILL_BASE: u32 = 0xfff0_2000;
/// Stock 0x38c0's eight descriptor words, read by the block through the uncached alias.
const FILL_DESCRIPTOR: u32 = 0x4000_42c0;
/// Stock 0x3868-0x3870 accepts 1..0x07ffffff words.
const FILL_MAX_WORDS: u32 = 0x07ff_ffff;
const FILL_COMPLETE: u32 = 0x40;
const FILL_ABORT: u32 = 8;

/// A fill length in 4-byte units, as stock 0x2298 hands 0x3844 `[segment+0x10] >> 2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Words(u32);

impl Words {
    /// Stock 0x2298's `lsr r1, r1, #2`: a byte length becomes whole words.
    pub const fn from_bytes(bytes: u32) -> Self {
        Self(bytes >> 2)
    }

    /// Stock 0x3928's `lsl r1, r1, #2`: the byte count 0x393c stores at 0xfff02008.
    const fn bytes(self) -> u32 {
        self.0 << 2
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
enum FillRegister {
    Descriptor = 0x00,
    Destination = 0x04,
    Bytes = 0x08,
    Stride0 = 0x0c,
    Stride1 = 0x10,
    Stride2 = 0x14,
    Status = 0x24,
    Start = 0x28,
    Mode = 0x2c,
    Offset0 = 0x30,
    Offset1 = 0x34,
}

impl FillRegister {
    fn register(self) -> PeripheralRegister {
        PeripheralRegister(FILL_BASE + self as u32)
    }
}

/// Stock 0x3860 and 0x3984 return -30 for these and fill on the CPU; the stage has no fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillError {
    Destination,
    Bytes,
}

/// Stock 0x387c-0x38bc picks the transfer width from the destination alignment and the
/// word count, then packs it into bits 12-19 of the mode word.
fn fill_mode(destination: u32, words: Words) -> u32 {
    let mode = if destination & 31 == 0 && words.0 & 7 == 0 {
        5
    } else if destination & 15 == 0 && words.0 & 3 == 0 {
        4
    } else if destination & 7 == 0 && words.0 & 1 == 0 {
        3
    } else {
        2
    };
    0x0150_0400 | mode << 12 | mode << 16
}

/// Stock 0x3844 programs and starts a uniform zero fill; `fill_wait` polls its completion.
pub fn fill_start(bus: &mut impl PeripheralBus, destination: u32, words: Words) -> Result<(), FillError> {
    if destination == 0 || destination & 3 != 0 {
        return Err(FillError::Destination);
    }
    if words.0 == 0 || words.0 > FILL_MAX_WORDS {
        return Err(FillError::Bytes);
    }
    for word in 0..8 {
        bus.write(PeripheralRegister(FILL_DESCRIPTOR + word * 4), 0);
    }
    bus.write(FillRegister::Mode.register(), fill_mode(destination, words));
    bus.write(FillRegister::Descriptor.register(), FILL_DESCRIPTOR);
    bus.write(FillRegister::Destination.register(), destination);
    bus.write(FillRegister::Bytes.register(), words.bytes());
    for register in [
        FillRegister::Stride0, FillRegister::Stride1, FillRegister::Stride2,
        FillRegister::Offset0, FillRegister::Offset1,
    ] {
        bus.write(register.register(), 0);
    }
    bus.write(FillRegister::Start.register(), FILL_ABORT);
    // Stock 0x3964 reads the mode word back: bit 2 of the start word follows bits 4-5.
    let cleared = bus.read(FillRegister::Mode.register()) & 0x30 == 0;
    bus.write(FillRegister::Start.register(), if cleared { 5 } else { 1 });
    Ok(())
}

/// Stock 0x39b0 polls the completion bit with no timeout and no failure exit.
pub fn fill_wait(bus: &mut impl PeripheralBus) {
    while bus.read(FillRegister::Status.register()) & FILL_COMPLETE == 0 {}
}

#[cfg(test)]
#[path = "stock_core_init_addresses.rs"]
mod stock_core_init_addresses;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RegisterFile {
        words: std::collections::BTreeMap<PeripheralRegister, u32>,
        writes: Vec<(PeripheralRegister, u32)>,
    }

    impl PeripheralBus for RegisterFile {
        fn read(&mut self, register: PeripheralRegister) -> u32 {
            self.words.get(&register).copied().unwrap_or(0)
        }

        fn write(&mut self, register: PeripheralRegister, value: u32) {
            self.words.insert(register, value);
            self.writes.push((register, value));
        }
    }

    #[test]
    fn peripheral_bus_write_addresses_and_order_match_stock() {
        let mut bus = RegisterFile::default();
        bus.words.insert(PeripheralRegister(0xfff8_8084), 0x1234_0000);
        initialize_peripherals(&mut bus);
        let addresses: Vec<_> = bus.writes.iter().map(|(register, _)| register.address()).collect();
        assert_eq!(addresses, stock_core_init_addresses::WRITE_ADDRESSES);
        assert_eq!(bus.read(PeripheralRegister(0xfff8_8084)), 0x1234_0001);
    }


    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    mod peripheral_effects {
        use super::*;
        use core::ffi::c_void;

        extern "C" {
            fn mmap(address: *mut c_void, length: usize, protection: i32, flags: i32, fd: i32, offset: i64) -> *mut c_void;
            fn munmap(address: *mut c_void, length: usize) -> i32;
        }

        struct Mapping {
            address: *mut c_void,
            length: usize,
        }

        impl Mapping {
            fn new(address: u32, length: usize) -> Self {
                let requested = address as *mut c_void;
                let mapped = unsafe { mmap(requested, length, 3, 0x100022, -1, 0) };
                assert_eq!(mapped, requested, "fixed peripheral test mapping unavailable");
                Self { address: mapped, length }
            }
        }

        impl Drop for Mapping {
            fn drop(&mut self) {
                unsafe { munmap(self.address, self.length); }
            }
        }

        #[test]
        fn peripheral_initialization_programs_clock_and_pin_banks_separately() {
            let _peripherals = Mapping::new(0xfff8_8000, 0x38000);
            let _snapshot = Mapping::new(0x400a_0000, 0x1000);
            unsafe {
                for port in 0..16 {
                    write(0xfff8_9014 + port * 64, 0xa500 + port);
                }
                core_init();
                // Boot 0x2610..0x262c addresses clock words through 0xfff88fff.
                assert_eq!(read(0xfff8_802c), 0x0800_0000);
                assert_eq!(read(0xfff8_8030), 0x810);
                assert_eq!(read(0xfff8_8024), 0);
                assert_eq!(read(0xfff8_8028), 0);
                assert_eq!(read(0xfff8_8084), 1);
                assert_eq!(read(0xfff8_902c), 0);
                assert_eq!(read(0xfff8_9030), 0);
                assert_eq!(read(0xfff8_9018), 0x154);
                assert_eq!(read(0xfff8_900c), 0xffff1554);
                for port in 0..16 {
                    assert_eq!(read(0x400a_0000 + port * 4), 0xa500 + port);
                }
            }
        }
    }

    #[test]
    fn erased_spare_is_not_a_bad_block() {
        assert!(!bad_block([u32::MAX; 4]));
        assert!(!bad_block([0, 0xffff, 0, 0]));
        assert!(bad_block([u32::MAX, 0xffff_ff00, u32::MAX, u32::MAX]));
    }

    #[test]
    fn bad_blocks_shift_logical_reads_across_adjacent_bad_blocks() {
        assert_eq!(map_block(0, &[1, 2]).unwrap().0, 0);
        assert_eq!(map_block(1, &[1, 2]).unwrap().0, 3);
        assert_eq!(map_block(3, &[1, 2]).unwrap().0, 5);
        assert!(matches!(map_block(1023, &[1]), Err(MediaError::AddressRange)));
        assert!(matches!(map_block(u32::MAX, &[0]), Err(MediaError::AddressRange)));
    }

    /// Boot 0x38c8-0x397c with the stock image-4 bss `0x89ec00`/`0x352e00`.
    #[test]
    fn the_bss_fill_programs_the_memory_dma_block_in_stock_order() {
        let mut bus = RegisterFile::default();
        bus.words.insert(PeripheralRegister(FILL_BASE + 0x24), 0x40);
        fill_start(&mut bus, 0x0089_ec00, Words::from_bytes(0x0035_2e00)).unwrap();
        fill_wait(&mut bus);
        let writes: Vec<(u32, u32)> = bus.writes.iter().map(|(register, value)| (register.address(), *value)).collect();
        assert_eq!(
            writes,
            vec![
                // 0x38c8-0x38e4: the eight value words of the descriptor.
                (0x4000_42c0, 0), (0x4000_42c4, 0), (0x4000_42c8, 0), (0x4000_42cc, 0),
                (0x4000_42d0, 0), (0x4000_42d4, 0), (0x4000_42d8, 0), (0x4000_42dc, 0),
                // 0x3930-0x393c: mode word, descriptor, destination, byte count.
                (0xfff0_202c, 0x0155_5400),
                (0xfff0_2000, 0x4000_42c0),
                (0xfff0_2004, 0x0089_ec00),
                (0xfff0_2008, 0x0035_2e00),
                // 0x3944-0x394c: strides, then 0x3950-0x3954: offsets.
                (0xfff0_200c, 0), (0xfff0_2010, 0), (0xfff0_2014, 0),
                (0xfff0_2030, 0), (0xfff0_2034, 0),
                // 0x395c: abort, then 0x397c: start after the mode read-back.
                (0xfff0_2028, 8),
                (0xfff0_2028, 5),
            ]
        );
    }

    #[test]
    fn the_fill_mode_follows_destination_alignment_and_word_count() {
        assert_eq!(fill_mode(0x0089_ec00, Words::from_bytes(0x0035_2e00)), 0x0155_5400);
        assert_eq!(fill_mode(0x0089_ec00, Words::from_bytes(0x0035_2e04)), 0x0152_2400);
        assert_eq!(fill_mode(0x0089_ec00, Words::from_bytes(0x0035_2e08)), 0x0153_3400);
        assert_eq!(fill_mode(0x0089_ec00, Words::from_bytes(0x0035_2e02)), 0x0155_5400);
        assert_eq!(fill_mode(0x0089_ec00, Words::from_bytes(0x0035_2e01)), 0x0155_5400);
        assert_eq!(fill_mode(0x0089_ec10, Words::from_bytes(0x0035_2e00)), 0x0154_4400);
        assert_eq!(fill_mode(0x0089_ec08, Words::from_bytes(0x0035_2e00)), 0x0153_3400);
        assert_eq!(fill_mode(0x0089_ec04, Words::from_bytes(0x0035_2e00)), 0x0152_2400);
        assert_eq!(fill_mode(0x0089_ec0c, Words::from_bytes(0x0035_2e00)), 0x0152_2400);
    }

    #[test]
    fn the_fill_refuses_only_the_requests_stock_reports_as_errors() {
        let mut bus = RegisterFile::default();
        assert_eq!(fill_start(&mut bus, 0, Words::from_bytes(16)), Err(FillError::Destination));
        assert_eq!(fill_start(&mut bus, 0x0089_ec02, Words::from_bytes(16)), Err(FillError::Destination));
        assert_eq!(fill_start(&mut bus, 0x0089_ec00, Words::from_bytes(0)), Err(FillError::Bytes));
        assert_eq!(fill_start(&mut bus, 0x0089_ec00, Words::from_bytes(1)), Err(FillError::Bytes));
        assert_eq!(fill_start(&mut bus, 0x0089_ec00, Words::from_bytes(0x2000_0000)), Err(FillError::Bytes));
        assert!(bus.writes.is_empty());
        assert!(fill_start(&mut bus, 0x0089_ec00, Words::from_bytes(0x1fff_fffc)).is_ok());
        assert!(fill_start(&mut bus, 0x0089_ec00, Words::from_bytes(0x0800_0004)).is_ok());
    }

    struct PollBus {
        status: Vec<u32>,
        reads: usize,
    }

    impl PeripheralBus for PollBus {
        fn read(&mut self, register: PeripheralRegister) -> u32 {
            assert_eq!(register.address(), FILL_BASE + 0x24);
            self.reads += 1;
            self.status.remove(0)
        }

        fn write(&mut self, _: PeripheralRegister, _: u32) {}
    }

    #[test]
    fn the_fill_wait_returns_only_on_the_completion_bit() {
        let mut bus = PollBus { status: vec![0, 1, 0x80, 0x40], reads: 0 };
        fill_wait(&mut bus);
        assert_eq!(bus.reads, 4);
    }
}
