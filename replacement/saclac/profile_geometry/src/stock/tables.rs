//! The image-5 read-only tables: where they live and how their rows are laid out.

use crate::numeric::FractionTables;
use crate::profile::{ClassIndex, ExpansionRatio, GeometryClass, ProfileEntry, ProfileHandle, ProfileKind, ProfileTables, ScalePair};

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
}

impl FractionTables for StockTables {
    fn fraction(&self, index: u8) -> u8 { read::<1>(FRACTIONS + index as usize)[0] }

    fn wide_fraction(&self, index: u8) -> u8 { read::<1>(WIDE_FRACTIONS + index as usize)[0] }
}
