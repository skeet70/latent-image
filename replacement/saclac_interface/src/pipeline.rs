#![forbid(unsafe_code)]

//! What the half-press pipeline asks of its peers (imgdc 0x3f1d, 0x3f1e, 0x3f1f and 0x3f20, calc
//! 0x3c04 and 0x3c05, statistics 0x03) and what they deliver in the completions 0x4e, 0x50,
//! 0x51, 0x91, 0x92 and 0x93.

/// Imgdc 0x3f1d's answer (completion 0x4e); zero asks for calc 0x3c03.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RawCheck(pub u32);

/// Imgdc 0x3f1f's first answer (completion 0x50).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToneProgress {
    /// Zero asks for the capture sample taken at `level`.
    pub status: u32,
    pub level: u16,
}

/// Imgdc 0x3f1f's second answer (completion 0x51); zero asks for calc 0x3c05.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToneStatus(pub u32);

pub const RAW_CALCULATION_BYTES: usize = 20;
pub const TONE_CALCULATION_BYTES: usize = 108;
pub const GRADATION_CALCULATION_BYTES: usize = 96;

/// Calc 0x3c03's result (completion 0x91).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RawCalculation(pub [u8; RAW_CALCULATION_BYTES]);

/// Calc 0x3c04's result (completion 0x92).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToneCalculation(pub [u8; TONE_CALCULATION_BYTES]);

impl Default for ToneCalculation {
    fn default() -> Self { Self([0; TONE_CALCULATION_BYTES]) }
}

/// Calc 0x3c05's result (completion 0x93).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GradationCalculation(pub [u8; GRADATION_CALCULATION_BYTES]);

impl Default for GradationCalculation {
    fn default() -> Self { Self([0; GRADATION_CALCULATION_BYTES]) }
}

/// The subject geometry the frame requests carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectGeometry {
    pub profile: u32,
    pub state: u32,
    pub horizontal: u16,
    pub vertical: u16,
    pub extent: u32,
}

/// The capture area the frame requests carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureArea {
    pub reference: u32,
    pub areas: [u32; 3],
    pub area: u32,
}

/// Imgdc 0x3f1d (completion 0x4e).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RawFrameRequest {
    pub subject: SubjectGeometry,
    pub area: CaptureArea,
    pub image_buffer: u32,
    pub raw_word: u32,
}

/// Imgdc 0x3f1e (completion 0x4f).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameRequest {
    pub subject: SubjectGeometry,
    pub kind: u8,
    pub area: CaptureArea,
    pub image_buffer: u32,
    pub sampling: u32,
    pub mode: u8,
}

pub const SCAN_AREA_BYTES: usize = 36;
pub const SCAN_WINDOWS_BYTES: usize = 48;

/// The scan window the lens block's mode selects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScanWindow {
    #[default]
    None,
    Area([u8; SCAN_AREA_BYTES]),
    Centred { centre: [u32; 2], first: [u32; 2], second: [u32; 2], held: bool },
}

impl Default for ScanRequest {
    fn default() -> Self {
        Self {
            start: 0,
            positions: [0; 3],
            span: 0,
            selection_word: 0,
            index_base: 0,
            weighting: 0,
            tolerance_kind: 0,
            window: ScanWindow::None,
            selection_setting: 0,
            windows: [0; SCAN_WINDOWS_BYTES],
            correction: 0,
        }
    }
}

/// Imgdc 0x3f1f (completions 0x50 and 0x51).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanRequest {
    /// 1 starts the scan from the capture's channel samples.
    pub start: u32,
    pub positions: [u16; 3],
    pub span: u16,
    pub selection_word: u32,
    pub index_base: u16,
    /// The second byte scales the statistics weights.
    pub weighting: u32,
    pub tolerance_kind: u32,
    pub window: ScanWindow,
    pub selection_setting: u32,
    pub windows: [u8; SCAN_WINDOWS_BYTES],
    pub correction: u16,
}

/// Imgdc 0x3f20.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionRequest(pub u32);

pub const TONE_LEVELS: usize = 17;

/// Calc 0x3c04 (completion 0x92).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToneRequest {
    pub levels: [i16; TONE_LEVELS],
    pub offset: u16,
    pub window: [u32; 6],
}

/// Calc 0x3c05 (completion 0x93).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GradationRequest {
    /// The scan block's merged tone result.
    pub tone: ToneCalculation,
    pub gated: bool,
    pub sampled: bool,
    pub exposure: u16,
}

/// Statistics 0x03 (completion 0x7a): the five blocks the request names.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsSetup {
    pub mode: u32,
    pub weighted: bool,
    pub selection: [u32; 3],
    pub area: [u8; 16],
    pub levels: [u32; 4],
    pub sampling: StatisticsSampling,
    pub capture: StatisticsCapture,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsSampling {
    pub capture: [u32; 7],
    pub lens: [u32; 2],
    pub halves: [u16; 4],
    pub sample: u32,
    pub flags: [u8; 3],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsCapture {
    pub words: [u32; 4],
    pub flags: [u8; 5],
    pub layout: u16,
}

pub const STATISTICS_WEIGHTS: usize = 14;

/// The statistics weight table (0x1042e9c) of a weighted statistics request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsWeights {
    pub weights: [u8; STATISTICS_WEIGHTS],
    pub mode: u8,
    pub extent: [u16; 2],
    pub origin: [u16; 2],
    pub size: [u16; 2],
}
