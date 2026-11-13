//! The exposure section of the client's dump frame: tag words at +0x35c8 and +0x116a8, words
//! from +0x35c8 on.

use xm1_saclac_interface::calc::SharedAddress;

const SECTION: u32 = 0x35c8;

/// 'XETS' (0xf614fc).
pub const SECTION_TAG: u32 = 0x5354_4558;
/// 'XEAL' (0xf6150c).
pub const END_TAG: u32 = 0x4c41_4558;

pub const fn section_tag(frame: SharedAddress) -> SharedAddress { SharedAddress(frame.0 + SECTION) }

pub const fn end_tag(frame: SharedAddress) -> SharedAddress { SharedAddress(frame.0 + 0x116a8) }

const fn at(frame: SharedAddress, offset: u32) -> SharedAddress { SharedAddress(frame.0 + SECTION + offset) }

/// The words 0xf6acfc clears at each preparation.
pub const fn cleared(frame: SharedAddress) -> (SharedAddress, usize) { (at(frame, 0xc), 0x1468) }

/// The export word 0xf61550 stores.
pub const fn export(frame: SharedAddress) -> SharedAddress { SharedAddress(frame.0 + 0xfd28) }

/// Words 0xf6ad14..0xf6ad4c store.
pub const fn configuration(frame: SharedAddress, index: usize) -> SharedAddress { at(frame, 0x106c + 4 * index as u32) }

pub const fn subjects_present(frame: SharedAddress) -> SharedAddress { at(frame, 0x1074) }

pub const fn treated(frame: SharedAddress) -> SharedAddress { at(frame, 0x1078) }

/// The record's level tables relative to the focus level, one word per point (0xf6ae48..0xf6aea8).
pub const fn level_table(frame: SharedAddress, table: usize, point: usize) -> SharedAddress {
    at(frame, 0x44 + 0x400 * table as u32 + 4 * point as u32)
}

pub const fn subject_level(frame: SharedAddress, subject: usize) -> SharedAddress { at(frame, 0x1044 + 4 * subject as u32) }

pub const fn level_bias(frame: SharedAddress) -> SharedAddress { at(frame, 0x107c) }

pub const fn lens_detail(frame: SharedAddress) -> SharedAddress { at(frame, 0x1080) }

pub const fn lens_base_level(frame: SharedAddress) -> SharedAddress { at(frame, 0x1084) }

pub const fn focus_level(frame: SharedAddress, index: usize) -> SharedAddress { at(frame, 0x1088 + 4 * index as u32) }

pub const fn level_class(frame: SharedAddress, point: usize) -> SharedAddress { at(frame, 0x1098 + 4 * point as u32) }

pub const fn weight(frame: SharedAddress, index: usize) -> SharedAddress { at(frame, 0x1498 + 4 * index as u32) }

/// The snapshot words 0xf6a0a4..0xf6a0bc store after the evaluation is prepared.
pub const fn evaluation(frame: SharedAddress, index: usize) -> SharedAddress { at(frame, 0x15e8 + 4 * index as u32) }

/// The reuse trace 0xf6b8a8..0xf6b930 stores: the result's three adjustment words, its kind and
/// subject count.
pub const fn reuse_result(frame: SharedAddress, index: usize) -> SharedAddress { at(frame, 0xc764 + 4 * index as u32) }

/// Cleared by the trace, set when the result's level is out of tolerance (0xf6b9d0).
pub const fn reuse_out_of_tolerance(frame: SharedAddress) -> SharedAddress { at(frame, 0xc7a0) }

pub const fn reuse_level(frame: SharedAddress) -> SharedAddress { at(frame, 0xc7a4) }

pub const fn reuse_limit(frame: SharedAddress) -> SharedAddress { at(frame, 0xc7a8) }

pub const fn reuse_tolerance(frame: SharedAddress) -> SharedAddress { at(frame, 0xc7ac) }
