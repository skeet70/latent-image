//! What the shot's selection grid reads from the engine: the lens step a cell's targets follow
//! (0xf69884), a tracked kind's target level (0xf699e4) and how far an area lies from the metered
//! and lens levels (0xf69788).

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::exposure_limits::{self, LimitQuery, Request, Selection, NO_INDEX};
use crate::exposure::context::ExposureContext;
use crate::exposure::ports::{Ports, Properties};
use crate::exposure::settings;
use crate::stock::calculation::Cell;
use crate::stock::property;

/// The near lens step of the record's limits for `cell` at `lens_reference`, without a bank or an
/// indexed limit.
pub fn target_reference(ports: &mut impl Ports, context: &ExposureContext, record: RecordHandle, cell: Cell, lens_reference: u32) -> u32 {
    let refinement_pass = context.binding.refinement_pass;
    let mut profile = ports.profile();
    let request = Request {
        selection: Selection {
            scene: profile.selection_kind(record, refinement_pass),
            flags: profile.selection_flags(record, refinement_pass),
            alternate: profile.read::<u8, _>(record, &group_00::AT_004B) != 0,
            variant: profile.read::<u8, _>(record, &group_00::AT_0066) != 0,
            unmeasured: context.levels.unmeasured,
            threshold: Selection::threshold_of(profile.read(record, &group_00::AT_0020)),
            auxiliary: profile.selection_auxiliary(record, refinement_pass) != 0,
        },
        class: u32::from(profile.read::<u8, _>(record, &group_00::AT_0032)),
        reference: lens_reference,
        policy: u32::from(profile.read::<u8, _>(record, &group_00::AT_0045)),
        armed: profile.header(record, SharedHeaderByte::At63) != 0,
        choice: u32::from(profile.read::<u8, _>(record, &group_00::AT_003E)),
        tracking: profile.read(record, &group_00::INTERVAL_TRACKING).0 != 0,
        bank: 0,
        cell,
        index: NO_INDEX,
        level: profile.read(record, &group_00::AT_001E),
        profile: u32::from(profile.read::<u8, _>(record, &group_00::AT_0044)),
        fixed_upper: false,
        capture_profile: context.levels.curve,
        minimum: profile.read(record, &group_00::AT_0024),
    };
    exposure_limits::limits(ports, &request).near_step
}

/// What a tracked kind's target level is read for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LevelQuery {
    /// The selection kind and flags (0xf612dc, 0xf61584).
    pub scene: u32,
    pub flags: u16,
    /// Record bytes +0x4b and +0x66.
    pub alternate: bool,
    pub variant: bool,
    pub limit: LimitQuery,
}

/// The clamped mode limit of an unmeasured selection without a threshold.
pub fn target_level(ports: &mut impl Properties, query: &LevelQuery) -> i16 {
    let selection = Selection { scene: query.scene, flags: query.flags, alternate: query.alternate, variant: query.variant, unmeasured: true, threshold: None, auxiliary: false };
    exposure_limits::clamped_limit(ports, &selection, &query.limit)
}

/// The halves of a calc 0x3c02 result area the distances read, named by offset.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaReading {
    pub at00: i16,
    /// The area's lens reference (+4).
    pub reference: u32,
    /// +0xa.
    pub level: i16,
    pub at44: i16,
    pub at48: i16,
}

/// Which limit an area's distances pass, the metered one first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistanceClass {
    Within,
    BeyondMetered,
    BeyondLens,
}

impl DistanceClass {
    /// The word the shot's row keeps.
    pub const fn code(self) -> u32 {
        match self {
            Self::Within => 1,
            Self::BeyondMetered => 2,
            Self::BeyondLens => 3,
        }
    }
}

/// An area's distances from the metered level and from its lens level, with the configured
/// limits in tenths. The default is what a row the shot has not flagged keeps: no class and zeros.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaDistances {
    pub class: Option<DistanceClass>,
    pub metered: i16,
    pub lens: i16,
    pub metered_limit: i16,
    pub lens_limit: i16,
}

impl AreaDistances {
    pub const fn code(&self) -> u32 {
        match self.class {
            Some(class) => class.code(),
            None => 0,
        }
    }
}

/// The distances of `area` at the active calculation input's lens reference.
pub fn area_distances(ports: &mut impl Ports, context: &ExposureContext, lens_reference: u32, area: &AreaReading) -> AreaDistances {
    let metered = context
        .metered_level
        .wrapping_sub(area.at44)
        .wrapping_add(settings::level_step(ports, 3))
        .wrapping_add(ports.reference_offset(lens_reference));
    let lens = area.at48.wrapping_add(area.at00).wrapping_sub(area.level).wrapping_sub(ports.level(area.reference, lens_reference));
    let metered_limit = settings::tenths(ports, property::METERED_DISTANCE_LIMIT) as i16;
    let lens_limit = settings::signed_tenths(ports, property::LENS_DISTANCE_LIMIT);
    let class = if metered > metered_limit {
        DistanceClass::BeyondMetered
    } else if lens > lens_limit {
        DistanceClass::BeyondLens
    } else {
        DistanceClass::Within
    };
    AreaDistances { class: Some(class), metered, lens, metered_limit, lens_limit }
}
