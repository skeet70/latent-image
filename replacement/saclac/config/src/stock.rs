//! The stock boundary: X-M1 1.50 entries and layouts behind the configuration layers' ports.

use core::ptr::{read_volatile, write_volatile};

use crate::channel::{Channel, FloatLog};
use crate::compiled::{CompiledScan, Optioned, StreamSnapshot};
use crate::dump::{ConfigurationDump, DumpDerivations};
use crate::table::{Row, Step, StepLimit};
use crate::face::{FaceTracking, TrackingBusy};
use crate::ports::{Derivations, ExportQuery};
use crate::pipeline::{Bank, Enable, EnableChange, FilterWindow, Grid, Part, Setting, Stage, StageBlock, StageMode};
use crate::registers::PipelineHardware;
use crate::scan::AppliedParts;
use crate::scan::{ScanBlocks, ScanLibrary, StockPart};
use crate::services::{KernelDelay, LensStatus};

/// X-M1 1.50 limit arguments use 0xffff for no limit (0x746cd4..0x746cdc).
pub fn decode_step_limit(word: u32) -> StepLimit {
    match word {
        0xffff => StepLimit::Unbounded,
        value => StepLimit::Value(value),
    }
}

macro_rules! entry {
    ($address:expr, fn($($argument:ty),*) $(-> $result:ty)?) => {{
        let entry: extern "C" fn($($argument),*) $(-> $result)? = unsafe { core::mem::transmute($address as usize as *const ()) };
        entry
    }};
}

/// Guest memory by address, which the stock layouts below are read from and written to.
pub trait GuestMemory {
    fn byte(&self, at: u32) -> u8;
    fn halfword(&self, at: u32) -> u16;
    fn word(&self, at: u32) -> u32;
    fn set_byte(&self, at: u32, value: u8);
    fn set_halfword(&self, at: u32, value: u16);
    fn set_word(&self, at: u32, value: u32);
    /// Stores `value` in `count` bytes from `at`.
    fn fill(&self, at: u32, value: u8, count: u32);
}

/// Guest memory at its own addresses; `fill` is the runtime's fill_bytes (0x7221c8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StockMemory;

impl GuestMemory for StockMemory {
    fn byte(&self, at: u32) -> u8 { unsafe { read_volatile(at as usize as *const u8) } }
    fn halfword(&self, at: u32) -> u16 { unsafe { read_volatile(at as usize as *const u16) } }
    fn word(&self, at: u32) -> u32 { unsafe { read_volatile(at as usize as *const u32) } }
    fn set_byte(&self, at: u32, value: u8) { unsafe { write_volatile(at as usize as *mut u8, value) } }
    fn set_halfword(&self, at: u32, value: u16) { unsafe { write_volatile(at as usize as *mut u16, value) } }
    fn set_word(&self, at: u32, value: u32) { unsafe { write_volatile(at as usize as *mut u32, value) } }
    fn fill(&self, at: u32, value: u8, count: u32) { entry!(0x72_21c8u32, fn(u32, u32, u32))(at, u32::from(value), count) }
}

impl<M: GuestMemory + ?Sized> GuestMemory for &M {
    fn byte(&self, at: u32) -> u8 { (**self).byte(at) }
    fn halfword(&self, at: u32) -> u16 { (**self).halfword(at) }
    fn word(&self, at: u32) -> u32 { (**self).word(at) }
    fn set_byte(&self, at: u32, value: u8) { (**self).set_byte(at, value) }
    fn set_halfword(&self, at: u32, value: u16) { (**self).set_halfword(at, value) }
    fn set_word(&self, at: u32, value: u32) { (**self).set_word(at, value) }
    fn fill(&self, at: u32, value: u8, count: u32) { (**self).fill(at, value, count) }
}

pub struct StockDerivations;

impl Derivations for StockDerivations {
    fn export_word(&mut self, channel: Channel, query: ExportQuery) -> i16 {
        let mut word: i16 = 0;
        entry!(0x74_98e0u32, fn(i32, u32, u32, u32, *mut i16, u32))(channel.0, query.selector, query.first, query.second, &mut word, 0);
        word
    }

    fn adjust_direction(&mut self, word: i16, direction: u32, mode: u32) -> i16 {
        entry!(0x74_7aacu32, fn(i32, u32, u32) -> i32)(i32::from(word), direction, mode) as i16
    }

    fn scaled_limit(&mut self, channel: Channel, first: u32, scale: u32) -> u16 {
        entry!(0x74_9254u32, fn(i32, u32, u32) -> u32)(channel.0, first, scale) as u16
    }

    fn channel_scale(&mut self, channel: Channel) -> u16 { entry!(0x74_8818u32, fn(i32) -> u32)(channel.0) as u16 }

    fn table_scale(&mut self, step: u32, channel: Channel) -> u16 { entry!(0x74_888cu32, fn(u32, i32) -> u32)(step, channel.0) as u16 }

    fn curve_position(&mut self, level: i16, channel: Channel) -> u32 {
        entry!(0x74_6120u32, fn(i32, i32) -> u32)(i32::from(level), channel.0)
    }

    fn curve_value(&mut self, position: u32, channel: Channel) -> i16 {
        entry!(0x74_61a8u32, fn(u32, i32) -> i32)(position, channel.0) as i16
    }

    fn channel_word(&mut self, channel: Channel, selector: u32, table: u32) -> i16 {
        entry!(0x74_7408u32, fn(i32, u32, u32) -> i32)(channel.0, selector, table) as i16
    }

    /// 0x749e40 passes 0, the mode, the channel and 0 in registers, then 0, 0 and the word's address.
    fn reference_word(&mut self, mode: u32, channel: Channel) -> i16 {
        let mut word: i16 = 0;
        entry!(0x74_7584u32, fn(u32, u32, i32, u32, u32, u32, *mut i16))(0, mode, channel.0, 0, 0, 0, &mut word);
        word
    }

    /// 0x749e40 passes the word, the channel, 0, -1 and 1.
    fn apply_reference(&mut self, word: i16, channel: Channel) {
        entry!(0x74_9be4u32, fn(i32, i32, u32, u32, u32))(i32::from(word), channel.0, 0, u32::MAX, 1);
    }

    fn lens_table(&mut self, selector: u32, first: u32, second: u32) -> u32 {
        entry!(0x74_62e8u32, fn(u32, u32, u32) -> u32)(selector, first, second)
    }

    fn lens_limits(&mut self, table: u32, [a, b, c, d, e, f, g, h, i, j, k]: [u32; 11]) -> u32 {
        entry!(0x74_b05cu32, fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32)(table, a, b, c, d, e, f, g, h, i, j, k)
    }
}

/// The runtime's logarithm, entered with the value's bits as the soft-float ABI passes them.
pub struct StockFloatLog;

impl FloatLog for StockFloatLog {
    fn ln(&mut self, value: f32) -> f32 { f32::from_bits(entry!(0x89_4778u32, fn(u32) -> u32)(value.to_bits())) }
}

pub struct StockKernelDelay;

impl KernelDelay for StockKernelDelay {
    fn delay(&mut self, ticks: u32) -> u32 { entry!(0x72_3ce8u32, fn(u32) -> u32)(ticks) }
}

/// The status byte 0x6c6b08 copies out of `0x93c544 + 0x64`, or `None` when it reports failure.
pub struct StockLensStatus;

impl LensStatus for StockLensStatus {
    fn status(&mut self) -> Option<u8> {
        let mut status = 0u8;
        if entry!(0x6c_6b08u32, fn(*mut u8) -> u32)(&mut status) == 0 { None } else { Some(status) }
    }
}

pub struct StockPipelineHardware;

impl PipelineHardware for StockPipelineHardware {
    fn acquire(&mut self, client: u32) -> u32 { entry!(0xda_1b4cu32, fn(u32) -> u32)(client) }

    fn release(&mut self, client: u32, grant: u32) { entry!(0xda_1d48u32, fn(u32, u32))(client, grant) }
}

/// A caller's block the stock face-tracking command reads or writes, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaceBlock(pub u32);

pub struct StockFaceTracking;

impl FaceTracking for StockFaceTracking {
    type Geometry = FaceBlock;
    type Output = FaceBlock;

    fn initialize(&mut self) -> Result<(), TrackingBusy> {
        if entry!(0x106_d02cu32, fn() -> i32)() == 0 { Ok(()) } else { Err(TrackingBusy) }
    }

    fn command(&mut self, mode: u32, geometry: &FaceBlock, output: &mut FaceBlock) -> i32 {
        entry!(0x106_a2f0u32, fn(u32, u32, u32) -> i32)(mode, geometry.0, output.0)
    }
}

/// A scan's configuration block (0x244 bytes) and request block, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockScanBlocks<M = StockMemory> {
    pub configuration: u32,
    pub request: u32,
    pub memory: M,
}

const CONFIGURATION_BYTES: u32 = 0x244;
const APPLIED_PARTS: u32 = 0x14c;
const REQUEST_WINDOW: u32 = 0x3e;
const CONFIGURED_WINDOW: u32 = 0x1c8;

#[derive(Clone, Copy)]
enum Width { Byte, Half, Word }

/// A configuration field by its stock offset; its meaning awaits the caller that names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanField {
    WordDc,
    WordE0,
    WordE4,
    WordE8,
    WordEc,
    WordF0,
    WordF4,
    WordF8,
    WordFc,
    Word100,
    Word104,
    Word108,
    Word10c,
    Word110,
    Word114,
    Half118,
    Byte11a,
    Byte11b,
    Word11c,
    Word124,
    Word128,
    Word13c,
    Word140,
    Word144,
    Word148,
    Word160,
    Word164,
    Half168,
    Half16a,
    Byte16c,
    Word17c,
    Word180,
    Word184,
    Word188,
    Word18c,
    Byte190,
    Word194,
    Half1a0,
    Byte1a2,
    Half1a4,
    Word1a8,
    Byte1ac,
    Byte1ad,
    Byte1ae,
    Byte1af,
    Word1b4,
    Word1bc,
    Half1c8,
    Half1ca,
    Half1cc,
    Word1d0,
    Word1d4,
    Word1d8,
    Word1dc,
    Word1e0,
    Word1e4,
    Half1e8,
    Half1ea,
}

fn layout(field: ScanField) -> (u32, Width) {
    use ScanField::*;
    match field {
        WordDc => (0xdc, Width::Word),
        WordE0 => (0xe0, Width::Word),
        WordE4 => (0xe4, Width::Word),
        WordE8 => (0xe8, Width::Word),
        WordEc => (0xec, Width::Word),
        WordF0 => (0xf0, Width::Word),
        WordF4 => (0xf4, Width::Word),
        WordF8 => (0xf8, Width::Word),
        WordFc => (0xfc, Width::Word),
        Word100 => (0x100, Width::Word),
        Word104 => (0x104, Width::Word),
        Word108 => (0x108, Width::Word),
        Word10c => (0x10c, Width::Word),
        Word110 => (0x110, Width::Word),
        Word114 => (0x114, Width::Word),
        Half118 => (0x118, Width::Half),
        Byte11a => (0x11a, Width::Byte),
        Byte11b => (0x11b, Width::Byte),
        Word11c => (0x11c, Width::Word),
        Word124 => (0x124, Width::Word),
        Word128 => (0x128, Width::Word),
        Word13c => (0x13c, Width::Word),
        Word140 => (0x140, Width::Word),
        Word144 => (0x144, Width::Word),
        Word148 => (0x148, Width::Word),
        Word160 => (0x160, Width::Word),
        Word164 => (0x164, Width::Word),
        Half168 => (0x168, Width::Half),
        Half16a => (0x16a, Width::Half),
        Byte16c => (0x16c, Width::Byte),
        Word17c => (0x17c, Width::Word),
        Word180 => (0x180, Width::Word),
        Word184 => (0x184, Width::Word),
        Word188 => (0x188, Width::Word),
        Word18c => (0x18c, Width::Word),
        Byte190 => (0x190, Width::Byte),
        Word194 => (0x194, Width::Word),
        Half1a0 => (0x1a0, Width::Half),
        Byte1a2 => (0x1a2, Width::Byte),
        Half1a4 => (0x1a4, Width::Half),
        Word1a8 => (0x1a8, Width::Word),
        Byte1ac => (0x1ac, Width::Byte),
        Byte1ad => (0x1ad, Width::Byte),
        Byte1ae => (0x1ae, Width::Byte),
        Byte1af => (0x1af, Width::Byte),
        Word1b4 => (0x1b4, Width::Word),
        Word1bc => (0x1bc, Width::Word),
        Half1c8 => (0x1c8, Width::Half),
        Half1ca => (0x1ca, Width::Half),
        Half1cc => (0x1cc, Width::Half),
        Word1d0 => (0x1d0, Width::Word),
        Word1d4 => (0x1d4, Width::Word),
        Word1d8 => (0x1d8, Width::Word),
        Word1dc => (0x1dc, Width::Word),
        Word1e0 => (0x1e0, Width::Word),
        Word1e4 => (0x1e4, Width::Word),
        Half1e8 => (0x1e8, Width::Half),
        Half1ea => (0x1ea, Width::Half),
    }
}

impl<M: GuestMemory> StockScanBlocks<M> {
    pub(crate) fn set_field(&mut self, field: ScanField, value: u32) {
        let (offset, width) = layout(field);
        let at = self.configuration + offset;
        match width {
            Width::Byte => self.memory.set_byte(at, value as u8),
            Width::Half => self.memory.set_halfword(at, value as u16),
            Width::Word => self.memory.set_word(at, value),
        }
    }


    /// 0xd78400.
    pub fn set_ec(&mut self, value: u32) { self.set_field(ScanField::WordEc, value) }

    /// Values above 7 become 8 (0xd784ac).
    pub fn set_140(&mut self, value: u32) { self.set_field(ScanField::Word140, value.min(8)) }

    /// 0xd784cc.
    pub fn set_144(&mut self, enabled: bool) { self.set_field(ScanField::Word144, u32::from(enabled)) }

    /// 0xd78674.
    pub fn set_11a(&mut self, value: u8) { self.set_field(ScanField::Byte11a, u32::from(value)) }

    /// Values above 8 become 9 (0xd7868c).
    pub fn set_13c(&mut self, value: u32) { self.set_field(ScanField::Word13c, value.min(9)) }

    /// Values above 2 become 3 (0xd789f0).
    pub fn set_148(&mut self, value: u32) { self.set_field(ScanField::Word148, value.min(3)) }

    /// Values above 4 become 0 (0xd79364).
    pub fn set_1dc(&mut self, value: u32) { self.set_field(ScanField::Word1dc, if value > 4 { 0 } else { value }) }

    /// Stores the defaults 0xd791b0 writes, then the library's finish entries.
    pub fn set_defaults(&mut self, library: &mut impl ScanLibrary<Self>) {
        for (field, value) in DEFAULTS {
            self.set_field(field, value);
        }
        library.finish_defaults(self);
    }
}

/// The defaults 0xd791b0 stores, in its order.
const DEFAULTS: [(ScanField, u32); 54] = [
    (ScanField::WordEc, 0),
    (ScanField::WordE4, 0),
    (ScanField::WordDc, 1),
    (ScanField::WordE0, 0),
    (ScanField::WordF0, 0),
    (ScanField::WordF4, 2),
    (ScanField::WordF8, 2),
    (ScanField::WordFc, 2),
    (ScanField::Word100, 2),
    (ScanField::Word104, 0),
    (ScanField::Word108, 0),
    (ScanField::Word10c, 2),
    (ScanField::Word110, 0),
    (ScanField::Word114, 2),
    (ScanField::Byte11b, 1),
    (ScanField::Word11c, 600),
    (ScanField::Half118, 0),
    (ScanField::Byte11a, 1),
    (ScanField::Word124, 1),
    (ScanField::Word160, 0),
    (ScanField::Word164, 0),
    (ScanField::Half168, 0),
    (ScanField::Half16a, 0),
    (ScanField::Byte16c, 0),
    (ScanField::Word180, 2),
    (ScanField::Word184, 0),
    (ScanField::Word188, 0),
    (ScanField::Word17c, 1),
    (ScanField::Word18c, 0),
    (ScanField::Byte190, 0),
    (ScanField::Word194, 2),
    (ScanField::Word128, 0),
    (ScanField::Half1a0, 0x2ba),
    (ScanField::Byte1a2, 0),
    (ScanField::Half1a4, 0x7e),
    (ScanField::Byte1ac, 0),
    (ScanField::Word1a8, 0),
    (ScanField::Byte1ad, 0),
    (ScanField::Byte1ae, 0),
    (ScanField::Byte1af, 0),
    (ScanField::Word1bc, 1),
    (ScanField::Word1e0, 0),
    (ScanField::WordE8, 0),
    (ScanField::Half1c8, 0),
    (ScanField::Half1ca, 0),
    (ScanField::Half1cc, 0),
    (ScanField::Word1d0, 0),
    (ScanField::Word1d4, 0),
    (ScanField::Word1d8, 0),
    (ScanField::Word1b4, 0),
    (ScanField::Word1dc, 0),
    (ScanField::Word1e4, 0),
    (ScanField::Half1e8, 0),
    (ScanField::Half1ea, 0),
];

impl<M: GuestMemory> ScanBlocks for StockScanBlocks<M> {
    /// 0xd77abc fills the block through fill_bytes (0x7221c8).
    fn clear_configuration(&mut self) { self.memory.fill(self.configuration, 0, CONFIGURATION_BYTES) }

    fn request_flag(&self, index: u8) -> u8 { self.memory.byte(self.request + u32::from(index)) }

    fn applied_parts(&self) -> AppliedParts { AppliedParts(self.memory.word(self.configuration + APPLIED_PARTS)) }

    fn mark_applied(&mut self, AppliedParts(bits): AppliedParts) {
        let at = self.configuration + APPLIED_PARTS;
        self.memory.set_word(at, self.memory.word(at) | bits);
    }

    fn clear_applied_parts(&mut self) { self.memory.set_word(self.configuration + APPLIED_PARTS, 0) }

    fn configured_window(&self) -> [u16; 3] {
        core::array::from_fn(|index| self.memory.halfword(self.configuration + CONFIGURED_WINDOW + 2 * index as u32))
    }

    fn set_configured_window(&mut self, limits: [u16; 3]) {
        for (index, limit) in limits.into_iter().enumerate() {
            self.memory.set_halfword(self.configuration + CONFIGURED_WINDOW + 2 * index as u32, limit);
        }
    }

    fn set_request_window(&mut self, limits: [u16; 3]) {
        for (index, limit) in limits.into_iter().enumerate() {
            self.memory.set_halfword(self.request + REQUEST_WINDOW + 2 * index as u32, limit);
        }
    }
}

pub struct StockScanLibrary;

impl ScanLibrary<StockScanBlocks> for StockScanLibrary {
    type Output = StockScanOutput;

    fn apply(&mut self, part: StockPart, blocks: &mut StockScanBlocks) {
        let (configuration, request) = (blocks.configuration, blocks.request);
        let both = |entry: u32| entry!(entry, fn(u32, u32))(configuration, request);
        match part {
            StockPart::Geometry => {
                entry!(0xd6_415cu32, fn(u32, u32, u32, u32, u32))(configuration, request, request + 6, request + 0xf, request + 0x13)
            }
            StockPart::Second => both(0xd6_4424),
            StockPart::Fourth => {
                both(0xd6_4800);
                both(0xd6_4914);
            }
            StockPart::Fifth => both(0xd6_49b4),
            StockPart::Sixth => both(0xd6_4b5c),
            StockPart::Eighth => both(0xd6_56ac),
            StockPart::Ninth => both(0xd6_53e8),
            StockPart::Tenth => both(0xd6_541c),
            StockPart::Eleventh => both(0xd6_5524),
            StockPart::Twelfth => both(0xd6_55f4),
            StockPart::Pair => {
                both(0xd6_579c);
                both(0xd6_5834);
            }
            StockPart::Sixteenth => both(0xd6_58a0),
            StockPart::Seventeenth => {
                for entry in [0xd6_59bc, 0xd6_5ce4, 0xd6_5e90, 0xd6_5ee8, 0xd6_5f94, 0xd6_60fc] {
                    both(entry);
                }
            }
            StockPart::Eighteenth => both(0xd6_62a0),
            StockPart::Nineteenth => both(0xd6_63e0),
        }
    }

    fn finish_defaults(&mut self, blocks: &mut StockScanBlocks) {
        entry!(0xd7_9178u32, fn(u32))(blocks.configuration);
        entry!(0xd7_9194u32, fn(u32))(blocks.configuration);
    }

    fn fill_settings(&mut self, blocks: &StockScanBlocks, output: &mut StockScanOutput) {
        entry!(0xd6_4840u32, fn(u32, u32))(blocks.configuration, output.0)
    }

    fn fill_geometry(&mut self, blocks: &StockScanBlocks, output: &mut StockScanOutput) {
        let (mut first, mut second) = ([0u32; 3], 0u16);
        entry!(0xd6_49e8u32, fn(u32, u32, u32, u32))(blocks.configuration, address_of(&mut first), output.0, address_of(&mut second));
    }

    fn nine(&mut self, blocks: &StockScanBlocks) -> Optioned<9> {
        let mut table = [0u16; 10];
        entry!(0xd6_4940u32, fn(u32, u32))(address_of(&mut table), blocks.configuration);
        optioned::<9, 10>(table)
    }

    fn four(&mut self, blocks: &StockScanBlocks) -> Optioned<4> {
        let mut table = [0u16; 5];
        entry!(0xd6_4e2cu32, fn(u32, u32))(address_of(&mut table), blocks.configuration);
        optioned::<4, 5>(table)
    }
}

/// A caller's region (0xda027c), parameter (0xd9f7c0, 0xd9f828) or geometry (0xd9fd54) block,
/// by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockStageBlock<M = StockMemory> {
    pub address: u32,
    pub memory: M,
}

const fn banked(bank: Bank, first: u32, second: u32) -> u32 {
    match bank {
        Bank::First => first,
        Bank::Second => second,
    }
}

const fn part_offset(part: Part) -> u32 {
    match part {
        Part::ProcessingOrigin => 0x24,
        Part::ProcessingFormat => 0x2a,
        Part::InputMode => 0x30,
        Part::FilterSelectors => 0x0c,
        Part::InputControl => 0x00,
        Part::Division => 0x0c,
        Part::Points => 0x18,
        Part::Pairs => 0x38,
        Part::SecondWindow | Part::SecondExtent => 0x44,
        Part::RotatedTables => 0x54,
        Part::FirstTables => 0x60,
        Part::SecondTables => 0x6c,
        Part::Area => 0x8c,
        Part::BlockLayout => 0x94,
        Part::InputRegister3c => 0xf4,
        Part::TransformOffset(bank) => banked(bank, 0x134, 0x1c4),
        Part::TransformThirdOffset(bank) => banked(bank, 0x138, 0x1c8),
        Part::TransformWindow(bank) => banked(bank, 0x140, 0x1d0),
        Part::TransformTable(bank) => banked(bank, 0x148, 0x1d8),
        Part::Window => 0x258,
        Part::Curves => 0x264,
        Part::InputRegister14 => 0x00,
        Part::TransformDimensions(bank) => banked(bank, 0x08, 0x40),
        Part::TransformGeometry(bank) => banked(bank, 0x0c, 0x44),
        Part::TransformSecondOffset(bank) => banked(bank, 0x14, 0x4c),
        Part::TransformSecondaryGeometry(bank) => banked(bank, 0x1c, 0x54),
        Part::TransformReadback(bank) => banked(bank, 0x24, 0x5c),
        Part::TransformAddress(bank) => banked(bank, 0x34, 0x6c),
        Part::TransformNibble(bank) => banked(bank, 0x3c, 0x74),
        Part::InputRegister34 => 0x88,
        Part::FilterGrid(Grid::Second) => 0x94,
        Part::FilterSecondBank => 0xa4,
        Part::FilterGrid(Grid::First) => 0xf4,
        Part::FilterFirstBank => 0x104,
        Part::FilterGrid(Grid::Third) => 0x154,
        Part::FilterWindow(FilterWindow::First) => 0x164,
        Part::FilterWindow(FilterWindow::Second) => 0x16c,
        Part::FilterWindow(FilterWindow::Third) => 0x176,
        Part::FilterWindow(FilterWindow::Fourth) => 0x180,
        Part::InputRegister28 => 0x190,
        Part::InputRegister34Alternate => 0x198,
        Part::Extent => 0x1a0,
    }
}

impl<M> StockStageBlock<M> {
    /// The address the part's stock entry takes; `Part::InputMode`'s entry takes the word there.
    pub const fn part(&self, part: Part) -> u32 { self.address + part_offset(part) }
}

impl<M: GuestMemory> StockStageBlock<M> {
    fn word(&self, offset: u32) -> u32 { self.memory.word(self.address + offset) }
}

impl<M: GuestMemory> StageBlock for StockStageBlock<M> {
    fn stage_mode(&self, stage: Stage) -> StageMode {
        StageMode::of(self.word(match stage {
            Stage::First => 0x14,
            Stage::Second => 0x30,
            Stage::Third => 0x40,
            Stage::Fourth => 0x88,
        }))
    }

    fn enable_change(&self, enable: Enable) -> EnableChange {
        let (keep, invert, set, inverts_on) = match enable {
            Enable::Gate => (0x34, 0x24, 0x28, 1),
            Enable::Mode => (0x84, 0x7c, 0x80, 0),
            Enable::Output => (0x290, 0x28c, 0x288, 0),
        };
        if self.word(keep) == 1 {
            EnableChange::Keep
        } else if self.word(invert) == inverts_on {
            EnableChange::Invert
        } else {
            EnableChange::Set(self.word(set))
        }
    }

    fn setting(&self, setting: Setting) -> u32 {
        match setting {
            Setting::PathEnable => self.word(0x5c),
            Setting::SecondGate => self.word(0x254),
            Setting::ModeLevel => u32::from(self.memory.byte(self.address + 0x78)),
        }
    }
}

fn address_of<T>(value: &mut T) -> u32 { value as *mut T as usize as u32 }

/// A compiled scan configuration by guest address: the scan library's configuration from
/// +0x101c (output kind, then the option bytes at +4 and +8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockCompiledScan<M = StockMemory> {
    pub address: u32,
    pub memory: M,
}

/// A caller's block the scan library fills, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockScanOutput(pub u32);

const LIBRARY_CONFIGURATION: u32 = 0x101c;

impl<M> StockCompiledScan<M> {
    /// The scan library's configuration, which 0xd5e1d4 passes 0xda21cc (0xd5e1e4..0xd5e1e8).
    pub fn library(&self) -> u32 { self.address + LIBRARY_CONFIGURATION }
}

/// The library's optioned table: the option byte, then the values from +2.
fn optioned<const N: usize, const W: usize>(raw: [u16; W]) -> Optioned<N> {
    Optioned { option: raw[0] as u8, values: core::array::from_fn(|index| raw[index + 1]) }
}

fn raw<const N: usize, const W: usize>(table: Optioned<N>) -> [u16; W] {
    core::array::from_fn(|index| if index == 0 { u16::from(table.option) } else { table.values[index - 1] })
}

impl<M: GuestMemory> CompiledScan for StockCompiledScan<M> {
    fn set_long_table(&mut self, values: &[u16; 22]) {
        let mut values = *values;
        entry!(0xd6_4850u32, fn(u32, u32))(address_of(&mut values), self.library());
    }

    fn set_wide_table(&mut self, values: &[u16; 28]) {
        let mut values = *values;
        entry!(0xd6_49f8u32, fn(u32, u32, u32, u32))(0, address_of(&mut values), 0, self.library());
    }

    fn set_nine(&mut self, table: Optioned<9>) {
        let mut table = raw::<9, 10>(table);
        entry!(0xd6_4950u32, fn(u32, u32))(address_of(&mut table), self.library());
    }

    fn set_four(&mut self, table: Optioned<4>) {
        let mut table = raw::<4, 5>(table);
        entry!(0xd6_537cu32, fn(u32, u32))(address_of(&mut table), self.library());
    }

    fn options(&self) -> [u8; 2] { [self.memory.byte(self.library() + 4), self.memory.byte(self.library() + 8)] }

    fn set_output_kind(&mut self, kind: u8) { self.memory.set_byte(self.library(), kind) }
}

/// The stream a snapshot's options name, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSource(pub u32);

/// A stream object and the snapshot block it is written to, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockStreamSnapshot<M = StockMemory> {
    pub stream: u32,
    pub snapshot: u32,
    pub memory: M,
}

const STREAM_OPTIONS: u32 = 0xb_d668;

impl StockStreamSnapshot {
    /// The stream's own snapshot block, which 0xd6b460 writes.
    pub const fn of_stream(stream: u32) -> Self { Self { stream, snapshot: stream + 0xb_d4e4, memory: StockMemory } }
}

impl<M: GuestMemory> StreamSnapshot for StockStreamSnapshot<M> {
    type Registers = [u32; 96];
    type Source = StreamSource;

    fn write_tag(&mut self, tag: [u8; 4]) {
        for (at, byte) in (self.snapshot..).zip(tag) {
            self.memory.set_byte(at, byte);
        }
    }

    fn read_registers(&mut self) -> [u32; 96] {
        let mut registers = [0u32; 96];
        entry!(0x69_3dccu32, fn(u32))(address_of(&mut registers));
        registers
    }

    fn pack(&mut self, registers: &[u32; 96]) {
        let mut registers = *registers;
        entry!(0x73_ed94u32, fn(u32, u32, u32, u32))(self.snapshot, address_of(&mut registers), 0xd6_baac, 96);
    }

    fn select(&mut self, source: StreamSource) { entry!(0xd6_ae18u32, fn(u32, u32, u32))(self.stream, 0, source.0) }

    fn set_options(&mut self, options: [u32; 2]) {
        self.memory.set_word(self.stream + STREAM_OPTIONS + 0x370, options[0]);
        self.memory.set_word(self.stream + STREAM_OPTIONS + 0x374, options[1]);
    }

    fn apply_options(&mut self, source: StreamSource) { entry!(0xd6_afe4u32, fn(u32, u32))(self.stream + STREAM_OPTIONS, source.0) }
}

pub struct StockDumpDerivations;

impl DumpDerivations for StockDumpDerivations {
    fn transformed_value(&mut self, step: Step, row: Row) -> u16 { entry!(0x74_68b8u32, fn(u32, u32) -> u32)(step.0, row.0) as u16 }

    fn transformed_word(&mut self, step: Step, row: Row) -> u16 { entry!(0x74_695cu32, fn(u32, u32) -> u32)(step.0, row.0) as u16 }

    fn lens_position_flags(&mut self) -> u32 { entry!(0x76_f340u32, fn() -> u32)() }

    fn selection_word(&mut self, channel: Channel, mode: u32, flags: u32, word: i16) -> u32 {
        entry!(0x74_8398u32, fn(i32, u32, u32, i32) -> u32)(channel.0, mode, flags, i32::from(word))
    }
}

/// A shot's dump buffer, by guest address; its configuration section starts at +0xb174c.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockDumpBuffer<M = StockMemory> {
    pub address: u32,
    pub memory: M,
}

const CONFIGURATION_SECTION: u32 = 0xb_174c;
const END_OF_STEPS: u16 = 0xfdfd;

impl<M: GuestMemory> StockDumpBuffer<M> {
    /// Writes the section in 0x748928's store order.
    pub fn write_configuration(&self, dump: &ConfigurationDump) {
        let section = self.address + CONFIGURATION_SECTION;
        let word = |offset: u32, value: u32| self.memory.set_word(section + offset, value);
        let half = |at: u32, value: u16| self.memory.set_halfword(at, value);
        word(0, u32::from_le_bytes(*b"ELTS"));
        word(0x2a4, u32::from_le_bytes(*b"ELAL"));
        let column = |index: u32, first: u32| section + 4 + 2 * (first + index);
        let scale = |index: u32| section + 2 * (184 + index);
        let steps = dump.steps.iter().take(dump.count);
        for (index, step) in (0u32..).zip(steps) {
            half(column(index, 0), step.rounded as u16);
            half(column(index, 32), step.value);
            half(column(index, 68), step.transformed_value);
            half(column(index, 100), step.transformed_word);
            half(scale(index), step.scale);
        }
        let end = dump.count as u32;
        for first in [0, 32, 68, 100] {
            half(column(end, first), END_OF_STEPS);
        }
        half(scale(end), END_OF_STEPS);
        let references = dump.references.unwrap_or([0; 3]);
        for (offset, value) in [0x130, 0x134, 0x140].into_iter().zip(references) {
            word(offset, value);
        }
        let record_word = i32::from(dump.record_word) as u32;
        word(0x84, record_word);
        word(0x88, record_word);
        let words = [0x10c, 0x110, 0x114, 0x118, 0x11c, 0x120, 0x124, 0x128, 0x12c, 0x138, 0x13c, 0x144, 0x148, 0x154, 0x158];
        let floors = [0x14c, 0x150];
        let late = [0x15c, 0x160, 0x164, 0x168];
        let ordered = words[..13].iter().zip(&dump.words[..13])
            .chain(floors.iter().zip(&dump.floors))
            .chain(words[13..].iter().zip(&dump.words[13..]))
            .chain(late.iter().zip(&dump.late_words));
        for (offset, value) in ordered {
            word(*offset, *value);
        }
        word(0x16c, u32::from(dump.channel_scale));
        for (offset, value) in (0x1b0..).step_by(4).zip(dump.limits.iter().flatten()) {
            word(offset, *value);
        }
        for (offset, value) in [0x278, 0x27c, 0x280, 0x284, 0x288, 0x28c, 0x298, 0x29c].into_iter().zip(dump.selections) {
            word(offset, value);
        }
        for (offset, value) in [0x290, 0x294, 0x2a0].into_iter().zip(dump.reference_selections.unwrap_or([0; 3])) {
            word(offset, value);
        }
    }
}
