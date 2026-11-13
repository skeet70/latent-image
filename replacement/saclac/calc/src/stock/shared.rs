//! The engine's tables in the work memory calc reads, and the per-slot buffers the evaluation
//! requests name.

use xm1_saclac_interface::calc::SharedAddress;

pub const LEVEL_CURVES: usize = 4;
pub const CURVE_POINTS: usize = 256;
pub const SUBJECT_POINTS: usize = 10;

/// Four curves of 256 halfwords (0xf6b328..0xf6b384); request word +0x1044 names each.
pub const fn level_curve(curve: usize, point: usize) -> SharedAddress {
    SharedAddress(0x5ffb_ece0 + 0x200 * curve as u32 + 2 * point as u32)
}

pub const fn level_curve_base(curve: usize) -> SharedAddress { level_curve(curve, 0) }

/// Four rows of ten halfwords (0xf6b38c..0xf6b404).
pub const fn subject_curve(curve: usize, point: usize) -> SharedAddress {
    SharedAddress(0x5ffb_f4e0 + 20 * curve as u32 + 2 * point as u32)
}

pub const fn subject_curve_base(curve: usize) -> SharedAddress { subject_curve(curve, 0) }

/// 256 class bytes (0xf6b448..0xf6b49c); every row names it (0xf69f48).
pub const fn level_class(point: usize) -> SharedAddress { SharedAddress(0x5ffb_f530 + point as u32) }

/// Ten bytes cleared at preparation (0xf6b4a0); every column names it (0xf69f60).
pub const SELECTION_FLAGS: SharedAddress = SharedAddress(0x5ffc_0130);
pub const SELECTION_FLAG_BYTES: usize = 10;

/// 256 halfwords another task fills; the snapshot lowers each by the focus terms (0xf69ae0).
pub const fn focus_curve(point: usize) -> SharedAddress { SharedAddress(0x5ffc_0340 + 2 * point as u32) }

const SLOT_STRIDE: u32 = 0x1150;

/// A row's two calc buffers and its column's buffer (tables 0xf66040, 0xf66060, 0xf66000).
pub const fn row_buffers(row: usize) -> [SharedAddress; 2] {
    let base = 0x5ffb_6120 + SLOT_STRIDE * row as u32;
    [SharedAddress(base), SharedAddress(base + 0x1000)]
}

pub const fn column_buffer(row: usize) -> SharedAddress { SharedAddress(0x5ffb_7130 + SLOT_STRIDE * row as u32) }

/// The two buffers the correction request names (0xf69004..0xf69018).
pub const CORRECTION_BUFFERS: [SharedAddress; 2] = [SharedAddress(0x5ffe_be8c), SharedAddress(0x5ffe_bf0c)];
