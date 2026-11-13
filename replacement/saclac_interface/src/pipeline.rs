#![forbid(unsafe_code)]

//! What the half-press pipeline's peers deliver in the completions 0x4e, 0x50, 0x51, 0x91, 0x92
//! and 0x93.

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
