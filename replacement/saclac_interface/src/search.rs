#![forbid(unsafe_code)]

//! What the contrast searches send to imgdc's lens bridge (live 0x3f2e, S1 0x3f44) and to calc
//! (0x3c09), and what the bridge (completions 0x46, 0x5a) and calc (0x89, 0x9d) report back.

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

/// Calc's result for a search's evaluation job (completions 0x89 and 0x9d point at it).
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

/// Where calc left an evaluation result (completions 0x89 and 0x9d).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultAddress(pub u32);

/// Which of the result's filter pairs calc evaluates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScanEvaluation {
    #[default]
    Plain,
    /// The scan pairs too, while the search still reads the plain ones.
    Scan,
    /// The scan pairs the search reads.
    Scanned,
}

/// What a search took in with a frame for calc 0x3c09 to evaluate. The live search's intake
/// and the S1 search fill it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchRecord {
    /// No evaluation has been accepted since the search began.
    pub first: bool,
    /// The record's scale fraction (record word +0x0), numerator first.
    pub scale: [u16; 2],
    /// The frame's snapshot status has bits 0 and 2 set; the S1 search always sets it.
    pub settled: bool,
    /// The live search: the profile's level matches (0xfaab18). The S1 search: the lens range
    /// is wider than the configured margin.
    pub level_matched: bool,
    pub selection: u32,
    pub mode: u32,
    pub ratio: [u32; 2],
    /// The lens positions the evaluation spans.
    pub range: [i16; 2],
    pub range_offset: i16,
    pub extent: [u32; 2],
    pub window: [u32; 2],
    pub frame_mode: u32,
    /// The search reads the scan pairs.
    pub scan: bool,
    pub profile_class: u32,
    pub resource: u32,
    pub scan_evaluation: ScanEvaluation,
    pub size: [u32; 2],
    pub timing: u32,
    pub decade: u16,
    pub correction: u32,
    pub limit: i16,
}

/// imgdc 0x3f44: the S1 search's lens drive. imgdc reports it with completion 0x5a, laid out as
/// a `BridgeReport`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S1DriveRequest {
    /// Signed lens steps; zero when `moves` is false.
    pub travel: i16,
    /// The step rate 0xfaab3c computed with the step.
    pub rate: u32,
    pub moves: bool,
    pub speed: u32,
    /// The lens mode 0x76f360 reports.
    pub lens_mode: u32,
}
