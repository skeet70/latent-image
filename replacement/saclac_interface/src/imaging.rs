#![forbid(unsafe_code)]

//! Payloads of the requests saclac posts to the imaging task (imgdc). The profile values they
//! carry keep the meaning imgdc gives them.

use crate::stock::StockCallbackWord;

/// The profile values the S1 statistics configuration hands its imaging job, taken at the
/// S1 begin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct S1Job {
    /// Shared-block halfwords +0x206, +0x208 and +0x238.
    pub levels: [i16; 3],
    pub lens_index: u32,
    /// Shared-block word +0x200.
    pub lens_word: u32,
    pub selection: u32,
    /// Shared-block word +0x230.
    pub step: u32,
    /// Shared-block halfword +0x234 and byte +0x236.
    pub step_limit: u16,
    pub step_mode: u8,
    pub curves: [u32; 2],
    pub output: u32,
    pub view_mode: u32,
    /// The descriptor for the lens's current value, then the descriptor pair.
    pub descriptors: [u32; 3],
    /// The descriptor for any value, then the descriptor pair.
    pub setup: [u32; 3],
    /// The statistics window block's last two words.
    pub window: [u32; 2],
    /// Shared-block word +0x218 and halfword +0x216.
    pub exposure: u32,
    pub exposure_offset: i16,
    pub view_active: u32,
    pub unselected_view: u32,
    pub view_byte: u8,
    pub classes: [u32; 2],
    pub ratio: u32,
    pub span: u32,
    pub class: u32,
    pub unselected: u32,
    pub scene: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagingPayload {
    /// The callbacks post completions 0x57 (configured), 0x58 (ended) and 0x59 (reported).
    BeginS1 { job: S1Job, callbacks: [StockCallbackWord; 3] },
    EndS1 { callback: StockCallbackWord },
}
