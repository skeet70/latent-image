//! Task 43's selection lists at 0x169e580, read where the correction request copies them.

use xm1_saclac_interface::calc::SharedAddress;

const LISTS: u32 = 0x0169_e580;

/// The words 0xf690dc..0xf69104 copy to request +0x38..+0x40.
pub const CORRECTION_WORDS: [SharedAddress; 3] = [SharedAddress(LISTS + 0x1cf4), SharedAddress(LISTS + 0x1cf8), SharedAddress(LISTS + 0x1cfc)];

/// The byte 0xf69108 copies to request +0x44.
pub const CORRECTION_BYTE: SharedAddress = SharedAddress(LISTS + 0x1f02);

/// The word 0xf68ff0 copies to request +0x84.
pub const CORRECTION_WORD: SharedAddress = SharedAddress(LISTS + 0x1cec);
