#![forbid(unsafe_code)]

//! X-M1 1.50 layouts the exposure conclusion 0xf1f4ec reads from the calc 0x3c01 correction and
//! writes into the selection plan (shot context +0x1cec) and the record's exposure table.

use crate::boundary::{ExposureSlot, SelectionPlan};
use xm1_saclac_interface::exposure::Correction;

/// Correction halfwords stored into record slots 0..=12 and 14..=19 (setters 0x750eb0 + 0x98 *
/// slot), in the order 0xf1f760..0xf1f970 stores them.
const SLOT_SOURCES: [(u8, usize); 19] = [
    (0, 3), (1, 12), (2, 6), (3, 7), (4, 6), (5, 9), (6, 8), (7, 11), (8, 5), (9, 13), (10, 14), (11, 15), (12, 4), (14, 30), (15, 31),
    (16, 32), (17, 33), (18, 16), (19, 17),
];
const SELECTED_LEVEL: usize = 14;
const PRIMARY_LEVEL: usize = 34;
const SECONDARY_LEVEL: usize = 35;
const ALTERNATE_LEVEL: usize = 36;
const THIRD_OFFSET: usize = 37;
const AREAS: usize = 38;
const PLAN_VALUE: usize = 48;
const PLAN_HALVES: [usize; 5] = [53, 58, 59, 60, 61];

pub fn corrected_slots(correction: &Correction) -> [(ExposureSlot, u16); SLOT_SOURCES.len()] {
    SLOT_SOURCES.map(|(slot, half)| (ExposureSlot(slot), correction.half(half)))
}

/// 0xf1f730..0xf1f748 overwrites the slot-10 halfword with the secondary level when the
/// calculation's variant word is nonzero, else with the primary level.
pub fn select_corrected_level(correction: &mut Correction, variant: bool) {
    correction.0[SELECTED_LEVEL] = correction.half(if variant { SECONDARY_LEVEL } else { PRIMARY_LEVEL });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectionOffsets {
    pub first: u16,
    pub alternate: u16,
    pub third: i16,
}

pub fn correction_offsets(correction: &Correction) -> CorrectionOffsets {
    CorrectionOffsets { first: correction.half(PRIMARY_LEVEL), alternate: correction.half(ALTERNATE_LEVEL), third: correction.half(THIRD_OFFSET) as i16 }
}

pub fn correction_areas(correction: &Correction) -> [u16; 10] { core::array::from_fn(|index| correction.half(AREAS + index)) }

const PLAN_KIND: usize = 0;
const PLAN_COUNT: usize = 4;
const PLAN_AREA_ARRAYS: [usize; 4] = [0x8, 0x5c, 0x68, 0x74];
const PLAN_TAIL: usize = 0x20c;

/// 0xf1f608..0xf1f6cc: a plan whose first word is 0 takes the correction's plan value for each of
/// its counted areas in four arrays; every plan takes five correction halfwords at +0x20c. Stock
/// writes past the plan for counts above 104; owned code stops at its end.
pub fn plan_take_exposure(plan: &mut SelectionPlan, correction: &Correction) {
    let bytes = &mut plan.0;
    let kind = u32::from_le_bytes([bytes[PLAN_KIND], bytes[PLAN_KIND + 1], bytes[PLAN_KIND + 2], bytes[PLAN_KIND + 3]]);
    let count = usize::from(bytes[PLAN_COUNT]);
    if kind == 0 {
        let value = correction.word(PLAN_VALUE).to_le_bytes();
        for index in 0..count {
            for base in PLAN_AREA_ARRAYS {
                if let Some(slot) = bytes.get_mut(base + 4 * index..base + 4 * index + 4) { slot.copy_from_slice(&value); }
            }
        }
    }
    for (index, half) in PLAN_HALVES.into_iter().enumerate() {
        let offset = PLAN_TAIL + 2 * index;
        bytes[offset..offset + 2].copy_from_slice(&correction.half(half).to_le_bytes());
    }
}
