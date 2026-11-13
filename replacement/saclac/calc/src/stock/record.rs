//! Byte layouts of the record fields the exposure engine parses.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;

use crate::exposure::context::{FocusResult, Subject, SubjectList, SUBJECTS, SUBJECT_DETAIL_BYTES};

pub const FOCUS_RESULT_BYTES: usize = 132;
pub const SUBJECT_LIST_BYTES: usize = 4 + SUBJECTS * (4 + SUBJECT_DETAIL_BYTES);

/// The handle word an unbound context passes: accessors read the selected record for it.
pub const UNBOUND: RecordHandle = xm1_saclac_interface::stock::record_handle_from_a2(0);

const fn base(address: Option<SharedAddress>) -> u32 {
    match address {
        Some(address) => address.0,
        None => 0,
    }
}

/// Point `point` of the four 256-halfword level tables a focus result names (0xf6ae38, 0xf6b354).
/// An absent table is read at address 0, as stock reads it.
pub const fn level_table(tables: Option<SharedAddress>, table: usize, point: usize) -> SharedAddress {
    SharedAddress(base(tables) + 0x200 * table as u32 + 2 * point as u32)
}

/// Point `point` of the four 10-halfword subject level rows a focus result names (0xf6aefc,
/// 0xf6b3cc).
pub const fn subject_level(levels: Option<SharedAddress>, row: usize, point: usize) -> SharedAddress {
    SharedAddress(base(levels) + 20 * row as u32 + 2 * point as u32)
}

/// The signed class byte of level point `point` (0xf6aeb0, 0xf6b470).
pub const fn level_class(classes: Option<SharedAddress>, point: usize) -> SharedAddress { SharedAddress(base(classes) + point as u32) }

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }

fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }

fn address(bytes: &[u8], at: usize) -> Option<SharedAddress> {
    match word(bytes, at) {
        0 => None,
        address => Some(SharedAddress(address)),
    }
}

/// The record's focus result (group 14, 0x756734) at the offsets 0xf69a4c, 0xf6ac90, 0xf65cf4
/// and 0xf6b890 read.
pub fn focus_result(bytes: &[u8; FOCUS_RESULT_BYTES]) -> FocusResult {
    FocusResult {
        reference: word(bytes, 0x00),
        valid: word(bytes, 0x04),
        lens: word(bytes, 0x08),
        terms: core::array::from_fn(|index| half(bytes, 0x0c + 2 * index)),
        limit: half(bytes, 0x26) as i16,
        pinned: word(bytes, 0x28),
        level_tables: address(bytes, 0x2c),
        subject_levels: address(bytes, 0x30),
        level_bias: half(bytes, 0x34) as i16,
        level_classes: address(bytes, 0x38),
        statistics: address(bytes, 0x48),
        weights: core::array::from_fn(|index| half(bytes, 0x3c + 2 * index)),
        lens_detail: word(bytes, 0x50),
        levels: core::array::from_fn(|index| half(bytes, 0x54 + 2 * index) as i16),
        extent: word(bytes, 0x60),
        adjustment: core::array::from_fn(|index| word(bytes, 0x68 + 4 * index)),
        adjustment_kind: half(bytes, 0x74),
        subject_count: word(bytes, 0x78),
        adjustment_level: half(bytes, 0x7c) as i16,
    }
}

/// A subject list as records and the exposure context hold it: the count, then ten entries of an
/// id and 100 detail bytes.
pub fn subject_list(bytes: &[u8; SUBJECT_LIST_BYTES]) -> SubjectList {
    SubjectList {
        count: word(bytes, 0),
        entries: core::array::from_fn(|index| {
            let at = 4 + index * (4 + SUBJECT_DETAIL_BYTES);
            let mut detail = [0; SUBJECT_DETAIL_BYTES];
            detail.copy_from_slice(&bytes[at + 4..at + 4 + SUBJECT_DETAIL_BYTES]);
            Subject { id: word(bytes, at), detail }
        }),
    }
}

pub fn words<const N: usize, const BYTES: usize>(bytes: &[u8; BYTES]) -> [u32; N] { core::array::from_fn(|index| word(bytes, 4 * index)) }
