#![forbid(unsafe_code)]

//! The exposure an acquisition settles on and its publication in the profile record.

use crate::pipeline::{GainStep, PipelineWord, TimingWord};
use xm1_saclac_interface::calc::AcquisitionGrid;
use xm1_saclac_interface::round_acquisition::{PrimaryLevels, SelectionLevels};
use xm1_saclac_interface::RecordHandle;

use super::ports::{ExposurePublication, ExposureRecordUpdate, Ports, Program, ProgramInputs, RecordView};

/// What the live acquisition keeps of the exposure it settled on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureSummary {
    pub base: u16,
    pub exposure: PipelineWord,
    pub level: u16,
    pub second: u16,
    pub offset: u16,
    pub aspect: u16,
    pub correction: u16,
    pub timing: TimingWord,
    pub code: u16,
    pub selected: u8,
    /// The calc library's rate pair; all ones until its first rate selection.
    pub rate_pair: [u32; 2],
}

impl ExposureSummary {
    pub const UNSET_RATE_PAIR: [u32; 2] = [u32::MAX; 2];
}

/// The exposure fields a publication sets as given.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExposureSetting {
    pub exposure: PipelineWord,
    pub level: u16,
    pub second: u16,
    pub reference: u16,
    pub decade: u16,
    pub offset: u16,
    pub correction: u16,
    pub gain_step: GainStep,
    pub timing: TimingWord,
    pub code: u16,
    pub selected: u8,
    /// One lowers the published reference level by 100.
    pub adjustment: u32,
    /// The word the exposure value pairs with the exposure: a lens resource or a sample's second exposure.
    pub second_exposure: PipelineWord,
}

/// Publishes an exposure with the levels and aspect the record derives from it.
pub(crate) fn publish(record: RecordHandle, setting: &ExposureSetting, ports: &mut impl Ports) {
    let value = ports.exposure_value([setting.exposure, setting.second_exposure]) as u16;
    let combined = setting.level.wrapping_sub(setting.second).wrapping_sub(setting.offset).wrapping_add(value);
    let reference = if setting.adjustment == 1 { setting.reference.wrapping_sub(100) } else { setting.reference };
    let aspect = ports.timing_offset(setting.timing);
    ports.publish_exposure(record, &ExposurePublication {
        exposure: setting.exposure,
        level: setting.level,
        second: setting.second,
        reference: setting.reference,
        decade: setting.decade,
        offset: setting.offset,
        correction: setting.correction,
        gain_step: setting.gain_step,
        timing: setting.timing,
        code: setting.code,
        selected: setting.selected,
        adjustment: setting.adjustment,
        combined,
        reference_level: combined.wrapping_add(reference),
        aspect,
    });
    if keeps_exposure_record(record, ports) {
        ports.update_exposure_record(record, ExposureRecordUpdate::GainStep(setting.gain_step));
    }
}

/// Publishes the primary levels and the grid they were taken over.
pub(crate) fn publish_primary(record: RecordHandle, levels: &PrimaryLevels, grid: &AcquisitionGrid, ports: &mut impl Ports) {
    ports.publish_primary_levels(record, levels);
    ports.publish_grid(record, grid);
    if keeps_exposure_record(record, ports) {
        ports.update_exposure_record(record, ExposureRecordUpdate::PrimaryPair([levels.levels[2], levels.levels[6]]));
    }
}

pub(crate) fn publish_selection(record: RecordHandle, levels: &SelectionLevels, ports: &mut impl Ports) {
    ports.publish_selection_levels(record, levels);
    if keeps_exposure_record(record, ports) {
        ports.update_exposure_record(record, ExposureRecordUpdate::SelectionTail { tail: levels.tail, last: levels.last });
    }
}

pub(crate) fn keeps_exposure_record(record: RecordHandle, ports: &mut impl Ports) -> bool {
    ports.record_view(record).mode_flag == 1
}

/// The primary and selection levels an initialization publishes for a program's base level.
pub(crate) fn initial_levels(base: u16) -> (PrimaryLevels, SelectionLevels) {
    (
        PrimaryLevels { base, levels: [base; 7], word: 0 },
        SelectionLevels { base, levels: [base; 3], zeroed: [0; 6], unsigned: 0, word: 0, last: 0, reference: base, tail: [base; 3] },
    )
}

/// The adjustment layer's program for `inputs`; the record's stored program when it chooses none.
/// Corrected: X-M1 1.50 reads an unwritten stack word there (spec/11, measurement round corrections).
pub(crate) fn chosen_program(record: RecordHandle, inputs: &ProgramInputs, ports: &mut impl Ports) -> Program {
    match ports.select_program(inputs) {
        Some(program) => program,
        None => ports.stored_program(record),
    }
}

/// The program selection an initialization makes for the record's own kind.
pub(crate) fn initial_program(view: &RecordView, kind: u32) -> ProgramInputs {
    ProgramInputs {
        selection: view.selection,
        adjustment: view.adjustment,
        kind,
        mode: view.mode,
        word: view.program_word[0],
        flag: 0,
        program_flag: view.program_flag(),
        option: 0,
        policy: false,
        percent: 10,
        bytes: [0, 2],
    }
}
