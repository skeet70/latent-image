#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the shot module.

use crate::ports::{AbortStatus, HalfPressMode, HalfPressNotifications};
use crate::Outcome;
use xm1_saclac_controllers::children::ShotControl;
use xm1_saclac_interface::CompletionStatus;

/// Codes retained parents pass to the row functions 0xefdab8 (0xeb, 0xec), 0xeffcfc (0x103)
/// and 0xf005b4 (0xac).
pub const fn control_code(control: ShotControl) -> u16 {
    match control {
        ShotControl::StartS1 => 0xeb,
        ShotControl::AdvanceS1 => 0xec,
        ShotControl::InitializeCapture => 0x103,
        ShotControl::StartLiveSequence => 0xac,
    }
}

pub const fn control(code: u16) -> Option<ShotControl> {
    match code {
        0xeb => Some(ShotControl::StartS1),
        0xec => Some(ShotControl::AdvanceS1),
        0x103 => Some(ShotControl::InitializeCapture),
        0xac => Some(ShotControl::StartLiveSequence),
        _ => None,
    }
}

/// Retained parents test 1 and 2; 21 is the finish's relative-done word (0xf043d4, 0xf043f8),
/// which only the half-press pipeline distinguishes (0xf21de4).
pub const fn outcome_word(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::Complete => 1,
        Outcome::Pending => 2,
        Outcome::Unhandled => 3,
        Outcome::Progress => 21,
        Outcome::Fault | Outcome::Aborted => 0,
    }
}

const MODE_OFFSET: usize = 4;
const NOTIFICATION_OFFSET: usize = 0x10;
const FRAME_RELEASED_BIT: u32 = 1 << 2;
const RELATIVE_DONE_BIT: u32 = 1 << 3;

fn word(block: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = block.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Word +4 of the command-6 block, read at 0xf17030, 0xf16608, 0xf17a6c and 0xf18530.
pub fn half_press_mode(block: &[u8]) -> Option<HalfPressMode> { word(block, MODE_OFFSET).map(HalfPressMode) }

/// Word +0x10 of the command-6 block: bit 2 at 0xf21350, bit 3 at 0xf17db4.
pub fn half_press_notifications(block: &[u8]) -> Option<HalfPressNotifications> {
    let mask = word(block, NOTIFICATION_OFFSET)?;
    Some(HalfPressNotifications { frame_released: mask & FRAME_RELEASED_BIT != 0, relative_lens_done: mask & RELATIVE_DONE_BIT != 0 })
}

/// Word +0x10 of 0x169d370 (0xf2e65c); 13 and 14 abort a capture (0xf0c420..0xf0c43c).
pub const fn abort_status(word: u32) -> Option<AbortStatus> {
    match word {
        13 | 14 => Some(AbortStatus(CompletionStatus(word as i32))),
        _ => None,
    }
}
