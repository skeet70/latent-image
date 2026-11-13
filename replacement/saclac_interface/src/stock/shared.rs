#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the requests saclac's shared layer posts.

use crate::shared::{LensComCommand, MoveMode, RelativeMove};
use crate::stock::StockCallbackWord;
use crate::LensComOperation;

pub const RELATIVE_MOVE_BYTES: usize = 28;

/// 0x76e1ac zeroes 28 bytes (0x76e1d4), stores the callback (0x76e1e4), the step as a halfword
/// (0x76e1e8), the word the mode selects from 0x81a170, which holds 0, 1 and 2 (0x76e1f4), and
/// the four words (0x76e1fc..0x76e214).
pub fn relative_move(request: &RelativeMove) -> [u8; RELATIVE_MOVE_BYTES] {
    let mode: u32 = match request.mode {
        MoveMode::First => 0,
        MoveMode::Second => 1,
        MoveMode::Third => 2,
    };
    let mut bytes = [0; RELATIVE_MOVE_BYTES];
    bytes[0..4].copy_from_slice(&request.callback.0.to_le_bytes());
    bytes[4..6].copy_from_slice(&request.step.to_le_bytes());
    bytes[8..12].copy_from_slice(&mode.to_le_bytes());
    for (chunk, word) in bytes[12..].chunks_exact_mut(4).zip(request.words) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

pub const LENS_SYNCHRONIZE_BYTES: usize = 8;

/// 0x76f660 zeroes 8 bytes (0x76f684) and stores the callback (0x76f688) and its second argument
/// as byte 4 (0x76f68c); the lens synchronization 0xf472bc passes 0.
pub fn lens_synchronize(callback: StockCallbackWord) -> [u8; LENS_SYNCHRONIZE_BYTES] {
    let mut bytes = [0; LENS_SYNCHRONIZE_BYTES];
    bytes[0..4].copy_from_slice(&callback.0.to_le_bytes());
    bytes
}

pub const LENS_COM_BYTES: usize = 48;

/// 0x6bbe60 stores the completion at byte 0 (0x6bbe9c), zeroes bytes 4..8 (0x6bbeb0), sets bits
/// 6..16 of the halfword at 4 to 0xa9 (0x6bbeb8..0x6bbec0), puts the code in the low nibble of
/// byte 6 (0x6bbec4..0x6bbed0) and 1 in byte 44 (0x6bbea4). Stock leaves bytes 8..44 and 45..48
/// unwritten.
pub fn lens_com(command: &LensComCommand) -> [u8; LENS_COM_BYTES] {
    let mut bytes = [0; LENS_COM_BYTES];
    bytes[0..4].copy_from_slice(&command.completion.0.to_le_bytes());
    bytes[4..6].copy_from_slice(&(0xa9u16 << 6).to_le_bytes());
    bytes[6] = command.code.get();
    bytes[44] = 1;
    bytes
}

pub const fn lens_com_operation(operation: LensComOperation) -> u16 {
    match operation {
        LensComOperation::Request2d => 0x722d,
    }
}
