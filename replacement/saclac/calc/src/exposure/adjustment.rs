//! The evaluation prepared when the snapshot's focus result settles the exposure (0xf6b4dc): one
//! program step at each end of the adjustment window the adjustment state names (0xf65cf4,
//! 0xf65a74), evaluated at fixed offsets against the snapshot's subjects.

use crate::exposure::context::{CandidateSpan, ExposureContext, ExposureStep};
use crate::exposure::correction::{self, SubjectSource};
use crate::exposure::evaluation::{areas, column, reference_value, request_constants};
use crate::exposure::ports::Ports;
use crate::exposure::program::profile_at_sensitivity;
use crate::exposure::settings;
use crate::selection::{BufferSlot, Candidate, Column, SelectionBank};
use crate::stock::property;
use crate::stock::tables::PROGRAM_PROFILES;

/// The rows the path binds; row `i` takes buffer slot `i + 1` and the last row none
/// (0xf6b698..0xf6b724). Suspected stock defect, kept: calc and the trace use these buffers.
const BOUND_ROWS: usize = 7;

/// One program step from `level`: raised toward the configured target within its limits, in the
/// profile of the band the raised level falls in (0xf65a74).
fn step(ports: &mut impl Ports, index: u8, level: i16, target: i16, lens: u32, lens_reference: u32) -> ExposureStep {
    let limit = settings::signed_word(ports, property::PROGRAM_LEVEL_LIMIT);
    let floor = settings::signed_word(ports, property::ADJUSTMENT_STEP_FLOOR);
    let raise_target = settings::signed_word(ports, property::ADJUSTMENT_RAISE_TARGET);
    let raise_limit = settings::level(ports, property::ADJUSTMENT_RAISE_LIMIT).wrapping_mul(10);
    let bands = property::ADJUSTMENT_BANDS.map(|id| settings::signed_word(ports, id));
    let sensitivities = property::ADJUSTMENT_SENSITIVITIES.map(|id| ports.property(id) as u16);
    let lens_level = ports.level(lens, lens_reference);
    let base = level.wrapping_add(ports.class_offset(0)).wrapping_sub(lens_level);
    let raise = raise_target.wrapping_sub(base).max(0).min(raise_limit);
    let raised = base.wrapping_add(raise).max(floor).min(limit);
    let band = bands.iter().position(|bound| raised >= *bound).unwrap_or(bands.len());
    let class = ports.sensitivity_class_for(u32::from(sensitivities[band]));
    let profile = profile_at_sensitivity(ports, class);
    let first_class = ports.sensitivity_class_for(u32::from(sensitivities[0]));
    let next_profile = profile_at_sensitivity(ports, first_class);
    ExposureStep {
        index,
        position: level.wrapping_add(500).wrapping_sub(target),
        offset: raise,
        level: raised,
        profile,
        next_profile,
        lens_step: lens,
        converted: ports.converted_level(profile, raised),
        extended: false,
    }
}

/// The adjustment window's ends in the table range, widened to the margin for state 3
/// (0xf65d58..0xf65eac).
fn window(ports: &mut impl Ports, context: &ExposureContext) -> (i16, i16) {
    let floor = settings::level(ports, property::TABLE_FLOOR).wrapping_mul(10);
    let ceiling = settings::level(ports, property::TABLE_CEILING).wrapping_mul(10);
    let margin = settings::level(ports, property::WINDOW_MARGIN).wrapping_mul(10);
    let fifth = match context.snapshot.focus.levels[4] {
        level if level <= 0 => 0,
        level if level > 99 => level - 100,
        level => level,
    };
    let level = context.adjustment.level as i16;
    let high = margin.wrapping_add(level).wrapping_add(fifth);
    let low = level.wrapping_sub(margin);
    let mut lower = if floor > low { floor } else { low.min(ceiling) };
    let mut upper = if ceiling < high { ceiling } else { high.max(floor) };
    if i32::from(upper) - i32::from(lower) < i32::from(margin) && context.adjustment.state == 3 {
        if ceiling < high {
            lower = ceiling.wrapping_sub(margin);
        } else if floor > low {
            upper = margin.wrapping_add(floor);
        }
    }
    (lower, upper)
}

/// The program's steps: the window's lower end for state 1, its upper end for state 2, both for
/// any other nonzero state. Returns how many (0xf65cf4).
fn program(ports: &mut impl Ports, context: &mut ExposureContext) -> u8 {
    let state = context.adjustment.state;
    if state == 0 {
        return 0;
    }
    let (lower, upper) = window(ports, context);
    let target = context.levels.target;
    let lens = ports.calibration_index(context.snapshot.focus.lens_detail);
    let lens_reference = context.binding.lens_reference;
    let steps = &mut context.program;
    let count = match state {
        1 => {
            steps[0] = step(ports, 0, lower, target, lens, lens_reference);
            1
        }
        2 => {
            steps[0] = step(ports, 0, upper, target, lens, lens_reference);
            1
        }
        _ => {
            steps[0] = step(ports, 0, lower, target, lens, lens_reference);
            steps[1] = step(ports, 1, upper, target, lens, lens_reference);
            if ports.offset_class(steps[0].next_profile) != ports.offset_class(steps[1].profile) {
                let sensitivity = ports.sensitivity_class(steps[1].profile);
                steps[1].profile = profile_at_sensitivity(ports, sensitivity);
            }
            steps[0].next_profile = steps[1].profile;
            2
        }
    };
    let sensitivity = ports.sensitivity_class(PROGRAM_PROFILES[PROGRAM_PROFILES.len() - 1]);
    steps[usize::from(count) - 1].next_profile = profile_at_sensitivity(ports, sensitivity);
    count
}

/// The code the shot reads for the adjustment state and step count (0xf6b7f4).
const fn code(state: u32, count: u8) -> u8 {
    match (state, count) {
        (0, _) => 0,
        (1, 1) => 3,
        (1, _) => 4,
        (2, 1) => 1,
        (2, _) => 2,
        (3, 2) => 5,
        (3, _) => 6,
        _ => 7,
    }
}

/// Prepares the evaluation of the adjustment path's steps. Their columns carry the snapshot's
/// subject regions with a count of 0 (0xf6b6d4; suspected stock defect, kept: calc reads it).
pub fn set_up(ports: &mut impl Ports, context: &mut ExposureContext) {
    let count = program(ports, context);
    let target = context.levels.target;
    let state = context.adjustment.state;
    context.span.last = count;
    context.areas = areas(&context.program, CandidateSpan { first: 1, last: count, ..CandidateSpan::default() }, target);
    let mut bank = SelectionBank { count, any_range: true, ..SelectionBank::default() };
    for (index, area) in context.areas.iter().enumerate().take(usize::from(count)) {
        let lens = ports.level(area.lens_step, context.binding.lens_reference);
        let offset = ports.level_offset(area.next_profile);
        let value = area.converted.wrapping_add(target as u16).wrapping_sub(area.level).wrapping_add(lens as u16).wrapping_sub(offset as u16) as i16;
        let fixed = match state {
            1 => 4,
            2 => 3,
            _ if index == 0 => 4,
            _ => 3,
        };
        bank.candidates[index] = Candidate { value, level: index as u8, state: fixed };
        bank.highest_below = index as u8;
        bank.expected.highest = index as u8;
        bank.filled += 1;
    }
    let regions = column(&context.snapshot.subjects);
    for (index, (row, bound)) in bank.rows.iter_mut().zip(bank.columns.iter_mut()).take(BOUND_ROWS).enumerate() {
        row.slot = BufferSlot::new(index as u8 + 1);
        *bound = Column { count: 0, ..regions };
    }
    context.selection = bank;
    let record = context.binding.record_handle();
    context.evaluation = request_constants(ports, record, context);
    let reference = reference_value(ports, context).wrapping_add(250);
    context.correction = correction::request(ports, context, SubjectSource::Snapshot, reference);
    context.adjustment.code = code(state, count);
}
