//! The exposure program (0xf64530): the level range an exposure may take, its division into at
//! most eight steps with the capture profile and lens step of each, and the first step the
//! evaluation spans.

use xm1_saclac_profile_records::stock::record::{group_00, group_13};
use xm1_saclac_profile_records::stock::shared::group_27;
use xm1_saclac_profile_records::{IntervalTracking, MeasurementSelection, SharedHeaderByte, WindowStatus};

use crate::adjust::result_dispatch::{self, ResultKind};
use crate::adjust::scene_settings::{self, RangeInputs, SceneRange, SceneSelection};
use crate::exposure::context::{CandidateSpan, ExposureContext, ExposureStep, StartBound, PROGRAM_STEPS};
use crate::exposure::ports::Ports;
use crate::exposure::settings;
use crate::stock::tables::PROGRAM_PROFILES;
use crate::stock::{property, shared};

/// What the unselected measurement's range reads (0xf72c3c).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrimaryInputs {
    pub kind: u32,
    pub flags: u16,
    pub lens_reference: u32,
    pub unmeasured: bool,
    pub policy: u32,
    pub focus_class: u8,
    pub variant: u8,
    pub armed: u8,
    pub record_3e: u8,
    pub interval_tracking: IntervalTracking,
    pub record_20: i16,
    pub record_44: u8,
    pub curve: u32,
    pub step_three: i16,
    pub auxiliary: u8,
}

/// The unselected measurement's range: its upper and lower levels, the lens steps it spans and
/// the floor it takes when nothing was measured.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrimaryRange {
    pub upper: i16,
    pub lower: i16,
    pub far_lens: u32,
    pub near_lens: u32,
    pub floor: i16,
}

/// What a selected measurement's program reads (0xf77c8c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedInputs {
    pub measurement_selection: MeasurementSelection,
    pub second_result: ResultKind,
    pub window_status: WindowStatus,
    pub kind: u32,
    pub record_20b0: u32,
}

/// The adjustment layer's ranges the program is built from.
pub trait Limits {
    /// 0xf72c3c.
    fn primary_range(&mut self, inputs: &PrimaryInputs) -> PrimaryRange;
    /// 0xf77c8c.
    fn selected_program(&mut self, inputs: &SelectedInputs) -> u32;
    /// The near and far lens steps of a selected program (0xf7809c).
    fn selected_lenses(&mut self, program: u32, lens_reference: u32) -> (u32, u32);
    /// 0xf78d24 with its second argument 0.
    fn selected_level(&mut self, program: u32, lens_reference: u32) -> i16;
    /// 0xf79140.
    fn selected_offset(&mut self, program: u32) -> i16;
}

/// How the first step was chosen; the evaluation accepts any range only after `Curve`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstChoice {
    /// No valid snapshot: the first step.
    Unmeasured,
    /// From the snapshot's offset against the focus curve.
    Curve,
    /// The configured step.
    Configured,
}

/// The highest program profile whose sensitivity class is at most `sensitivity`, else the first
/// (0xf643a4).
fn profile_at_sensitivity(ports: &mut impl Ports, sensitivity: u8) -> u32 {
    PROGRAM_PROFILES.iter().rev().copied().find(|profile| ports.sensitivity_class(*profile) <= sensitivity).unwrap_or(PROGRAM_PROFILES[0])
}

/// The profile at the sensitivity of the highest program profile, past the first, whose level is
/// at most `level` (0xf64410).
fn profile_at_or_below(ports: &mut impl Ports, level: i16) -> u32 {
    let at = (1..PROGRAM_PROFILES.len()).rev().find(|index| ports.profile_level(PROGRAM_PROFILES[*index]) <= level).unwrap_or(0);
    let sensitivity = ports.sensitivity_class(PROGRAM_PROFILES[at]);
    profile_at_sensitivity(ports, sensitivity)
}

/// The level of the profile at `sensitivity`, raised to `base` (0xf64480).
fn level_at_sensitivity(ports: &mut impl Ports, sensitivity: u8, base: i16) -> i16 {
    let profile = profile_at_sensitivity(ports, sensitivity);
    ports.profile_level(profile).max(base)
}

/// A step's value relative to the target level (0xf644c4).
pub fn step_value(ports: &mut impl Ports, step: &ExposureStep, lens_reference: u32) -> i16 {
    let lens = ports.level(step.lens_step, lens_reference);
    let offset = ports.level_offset(step.next_profile);
    let value = step.converted.wrapping_sub(step.offset as u16).wrapping_add(lens as u16).wrapping_sub(offset as u16);
    let value = if step.extended { value.wrapping_add(settings::word(ports, property::EXTENDED_OFFSET)) } else { value };
    value as i16
}

struct Builder {
    lens_reference: u32,
    target: i16,
    base: i16,
    level_limit: i16,
    reference_offset: i16,
    end: i16,
    far_lens: u32,
}

impl Builder {
    /// Moves the step's lens toward the far lens while the remaining level pays for it.
    fn move_lens(&self, ports: &mut impl Ports, step: &mut ExposureStep, remaining: i16) -> i16 {
        let threshold = self.target.wrapping_add(self.end).wrapping_sub(500).wrapping_sub(self.level_limit).wrapping_add(self.reference_offset);
        let near = step.lens_step;
        if self.far_lens <= near {
            return remaining;
        }
        let reached = (near..self.far_lens).find(|lens| ports.level(*lens, self.lens_reference) >= threshold).unwrap_or(self.far_lens);
        let near_level = ports.level(near, self.lens_reference);
        for lens in (near + 1..=reached).rev() {
            let gain = ports.level(lens, self.lens_reference).wrapping_sub(near_level);
            if remaining >= gain {
                step.lens_step = lens;
                return remaining.wrapping_sub(gain);
            }
        }
        remaining
    }

    /// Spends `rise` on the step: a higher level, a lens step, a profile of higher sensitivity,
    /// then the carried offset.
    fn advance(&self, ports: &mut impl Ports, step: &mut ExposureStep, rise: i16) {
        let threshold = self
            .level_limit
            .wrapping_add(step.offset)
            .wrapping_sub(step.level)
            .wrapping_add(ports.level_offset(step.next_profile))
            .wrapping_sub(self.reference_offset);
        let mut remaining = rise;
        if rise > threshold {
            let sensitivity = ports.sensitivity_class(step.profile);
            let level = level_at_sensitivity(ports, sensitivity, self.base);
            remaining = step.level.wrapping_add(rise).wrapping_sub(level);
            step.level = level;
            if remaining > 0 {
                remaining = self.move_lens(ports, step, remaining);
            }
            if remaining <= 0 {
                return;
            }
        }
        let capped = (i32::from(remaining) + i32::from(step.level)).min(i32::from(self.level_limit)) as i16;
        let current = ports.sensitivity_class(step.profile);
        let candidate = profile_at_or_below(ports, capped);
        if current < ports.sensitivity_class(candidate) {
            let level = if step.offset > 0 || ports.offset_class(step.next_profile) != 0 {
                let sensitivity = ports.sensitivity_class(candidate);
                level_at_sensitivity(ports, sensitivity, self.base)
            } else {
                capped
            };
            remaining = step.level.wrapping_add(remaining).wrapping_sub(level);
            step.level = level;
            let profile = profile_at_or_below(ports, level);
            step.profile = profile;
            step.next_profile = profile;
            if remaining <= 0 {
                return;
            }
        }
        if step.offset > 0 {
            if step.offset > remaining {
                step.offset = step.offset.wrapping_sub(remaining);
                remaining = 0;
            } else {
                remaining = remaining.wrapping_sub(step.offset);
                step.offset = 0;
            }
        }
        let offset = ports.level_offset(step.next_profile);
        if offset > 0 && remaining > 0 && offset != self.reference_offset {
            let sensitivity = ports.sensitivity_class(step.profile);
            let level = level_at_sensitivity(ports, sensitivity, self.base);
            remaining = remaining.wrapping_add(step.level).wrapping_sub(level).wrapping_sub(offset.wrapping_sub(self.reference_offset));
            step.level = level;
            let profile = profile_at_sensitivity(ports, sensitivity);
            step.profile = profile;
            step.next_profile = profile;
        }
        if remaining > 0 {
            step.level = remaining.wrapping_add(step.level).min(self.level_limit);
        }
    }
}

struct Range {
    low: i16,
    high: i16,
    near_lens: u32,
    far_lens: u32,
}

fn range(ports: &mut impl Ports, limits: &mut impl Limits, context: &ExposureContext, scene: SceneRange) -> Range {
    let binding = &context.binding;
    let record = binding.record_handle();
    let levels = context.levels;
    let refinement_pass = binding.refinement_pass;
    let selection = ports.profile().measurement_selection(record);
    let kind = ports.profile().selection_kind(record, refinement_pass);
    let step_three = settings::level_step(ports, 3);
    if selection == MeasurementSelection::Unselected {
        let inputs = PrimaryInputs {
            kind,
            flags: ports.profile().selection_flags(record, refinement_pass),
            lens_reference: binding.lens_reference,
            unmeasured: levels.unmeasured,
            policy: levels.policy,
            focus_class: ports.profile().read(record, &group_00::AT_004B),
            variant: ports.profile().read(record, &group_00::AT_0066),
            armed: ports.profile().header(record, SharedHeaderByte::At63),
            record_3e: ports.profile().read(record, &group_00::AT_003E),
            interval_tracking: ports.profile().read(record, &group_00::INTERVAL_TRACKING),
            record_20: ports.profile().read(record, &group_00::AT_0020),
            record_44: ports.profile().read(record, &group_00::AT_0044),
            curve: levels.curve,
            step_three,
            auxiliary: ports.profile().selection_auxiliary(record, refinement_pass),
        };
        let primary = limits.primary_range(&inputs);
        let near = ports.base_level(primary.near_lens);
        let far = ports.base_level(primary.far_lens);
        let low = primary.lower.wrapping_sub(scene.below as i16).wrapping_add(near);
        let high = (scene.above as i16).wrapping_add(primary.upper).wrapping_add(far);
        let (low, high) = if levels.unmeasured {
            (low.wrapping_add(500).wrapping_sub(primary.floor).wrapping_sub(step_three), high.wrapping_add(500).wrapping_sub(step_three))
        } else {
            let measured = levels.measured as i16;
            (low.wrapping_add(500).wrapping_sub(measured), high.wrapping_add(500).wrapping_sub(measured))
        };
        Range { low, high: high.max(low), near_lens: primary.near_lens, far_lens: primary.far_lens }
    } else {
        let inputs = SelectedInputs {
            measurement_selection: selection,
            second_result: result_dispatch::second_result(ports, record),
            window_status: ports.profile().read_shared(record, &group_27::WINDOW_STATUS),
            kind,
            record_20b0: ports.profile().read(record, &group_13::AT_20B0_4),
        };
        let program = limits.selected_program(&inputs);
        let (near_lens, far_lens) = limits.selected_lenses(program, binding.lens_reference);
        let offset = limits.selected_offset(program);
        let low = limits.selected_level(program, binding.lens_reference).wrapping_sub(scene.below as i16);
        let far = ports.level(far_lens, binding.lens_reference);
        let high = (scene.above as i16).wrapping_add(far).wrapping_add(settings::signed_word(ports, property::SELECTED_HIGH_OFFSET));
        let low = low.wrapping_sub(step_three.wrapping_add(offset)).wrapping_add(500);
        let high = high.wrapping_add(500).wrapping_sub(step_three).wrapping_sub(offset);
        Range { low, high: high.max(low), near_lens, far_lens }
    }
}

pub struct Program {
    pub span: CandidateSpan,
    pub steps: [ExposureStep; PROGRAM_STEPS],
    pub choice: FirstChoice,
}

pub fn build(ports: &mut impl Ports, limits: &mut impl Limits, context: &ExposureContext) -> Program {
    let record = context.binding.record_handle();
    let refinement_pass = context.binding.refinement_pass;
    let levels = context.levels;
    let inputs = RangeInputs {
        selection: SceneSelection {
            measurement_selection: ports.profile().measurement_selection(record),
            scene: ports.profile().selection_kind(record, refinement_pass),
            flags: ports.profile().selection_flags(record, refinement_pass),
        },
        class: ports.profile().read(record, &group_00::AT_0034),
        unmeasured: levels.unmeasured,
        policy: levels.policy as u8,
        armed: ports.profile().header(record, SharedHeaderByte::At63) != 0,
        variant: ports.profile().read(record, &group_00::AT_0066),
    };
    let scene = scene_settings::range(ports, inputs);
    let program_offset = settings::signed_word(ports, property::PROGRAM_OFFSET);
    let level_limit = settings::signed_word(ports, property::PROGRAM_LEVEL_LIMIT);
    let start_margin = settings::level(ports, property::START_MARGIN).wrapping_mul(10);
    let step_limit = settings::level(ports, property::STEP_LIMIT).wrapping_mul(10);
    let target = levels.target;
    let lens_reference = context.binding.lens_reference;
    let Range { low, high, near_lens, far_lens } = range(ports, limits, context, scene);
    let reference_offset = ports.level_offset(PROGRAM_PROFILES[0]);
    let first_profile = profile_at_or_below(ports, scene.offset);
    let tolerance = settings::tenths(ports, property::UPPER_TOLERANCE) as i16;
    let below_high = |level: i16| if i32::from(level) + i32::from(tolerance) > i32::from(high) { high.wrapping_sub(tolerance) } else { level };

    let mut start = below_high(start_margin.wrapping_add(low));
    let near = ports.level(near_lens, lens_reference);
    let floor = scene.offset.wrapping_add(500).wrapping_sub(program_offset).wrapping_sub(target).wrapping_add(near).wrapping_sub(reference_offset);
    let far = ports.level(far_lens, lens_reference);
    let ceiling = level_limit.wrapping_add(500).wrapping_sub(target).wrapping_add(far).wrapping_sub(reference_offset);
    let mut bound = StartBound::Margin;
    let snapshot = &context.snapshot;
    if snapshot.valid != 0 && snapshot.focus.pinned != 0 {
        let pinned = snapshot.offset.wrapping_add(500).wrapping_sub(settings::tenths(ports, property::PINNED_MARGIN) as i16);
        if start <= pinned {
            start = below_high(pinned);
            bound = StartBound::Pinned;
        }
    }
    let first_step = if floor >= start {
        start = floor;
        bound = StartBound::Floor;
        0
    } else {
        if ceiling <= start {
            start = ceiling;
            bound = StartBound::Margin;
        }
        start.wrapping_sub(floor)
    };
    // A positive program offset keeps the last step for the extended step (0xf64b4c..0xf64b5c).
    let extends = program_offset > 0;
    let reserved = i16::from(extends);
    let room = (i32::from(ceiling) - i32::from(start)).min(i32::from(step_limit.wrapping_mul(7 - reserved))) as i16;
    let headroom = high.wrapping_sub(start).wrapping_sub(tolerance);
    let (end, room, extends) = if room < headroom { (start.wrapping_add(room), room, extends) } else { (high.wrapping_sub(tolerance), headroom, false) };
    let (steps, step) = steps(room, step_limit, PROGRAM_STEPS as i16 - 1 - reserved);

    let builder = Builder { lens_reference, target, base: scene.offset, level_limit, reference_offset, end, far_lens };
    let mut program = [ExposureStep::default(); PROGRAM_STEPS];
    let mut previous = ExposureStep {
        offset: program_offset,
        level: scene.offset,
        profile: first_profile,
        next_profile: first_profile,
        lens_step: near_lens,
        ..ExposureStep::default()
    };
    for index in 0..usize::from(steps) {
        let rise = if index == 0 {
            first_step
        } else if index + 1 == usize::from(steps) {
            let position = (index as i16).wrapping_mul(step).wrapping_add(start);
            if position > end { step.wrapping_add(end).wrapping_sub(position) } else { step }
        } else {
            step
        };
        let mut entry = previous;
        if rise > 0 {
            builder.advance(ports, &mut entry, rise);
        }
        entry.index = index as u8;
        if index == 0 {
            entry.position = start;
        } else {
            entry.position = rise.wrapping_add(previous.position);
            if ports.offset_class(previous.next_profile) != ports.offset_class(entry.profile) {
                let sensitivity = ports.sensitivity_class(entry.profile);
                entry.profile = profile_at_sensitivity(ports, sensitivity);
            }
            program[index - 1].next_profile = entry.profile;
        }
        program[index] = entry;
        previous = entry;
    }
    let count = if extends {
        let last = program[usize::from(steps) - 1];
        program[usize::from(steps)] = ExposureStep {
            index: last.index + 1,
            position: last.position.wrapping_add(program_offset),
            extended: true,
            ..last
        };
        steps + 1
    } else {
        steps
    };
    for entry in &mut program[..usize::from(count)] {
        entry.converted = ports.converted_level(entry.profile, entry.level);
    }

    let mut span = CandidateSpan { first: 1, last: count, low, high, start, end, first_step, step, bound };
    let configured = ports.property(property::FIRST_STEP) as u8;
    let choice = if configured != u8::MAX {
        span.first = if (1..=8).contains(&configured) && configured <= count { configured } else { 1 };
        FirstChoice::Configured
    } else if snapshot.valid == 0 {
        FirstChoice::Unmeasured
    } else {
        span.first = first_from_curve(ports, &program[..usize::from(count)], context);
        FirstChoice::Curve
    };
    Program { span, steps: program, choice }
}

/// The number of steps and the step size that divide `room` into at most `parts` steps of at most
/// `limit` after the first.
fn steps(room: i16, limit: i16, parts: i16) -> (u8, i16) {
    if room <= 0 {
        return (1, 0);
    }
    let mut size = 0;
    for parts in 1..=parts {
        size = (room + parts - 1) / parts;
        if limit >= size {
            return (parts as u8 + 1, size);
        }
    }
    (1, size)
}

/// The first step the snapshot's offset reaches, or the first step when the focus curve falls
/// below the tolerance band of the offset or of that step (0xf658c8..0xf65a4c, table 0xf66020).
fn first_from_curve(ports: &mut impl Ports, program: &[ExposureStep], context: &ExposureContext) -> u8 {
    let target = context.levels.target;
    let offset = context.snapshot.offset;
    let lens_reference = context.binding.lens_reference;
    let band = settings::tolerance(ports);
    let searched = program.len().saturating_sub(1);
    let mut reached = 0;
    while reached < searched {
        let value = step_value(ports, &program[reached], lens_reference);
        if i32::from(offset) < i32::from(value) + i32::from(band.above) - i32::from(target) {
            break;
        }
        reached += 1;
    }
    let distance = step_value(ports, &program[reached], lens_reference).wrapping_sub(target);
    let mut below = false;
    for point in 0..shared::CURVE_POINTS {
        let value = ports.read_u16(shared::focus_curve(point)) as i16;
        let above = band.above.wrapping_add(offset) < value || distance.wrapping_add(band.above) < value;
        if !above && (band.below.wrapping_add(offset) > value || distance.wrapping_add(band.below) > value) {
            below = true;
        }
    }
    if below { 1 } else { reached as u8 + 1 }
}

