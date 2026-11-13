#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the S1 search's snapshot and completions.

use super::evaluation::evaluated;
use super::{half, word};
use crate::contrast::LensIdentity;
use crate::s1::{S1Completion, S1Snapshot};
use xm1_saclac_interface::search::ResultAddress;
use xm1_saclac_interface::stock::search::{bridge_report, EVALUATION_BYTES};
use xm1_saclac_interface::{Completed, Completion};

/// The bytes 0xfeaadc(1) copies for the S1 frame.
pub const S1_SNAPSHOT_BYTES: usize = 96;

/// The snapshot as 0xf2711c reads it.
pub fn s1_snapshot(bytes: &[u8; S1_SNAPSHOT_BYTES]) -> S1Snapshot {
    let pair = |offset: usize| [word(bytes, offset), word(bytes, offset + 4)];
    S1Snapshot {
        size: pair(0x00),
        lens: LensIdentity(word(bytes, 0x28) as i32),
        windows: [pair(0x2c), pair(0x3c)],
        extent: pair(0x34),
        frame_mode: word(bytes, 0x44),
        pinned_setting: half(bytes, 0x4c),
    }
}

/// Completions 0x9d, whose result `read` copies out of calc's memory, and 0x5a. `None` for
/// any other completion and for a payload shorter than its layout.
pub fn completion(completed: Completed<'_>, read: impl FnOnce(ResultAddress) -> [u8; EVALUATION_BYTES]) -> Option<S1Completion> {
    let payload = completed.parameters.bytes();
    match completed.completion {
        Completion::Calculation9d => evaluated(payload, read).map(S1Completion::Evaluated),
        Completion::Image5a => bridge_report(payload).map(S1Completion::LensReported),
        _ => None,
    }
}
