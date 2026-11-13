//! The image-5 read-only tables: where they live and how their rows are laid out.

use crate::numeric::FractionTables;
use crate::profile::{
    ClassIndex, ExpansionRatio, GeometryClass, PipelineGroup, ProfileEntry, ProfileHandle, ProfileKind, ProfileRow, ProfileTables,
    QuadGroup, RectangleKind, ScalePair, ScanSlot,
};

const ENTRIES: usize = 0xef6de4;
pub const ENTRY_BYTES: usize = 0x14;
const CLASSES: usize = 0xef6aa4;
pub const CLASS_BYTES: usize = 0x30;
const THRESHOLDS: usize = 0xef66d4;
const TEMPLATE: usize = 0xef6dd4;
const DEFAULT_PAIR: usize = 0xef6ddc;
const FRACTIONS: usize = 0xee9ef4;
const WIDE_FRACTIONS: usize = 0xee9d60;
const CURVE_KNOTS: usize = 0xef50c4;
const FEATURE_SCALES: usize = 0xef70b4;
const ROWS: usize = 0xed7e08;
pub const ROW_BYTES: usize = 0x10;
const ASPECT_VALUES: usize = 0xef66d0;
const RECTANGLES: [usize; 3] = [0xee8664, 0xee8764, 0xee8864];
pub const RECTANGLE_BYTES: usize = 0x10;
const SCAN_SLOTS: usize = 0xede9a8;
pub const SCAN_SLOT_BYTES: usize = 0xc;
const RECORD_CODES: usize = 0xeef628;

fn u16_at(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }

fn u32_at(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }

pub fn parse_entry(bytes: &[u8; ENTRY_BYTES]) -> ProfileEntry {
    ProfileEntry {
        class: ClassIndex(u32_at(bytes, 0x4)),
        kind: ProfileKind(bytes[0x8]),
        flags: u16_at(bytes, 0xa),
        threshold_index: bytes[0xc],
        feature: bytes[0xd],
        aspect_index: bytes[0xe],
    }
}

pub fn parse_class(bytes: &[u8; CLASS_BYTES]) -> GeometryClass {
    GeometryClass {
        height: u16_at(bytes, 0x0),
        width: u16_at(bytes, 0x2),
        top: u16_at(bytes, 0xc),
        left: u16_at(bytes, 0xe),
        crop_height: u16_at(bytes, 0x10),
        crop_width: u16_at(bytes, 0x12),
        first_scale: bytes[0x24],
        second_scale: bytes[0x25],
        first_flag: bytes[0x26],
        second_flag: bytes[0x27],
        scale_selection: bytes[0x28],
        attributes: [bytes[0x29], bytes[0x2a]],
        kind: bytes[0x2b],
        magnification_index: bytes[0x2c],
        limit: bytes[0x2e],
    }
}

pub fn parse_expansion(template: &[u8; 5]) -> ExpansionRatio { ExpansionRatio { numerator: template[1], divisor: template[4] } }

pub fn parse_row(bytes: &[u8; ROW_BYTES]) -> ProfileRow {
    ProfileRow {
        rectangle_row: u32_at(bytes, 0x0),
        pipeline_group: PipelineGroup(u32_at(bytes, 0x4)),
        policy: u32_at(bytes, 0x8),
        quad_group: QuadGroup(u32_at(bytes, 0xc)),
    }
}

/// A rectangle configuration row's four words, in rectangle order.
pub fn parse_rectangle_configuration(bytes: &[u8; RECTANGLE_BYTES]) -> [u32; 4] {
    core::array::from_fn(|index| u32_at(bytes, index * 4))
}

pub fn parse_scan_slot(bytes: &[u8; SCAN_SLOT_BYTES]) -> ScanSlot {
    ScanSlot {
        margin: u16_at(bytes, 0x0),
        base: u16_at(bytes, 0x2),
        divisor: u16_at(bytes, 0x4),
        height: u16_at(bytes, 0x8),
        width: u16_at(bytes, 0xa),
    }
}

fn read<const N: usize>(address: usize) -> [u8; N] {
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = unsafe { core::ptr::read_volatile((address + index) as *const u8) };
    }
    bytes
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StockTables;

impl ProfileTables for StockTables {
    fn entry(&self, profile: ProfileHandle) -> ProfileEntry {
        parse_entry(&read(ENTRIES.wrapping_add((profile.0 as usize).wrapping_mul(ENTRY_BYTES))))
    }

    fn class(&self, class: ClassIndex) -> GeometryClass {
        parse_class(&read(CLASSES.wrapping_add((class.0 as usize).wrapping_mul(CLASS_BYTES))))
    }

    fn threshold(&self, index: u8) -> u16 {
        let bytes: [u8; 2] = read(THRESHOLDS + index as usize * 2);
        u16::from_le_bytes(bytes)
    }

    fn default_pair(&self) -> ScalePair {
        let bytes: [u8; 8] = read(DEFAULT_PAIR);
        ScalePair { first: u32_at(&bytes, 0), second: u32_at(&bytes, 4) }
    }

    fn expansion(&self) -> ExpansionRatio { parse_expansion(&read(TEMPLATE)) }

    fn curve_knot(&self, index: usize) -> i16 { i16::from_le_bytes(read(CURVE_KNOTS + index * 2)) }

    fn feature_scale_offsets(&self, row: u32) -> [u32; 2] {
        let bytes: [u8; 8] = read(FEATURE_SCALES + row as usize * 8);
        [u32_at(&bytes, 0), u32_at(&bytes, 4)]
    }

    fn row(&self, index: u32) -> ProfileRow { parse_row(&read(ROWS + index as usize * ROW_BYTES)) }

    fn aspect_value(&self, index: u8) -> i16 { i16::from_le_bytes(read(ASPECT_VALUES + index as usize * 2)) }

    fn rectangle_configuration(&self, kind: RectangleKind, row: u32) -> [u32; 4] {
        let table = RECTANGLES[match kind {
            RectangleKind::Plain => 0,
            RectangleKind::Rows => 1,
            RectangleKind::Columns => 2,
        }];
        parse_rectangle_configuration(&read(table + row as usize * RECTANGLE_BYTES))
    }

    fn scan_slot(&self, slot: u32) -> ScanSlot { parse_scan_slot(&read(SCAN_SLOTS + slot as usize * SCAN_SLOT_BYTES)) }

    fn record_code(&self, index: u32) -> [u16; 2] {
        let bytes: [u8; 4] = read(RECORD_CODES + index as usize * 4);
        [u16_at(&bytes, 0), u16_at(&bytes, 2)]
    }
}

impl FractionTables for StockTables {
    fn fraction(&self, index: u8) -> u8 { read::<1>(FRACTIONS + index as usize)[0] }

    fn wide_fraction(&self, index: u8) -> u8 { read::<1>(WIDE_FRACTIONS + index as usize)[0] }
}
