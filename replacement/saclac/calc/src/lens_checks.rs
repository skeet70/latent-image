//! Lens checks the shot asks the library for.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::shared::{group_14, group_26, group_27};

use crate::adjust::motion::ports::{LensCalibration, LensTables, LimitQuery};
use crate::adjust::timing::LevelScale;
use crate::exposure::ports::{Lens, ProfileAccess, Properties};
use crate::scene_recognition::frame;
use crate::scene_recognition::ports::LensServices;
use crate::stock::lens_checks as stock;
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

/// A lens position and the side it was taken on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensPoint {
    pub position: i16,
    pub side: u32,
}

/// Where a lens point lies on the span from the lens's target to its far table position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointZone {
    BeyondFar,
    /// Beyond fifteen sixteenths of the span.
    BeyondSpan,
    /// Beyond the sixteenth of the span nearest the lens's response.
    BeyondResponse,
    Within,
}

impl PointZone {
    pub const fn code(self) -> u32 {
        match self {
            Self::BeyondFar => 1,
            Self::BeyondSpan => 2,
            Self::BeyondResponse => 3,
            Self::Within => 4,
        }
    }
}

/// The side 0xf4e620 moves a point's position to before placing it.
const PLACED_SIDE: u32 = 1;

/// The zone of `point` for `lens` at `step`, or the configured zone while the override is set
/// (0xf4e620).
pub fn point_zone(ports: &mut (impl Properties + LensTables + LensServices), point: &LensPoint, lens: u32, step: u32) -> PointZone {
    if ports.property(stock::ZONE_OVERRIDE) == 1 {
        return match ports.property(stock::OVERRIDDEN_ZONE) {
            0 => PointZone::Within,
            1 => PointZone::BeyondSpan,
            _ => PointZone::BeyondFar,
        };
    }
    let target = frame::target(ports, lens, step);
    let reference = ports.lens_reference();
    let far = ports.channel_positions(lens, step, reference, 0)[1];
    let span = i32::from(far) - i32::from(target);
    let sixteenths = |count: i32| count.wrapping_mul(span) / 16;
    let response = frame::response(ports, lens, step);
    let distance = |count: i32| sixteenths(count).wrapping_sub(response).wrapping_abs() as u16;
    let nearest = (1..16).fold(0, |nearest, count| if distance(count) < distance(nearest) { count } else { nearest });
    let at = |count: i32| target.wrapping_add(sixteenths(count) as i16);
    let position = ports.side_adjusted(point.position, point.side, PLACED_SIDE);
    if position > far {
        PointZone::BeyondFar
    } else if at(15) < position {
        PointZone::BeyondSpan
    } else if at(nearest) < position {
        PointZone::BeyondResponse
    } else {
        PointZone::Within
    }
}
