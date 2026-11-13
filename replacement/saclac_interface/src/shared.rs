#![forbid(unsafe_code)]

//! Requests saclac's shared layer posts on behalf of every module.

use crate::stock::StockCallbackWord;

/// The lens control's mode for a relative move, an index into its three-word mode table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveMode { First, Second, Third }

/// A lensdc 0x420b relative move.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelativeMove {
    pub callback: StockCallbackWord,
    pub step: i16,
    pub mode: MoveMode,
    /// The payload words after the mode.
    pub words: [u32; 4],
}

/// What a lenscom 0x722d request asks for, carried in the low nibble of payload byte 6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensComCode(u8);

impl LensComCode {
    pub const fn new(code: u8) -> Option<Self> {
        if code < 0x10 { Some(Self(code)) } else { None }
    }

    pub const fn get(self) -> u8 { self.0 }
}

/// A lenscom 0x722d request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensComCommand {
    pub code: LensComCode,
    pub completion: StockCallbackWord,
}
