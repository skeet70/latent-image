//! The evaluation prepared when the adjustment path is not taken (0xf69c48).

use xm1_saclac_interface::exposure::EvaluationRequest;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::shared::group_22;

use crate::exposure::context::{CandidateSpan, EvaluationArea, ExposureContext, ExposureStep, SubjectList, EVALUATION_AREAS};
use crate::exposure::correction::{self, SubjectSource};
use crate::exposure::levels;
use crate::exposure::ports::Ports;
use crate::exposure::program::{self, FirstChoice, Limits};
use crate::exposure::settings;
use crate::selection::{Candidate, Column, LevelRange, SelectionBank, ROWS};
use crate::stock::{dump, property, shared};

/// The floor every evaluation request starts with (0xf6925c).
const REQUEST_FLOOR: i16 = -900;

/// The steps `first..=last` as the image task measures them (0xf68b14).
fn areas(program: &[ExposureStep], span: CandidateSpan, target: i16) -> [EvaluationArea; EVALUATION_AREAS] {
    let mut areas = [EvaluationArea::default(); EVALUATION_AREAS];
    let covered = program.iter().take(usize::from(span.last)).skip(usize::from(span.first).saturating_sub(1));
    for (area, step) in areas.iter_mut().zip(covered) {
        *area = EvaluationArea {
            profile: step.profile,
            next_profile: step.next_profile,
            lens_step: step.lens_step,
            converted: step.converted,
            level: step.offset.wrapping_add(target) as u16,
            extended: step.extended,
        };
    }
    areas
}

/// The subject regions a column covers.
fn column(subjects: &SubjectList) -> Column {
    let mut column = Column { count: subjects.count, ..Column::default() };
    for (region, subject) in column.regions.iter_mut().zip(subjects.listed()) {
        region.copy_from_slice(&subject.detail[4..12]);
    }
    column
}

/// One candidate per step from `first`.
fn bank(ports: &mut impl Ports, context: &ExposureContext, choice: FirstChoice) -> SelectionBank {
    let span = context.span;
    let mut bank = SelectionBank {
        count: span.last.wrapping_add(1).wrapping_sub(span.first),
        first: span.first,
        any_range: choice == FirstChoice::Curve,
        expected: LevelRange { lowest: u8::MAX, highest: 0 },
        columns: [column(&context.binding.subjects); ROWS],
        ..SelectionBank::default()
    };
    for (index, step) in context.program.iter().enumerate().take(usize::from(span.last)) {
        bank.highest_below = bank.highest_below.max(step.index);
        if index + 1 < usize::from(span.first) {
            continue;
        }
        let value = program::step_value(ports, step, context.binding.lens_reference);
        bank.candidates[usize::from(bank.filled)] = Candidate { value, level: step.index, state: 0 };
        bank.filled += 1;
        bank.expected.lowest = bank.expected.lowest.min(step.index);
        bank.expected.highest = bank.expected.highest.max(step.index);
    }
    bank
}

/// The record's selection dimensions (0xf4c668).
fn dimensions(ports: &mut impl Ports, record: RecordHandle) -> [u16; 3] {
    let bytes: [u8; 20] = ports.profile().read_shared(record, &group_22::AT_0180);
    let half = |index: usize| u16::from_le_bytes([bytes[2 * index], bytes[2 * index + 1]]);
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    match (word(12), word(16)) {
        (1, 0) => [half(3), half(4), half(5)],
        (1, _) => [half(0), half(1), half(2)],
        _ => ports.default_dimensions(),
    }
}

/// The constants every evaluation request of this exposure carries (0xf69224).
fn request_constants(ports: &mut impl Ports, record: RecordHandle, context: &ExposureContext) -> EvaluationRequest {
    let tolerance = settings::tolerance(ports);
    EvaluationRequest {
        tolerance_above: tolerance.above,
        tolerance_below: tolerance.below,
        floor: REQUEST_FLOOR,
        dimensions: dimensions(ports, record),
        level_curves: core::array::from_fn(|curve| Some(shared::level_curve_base(curve))),
        frame: context.binding.frame,
        subject_curves: core::array::from_fn(|curve| Some(shared::subject_curve_base(curve))),
        ..EvaluationRequest::default()
    }
}

/// The level the correction is referred to (0xf6911c).
pub fn reference_value(ports: &mut impl Ports, context: &ExposureContext) -> i16 {
    if context.outcome.adjusting {
        let lens = ports.calibration_index(context.snapshot.focus.lens_detail);
        let level = ports.level(lens, context.binding.lens_reference);
        let limit = settings::signed_word(ports, property::PROGRAM_LEVEL_LIMIT);
        return level.wrapping_add(limit).wrapping_sub(ports.class_offset(0));
    }
    let bank = &context.selection;
    let count = usize::from(bank.count).min(ROWS);
    let index = bank.candidates[..count].iter().position(|candidate| candidate.level == bank.expected.highest).unwrap_or(count);
    bank.candidates.get(index).map_or(0, |candidate| candidate.value)
}

/// Prepares the evaluation of the bound record.
pub fn set_up(ports: &mut impl Ports, limits: &mut impl Limits, context: &mut ExposureContext) {
    let record = context.binding.record_handle();
    context.levels = levels::start(ports, record, false);
    let program = program::build(ports, limits, context);
    context.span = program.span;
    context.program = program.steps;
    context.areas = areas(&context.program, context.span, context.levels.target);
    context.selection = bank(ports, context, program.choice);
    context.evaluation = request_constants(ports, record, context);
    let reference = reference_value(ports, context).wrapping_add(250);
    context.correction = correction::request(ports, context, SubjectSource::Bound, reference);
    if let Some(frame) = context.binding.frame {
        ports.write_u32(dump::evaluation(frame, 0), context.snapshot.valid);
        ports.write_u32(dump::evaluation(frame, 1), context.snapshot.focus.pinned);
        ports.write_u32(dump::evaluation(frame, 2), context.span.bound.code());
    }
}
