//! The evaluation prepared when the adjustment path is not taken (0xf69c48), and the evaluation
//! request calc answers for each row of its bank (calc 0x3c00, 0xf6a0cc).

use xm1_saclac_interface::calc::CalcPayload;
use xm1_saclac_interface::exposure::EvaluationRequest;
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::shared::group_22;

use crate::adjust::rate_policy;
use crate::exposure::context::{CandidateSpan, EvaluationArea, ExposureContext, ExposureStep, SubjectList, EVALUATION_AREAS};
use crate::exposure::correction::{self, SubjectSource};
use crate::exposure::levels;
use crate::exposure::ports::{InPlace, Ports};
use crate::exposure::program::{self, FirstChoice, Limits};
use crate::exposure::settings;
use crate::requests::Peers;
use crate::selection::{BufferSlot, Candidate, Column, LevelRange, Row, RowIndex, RowPhase, SelectionBank, ROWS};
use crate::stock::{dump, property, shared};

/// The floor every evaluation request starts with (0xf6925c).
const REQUEST_FLOOR: i16 = -900;
/// The floor of a row of fixed offset 3 (0xf6a274).
const OFFSET_THREE_FLOOR: i16 = -600;
/// `lower_value` when no candidate is one level below the row's (0xf6a1e0).
const NO_LOWER_VALUE: u16 = 0x8000;

/// The steps `first..=last` as the image task measures them (0xf68b14).
pub(crate) fn areas(program: &[ExposureStep], span: CandidateSpan, target: i16) -> [EvaluationArea; EVALUATION_AREAS] {
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
pub(crate) fn column(subjects: &SubjectList) -> Column {
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
        rows: core::array::from_fn(|index| Row { slot: BufferSlot::new(index as u8), ..Row::default() }),
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
pub(crate) fn request_constants(ports: &mut impl Ports, record: RecordHandle, context: &ExposureContext) -> EvaluationRequest {
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

/// Calc's rate words for a row (0xf5eb34 with both factors 1).
fn rates(ports: &mut impl Ports) -> [u32; 4] {
    let rate = rate_policy::scaled_by_configured_factor(ports, 1);
    let tenth = rate / 10;
    [rate, tenth, tenth * 2, tenth]
}

/// The value of the last candidate one level below `level`.
fn lower_value(bank: &SelectionBank, level: u8) -> u16 {
    let count = usize::from(bank.count).min(ROWS);
    let below = level.checked_sub(1);
    bank.candidates[..count].iter().rev().find(|candidate| Some(candidate.level) == below).map_or(NO_LOWER_VALUE, |candidate| candidate.value as u16)
}

/// 2 when the first applied row's level is above `level`, else 1; 0 when no row is applied.
fn direction_from_applied(bank: &SelectionBank, level: u8) -> u32 {
    let count = usize::from(bank.count).min(ROWS);
    let applied = bank.rows[..count].iter().position(|row| row.phase == RowPhase::Applied);
    applied.map_or(0, |index| if bank.candidates[index].level > level { 2 } else { 1 })
}

/// Fills the request's row fields from the bank (0xf6a0cc).
fn fill_row(ports: &mut impl Ports, context: &mut ExposureContext, row: RowIndex) {
    let index = row.get();
    let rates = rates(ports);
    let column_regions = ports.address(InPlace::ColumnRegions(index));
    let bank = &context.selection;
    let candidate = bank.candidates[index];
    let Row { measurement, slot, .. } = bank.rows[index];
    let half = |pair: [u32; 2]| [pair[1] as u16, pair[0] as u16];
    let request = &mut context.evaluation;
    request.row = index as u32;
    request.measured = [half(measurement.pairs[0]), half(measurement.pairs[2]), half(measurement.pairs[1])];
    request.statistics = measurement.statistics.map(|statistic| statistic as u16);
    request.row_buffer = slot.map(|slot| shared::row_buffers(slot)[0]);
    request.rates = rates;
    request.value = candidate.value as u16;
    request.lower_value = lower_value(bank, candidate.level);
    request.after_reference = !bank.any_range && u16::from(candidate.level) == u16::from(bank.reference_level) + 1;
    request.direction = match candidate.state {
        state @ (3 | 4) => {
            request.floor = if state == 3 { OFFSET_THREE_FLOOR } else { REQUEST_FLOOR };
            state
        }
        _ => direction_from_applied(bank, candidate.level),
    };
    request.column_count = bank.columns[index].count;
    request.column_regions = Some(column_regions);
    request.column_buffer = slot.map(shared::column_buffer);
    request.next_profile = context.areas[index].next_profile;
    request.level_classes = slot.map(|_| shared::level_class(0));
    request.selection_flags = slot.map(|_| shared::SELECTION_FLAGS);
}

/// Posts calc 0x3c00 for `row` and marks the row requested (0xf5fd20).
pub fn request_row<P: Peers>(ports: &mut impl Ports, peers: &mut P, context: &mut ExposureContext, row: RowIndex, callback: StockCallbackWord) -> Result<(), P::Error> {
    fill_row(ports, context, row);
    context.selection.mark_requested(row.get());
    peers.calc(&CalcPayload::Evaluation { request: context.evaluation, callback })
}
