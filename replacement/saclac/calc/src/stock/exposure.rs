//! The shot's eight 20-byte evaluation areas and the adjustment context's flag readings.

use xm1_saclac_interface::exposure::STATISTICS_AREAS_BYTES;
use xm1_saclac_profile_records::MeasurementSelection;

use crate::exposure::context::{EvaluationArea, EVALUATION_AREAS};
use crate::adjust::exposure_limits::{Selection, NO_INDEX};
use crate::adjust::profile_mode::{FlagInputs, Positions, Readiness};
use crate::exposure::flags::AdjustmentFlagInput;

pub fn evaluation_areas(areas: &[EvaluationArea; EVALUATION_AREAS]) -> [u8; STATISTICS_AREAS_BYTES] {
    let mut bytes = [0; STATISTICS_AREAS_BYTES];
    for (area, record) in areas.iter().zip(bytes.chunks_exact_mut(20)) {
        record[0..4].copy_from_slice(&area.profile.to_le_bytes());
        record[4..8].copy_from_slice(&area.next_profile.to_le_bytes());
        record[8..12].copy_from_slice(&area.lens_step.to_le_bytes());
        record[12..14].copy_from_slice(&area.converted.to_le_bytes());
        record[14..16].copy_from_slice(&area.level.to_le_bytes());
        record[16..20].copy_from_slice(&u32::from(area.extended).to_le_bytes());
    }
    bytes
}

pub const ADJUSTMENT_FLAG_BYTES: usize = 0x160;

/// The context 0xf740e8 reads; the capture profile and calibrated minimum are read by the policy.
pub fn adjustment_flags(bytes: &[u8; ADJUSTMENT_FLAG_BYTES]) -> AdjustmentFlagInput {
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let half = |at: usize| i16::from_le_bytes([bytes[at], bytes[at + 1]]);
    AdjustmentFlagInput {
        flags: FlagInputs {
            measurement_selection: match word(0xd8) {
                0 => MeasurementSelection::Unselected,
                1 => MeasurementSelection::Record,
                other => MeasurementSelection::Other(other as u8),
            },
            scene: word(0xac),
            flags: half(0xb0) as u16,
            auxiliary: word(0xb8) as u8,
            class: word(0xbc),
            style: word(0xb4) as u8,
            alternate: word(0x10c) != 0,
            variant: word(0x138) != 0,
            armed: word(0x130) != 0,
            choice: word(0x15c),
            threshold: Selection::threshold_of(half(0xf0)),
            profile: word(0xec),
            level: half(0xea),
            reference: word(0xc4),
            policy: word(0x104),
            index: NO_INDEX,
            unmeasured: word(0xf8) == 1,
            tracking: word(0x154) != 0,
            readiness: Readiness::Ask,
            positions: Positions::default(),
            ..FlagInputs::default()
        },
        target: half(0xcc),
        reference: half(0xfc),
        step: word(0xf4),
    }
}
