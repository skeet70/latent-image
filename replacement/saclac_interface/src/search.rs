#![forbid(unsafe_code)]

//! What the contrast search sends to imgdc's lens bridge (0x3f2e) and what the bridge reports
//! back (completion 0x46).

/// The search phase the bridge is told a request belongs to (0xf3e8e0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgePhase {
    Direction,
    PeakSearch,
    Peak,
    Return,
    Focus,
    FocusDrive,
    Approach,
    Monitor,
    Other,
}

/// imgdc 0x3f2e.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeRequest {
    pub phase: BridgePhase,
    /// A peak search that evaluates frames while the lens moves.
    pub scanning: bool,
    pub window: [u16; 2],
    /// Signed lens steps; zero when `moves` is false.
    pub travel: i16,
    /// The step rate 0xfaab3c computed with the step.
    pub rate: u32,
    pub moves: bool,
    pub speed: u32,
    /// The lens mode 0x76f360 reported when the step was computed.
    pub lens_mode: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeStatus {
    Moved,
    /// imgdc reports 21 from 0xff91dc.
    Deferred,
    /// -94.
    Fault,
    Other(i32),
}

/// The lens table class imgdc reads through 0x76dad4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensClass(pub u32);

/// imgdc's report on a bridge request (completion 0x46).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeReport {
    pub status: BridgeStatus,
    /// The lens position halfword (0x76da84).
    pub position: i16,
    pub class: LensClass,
}

/// Calc's result for the search's evaluation job (completion 0x89 points at it).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Evaluation {
    /// Three filter pairs, each its first and second sum (words 0..6).
    pub pairs: [[u32; 2]; 3],
    /// Low halves of words 10 and 11.
    pub levels: [u16; 2],
    /// The filter pairs the peak search reads instead (words 12..18).
    pub scan_pairs: [[u32; 2]; 3],
    /// Low halves of words 22 and 23.
    pub scan_levels: [u16; 2],
    pub reference: u32,
    /// Word 25.
    pub settled: u32,
    /// Word 26.
    pub flag: u32,
    /// The halfwords at 0x6c and 0x6e.
    pub focus_values: [u16; 2],
}
