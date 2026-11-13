#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the record fields the search reads and writes.

use crate::contrast::settings::{Setting, TrendBasis};
use crate::contrast::{FocusStatus, RetainedFocus, StageRecord};
use xm1_saclac_controllers::search::LensPosition;

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn half(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// Shared +0x84, 32 bytes; both writers (0xf3c17c, 0xf40300) store 2 at +0x14.
pub fn focus_status(status: FocusStatus) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[0..4].copy_from_slice(&status.active.to_le_bytes());
    bytes[4..8].copy_from_slice(&status.mode.to_le_bytes());
    bytes[8..10].copy_from_slice(&status.position.0.to_le_bytes());
    bytes[12..16].copy_from_slice(&status.snapshot[0].to_le_bytes());
    bytes[16..20].copy_from_slice(&status.snapshot[1].to_le_bytes());
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&status.flag.to_le_bytes());
    bytes[28..32].copy_from_slice(&status.stage.to_le_bytes());
    bytes
}

/// Shared +0x74c, 24 bytes (0xf3c394..0xf3c3ac).
pub fn stage_record(stage: StageRecord) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[0..4].copy_from_slice(&stage.stage.to_le_bytes());
    bytes[4] = stage.first_pass;
    bytes[8..12].copy_from_slice(&stage.words[0].to_le_bytes());
    bytes[12..14].copy_from_slice(&stage.halves[0].to_le_bytes());
    bytes[16..20].copy_from_slice(&stage.words[1].to_le_bytes());
    bytes[20..22].copy_from_slice(&stage.halves[1].to_le_bytes());
    bytes[22] = stage.last;
    bytes
}

/// Shared +0x120, 28 bytes as 0xf3e6f0 reads them.
pub fn retained_focus(bytes: &[u8; 28]) -> RetainedFocus {
    RetainedFocus {
        valid: word(bytes, 0),
        spread: word(bytes, 4),
        position: LensPosition(half(bytes, 8) as i16),
        profile: word(bytes, 16),
    }
}

/// How an adjustment is read: 0x69f8fc for a byte, 0x69fec8 for a halfword.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjustmentWidth {
    Byte,
    Half,
}

/// The adjustment key behind each setting.
pub const fn setting_key(setting: Setting) -> (u32, AdjustmentWidth) {
    use AdjustmentWidth::{Byte, Half};
    let key = match setting {
        Setting::PairedEvaluation => 0x8778,
        Setting::LevelThousands => 0x8782,
        Setting::LevelHundreds => 0x8781,
        Setting::LevelTens => 0x8780,
        Setting::LevelUnits => 0x877f,
        Setting::DirectionSecondChannel => 0x8805,
        Setting::SecondChannel => 0x8804,
        Setting::StageReport => 0x94ea,
        Setting::ScanDriveDisabled => 0x87fd,
        Setting::SlowBridge => 0x3e_0246,
        Setting::SlowBridgeAlternate => 0x878b,
        Setting::HoldDrives => 0x8801,
        Setting::ReturnStepLimit => 0x8838,
        Setting::ModelLetter => 0x3e_0002,
        Setting::ModelScaling => 0x3e_2713,
        Setting::DirectionSettle => 0x8775,
        Setting::DirectionSettleScale => 0x8783,
        Setting::DirectionScale { band, model_scaled: true } => 0x8754 - band as u32,
        Setting::DirectionScale { band, model_scaled: false } => 0x877d - band as u32,
        Setting::DirectionStepDivisor => 0x8776,
        Setting::PositionJump => 0x94e1,
        Setting::FlagRestart => 0x8877,
        Setting::SelectedFlagRestart => 0x876d,
        Setting::DirectionSamples => 0x87d3,
        Setting::MotionReturn => 0x8842,
        Setting::MotionReturns => 0x8802,
        Setting::TrendDirection => 0x8818,
        Setting::ProbeAtOnce => 0x880f,
        Setting::ProbeSamples => 0x8816,
        Setting::ProbeContrast => 0x8814,
        Setting::ProbeDirection => 0x880e,
        Setting::TrendSamples => 0x8811,
        Setting::TrendLimit { selected: true, basis: TrendBasis::Settling } => 0x8774,
        Setting::TrendLimit { selected: true, basis: TrendBasis::StepClass } => 0x8771,
        Setting::TrendLimit { selected: true, basis: TrendBasis::Plain } => 0x8813,
        Setting::TrendLimit { selected: false, basis: TrendBasis::Settling } => 0x8773,
        Setting::TrendLimit { selected: false, basis: TrendBasis::StepClass } => 0x8770,
        Setting::TrendLimit { selected: false, basis: TrendBasis::Plain } => 0x8812,
        Setting::HandOffLevel => 0x877e,
        Setting::StillSamples => 0x881e,
        Setting::StillChange => return (0x881a, Half),
        Setting::TrendOverride => 0x8819,
        Setting::OverrideSamples => 0x8810,
        Setting::PairedMargin => 0x8788,
        Setting::EndSteps => 0x8835,
        Setting::ReversalSteps => 0x878c,
        Setting::ReversalStep => return (0x3e_0240, Half),
        Setting::FocusCorrection => 0x8b11,
        Setting::SelectedLevelSamples => 0x876b,
        Setting::PinnedAmount => 0xa4e0,
        Setting::DebugDisplay => 0x8791,
        Setting::FocusLog => 0x24c,
        Setting::PeakReturns => 0x883d,
        Setting::FocusDriveKeeps => 0x87ee,
        Setting::FocusDriveTurn => 0x883e,
        Setting::RecentPeak => 0x8803,
        Setting::RecentPeakSamples => 0x944c,
        Setting::RecentPeakLevel => return (0x944f, Half),
        Setting::PeakBothChannels => 0x8821,
        Setting::LevelJump => 0x8839,
        Setting::LevelJumpTens => 0x883a,
        Setting::EndStep => 0x8837,
        Setting::HoldMargin => 0x8807,
        Setting::LevelDrop => 0x8787,
        Setting::RiseHigh => 0x8829,
        Setting::RiseLow => 0x8822,
        Setting::RiseFrom => 0x882b,
        Setting::RiseTo => 0x882c,
        Setting::FallHigh => 0x882a,
        Setting::FallLow => 0x8826,
        Setting::FallFrom => 0x882d,
        Setting::FallTo => 0x882e,
        Setting::FallLevel => 0x882f,
        Setting::ScanTurns => 0x878f,
        Setting::ScanStalls => 0x878e,
        Setting::FitPeak => 0x87ff,
        Setting::TrackMarks => 0x944b,
        Setting::FocusDisplay => 0x8b10,
        Setting::DebugRate => 0x8792,
        Setting::DebugRowsFirst => 0x8793,
        Setting::DebugRowsSecond => 0x8794,
        Setting::DebugRowsThird => 0x8795,
        Setting::FocusChangePercent => 0x886a,
        Setting::FocusChangeTens => 0x8868,
        Setting::FocusLowTens => 0x8866,
        Setting::FocusLevelTens => 0x8860,
        Setting::MotionCheck => 0x87f6,
        Setting::SelectedMotionCheck => 0x876e,
        Setting::MotionSamples => 0x87f8,
        Setting::MotionScale => 0x87f9,
        Setting::MotionFloor => 0x87fa,
        Setting::MotionCeiling => 0x87fb,
        Setting::MotionHolds => 0x87f7,
    };
    (key, Byte)
}
