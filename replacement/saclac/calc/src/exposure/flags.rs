//! Selection spread and per-area limit flags, including the unmeasured level rebasing
//! (0xf692dc, 0xf69418, 0xf69514), and the adjustment path's level and flags (0xf740e8).

use xm1_saclac_interface::exposure::CalculationInput;
use xm1_saclac_profile_records::MeasurementSelection;

use crate::adjust::configuration::MotionSource;
use crate::adjust::exposure_limits::LimitPorts;
use crate::adjust::profile_mode::{self, DeviceReadiness, FlagInputs, FlagStyle, LimitFlags, LimitLevels, Positions, Readiness};
use crate::exposure::context::Levels;
use crate::exposure::ports::{CaptureProfiles, Lens, Properties};
use crate::exposure::settings;
use crate::stock::adjust::motion_enable;

/// Positions and limit controls supplied beside the calculation input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagQuery {
    pub reference: u32,
    pub readiness: Readiness,
    pub positions: Positions,
    pub index: u32,
    pub limit_offset: i16,
}

fn inputs(ports: &mut (impl Properties + CaptureProfiles), input: &CalculationInput, query: &FlagQuery) -> FlagInputs {
    let tracking = ports.property(motion_enable(MotionSource::Live)) & 3 != 0 && input.interval_tracking != 0;
    FlagInputs {
        measurement_selection: match input.measurement_selection {
            0 => MeasurementSelection::Unselected,
            1 => MeasurementSelection::Record,
            other => MeasurementSelection::Other(other as u8),
        },
        scene: input.selection_kind,
        flags: input.selection_flags,
        auxiliary: input.selection_auxiliary as u8,
        style: input.selection_style as u8,
        class: input.record_32,
        alternate: input.record_4b != 0,
        variant: input.record_66 != 0,
        armed: input.header_63 != 0,
        choice: input.record_3e,
        policy: input.record_45,
        threshold: crate::adjust::exposure_limits::Selection::threshold_of(input.record_20),
        unmeasured: false,
        limit_offset: query.limit_offset,
        index: query.index,
        level: input.lead,
        profile: input.source,
        capture_profile: ports.record_profile(),
        reference: query.reference,
        tracking,
        minimum: input.record_24,
        readiness: query.readiness,
        positions: query.positions,
    }
}

fn rebase(ports: &mut impl Properties, inputs: &mut FlagInputs, levels: &Levels, per_area: bool) {
    inputs.unmeasured = levels.unmeasured;
    if levels.unmeasured && inputs.limit_offset == 0 && (per_area || !matches!(inputs.scene, 45 | 55)) {
        inputs.positions.position = inputs.positions.position.wrapping_sub(levels.start).wrapping_add(settings::level_step(ports, 3));
        inputs.limit_offset = settings::level_step(ports, 3).wrapping_sub(levels.start);
    }
}

/// Whether the plain selection exceeds its spread margin, and the levels it was compared with.
pub fn selection_spread(ports: &mut (impl LimitPorts + Lens + CaptureProfiles), input: &CalculationInput, levels: &Levels, query: &FlagQuery) -> (bool, LimitLevels) {
    let mut inputs = inputs(ports, input, query);
    inputs.readiness = Readiness::Ready;
    rebase(ports, &mut inputs, levels, false);
    let (levels, flags) = profile_mode::limit_levels(ports, &inputs, FlagStyle::Plain);
    (flags.contains(LimitFlags::SPREAD), levels)
}

/// The caller's other row fields are preserved for absent areas and copied with a shared result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaFlagRow<T> {
    pub flags: LimitFlags,
    pub retained: T,
}

/// A class-5 input with a presence list computes each present row separately. Other inputs
/// compute the first row once and copy its complete result to the remaining rows.
pub fn area_flags<T: Copy>(
    ports: &mut (impl LimitPorts + Lens + CaptureProfiles + DeviceReadiness),
    input: &CalculationInput,
    levels: &Levels,
    query: &FlagQuery,
    present: Option<&[bool]>,
    rows: &mut [AreaFlagRow<T>],
) {
    let per_area = present.filter(|_| input.record_32 == 5);
    if let Some(present) = per_area {
        for (row, &present) in rows.iter_mut().zip(present) {
            if !present {
                row.flags = LimitFlags::default();
                continue;
            }
            let mut inputs = inputs(ports, input, query);
            rebase(ports, &mut inputs, levels, true);
            row.flags = profile_mode::limit_flags(ports, &inputs);
        }
    } else if let Some((first, rest)) = rows.split_first_mut() {
        let mut inputs = inputs(ports, input, query);
        rebase(ports, &mut inputs, levels, false);
        first.flags = profile_mode::limit_flags(ports, &inputs);
        rest.fill(*first);
    }
}

/// Adjustment context readings besides the input level. `stock::exposure` decodes them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdjustmentFlagInput {
    pub flags: FlagInputs,
    pub target: i16,
    pub reference: i16,
    pub step: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdjustedFlags {
    pub level: i16,
    /// The change from the context's target level.
    pub change: i16,
    pub flags: LimitFlags,
}

/// The adjusted level returned to the caller, its relative change and the flags it takes.
pub fn adjusted_flags(ports: &mut (impl LimitPorts + Lens + CaptureProfiles + DeviceReadiness), input: &AdjustmentFlagInput, level: i16) -> AdjustedFlags {
    let step = match input.step { 0..=10 => input.step as u8, _ => 3 };
    let level = input.target.wrapping_sub(input.reference).wrapping_add(level).wrapping_sub(settings::level_step(ports, step));
    let mut flags = input.flags;
    flags.capture_profile = ports.record_profile();
    flags.minimum = settings::level_step(ports, 3);
    flags.positions.position = level;
    AdjustedFlags { level, change: level.wrapping_sub(input.target), flags: profile_mode::limit_flags(ports, &flags) }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionLevels {
    pub selected: i16,
    pub reference: i16,
    pub baseline: i16,
}

/// The area flags and the three levels the shot keeps beside them (0xf60048). In the
/// per-area path the last present row chooses the selected level, but the first row's flags
/// decide whether the reference and baseline have the record's offsets removed (0xf5f550).
pub fn evaluate_selection<T: Copy>(
    ports: &mut (impl LimitPorts + Lens + CaptureProfiles + DeviceReadiness),
    input: &CalculationInput,
    levels: &Levels,
    query: &FlagQuery,
    preferred: i16,
    present: Option<&[bool]>,
    rows: &mut [AreaFlagRow<T>],
) -> SelectionLevels {
    area_flags(ports, input, levels, query, present, rows);
    let first = rows.first().map_or(LimitFlags::default(), |row| row.flags);
    let last = if input.record_32 == 5 {
        present.and_then(|present| rows.iter().zip(present).rev().find(|(_, present)| **present).map(|(row, _)| row.flags)).unwrap_or(first)
    } else {
        first
    };
    let selected = if last.0 & (LimitFlags::AT_OR_ABOVE | LimitFlags::SPREAD) != 0 { preferred } else { query.positions.position };
    let mut result = SelectionLevels { selected, reference: selected, baseline: query.positions.position };
    if input.selection_style > 1 || !first.contains(LimitFlags::BELOW) {
        result.reference = result.reference.wrapping_sub(input.record_20).wrapping_sub(input.record_level);
        result.baseline = result.baseline.wrapping_sub(input.record_20).wrapping_sub(input.record_level);
    }
    result
}
