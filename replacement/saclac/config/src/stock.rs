//! The stock boundary: X-M1 1.50 entries and layouts behind the configuration layers' ports.

use core::ptr::{read_volatile, write_volatile};

use crate::channel::{Channel, FloatLog};
use crate::compiled::{CompiledScan, Optioned, StreamSnapshot};
use crate::face::{FaceTracking, TrackingBusy};
use crate::ports::{Derivations, ExportQuery};
use crate::pipeline::{Bank, Enable, EnableChange, FilterWindow, Grid, Part, Setting, Stage, StageBlock};
use crate::registers::PipelineHardware;
use crate::scan::{ScanBlocks, ScanField, ScanLibrary, StockPart};
use crate::services::LensRequests;

macro_rules! entry {
    ($address:expr, fn($($argument:ty),*) $(-> $result:ty)?) => {{
        let entry: extern "C" fn($($argument),*) $(-> $result)? = unsafe { core::mem::transmute($address as usize as *const ()) };
        entry
    }};
}

fn read_u8(address: u32) -> u8 { unsafe { read_volatile(address as usize as *const u8) } }
fn read_u16(address: u32) -> u16 { unsafe { read_volatile(address as usize as *const u16) } }
fn read_u32(address: u32) -> u32 { unsafe { read_volatile(address as usize as *const u32) } }
fn write_u8(address: u32, value: u8) { unsafe { write_volatile(address as usize as *mut u8, value) } }
fn write_u16(address: u32, value: u16) { unsafe { write_volatile(address as usize as *mut u16, value) } }
fn write_u32(address: u32, value: u32) { unsafe { write_volatile(address as usize as *mut u32, value) } }

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

pub struct StockPipelineHardware;

impl PipelineHardware for StockPipelineHardware {
    fn acquire(&mut self, client: u32) -> u32 { entry!(0xda_1b4cu32, fn(u32) -> u32)(client) }

    fn release(&mut self, client: u32, grant: u32) { entry!(0xda_1d48u32, fn(u32, u32))(client, grant) }
}

/// The word 0x742a6c stores the request's callback in.
const LENS_REQUEST_CALLBACK: u32 = 0x96_bd04;

pub struct StockLensRequests;

impl LensRequests for StockLensRequests {
    fn store_callback(&mut self, callback: u32) { write_u32(LENS_REQUEST_CALLBACK, callback) }

    fn request(&mut self, number: u8, callback: u32) -> u32 { entry!(0x6b_7750u32, fn(u32, u32) -> u32)(u32::from(number), callback) }
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
pub struct StockScanBlocks {
    pub configuration: u32,
    pub request: u32,
}

const CONFIGURATION_BYTES: u32 = 0x244;
const APPLIED_PARTS: u32 = 0x14c;
const REQUEST_WINDOW: u32 = 0x3e;

#[derive(Clone, Copy)]
enum Width { Byte, Half, Word }

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

impl ScanBlocks for StockScanBlocks {
    /// 0xd77abc fills the block through fill_bytes (0x7221c8).
    fn clear_configuration(&mut self) {
        entry!(0x72_21c8u32, fn(u32, u32, u32))(self.configuration, 0, CONFIGURATION_BYTES);
    }

    fn set(&mut self, field: ScanField, value: u32) {
        let (offset, width) = layout(field);
        let at = self.configuration + offset;
        match width {
            Width::Byte => write_u8(at, value as u8),
            Width::Half => write_u16(at, value as u16),
            Width::Word => write_u32(at, value),
        }
    }

    fn configuration(&self, field: ScanField) -> u32 {
        let (offset, width) = layout(field);
        let at = self.configuration + offset;
        match width {
            Width::Byte => u32::from(read_u8(at)),
            Width::Half => u32::from(read_u16(at)),
            Width::Word => read_u32(at),
        }
    }

    fn request_flag(&self, index: u8) -> u8 { read_u8(self.request + u32::from(index)) }

    fn mark_applied(&mut self, bits: u32) {
        let at = self.configuration + APPLIED_PARTS;
        write_u32(at, read_u32(at) | bits);
    }

    fn set_request_window(&mut self, limits: [u16; 3]) {
        for (index, limit) in limits.into_iter().enumerate() {
            write_u16(self.request + REQUEST_WINDOW + 2 * index as u32, limit);
        }
    }
}

pub struct StockScanLibrary;

impl ScanLibrary<StockScanBlocks> for StockScanLibrary {
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
}

/// A caller's region (0xda027c), parameter (0xd9f7c0, 0xd9f828) or geometry (0xd9fd54) block,
/// by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockStageBlock(pub u32);

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

impl StockStageBlock {
    /// The address the part's stock entry takes; `Part::InputMode`'s entry takes the word there.
    pub const fn part(self, part: Part) -> u32 { self.0 + part_offset(part) }

    fn word(self, offset: u32) -> u32 { read_u32(self.0 + offset) }
}

impl StageBlock for StockStageBlock {
    fn stage_mode(&self, stage: Stage) -> u32 {
        self.word(match stage {
            Stage::Stage400 => 0x14,
            Stage::Stage4 => 0x30,
            Stage::Stage8 => 0x40,
            Stage::Stage10 => 0x88,
        })
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
            Setting::ModeLevel => u32::from(read_u8(self.0 + 0x78)),
        }
    }
}

fn address_of<T>(value: &mut T) -> u32 { value as *mut T as usize as u32 }

/// A compiled scan configuration by guest address: stage flags at +0 and the scan library's
/// configuration from +0x101c (output kind, then the option bytes at +4 and +8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockCompiledScan(pub u32);

/// A caller's block the scan library fills, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockScanOutput(pub u32);

const LIBRARY_CONFIGURATION: u32 = 0x101c;

impl StockCompiledScan {
    fn library(self) -> u32 { self.0 + LIBRARY_CONFIGURATION }
}

/// The library's optioned table: the option byte, then the values from +2.
fn optioned<const N: usize, const W: usize>(raw: [u16; W]) -> Optioned<N> {
    Optioned { option: raw[0] as u8, values: core::array::from_fn(|index| raw[index + 1]) }
}

fn raw<const N: usize, const W: usize>(table: Optioned<N>) -> [u16; W] {
    core::array::from_fn(|index| if index == 0 { u16::from(table.option) } else { table.values[index - 1] })
}

impl CompiledScan for StockCompiledScan {
    type Block = StockScanOutput;

    fn fill_settings(&mut self, block: &mut StockScanOutput) { entry!(0xd6_4840u32, fn(u32, u32))(self.0, block.0) }

    fn fill_geometry(&mut self, block: &mut StockScanOutput) {
        let (mut first, mut second) = ([0u32; 3], 0u16);
        entry!(0xd6_49e8u32, fn(u32, u32, u32, u32))(self.0, address_of(&mut first), block.0, address_of(&mut second));
    }

    fn set_long_table(&mut self, values: &[u16; 22]) {
        let mut values = *values;
        entry!(0xd6_4850u32, fn(u32, u32))(address_of(&mut values), self.library());
    }

    fn set_wide_table(&mut self, values: &[u16; 28]) {
        let mut values = *values;
        entry!(0xd6_49f8u32, fn(u32, u32, u32, u32))(0, address_of(&mut values), 0, self.library());
    }

    fn nine(&mut self) -> Optioned<9> {
        let mut table = [0u16; 10];
        entry!(0xd6_4940u32, fn(u32, u32))(address_of(&mut table), self.0);
        optioned::<9, 10>(table)
    }

    fn set_nine(&mut self, table: Optioned<9>) {
        let mut table = raw::<9, 10>(table);
        entry!(0xd6_4950u32, fn(u32, u32))(address_of(&mut table), self.library());
    }

    fn four(&mut self) -> Optioned<4> {
        let mut table = [0u16; 5];
        entry!(0xd6_4e2cu32, fn(u32, u32))(address_of(&mut table), self.0);
        optioned::<4, 5>(table)
    }

    fn set_four(&mut self, table: Optioned<4>) {
        let mut table = raw::<4, 5>(table);
        entry!(0xd6_537cu32, fn(u32, u32))(address_of(&mut table), self.library());
    }

    fn options(&self) -> [u8; 2] { [read_u8(self.library() + 4), read_u8(self.library() + 8)] }

    fn set_output_kind(&mut self, kind: u8) { write_u8(self.library(), kind) }

    fn stage_flags(&self) -> u32 { read_u32(self.0) }
}

/// The stream a snapshot's options name, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSource(pub u32);

/// A stream object and the snapshot block it is written to, by guest address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockStreamSnapshot {
    pub stream: u32,
    pub snapshot: u32,
}

const STREAM_OPTIONS: u32 = 0xb_d668;

impl StockStreamSnapshot {
    /// The stream's own snapshot block, which 0xd6b460 writes.
    pub const fn of_stream(stream: u32) -> Self { Self { stream, snapshot: stream + 0xb_d4e4 } }
}

impl StreamSnapshot for StockStreamSnapshot {
    type Registers = [u32; 96];
    type Source = StreamSource;

    fn write_tag(&mut self, tag: [u8; 4]) {
        for (at, byte) in (self.snapshot..).zip(tag) {
            write_u8(at, byte);
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
        write_u32(self.stream + STREAM_OPTIONS + 0x370, options[0]);
        write_u32(self.stream + STREAM_OPTIONS + 0x374, options[1]);
    }

    fn apply_options(&mut self, source: StreamSource) { entry!(0xd6_afe4u32, fn(u32, u32))(self.stream + STREAM_OPTIONS, source.0) }
}
