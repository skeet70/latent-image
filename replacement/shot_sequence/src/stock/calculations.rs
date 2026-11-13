#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the capture's calculations job (machine 16, 0xf2c210..0xf2ef08).
//! Its context 0x16b79a8 has no reader outside machine 16 but the calc library, which reads the
//! processing configuration at +0x10 through the request's pointer; the result copies at
//! +0xa8..+0xee have no reader and are not kept.

use crate::calculations_work::{BinnedPlanes, ProcessingSelection, SecondJobInput, SecondaryMeasurement, StatisticsPlanes, BINNED, GRID, PLANES};
use xm1_saclac_interface::stock::record_handle_word;

pub const PROCESSING: u32 = 0x16b_79a8 + 0x10;

/// The capture command block 0x169d09c: job bits at +4, the detail word at +0x50 of the block
/// +0xc names (with configuration 0xa8 bit 7), the processing client word at +0x48 of the same
/// block, the fourth job's client target at +0x20, and the client structure at +0x38 that takes
/// the readout words at +0x18/+0x1c and the planes at +0x28..+0x3c (0xf2e774, 0xf2e7f8..0xf2e808,
/// 0xf2e9b8..0xf2ea08, 0xf2ee34).
pub const CAPTURE_COMMAND: u32 = 0x169_d09c;
pub const DETAIL_CONFIGURATION: u32 = 0xa8;

/// The statistics buffer 0xfeacac fills (four 256-word planes) and the four 64-word planes the
/// client reads (0xf2e874..0xf2e9e4).
pub const STATISTICS: u32 = 0x5ffb_6120;
pub const BINNED_PLANES: [u32; PLANES] = [0x5ffb_7140, 0x5ffb_7240, 0x5ffb_7340, 0x5ffb_7440];
/// Bytes +0x38 and +0x39 of the client structure: the binned grid's columns and rows.
pub const BINNED_DIMENSIONS: [u8; 2] = [BINNED as u8, BINNED as u8];

pub fn statistics_planes(words: &[u32; PLANES * GRID * GRID]) -> StatisticsPlanes {
    let mut planes = [[0; GRID * GRID]; PLANES];
    for (plane, chunk) in planes.iter_mut().zip(words.chunks_exact(GRID * GRID)) { plane.copy_from_slice(chunk); }
    planes
}

pub fn binned_words(planes: &BinnedPlanes) -> [u32; PLANES * BINNED * BINNED] {
    let mut words = [0; PLANES * BINNED * BINNED];
    for (chunk, plane) in words.chunks_exact_mut(BINNED * BINNED).zip(planes) { chunk.copy_from_slice(plane); }
    words
}

/// The capture pipeline's configuration entry 0xfeacac (source, buffer, processing, client word,
/// detail, selection).
pub const CONFIGURE_PROCESSING: u32 = 0xfeacac;
pub const SELECTION_BYTES: usize = 0x24;

/// The selection block 0xf2e7bc builds on its stack: the variant byte at +0, the variant-mode word
/// at +0x18, the readout mode and value at +0x1c and +0x20; stock leaves the rest unwritten.
pub fn selection_bytes(selection: &ProcessingSelection) -> [u8; SELECTION_BYTES] {
    let mut bytes = [0; SELECTION_BYTES];
    bytes[0] = selection.variant;
    bytes[0x18..0x1c].copy_from_slice(&u32::from(selection.variant_mode).to_le_bytes());
    bytes[0x1c..0x20].copy_from_slice(&selection.mode.to_le_bytes());
    bytes[0x20..0x24].copy_from_slice(&selection.value.to_le_bytes());
    bytes
}

/// Calc-library entries with their completion callbacks: 0xf4d448 (0xf2e6b4), 0xf51bcc (0xf2e710),
/// 0xf4d350 (0xf2e6e4), 0xf52120 (0xf2e67c). The first passes {record, processing}, the third and
/// fourth {record, processing, detail}, the second `second_input_bytes` with a zero second
/// argument.
pub const REQUEST_FIRST: u32 = 0xf4d448;
pub const REQUEST_SECOND: u32 = 0xf51bcc;
pub const REQUEST_THIRD: u32 = 0xf4d350;
pub const REQUEST_FOURTH: u32 = 0xf52120;

pub const SECOND_INPUT_BYTES: usize = 0x50;

/// The block 0xf2ea8c builds at r11-92 for 0xf51bcc; bytes +0x1e, +0x49..+0x4c and +0x4e are
/// never written by stock.
pub fn second_input_bytes(input: &SecondJobInput, processing: u32) -> [u8; SECOND_INPUT_BYTES] {
    let mut bytes = [0; SECOND_INPUT_BYTES];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    let measurement = input.measurement;
    put(0x00, &input.record.map_or(0, record_handle_word).to_le_bytes());
    put(0x04, &processing.to_le_bytes());
    put(0x08, &measurement.first.to_le_bytes());
    put(0x0a, &measurement.second.to_le_bytes());
    put(0x0c, &input.base.to_le_bytes());
    put(0x0e, &measurement.third.to_le_bytes());
    put(0x10, &measurement.fourth.to_le_bytes());
    put(0x12, &input.slot[0..6]);
    put(0x18, &input.slot[6..12]);
    put(0x20, &input.detail.to_le_bytes());
    put(0x24, &measurement.fifth.to_le_bytes());
    put(0x26, &measurement.sixth.to_le_bytes());
    put(0x28, &u32::from(input.paired).to_le_bytes());
    put(0x2c, &input.profile_kind.to_le_bytes());
    put(0x30, &measurement.block);
    put(0x48, &[measurement.count]);
    put(0x4c, &measurement.seventh.to_le_bytes());
    bytes
}

/// The secondary job's context 0x16b751c as 0xf2c210..0xf2c3c0 and 0xf2c878 read it.
pub const SECONDARY: u32 = 0x16b_751c;
pub const SECONDARY_BYTES: usize = 0x19c;

pub fn secondary_measurement(context: &[u8; SECONDARY_BYTES]) -> SecondaryMeasurement {
    let half = |offset: usize| i16::from_le_bytes([context[offset], context[offset + 1]]);
    let word = |offset: usize| u32::from_le_bytes([context[offset], context[offset + 1], context[offset + 2], context[offset + 3]]);
    let mut block = [0; 24];
    block.copy_from_slice(&context[0x16c..0x184]);
    SecondaryMeasurement {
        first: half(0x14a),
        second: half(0x148),
        base: half(0x158) as u16,
        refined: word(0x15c) != 0,
        third: half(0x160),
        fourth: half(0x166),
        fifth: half(0x162),
        sixth: half(0x164),
        count: context[0x168],
        block,
        seventh: half(0x198),
        marked: context[0x18c] != 0,
    }
}

/// Configuration entries: 0x80f2 through 0x69ff68 for the second job's offset unit.
pub const OFFSET_UNIT: u32 = 0x80f2;

/// Image-4 entries: paired mode 0x753c20 (record, 0), profile kind 0x74f564, slot 0x756e24
/// (12 bytes). Result stores: first 0x75263c (12 bytes) and 0x7526cc (word +8); second 0x7524fc
/// (word +0), 0x75259c (+4), 0x75466c (word +0xc), 0x75470c (+0x10), 0x7547ac (+0x16), 0x75484c
/// (+0x1c), 0x754a9c (+0x28), 0x7548dc (half +0x22), 0x75496c (half +0x24), 0x7549fc (half +0x26);
/// third 0x754b88; fourth 0x754c20, 0x754cb8, 0x754d50.
pub const PAIRED_MODE: u32 = 0x753c20;
pub const PROFILE_KIND: u32 = 0x74f564;
pub const SLOT: u32 = 0x756e24;

/// Client publications after each result: first 0xefaa80, second 0xefa968 then 0xefaa34, third
/// 0xefa9f0, fourth 0xefaacc.
pub const PUBLISH: [&[u32]; 4] = [&[0xefaa80], &[0xefa968, 0xefaa34], &[0xefa9f0], &[0xefaacc]];
