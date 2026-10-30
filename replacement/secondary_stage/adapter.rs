//! Recovered X-M1 1.50 record-2 addresses and retained-service bindings.
//! Addresses are the guest addresses the stock record-2 instructions form.
//! Peripheral names are their stock addresses; their register semantics are
//! not established.

use core::arch::asm;

pub(crate) const DISCRIMINATOR_PRIMARY: u32 = 0x4003;

pub(crate) const PUBLICATION_GLOBALS_START: *mut u32 = 0x0001_4c80 as *mut u32;
pub(crate) const PUBLICATION_GLOBALS_WORDS: usize = 8;

pub(crate) const CELL_PRIMARY: *const u32 = 0x4002_0000 as *const u32;
pub(crate) const CELL_SECONDARY: *mut u32 = 0x4002_0004 as *mut u32;
pub(crate) const PEER_SENTINEL: u32 = 0x22b;
pub(crate) const PEER_DELAY_ITERATIONS: u32 = 100;

pub(crate) const LOG_POINTER: *mut u32 = 0x4001_4c60 as *mut u32;
pub(crate) const LOG_INDEX_0: *mut u32 = 0x4001_4c64 as *mut u32;
pub(crate) const LOG_INDEX_1: *mut u32 = 0x4001_4c68 as *mut u32;
pub(crate) const LOG_LAST_SLOT_0: u32 = 15;
pub(crate) const LOG_LAST_SLOT_1: u32 = 31;
pub(crate) const LOG_STORE: u32 = 0x400a_0040;
pub(crate) const LOG_DISABLED: u32 = 0x3333_3333;
pub(crate) const LOG_END: u32 = 0x5354_4544;
pub(crate) const TIMER_COUNTER: *const u32 = 0xfff9_1000 as *const u32;

pub(crate) const COMPANION_DESCRIPTOR: *const CompanionDescriptor = 0x0001_46a8 as *const CompanionDescriptor;
pub(crate) const COMPANION_SAMPLE: *mut CompanionSample = 0x400a_00c0 as *mut CompanionSample;
pub(crate) const CONFIG_VALID: *const u32 = 0x400a_00cc as *const u32;
pub(crate) const CONFIG_RECORD: *const u8 = 0x400a_00d0 as *const u8;
pub(crate) const CONFIG_RECORD_CLASS: usize = 0xb;
pub(crate) const CONFIG_CLASS_SELECTABLE: u8 = 7;
pub(crate) const CONVERSION: *mut ConversionTable = 0x400a_0110 as *mut ConversionTable;
pub(crate) const COMPANION_POWER: *mut CompanionPower = 0x400a_0140 as *mut CompanionPower;
pub(crate) const STRUCTURE_STATUS: *mut StructureStatus = 0x400a_0144 as *mut StructureStatus;
pub(crate) const STRUCTURE: *mut u32 = 0x400a_0148 as *mut u32;

/// Image-4 header word `+0xc`, published as the loader-B argument.
pub(crate) const IMAGE4_ENTRY: *const u32 = 0x0061_c40c as *const u32;
pub(crate) const SEGMENT_RECORD_HEADER: u32 = 4;
pub(crate) const SEGMENT_HEADER_ENTRY: u32 = 0xc;

pub(crate) const PERIPHERAL_F55104: *const u32 = 0xfff5_5104 as *const u32;
pub(crate) const PERIPHERAL_F8900C: *mut u32 = 0xfff8_900c as *mut u32;
pub(crate) const PERIPHERAL_F89014: *const u32 = 0xfff8_9014 as *const u32;
pub(crate) const PERIPHERAL_F89080: *mut u32 = 0xfff8_9080 as *mut u32;
pub(crate) const PERIPHERAL_F89084: *mut u32 = 0xfff8_9084 as *mut u32;
pub(crate) const PERIPHERAL_F89090: *mut u32 = 0xfff8_9090 as *mut u32;
pub(crate) const PERIPHERAL_F89094: *const u32 = 0xfff8_9094 as *const u32;
pub(crate) const PERIPHERAL_F89344: *mut u32 = 0xfff8_9344 as *mut u32;
pub(crate) const PERIPHERAL_F8934C: *mut u32 = 0xfff8_934c as *mut u32;
pub(crate) const PERIPHERAL_F89350: *mut u32 = 0xfff8_9350 as *mut u32;
pub(crate) const PERIPHERAL_F89354: *const u32 = 0xfff8_9354 as *const u32;
pub(crate) const PERIPHERAL_F89358: *mut u32 = 0xfff8_9358 as *mut u32;
pub(crate) const PERIPHERAL_FB3040: *const u32 = 0xfffb_3040 as *const u32;
pub(crate) const PERIPHERAL_FB3060: *mut u32 = 0xfffb_3060 as *mut u32;
pub(crate) const PERIPHERAL_FB3100: *mut u32 = 0xfffb_3100 as *mut u32;
pub(crate) const PERIPHERAL_FB3120: *mut u32 = 0xfffb_3120 as *mut u32;
pub(crate) const PERIPHERAL_FB3140: *mut u32 = 0xfffb_3140 as *mut u32;
pub(crate) const PERIPHERAL_FB3180: *mut u32 = 0xfffb_3180 as *mut u32;
pub(crate) const PERIPHERAL_FB343C: *mut u32 = 0xfffb_343c as *mut u32;
pub(crate) const PERIPHERAL_FB383C: *mut u32 = 0xfffb_383c as *mut u32;

/// Stock `0x119e4` region programming: number, access control (`c6,c1,4`),
/// base (`c6,c1,0`) and size/enable (`c6,c1,2`).
pub(crate) struct MpuRegion {
    pub(crate) number: u32,
    pub(crate) access: u32,
    pub(crate) base: u32,
    pub(crate) size_enable: u32,
}

pub(crate) const MPU_SHARED_REGIONS: [MpuRegion; 4] = [
    MpuRegion { number: 0, access: 0x303, base: 0x0000_0000, size_enable: 0x3b },
    MpuRegion { number: 1, access: 0x30c, base: 0x4000_0000, size_enable: 0x3b },
    MpuRegion { number: 2, access: 0x305, base: 0x8000_0000, size_enable: 0x3d },
    MpuRegion { number: 3, access: 0x30c, base: 0xfff0_0000, size_enable: 0x17 },
];
pub(crate) const MPU_NON_PRIMARY_REGION: MpuRegion =
    MpuRegion { number: 4, access: 0x303, base: 0xffff_0000, size_enable: 0x1f };
pub(crate) const SCTLR_ENABLE: u32 = 0x1005;

/// A 32-bit guest address with wrapping offset arithmetic.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct GuestAddress(u32);

impl GuestAddress {
    pub(crate) const fn from_word(word: u32) -> Self {
        GuestAddress(word)
    }

    pub(crate) const fn offset(self, bytes: u32) -> Self {
        GuestAddress(self.0.wrapping_add(bytes))
    }
}

/// One `ldr` from any guest address, including zero, as stock `0x11f90`-`0x11f94` perform.
#[inline(always)]
pub(crate) unsafe fn load_guest_word(address: GuestAddress) -> u32 {
    let value: u32;
    asm!(
        "ldr {value}, [{address}]",
        address = in(reg) address.0,
        value = lateout(reg) value,
        options(nostack, readonly, preserves_flags),
    );
    value
}

/// Nine-word companion transfer descriptor; `0x12ad0` reads it only during the call.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct CompanionDescriptor(pub(crate) [u32; 9]);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct CompanionCommand(u32);

pub(crate) const COMPANION_READ_REGISTER_60: CompanionCommand = CompanionCommand(0x6000);

/// `0x12ad0` response halfword, zero-extended; also the `0x137a8` sample argument.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct CompanionSample(u32);

impl CompanionSample {
    pub(crate) fn bits(self, mask: u32) -> u32 {
        self.0 & mask
    }
}

/// `0x137a8` result bits `0x20`, `4`, `2` and `1`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompanionFlags(u32);

pub(crate) const COMPANION_FLAGS_SELECTABLE: CompanionFlags = CompanionFlags(1);

/// Ten words `0x13124` writes when the table address is nonzero.
#[repr(transparent)]
pub(crate) struct ConversionTable(pub(crate) [u32; 10]);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigValidity {
    Valid,
    Invalid,
}

impl ConfigValidity {
    /// `0x13244` stores 1 or 0; `0x1386c` accepts only 1.
    pub(crate) fn from_word(word: u32) -> Self {
        if word == 1 {
            ConfigValidity::Valid
        } else {
            ConfigValidity::Invalid
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub(crate) enum Selection {
    NotSelected = 0,
    Selected = 1,
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub(crate) enum CompanionPower {
    Off = 0,
    On = 1,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct DelayUnits(u32);

pub(crate) const POWER_SETTLE_DELAY: DelayUnits = DelayUnits(20);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct StructureLength(u32);

pub(crate) const STRUCTURE_LENGTH: StructureLength = StructureLength(0x400);

/// `0x13b80` result; zero on every return path.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct StructureStatus(u32);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct SegmentId(u32);

pub(crate) const LOADER_B_SEGMENT: SegmentId = SegmentId(1);

extern "C" {
    /// Record-2 publication cells `0x14c80`/`0x14c84`, read by the handoff.
    pub(crate) static mut xm1_publication_target: u32;
    pub(crate) static mut xm1_publication_argument: u32;

    /// `0x12204`: peripheral and pin initialization on the `0x4003` path.
    pub(crate) fn xm1_retained_peripheral_init();
    /// `0x12ad0`.
    pub(crate) fn xm1_retained_companion_transfer(
        descriptor: &CompanionDescriptor,
        command: CompanionCommand,
    ) -> CompanionSample;
    /// `0x13244`: reads and checks the 16-byte record at `0x400a00d0`, setting `0x400a00cc`.
    /// Its status return is not read.
    pub(crate) fn xm1_retained_config_record();
    /// `0x13124`; its zero return is not read.
    pub(crate) fn xm1_retained_conversion(table: *mut ConversionTable);
    /// `0x137a8`; also stores the sample at `0x14c90`.
    pub(crate) fn xm1_retained_companion_flags(sample: CompanionSample, table: *mut ConversionTable) -> CompanionFlags;
    /// `0x1270c`: timer-channel delay.
    pub(crate) fn xm1_retained_delay(units: DelayUnits);
    /// `0x13b80`; does not read `selected`.
    pub(crate) fn xm1_retained_structure_init(selected: Selection, base: *mut u32, length: StructureLength) -> StructureStatus;
    /// `0x130c8`: writes `0x8004` to `0xfff8a000`; its zero return is not read.
    pub(crate) fn xm1_retained_peripheral_final();
    /// `0x13574`; zero when the id is absent.
    pub(crate) fn xm1_retained_segment_lookup(id: SegmentId) -> GuestAddress;
}
