//! Lens checks the shot asks the library for.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::shared::{group_14, group_26, group_27};

use crate::adjust::motion::ports::{LensCalibration, LimitQuery};
use crate::adjust::timing::LevelScale;
use crate::exposure::ports::{Lens, ProfileAccess};
use crate::stock::record;

/// Image 4's lens control task.
pub trait LensControl {
    /// The reference the task holds (0x76d9b8).
    fn held_reference(&mut self) -> u32;
}

/// Whether `lens`'s high limit at `step`, with the calibration margins, lies at or below `limit`
/// (0xf4d2dc).
pub fn high_limit_within(calibration: &mut impl LensCalibration, lens: u32, step: u32, reference: u32, limit: i32) -> bool {
    let query = LimitQuery { channel: 0, lens, step, reference, with_margins: true, lens_state: 0 };
    i32::from(calibration.lens_limits(&query).high) <= limit
}

/// What `level_rise` gives when the record's level has not risen.
pub const NO_RISE: u32 = 100;

/// How far the record's current level lies above the level of its focus result, as the linear
/// percentage of the difference (0xf4d604).
pub fn level_rise(ports: &mut (impl ProfileAccess + Lens + LensControl + LevelScale), record: RecordHandle) -> u32 {
    let focus = record::focus_result(&ports.profile().read_shared(record, &group_14::FOCUS_RESULT));
    if focus.valid == 0 {
        return NO_RISE;
    }
    let [first, second, third, fourth, _, sixth] = focus.terms;
    let focus_lens = ports.level(focus.lens, focus.reference) as u16;
    let focused = first.wrapping_sub(sixth.wrapping_add(second)).wrapping_sub(third).wrapping_sub(fourth).wrapping_add(focus_lens);
    let (lowered, offset, lens, deductions) = {
        let profile = ports.profile();
        let deductions = [&group_26::AT_0208, &group_26::AT_020C, &group_26::AT_0216].map(|field| profile.read_shared(record, field) as u16);
        (profile.read_shared(record, &group_27::AT_072C) != 0, profile.read_shared(record, &group_26::AT_0206) as u16, profile.read_shared(record, &group_26::AT_0200), deductions)
    };
    let focused = if lowered { focused.wrapping_sub(100) } else { focused };
    let reference = ports.held_reference();
    let current = offset.wrapping_add(ports.level(lens, reference) as u16);
    let current = deductions.into_iter().fold(current, u16::wrapping_sub);
    if focused as i16 >= current as i16 {
        return NO_RISE;
    }
    ports.linear(current.wrapping_sub(focused) as i16)
}
