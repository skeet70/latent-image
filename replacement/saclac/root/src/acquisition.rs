#![forbid(unsafe_code)]

//! The live and S1 acquisitions share one selection and use the same record, calculation and
//! peer bindings. Guest layouts and retained entry calls stay in `stock::acquisition`.

use xm1_focus_measurement_round::acquisition::ports::*;
use xm1_focus_measurement_round::acquisition::selection::SelectionMemory;
use xm1_focus_measurement_round::auxiliary::ResourceIndex;
use xm1_focus_measurement_round::pipeline::{Exposure, GainStep, HistoryStatistics, Pipeline, PipelineSnapshot, PipelineWord, Statistics, TimingWord};
use xm1_focus_measurement_round::profile::{FrameInterval, GridCell, ProfileSelection, RetainedWord};
use xm1_focus_measurement_round::record::{Adjustment, AdjustmentWords, Trace, TraceMark};
use xm1_saclac_calc::adjust::{configuration, profile_adjustment as programs, rate_policy, sample_grid};
use xm1_saclac_calc::exposure::ports::{Lens, Properties, Property, StockMemory};
use xm1_saclac_config::table::{self, Bounds, Row, Step};
use xm1_saclac_interface::calc::{AcquisitionGrid, Calc1Payload, CalcPayload, SharedAddress};
use xm1_saclac_interface::round_acquisition::*;
use xm1_saclac_interface::stock::{self as wire, round_acquisition as codec};
use xm1_saclac_interface::{Calc, Calc1, Calc1Request, CalcRequest, Completed, ImgDc, ImgDcOperation, ImgDcRequest, RecordHandle};
use xm1_saclac_profile_records::field::Index;
use xm1_saclac_profile_records::stock::{record, shared};
use xm1_saclac_profile_records::{MeasurementSelection, Memory, SharedHeaderByte, StockEntries, WindowStatus};
use xm1_saclac_shared::lens::{self, LensControl};
use xm1_saclac_shared::settings::{AdjustmentKey, Adjustments};
use xm1_saclac_shared::stock::{Machine, Stock};

use crate::services::Services;
use crate::stock::acquisition as boundary;

/// The state retained between the two children and the controllers' selection-value setter.
pub struct AcquisitionState {
    pub(crate) selection: SelectionMemory,
    grids: [AcquisitionGrid; 2],
}

impl Default for AcquisitionState {
    fn default() -> Self { Self { selection: SelectionMemory::new(), grids: [AcquisitionGrid([0; 1236]); 2] } }
}

fn branch_index(branch: Branch) -> usize { match branch { Branch::Live => 0, Branch::S1 => 1 } }
fn profile_selection(word: u32) -> ProfileSelection { match word { 0 => ProfileSelection::Unselected, 1 => ProfileSelection::Record, other => ProfileSelection::Other(other as u8) } }
fn selection_word(selection: ProfileSelection) -> u32 { match selection { ProfileSelection::Unselected => 0, ProfileSelection::Record => 1, ProfileSelection::Other(word) => u32::from(word) } }
static EMPTY_GRID: AcquisitionGrid = AcquisitionGrid([0; 1236]);

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    /// The controllers' 0xf251c4 report; S1 consumes it after restoring the selection.
    pub fn mark_selection_changed(&mut self) { self.acquisition.selection.mark_value_changed(); }
}

/// Calculation-only ports, independent of the focus search's binding implementations.
struct Calculations<'a, M>(&'a M, &'a Stock<M>);

impl<M: Machine> Properties for Calculations<'_, M> {
    fn property(&mut self, property: Property) -> u32 { u32::from(self.1.byte(AdjustmentKey(property.key()))) }
}

impl<M: Machine> Lens for Calculations<'_, M> {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { table::combined_word(self.1, Step(lens), Row(reference)) }
    fn base_level(&mut self, lens: u32) -> i16 { table::primary_rounded_word(self.1, Step(lens)) }
    fn reference_offset(&mut self, reference: u32) -> i16 { table::relative_word(self.1, Row(reference)) }
    fn calibration_index(&mut self, lens: u32) -> u32 { table::column(self.1, Step(lens)) }
}

impl<M: Machine> programs::LensSteps for Calculations<'_, M> {
    fn near_step(&mut self, reference: u32, limit: u16, which: programs::StepTable) -> u32 {
        let which = match which { programs::StepTable::Standard => Bounds::Primary, programs::StepTable::Lens => Bounds::Selected };
        table::lower_step(self.1, Row(reference), xm1_saclac_config::stock::decode_step_limit(u32::from(limit)), which).0
    }
    fn far_step(&mut self, reference: u32, limit: u16, which: programs::StepTable) -> u32 {
        let which = match which { programs::StepTable::Standard => Bounds::Primary, programs::StepTable::Lens => Bounds::Selected };
        table::upper_step(self.1, Row(reference), xm1_saclac_config::stock::decode_step_limit(u32::from(limit)), which).0
    }
    fn lens_step(&mut self, step: u32) -> u32 { boundary::lens_step(self.0, step) }
}

impl<M: Machine> programs::StepRatios for Calculations<'_, M> {
    fn step_ratio(&mut self, code: u16, selector: u8) -> u32 { boundary::step_ratio(self.0, code, selector) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> StockMemory for Services<M, R, E, P> {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { boundary::read_record::<1>(&self.machine, at)[0] }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.machine.read_u16(at.0) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.machine.read_u32(at.0) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.machine.write_u8(at.0, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { boundary::place(&mut self.machine, at.0, &value.to_le_bytes()) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.machine.write_u32(at.0, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) {
        for offset in 0..bytes { self.machine.write_u8(at.0.wrapping_add(offset as u32), value); }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Pipeline for Services<M, R, E, P> {
    fn capture(&mut self, statistics: Statistics) { boundary::capture(&self.machine, statistics) }
    fn snapshot(&mut self, statistics: Statistics) -> PipelineSnapshot {
        xm1_focus_measurement_round::stock::pipeline_snapshot(&boundary::snapshot(&self.machine, statistics))
    }
    fn timing(&mut self) -> TimingWord { boundary::timing(&self.machine) }
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval> { boundary::frame_interval(&self.machine, timing).map(FrameInterval) }
    fn request_mode_six_statistics(&mut self) { boundary::request_mode_six_statistics(&self.machine) }
    fn take_window_refresh(&mut self) -> bool { boundary::take_window_refresh(&self.machine) }
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics { boundary::history_statistics(&self.machine, cell) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Exposure for Services<M, R, E, P> {
    fn gain_offset(&mut self, step: GainStep) -> u32 { self.stock.zone_limit(step.0 as u8) as i32 as u32 }
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16 { table::combined_word(&self.stock, Step(exposure[0].0), Row(exposure[1].0)) }
    fn timing_offset(&mut self, timing: TimingWord) -> u16 { boundary::timing_offset(&self.machine, timing) }
    fn relative_exposure(&mut self, exposure: PipelineWord) -> i16 { table::relative_word(&self.stock, Row(exposure.0)) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Adjustment for Services<M, R, E, P> {
    fn adjustment_words(&mut self) -> AdjustmentWords {
        AdjustmentWords {
            mode: RetainedWord(u32::from(configuration::shared::mode_is_one(self))),
            recording: RetainedWord(configuration::shared::word(self)),
            controllers: RetainedWord(u32::from(self.configuration.controller_flag(configuration::ControllerFlag::First))),
            search: RetainedWord(u32::from(self.configuration.refresh())),
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Trace for Services<M, R, E, P> {
    fn trace(&mut self, mark: TraceMark) { boundary::trace(&self.machine, boundary::trace_point(mark)) }
}

fn program(program: Program) -> programs::AdjustmentProfile {
    programs::AdjustmentProfile::new(program.0).expect("an acquisition program is a profile table index")
}

fn selection(code: u32) -> MeasurementSelection {
    match code { 0 => MeasurementSelection::Unselected, 1 => MeasurementSelection::Record, other => MeasurementSelection::Other(other as u8) }
}

fn treatments(inputs: &ModeInputs) -> xm1_saclac_calc::adjust::scene_policy::TreatmentInputs {
    xm1_saclac_calc::adjust::scene_policy::TreatmentInputs {
        active: inputs.grid_kind, subjects: u32::from(inputs.grid_byte),
        measurement_selection: selection(selection_word(inputs.selection)), scene: inputs.updated_position,
        class: inputs.category, callback_armed: inputs.mode_flag,
        held: inputs.change_category, single_subject: inputs.grid_flag,
    }
}

fn slot_index(slot: PositionSlot) -> Index<5> {
    Index::new(match slot { PositionSlot::Reported => 1, PositionSlot::Lens => 2, PositionSlot::Current => 3, PositionSlot::Selected => 4 }).unwrap()
}

macro_rules! store {
    ($this:ident, $record:expr, $field:path, $value:expr) => { let _refused = $this.records.write_shared($record, &$field, $value); };
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1 + ImgDc> Ports for Services<M, R, E, P> {
    fn selection_memory(&mut self) -> &mut SelectionMemory { &mut self.acquisition.selection }
    fn acquisition_settings(&mut self) -> AcquisitionSettings { boundary::acquisition_settings(|key| self.stock.byte(AdjustmentKey(key))) }

    fn record_view(&mut self, record: RecordHandle) -> RecordView {
        use record::group_00 as h;
        use shared::{group_20 as c, group_27 as x};
        let profile_index = u32::from(xm1_saclac_calc::profile::Profile::new(&self.records, &mut self.entries).grid_layout(record));
        let entries = boundary::record_entries(&self.machine, record, profile_index);
        let category = self.entries.header_byte(record, SharedHeaderByte::At47);
        let cell = self.focus_cell(record, category);
        let (block_reference, block_word) = boundary::lens_snapshot_reference(&self.shared_bytes(record, &shared::group_17::LENS_SNAPSHOT));
        let selected = xm1_saclac_calc::profile::selection_code(self.entries.measurement_selection(record));
        RecordView {
            start: Start::of(self.shared(record, &c::AT_0140)), selection: profile_selection(u32::from(selected)),
            s1_mode: AcquisitionMode(u32::from(self.entries.header_byte(record, SharedHeaderByte::At31))),
            halfword: self.field(record, &h::AT_002E), mode_word: u32::from(self.field(record, &h::AT_0032)),
            byte: self.field(record, &h::AT_0033), second_halfword: self.field(record, &h::AT_0026),
            setting_word: u32::from(self.field(record, &h::AT_005B)), grid_kind: u32::from(self.field(record, &h::AT_005D)),
            focus_word: u32::from(self.field(record, &h::AT_0034)), window: self.field(record, &h::AT_0000),
            seventh_word: RetainedWord(u32::from(self.field(record, &h::AT_0036))), fourth_word: RetainedWord(u32::from(self.field(record, &h::AT_003D))),
            third_word: RetainedWord(u32::from(self.field(record, &h::AT_003F))), request: u32::from(self.field(record, &h::AT_003E)),
            detail_flag: u32::from(self.field(record, &h::AT_0041)), word: u32::from(self.field(record, &h::AT_0043)),
            reference: self.field(record, &h::AT_001C) as u16, selection_word: self.field(record, &h::AT_0014),
            lens_halfword: self.field(record, &h::AT_001E) as u16, lens_reference: u32::from(self.field(record, &h::AT_0044)),
            halfwords: [self.field(record, &h::AT_0020) as u16, self.field(record, &h::AT_0024) as u16],
            zero_word: RetainedWord(u32::from(self.entries.header_byte(record, SharedHeaderByte::At46))), category: u32::from(category),
            pair: boundary::words(&self.field(record, &h::AT_0004)), tenth_word: RetainedWord(u32::from(self.field(record, &h::AT_005F))),
            change_category: u32::from(self.entries.header_byte(record, SharedHeaderByte::At61)),
            signed_word: if selected == 1 { self.stock.zone_limit(2) as u16 } else { self.field(record, &h::AT_0022) as u16 },
            eighth_word: RetainedWord(u32::from(self.field(record, &h::AT_0045))), second_word: RetainedWord(u32::from(self.field(record, &h::AT_004B))),
            mode_flag: u32::from(self.entries.header_byte(record, SharedHeaderByte::At63)),
            rate_enabled: u32::from(self.field(record, &h::INTERVAL_TRACKING).0), fifth_word: RetainedWord(u32::from(self.field(record, &h::AT_0051))),
            forced: u32::from(self.field(record, &h::AT_006B)), ninth_word: RetainedWord(u32::from(self.field(record, &h::AT_0066))),
            profile_flags: [u32::from(self.field(record, &h::AT_0067)), u32::from(self.field(record, &h::AT_0068)), u32::from(self.field(record, &h::AT_0069)),
                self.field(record, &record::group_13::AT_20C8), self.field(record, &record::group_13::AT_20CC), self.field(record, &record::group_31::AT_2110),
                self.field(record, &record::group_13::AT_20D4), self.field(record, &record::group_13::AT_20B0_4)],
            lens_word_index: self.shared(record, &c::OFFERED_MEASUREMENTS).0, adjustment: self.shared(record, &c::AT_014C),
            kind: self.shared(record, &x::WINDOW_STATUS).0, kind_override: self.shared(record, &x::AT_0738), pair_word: boundary::position_word(self.shared(record, &x::WINDOW_POSITION)),
            reuse_blocked: self.shared(record, &x::AT_0744), restored: u32::from(self.field(record, &h::AT_0070)),
            program_word: boundary::words(&self.shared_bytes(record, &shared::group_37::AT_085C)), restore_position: u32::from(self.field(record, &h::AT_006F)),
            s1_byte: self.shared(record, &shared::group_26::AT_0236), s1_halfword: self.shared(record, &shared::group_26::AT_0238) as u16,
            release_halfword: self.shared(record, &x::AT_0246) as u16, area: boundary::words(&self.shared_bytes(record, &shared::group_22::AT_0180)),
            mode: AcquisitionMode(entries.mode), updated_position: entries.updated_position, position_halfword: entries.position_halfword,
            converted_level: entries.converted_level, restore_adjustment: entries.restore_adjustment, decade: entries.decade,
            profile_index, ratio: [cell.column, cell.row], lens_limit: entries.lens_limit, block_reference, block_word,
            s1_exposure: PipelineWord(self.shared(record, &shared::group_26::AT_0200)), s1_level: self.shared(record, &shared::group_26::AT_0206) as u16,
            s1_second: self.shared(record, &shared::group_26::AT_0208) as u16, s1_timing: TimingWord(self.shared(record, &shared::group_26::AT_0230)),
        }
    }
    fn s1_rate_enabled(&mut self, record: RecordHandle) -> bool { self.field(record, &record::group_00::INTERVAL_TRACKING).0 == 1 }

    fn published_exposure(&mut self, record: RecordHandle) -> PublishedExposure {
        use shared::group_27 as x;
        PublishedExposure {
            combined: self.shared(record, &x::AT_0248) as u16, reference_level: self.shared(record, &x::AT_024A) as u16,
            exposure: PipelineWord(self.shared(record, &x::AT_0708)), level: self.shared(record, &x::AT_070C) as u16,
            second: self.shared(record, &x::AT_070E) as u16, correction: self.shared(record, &x::AT_0710) as u16,
            aspect: self.shared(record, &x::AT_0712) as u16, reference: self.shared(record, &x::AT_0718) as u16,
            offset: self.shared(record, &x::AT_0716) as u16, gain_step: GainStep(self.shared(record, &x::AT_071C)),
            lens: ResourceIndex(self.shared(record, &x::AT_0720)), timing: TimingWord(self.shared(record, &x::AT_0724)),
            code: self.shared(record, &x::AT_0728), selected: self.shared(record, &x::AT_072A), adjustment: self.shared(record, &x::AT_072C),
            selection_reference: self.shared(record, &x::AT_024C) as u16,
            primary_levels: [self.shared(record, &x::AT_024E) as u16, self.shared(record, &x::AT_0250) as u16, self.shared(record, &x::AT_0252) as u16, self.shared(record, &x::AT_0254) as u16],
        }
    }

    fn publish_exposure(&mut self, record: RecordHandle, p: &ExposurePublication) {
        use shared::group_27 as x;
        store!(self, record, x::AT_0708, p.exposure.0); store!(self, record, x::AT_070C, p.level as i16);
        store!(self, record, x::AT_070E, p.second as i16); store!(self, record, x::AT_0718, p.reference as i16);
        store!(self, record, x::AT_0714, p.decade as i16); store!(self, record, x::AT_0716, p.offset as i16);
        store!(self, record, x::AT_0710, p.correction as i16); store!(self, record, x::AT_071C, p.gain_step.0);
        store!(self, record, x::AT_0724, p.timing.0); store!(self, record, x::AT_0728, p.code);
        store!(self, record, x::AT_072A, p.selected); store!(self, record, x::AT_072C, p.adjustment);
        store!(self, record, x::AT_0248, p.combined as i16); store!(self, record, x::AT_024A, p.reference_level as i16);
        store!(self, record, x::AT_0712, p.aspect as i16);
    }

    fn publish_primary_levels(&mut self, record: RecordHandle, p: &PrimaryLevels) {
        use shared::group_27 as x;
        store!(self, record, x::AT_0246, p.base as i16); store!(self, record, x::AT_0258, p.word);
        store!(self, record, x::AT_024E, p.levels[0] as i16); store!(self, record, x::AT_0250, p.levels[1] as i16);
        store!(self, record, x::AT_0252, p.levels[2] as i16); store!(self, record, x::AT_0254, p.levels[3] as i16);
        store!(self, record, x::AT_025E, p.levels[4]); store!(self, record, x::AT_0260, p.levels[5]); store!(self, record, x::AT_0266, p.levels[6]);
    }

    fn publish_grid(&mut self, record: RecordHandle, branch: Branch) {
        let (cells, window, entries) = boundary::grid_records(&self.acquisition.grids[branch_index(branch)]);
        store!(self, record, shared::group_27::AT_026C, cells); store!(self, record, shared::group_27::AT_0680, window);
        store!(self, record, shared::group_27::AT_06A8, entries);
    }

    fn publish_selection_levels(&mut self, record: RecordHandle, p: &SelectionLevels) {
        use shared::group_27 as x;
        store!(self, record, x::AT_0244, p.base as i16);
        store!(self, record, x::AT_0262, p.levels[0] as i16); store!(self, record, x::AT_0264, p.levels[1] as i16); store!(self, record, x::AT_0268, p.levels[2] as i16);
        store!(self, record, x::AT_026A, p.zeroed[0] as i16); store!(self, record, x::AT_06F8, p.zeroed[1] as i16);
        store!(self, record, x::AT_06FA, p.zeroed[2] as i16); store!(self, record, x::AT_06FC, p.zeroed[3] as i16);
        store!(self, record, x::AT_06FE, p.zeroed[4] as i16); store!(self, record, x::AT_0700, p.zeroed[5] as i16);
        store!(self, record, x::AT_0702, p.unsigned); store!(self, record, x::AT_0704, p.word); store!(self, record, x::AT_024C, p.reference as i16);
    }

    fn update_exposure_record(&mut self, record: RecordHandle, update: ExposureRecordUpdate) {
        if self.entries.header_byte(record, SharedHeaderByte::At63) != 1 { return; }
        let mut bytes = self.shared_bytes(record, &shared::group_33::AT_07A0);
        boundary::update_exposure_record(&mut bytes, update);
        store!(self, record, shared::group_33::AT_07A0, bytes);
    }
    fn store_lens(&mut self, record: RecordHandle, lens: ResourceIndex) { store!(self, record, shared::group_27::AT_0720, lens.0); }
    fn store_reference(&mut self, record: RecordHandle, reference: u16) { store!(self, record, shared::group_27::AT_0718, reference as i16); }
    fn store_selection_reference(&mut self, record: RecordHandle, reference: u16) { store!(self, record, shared::group_27::AT_024C, reference as i16); }
    fn program_kind(&mut self, record: RecordHandle) -> u32 { self.shared(record, &shared::group_27::AT_0748) }
    fn store_program_kind(&mut self, record: RecordHandle, kind: u32) { store!(self, record, shared::group_27::AT_0748, kind); }
    fn stored_program(&mut self, record: RecordHandle) -> Program { Program(u32::from(self.shared(record, &shared::group_27::AT_0742))) }
    fn store_program(&mut self, record: RecordHandle, program: Program) { store!(self, record, shared::group_27::AT_0742, program.0 as u8); }
    fn store_program_word(&mut self, record: RecordHandle, word: [u32; 2]) {
        let mut bytes = [0; 8]; boundary::put_words(&mut bytes, word); store!(self, record, shared::group_37::AT_085C, bytes);
    }
    fn store_program_choice(&mut self, record: RecordHandle, choice: &ProgramChoice) {
        let mut bytes = self.shared_bytes(record, &shared::group_15::AT_074C); boundary::put_program_choice(&mut bytes, choice);
        store!(self, record, shared::group_15::AT_074C, bytes);
    }
    fn acquisition_block(&mut self, record: RecordHandle) -> AcquisitionBlock { boundary::acquisition_block(&self.shared_bytes(record, &shared::group_14::FOCUS_RESULT)) }
    fn store_acquisition_block(&mut self, record: RecordHandle, block: &AcquisitionBlock) {
        let mut bytes = self.shared_bytes(record, &shared::group_14::FOCUS_RESULT); boundary::put_acquisition_block(&mut bytes, block);
        store!(self, record, shared::group_14::FOCUS_RESULT, bytes);
    }
    fn selection_record(&mut self, record: RecordHandle) -> SelectionRecord { boundary::selection_record(&self.shared_bytes(record, &shared::group_41::AT_099C)) }
    fn store_selection_record(&mut self, record: RecordHandle, selection: &SelectionRecord) {
        let mut bytes = self.shared_bytes(record, &shared::group_41::AT_099C); boundary::put_selection_record(&mut bytes, selection);
        store!(self, record, shared::group_41::AT_099C, bytes);
    }
    fn selection_slot(&mut self, record: RecordHandle) -> SelectionSlot {
        use record::{group_05 as a, group_06 as b};
        let i = Index::new(0).unwrap();
        SelectionSlot { word: self.field(record, &a::AT_0170.at(i)), level: self.field(record, &a::AT_0176.at(i)) as u16,
            kind: self.field(record, &b::AT_01A0.at(i)), limit_level: self.field(record, &b::AT_01A4.at(i)) as u16,
            flags: [self.field(record, &b::AT_01A8.at(i)), self.field(record, &b::AT_01AC.at(i))],
            levels: [self.field(record, &a::AT_0178.at(i)) as u16, self.field(record, &a::AT_017A.at(i)) as u16, self.field(record, &a::AT_017C.at(i)) as u16, self.field(record, &a::AT_0186.at(i)) as u16] }
    }
    fn publish_selection_slot(&mut self, record: RecordHandle, slot: &SelectionSlot) {
        use record::{group_05 as a, group_06 as b};
        let i = Index::new(0).unwrap();
        let _ = self.records.write(record, &a::AT_0170.at(i), slot.word);
        let _ = self.records.write(record, &a::AT_0176.at(i), slot.level as i16);
        let _ = self.records.write(record, &b::AT_01A0.at(i), slot.kind);
        let _ = self.records.write(record, &b::AT_01A4.at(i), slot.limit_level as i16);
        let _ = self.records.write(record, &b::AT_01A8.at(i), slot.flags[0]); let _ = self.records.write(record, &b::AT_01AC.at(i), slot.flags[1]);
        let _ = self.records.write(record, &a::AT_0178.at(i), slot.levels[0] as i16); let _ = self.records.write(record, &a::AT_017A.at(i), slot.levels[1] as i16);
        let _ = self.records.write(record, &a::AT_017C.at(i), slot.levels[2] as i16); let _ = self.records.write(record, &a::AT_0186.at(i), slot.levels[3] as i16);
    }
    fn store_slot_word(&mut self, record: RecordHandle, word: u32) { let _ = self.records.write(record, &record::group_05::AT_0170.at(Index::new(0).unwrap()), word); }
    fn position_pair(&mut self, record: RecordHandle, slot: PositionSlot) -> [u32; 2] { boundary::words(&self.shared_bytes(record, &shared::group_41::AT_0974.at(slot_index(slot)))) }
    fn store_position_pair(&mut self, record: RecordHandle, slot: PositionSlot, pair: [u32; 2]) {
        let mut bytes = [0; 8]; boundary::put_words(&mut bytes, pair); let _ = self.records.write_shared(record, &shared::group_41::AT_0974.at(slot_index(slot)), bytes);
    }
    fn store_restore_levels(&mut self, record: RecordHandle, reference: u16, levels: [u16; 2]) {
        store!(self, record, shared::group_41::AT_0972, reference as i16); store!(self, record, shared::group_41::AT_096C, levels[0] as i16); store!(self, record, shared::group_41::AT_096E, levels[1] as i16);
    }
    fn store_s1_level(&mut self, record: RecordHandle, level: i16) { store!(self, record, shared::group_41::AT_0970, level); }
    fn clear_grid_selection(&mut self, record: RecordHandle) { store!(self, record, shared::group_39::AT_0898, [0; 48]); }
    fn rate_record(&mut self, record: RecordHandle) -> RateRecord { boundary::rate_record(&self.shared_bytes(record, &shared::group_27::AT_07EC)) }
    fn store_rate_selection(&mut self, record: RecordHandle, selected: u32) {
        let mut bytes = self.shared_bytes(record, &shared::group_27::AT_07EC); boundary::put_rate_selection(&mut bytes, selected); store!(self, record, shared::group_27::AT_07EC, bytes);
    }
    fn clear_selection_reuse(&mut self, record: RecordHandle) { store!(self, record, shared::group_27::AT_0744, 0); }
    fn clear_controller_reuse(&mut self) { let record = boundary::controller_record(&self.machine); self.clear_selection_reuse(record); }
    fn notify_selection(&mut self, record: RecordHandle, notice: &SelectionNotice) { boundary::enter_selection_hook(&self.machine, wire::StockCallbackWord(self.shared(record, &shared::group_20::AT_0158)), notice) }
    fn notify_rate(&mut self, record: RecordHandle, notice: &RateNotice) { boundary::enter_rate_hook(&self.machine, wire::StockCallbackWord(self.shared(record, &shared::group_20::AT_016C)), notice) }

    fn current_lens(&mut self) -> ResourceIndex { ResourceIndex(boundary::current_lens(&self.machine)) }
    fn lens_word(&mut self, record: RecordHandle, lens: Option<ResourceIndex>) -> u32 { boundary::lens_word(&self.machine, record, lens) }
    fn collapse_index(&mut self, exposure: PipelineWord) -> u32 { table::column(&self.stock, Step(exposure.0)) }
    fn mapped_value(&mut self, first: u32, second: u32) -> u32 { u32::from(table::value(&self.stock, Step(first), Row(second))) }
    fn lens_position(&mut self, pair: [u32; 2], position: u32) -> u32 { table::nearest_step(&self.stock, Row(pair[0]), Step(pair[1]), Row(position)).0 }
    fn gain_step_for(&mut self, offset: i32) -> GainStep { GainStep(u32::from(lens::zone(offset, &mut self.stock).get())) }
    fn acquisition_gain_step(&mut self, adjustment: u32) -> GainStep { GainStep(boundary::acquisition_gain_step(&self.machine, adjustment)) }
    fn kind_decade(&mut self, kind: u32) -> u32 { boundary::kind_decade(&self.machine, kind) }
    fn change_positions(&mut self, record: RecordHandle) -> [u32; 2] { [self.field(record, &record::group_04::AT_0164), self.field(record, &record::group_04::AT_0168)] }

    fn select_program(&mut self, inputs: &ProgramInputs) -> Option<Program> {
        let choice = programs::ProfileChoice { selection: selection(selection_word(inputs.selection)), result: inputs.adjustment, window_status: WindowStatus(inputs.kind), mode: inputs.mode.0,
            flagged: inputs.word == 1, banded: inputs.option == 1, percent: inputs.percent, held: inputs.bytes[0] != 0, held_variant: inputs.bytes[1] };
        programs::choose(&mut Calculations(&self.machine, &self.stock), &choice).map(|profile| Program(profile.id()))
    }
    fn program_adjustment(&mut self, p: Program) -> u32 { program(p).record().adjustment }
    fn program_range_limit(&mut self, p: Program) -> u32 { program(p).record().range_limit }
    fn program_policy(&mut self, p: Program) -> u32 { program(p).record().policy }
    fn program_secondary_gate(&mut self, p: Program) -> u32 { u32::from(program(p).record().secondary_gate) }
    fn program_scale(&mut self, p: Program) -> u32 { program(p).record().scale }
    fn program_percent(&mut self, p: Program) -> u16 { programs::selection_percent(&mut Calculations(&self.machine, &self.stock), program(p)) as u16 }
    fn program_target(&mut self, inputs: &TargetInputs) -> ProgramTarget { boundary::program_target(&self.machine, inputs) }
    fn program_range(&mut self, inputs: &RangeInputs) -> Range {
        let lower = self.program_lower_bound(inputs.program, inputs.lens);
        boundary::program_range(&self.machine, inputs, lower)
    }
    fn program_lower_bound(&mut self, p: Program, lens: u32) -> u32 { programs::lens_limits(&mut Calculations(&self.machine, &self.stock), program(p), lens).near }
    fn profile_percent(&mut self, p: Program, combined: u16, lens: ResourceIndex, bound: u32) -> u16 { programs::choose_percent(&mut Calculations(&self.machine, &self.stock), program(p), i32::from(combined as i16), lens.0, bound) }

    fn grid_count(&mut self, selection: ProfileSelection, mode: AcquisitionMode, adjustment: u32, grid_kind: u32) -> u32 { boundary::grid_count(&self.machine, selection_word(selection), mode.0, adjustment, grid_kind) }
    fn indexed_configuration(&mut self, selection: ProfileSelection, adjustment: u32, index: u32) -> u8 { rate_policy::policy_setting(&mut Calculations(&self.machine, &self.stock), selection == ProfileSelection::Record, adjustment, index == 1) as u8 }
    fn level_category(&mut self, level: i16, selection: ProfileSelection, adjustment: u32) -> u32 { boundary::level_category(&self.machine, level, selection_word(selection), adjustment) }
    fn selection_limits(&mut self, selection: ProfileSelection, adjustment: u32, grid_count: u32, request: u32) -> SelectionLimits {
        let limits = rate_policy::rate_settings(&mut Calculations(&self.machine, &self.stock), selection == ProfileSelection::Record, adjustment, grid_count != 0, request == 2);
        SelectionLimits { step: limits.base, lower: limits.second as u16, upper: limits.first as u16, byte: limits.byte }
    }
    fn grid_lead(&mut self, branch: Branch) -> codec::GridLead { codec::grid_lead(&self.acquisition.grids[branch_index(branch)]) }
    fn set_grid_count(&mut self, branch: Branch, count: u32) { codec::set_grid_count(&mut self.acquisition.grids[branch_index(branch)], count); }
    fn clear_grid_lead(&mut self, branch: Branch) { codec::clear_grid_lead(&mut self.acquisition.grids[branch_index(branch)]); }
    fn set_grid_configuration(&mut self, branch: Branch, configuration: u8) { codec::set_grid_configuration(&mut self.acquisition.grids[branch_index(branch)], configuration); }
    fn set_grid_inputs(&mut self, branch: Branch, inputs: &GridInputs) { codec::set_grid_inputs(&mut self.acquisition.grids[branch_index(branch)], inputs.timing.0, inputs.lens_word, inputs.positions, inputs.window); }
    fn fill_grid(&mut self, branch: Branch, inputs: &GridInputs) { boundary::fill_grid(&self.machine, inputs, &mut self.acquisition.grids[branch_index(branch)]); }
    fn snapshot_grid(&mut self, branch: Branch, statistics: Statistics) { codec::grid_from_snapshot(&mut self.acquisition.grids[branch_index(branch)], &boundary::snapshot(&self.machine, statistics)); }
    fn adjustment_class(&mut self, adjustment: u32, reference: i16) -> u8 { rate_policy::level_selection(adjustment, reference) }
    fn adjustment_option(&mut self) -> u32 { u32::from(self.configuration.refresh()) }
    fn refreshed_position(&mut self) -> u32 { boundary::refreshed_position(&self.machine) }
    fn mode_one(&mut self, inputs: &ModeInputs) -> u32 { u32::from(treatments(inputs).admits(xm1_saclac_calc::adjust::scene_policy::Treatment::First)) }
    fn both_modes(&mut self, inputs: &ModeInputs) -> u32 { u32::from(treatments(inputs).admits_any()) }
    fn five_parameter_adjustment(&mut self, selection: ProfileSelection, position: u32, mode_flag: u32, modes: u32, grid_word: u32) -> u32 { boundary::five_parameter_adjustment(&self.machine, [selection_word(selection), position, mode_flag, modes, grid_word]) }
    fn rate_divisor(&mut self, position: u32, category: u32, lens: ResourceIndex, halfword: u16) -> u32 { boundary::rate_divisor(&self.machine, position, category, lens, halfword) }
    fn rate_limit(&mut self, selection: ProfileSelection, position: u32, halfword: u16) -> u32 { boundary::rate_limit(&self.machine, selection_word(selection), position, halfword) }
    fn rate_mode(&mut self, position: u32, halfword: u16, word: u32, mode_flag: u32) -> u16 { boundary::rate_mode(&self.machine, [position, u32::from(halfword), word, mode_flag]) }
    fn selection_mode_category(&mut self, mode_word: u32) -> u32 { u32::from(xm1_saclac_calc::policy::SelectionMode(mode_word).category()) }
    fn offset_position(&mut self, level: i16, reference: u16, flag: u32) -> u16 { xm1_saclac_calc::policy::offset_position(level, i32::from(reference), flag == 1) as u16 }
    fn clamp_selection_difference(&mut self, mode: AcquisitionMode, lens: ResourceIndex, level: i16, halfword: u16, reference: u32, target: i16) -> i16 { boundary::clamp_selection_difference(&self.machine, mode.0, lens, level, halfword, reference, target) }
    fn has_valid_offset(&mut self, selection: ProfileSelection, mode: AcquisitionMode, halfword: i16) -> u32 { u32::from(sample_grid::scene_offset_valid(selection_word(selection), mode.0, halfword)) }
    fn allows_unflagged_selection(&mut self, position: u32, halfword: u16) -> bool { boundary::allows_unflagged_selection(&self.machine, position, halfword) }
    fn capture_selection_position(&mut self, branch: Branch, inputs: &PositionInputs) -> SelectionPosition { boundary::capture_selection_position(&self.machine, boundary::region(branch).setting, branch, inputs) }
    fn copy_rate_grid_source(&mut self, buffer: StatisticsBuffer) { boundary::copy_rate_grid_source(&self.machine, buffer) }
    fn rate_pair(&mut self, record: RecordHandle, result: SharedAddress, flag: u32, pair: &mut [u32; 2]) { boundary::rate_pair(&self.machine, record, result, flag, pair) }
    fn clear_rate_configuration(&mut self, record: RecordHandle) {
        xm1_focus_measurement_round::profile::ProfileRecords::set_interval_history(self, record, Default::default());
    }
    fn rate_flag(&mut self, record: RecordHandle, flag: u32) { boundary::rate_flag(&self.machine, record, flag) }
    fn draw_rate(&mut self, result: SharedAddress, flag: u32) { boundary::draw_rate(&self.machine, result, flag) }
    fn draw_rate_status(&mut self, level: u16, word: u32, flag: u32) { boundary::draw_rate_status(&self.machine, level, word, flag) }
    fn rate_position(&mut self, record: RecordHandle) -> (u32, u16) { boundary::rate_position(&self.machine, record, &self.shared_bytes(record, &shared::group_27::AT_07EC)) }

    fn statistics_buffer(&mut self, buffer: StatisticsBuffer) -> [u16; 256] { boundary::statistics_values(&self.machine, buffer) }
    fn extra_statistics(&mut self, values: &mut [u16]) { boundary::extra_statistics(&self.machine, values) }
    fn store_release(&mut self, release: &Release<'_>) { boundary::store_release(&mut self.machine, release) }
    fn set_pipeline_reference(&mut self, level: i16) { boundary::set_pipeline_reference(&self.machine, level) }
    fn prepare_s1_sample(&mut self) { boundary::prepare_s1_sample(&self.machine) }
    fn s1_sample(&mut self) -> PipelineSnapshot { xm1_focus_measurement_round::stock::s1_sample(&boundary::s1_sample(&self.machine)) }
    fn post_change_notice(&mut self, notice: &ChangeNotice) {
        let bytes = codec::change_notice(notice);
        if ImgDc::request(&mut self.peers, ImgDcRequest { operation: ImgDcOperation::Request2d, parameters: wire::parameters(&bytes), completions: &[] }).is_err() { boundary::fault(&self.machine); }
    }
    fn post_selection_table(&mut self, table: &SelectionTable) {
        boundary::place(&mut self.machine, boundary::SELECTION_TABLE, &codec::selection_table(table));
        if ImgDc::request(&mut self.peers, ImgDcRequest { operation: ImgDcOperation::Request2c, parameters: wire::parameters(&[]), completions: &[] }).is_err() { boundary::fault(&self.machine); }
    }
    fn post_focus_table(&mut self, table: &FocusTable) {
        boundary::place(&mut self.machine, boundary::FOCUS_TABLE, &codec::focus_table(table));
        if ImgDc::request(&mut self.peers, ImgDcRequest { operation: ImgDcOperation::Request45, parameters: wire::parameters(&[]), completions: &[] }).is_err() { boundary::fault(&self.machine); }
    }

    fn shot_reference_level(&mut self) -> u16 { boundary::shot_reference_level(&self.machine) }
    fn shot_selection_word(&mut self) -> u32 { boundary::shot_selection_word(&self.machine) }
    fn prepare_selection_defaults(&mut self, record: RecordHandle) -> ShotContext { boundary::prepare_selection_defaults(&self.machine, record) }
    fn restore_selection_setting(&mut self, branch: Branch, lens: ResourceIndex) { boundary::restore_selection_setting(&self.machine, branch, lens) }
    fn copy_reference_backup(&mut self, branch: Branch) { boundary::copy_reference_backup(&self.machine, branch) }
    fn write_saved_settings(&mut self, record: SharedAddress) { boundary::write_saved_settings(&self.machine, record) }
    fn copy_selection_to_saved(&mut self, record: SharedAddress) { boundary::copy_selection_to_saved(&self.machine, record) }
    fn restore_selection_if_enabled(&mut self, record: RecordHandle, position: u32) { boundary::restore_selection_if_enabled(&self.machine, record, position) }
    fn apply_selection_value_change(&mut self) { boundary::apply_selection_value_change(&self.machine) }
    fn save_selection(&mut self, position: u32, saved: &SavedSelection, lens_position: u32) { boundary::save_selection(&self.machine, position, saved, lens_position) }
    fn selected_grid_reference(&mut self, record: RecordHandle, reference: GridReference) -> GridReference { boundary::selected_grid_reference(&self.machine, record, &self.shared_bytes(record, &shared::group_27::AT_07EC), reference) }

    fn refresh_hardware_state(&mut self) { boundary::refresh_hardware_state(&self.machine) }
    fn hardware_ready(&mut self) -> bool { boundary::hardware_ready(&self.machine) }
    fn clock_word(&mut self) -> u32 { boundary::clock_word(&self.machine) }
    fn store_selection_window(&mut self, branch: Branch, window: &SelectionWindow) {
        boundary::place(&mut self.machine, boundary::grid_address(branch).0, &self.acquisition.grids[branch_index(branch)].0);
        codec::selection_window(window, boundary::grid_address(branch), |at, bytes| boundary::write_record(&mut self.machine, boundary::region(branch).window, at, bytes));
    }
    fn store_selection_setting(&mut self, branch: Branch, setting: &SelectionSetting) {
        let address = boundary::region(branch).setting;
        codec::selection_setting(setting, address, |at, bytes| boundary::write_record(&mut self.machine, address, at, bytes));
    }
    fn store_restored_setting(&mut self, branch: Branch, setting: &RestoredSetting) {
        codec::restored_setting(setting, |at, bytes| boundary::write_record(&mut self.machine, boundary::region(branch).setting, at, bytes));
    }
    fn setting_echo(&mut self, branch: Branch) -> SettingEcho {
        let (ready, updated_position, position_halfword, shot_word) = codec::setting_echo(&boundary::read_record(&self.machine, boundary::region(branch).setting));
        SettingEcho { ready, updated_position, position_halfword, shot_word }
    }
    fn request_acquisition(&mut self, branch: Branch, request: &AcquisitionRequest, grid: EvaluationGrid) {
        let grid = match grid { EvaluationGrid::Current => &self.acquisition.grids[branch_index(branch)], EvaluationGrid::Empty => &EMPTY_GRID };
        let mut peers = CalculationPeers { machine: &mut self.machine, peers: &mut self.peers };
        if xm1_saclac_calc::requests::request_acquisition(&mut peers, grid, *request, boundary::acquisition_callback(branch).0).is_err() { boundary::fault(&self.machine); }
    }
    fn request_selection(&mut self, branch: Branch, request: &SelectionRequest) {
        let payload = Calc1Payload::Selection { request: *request, region: boundary::region(branch), callback: boundary::selection_callback(branch).0 };
        if xm1_saclac_calc::requests::Peers::calc1(self, &payload).is_err() { boundary::fault(&self.machine); }
    }
    fn acquisition_result(&mut self, completed: Completed<'_>) -> Option<AcquisitionResult> {
        let address = boundary::result_address(completed.parameters.bytes())?;
        Some(codec::acquisition_result(address, &boundary::read_record(&self.machine, address)))
    }
    fn selection_result(&mut self, completed: Completed<'_>) -> Option<SelectionResult> {
        let address = boundary::result_address(completed.parameters.bytes())?;
        Some(codec::selection_result(address, &boundary::read_record(&self.machine, address)))
    }
    fn shot_record_levels(&mut self, record: SharedAddress) -> ShotRecordLevels { codec::shot_record_levels(&boundary::read_record(&self.machine, record)) }
    fn saved_exposure(&mut self, record: SharedAddress) -> SavedExposure { codec::saved_exposure(&boundary::read_record(&self.machine, record)) }
    fn publish_saved_exposure(&mut self, record: RecordHandle, exposure: &SavedExposure) {
        store!(self, record, shared::group_27::AT_0708, exposure.exposure); store!(self, record, shared::group_27::AT_070C, exposure.level as i16);
        store!(self, record, shared::group_27::AT_070E, exposure.second as i16); store!(self, record, shared::group_27::AT_0712, exposure.aspect as i16);
        store!(self, record, shared::group_27::AT_0716, exposure.offset as i16);
    }
    fn release_result(&mut self, released: ReleasedResult) {
        match released {
            ReleasedResult::Acquisition(address) => boundary::release(&self.machine, address, codec::ACQUISITION_RESULT_BYTES),
            ReleasedResult::Selection { result, shot } => {
                boundary::release(&self.machine, result, codec::SELECTION_RESULT_BYTES);
                if let Some((shot, saved)) = shot { boundary::release(&self.machine, shot, boundary::SHOT_RECORD_BYTES); boundary::release(&self.machine, saved, boundary::SAVED_RECORD_BYTES); }
            }
        }
    }
}

struct CalculationPeers<'a, M, P> { machine: &'a mut M, peers: &'a mut P }

impl<M: Machine, P: Calc + Calc1> xm1_saclac_calc::requests::Peers for CalculationPeers<'_, M, P> {
    type Error = ();
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), ()> {
        let (operation, bytes) = wire::calc_payload(payload);
        Calc::request(self.peers, CalcRequest { operation, parameters: wire::parameters(bytes.bytes()), completions: &[] }).map_err(|_| ())
    }
    fn calc1(&mut self, payload: &Calc1Payload) -> Result<(), ()> {
        let (operation, bytes) = wire::calc1_payload(payload);
        Calc1::request(self.peers, Calc1Request { operation, parameters: wire::parameters(bytes.bytes()), completions: &[] }).map_err(|_| ())
    }
    fn store_acquisition_grid(&mut self, grid: &AcquisitionGrid) {
        boundary::place(self.machine, boundary::CALC1_GRID.0, &grid.0);
        boundary::clean(self.machine, boundary::CALC1_GRID.0, wire::ACQUISITION_GRID.2);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1> xm1_saclac_calc::requests::Peers for Services<M, R, E, P> {
    type Error = ();
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), ()> {
        xm1_saclac_calc::requests::Peers::calc(&mut CalculationPeers { machine: &mut self.machine, peers: &mut self.peers }, payload)
    }
    fn calc1(&mut self, payload: &Calc1Payload) -> Result<(), ()> {
        xm1_saclac_calc::requests::Peers::calc1(&mut CalculationPeers { machine: &mut self.machine, peers: &mut self.peers }, payload)
    }
    fn store_acquisition_grid(&mut self, grid: &AcquisitionGrid) {
        xm1_saclac_calc::requests::Peers::store_acquisition_grid(&mut CalculationPeers { machine: &mut self.machine, peers: &mut self.peers }, grid)
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1 + ImgDc> xm1_focus_measurement_round::children::Ports for Services<M, R, E, P> {
    fn fault(&mut self) { boundary::fault(&self.machine) }
}
