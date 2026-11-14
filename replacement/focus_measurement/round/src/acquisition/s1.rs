#![forbid(unsafe_code)]

//! The S1 acquisition: calc1's primary evaluation over the S1 sample on each pass, then one
//! selection whose result restores the shot's saved selection, publishes the record's selection
//! and posts the focus table to imgdc.

use crate::auxiliary::ResourceIndex;
use crate::pipeline::{GainStep, PipelineWord};
use crate::record::TraceMark;
use xm1_saclac_interface::calc::AcquisitionGrid;
use xm1_saclac_interface::round_acquisition::{
    AcquisitionRequest, AcquisitionSample, FocusTable, RestoredSetting, SelectionEntry, SelectionRequest, SelectionS1,
};
use xm1_saclac_interface::{Completed, RecordHandle};

use super::live::{acquisition_profile, fill_limits, fill_selection};
use super::ports::{GridInputs, GridReference, PositionSlot, Ports, Program, ProgramInputs, RateNotice, ReleasedResult};
use super::selection::{self, LensPreset, Publication};
use super::Branch;

/// The gain step the S1 acquisition's level and sample are taken at.
const GAIN_STEP: GainStep = GainStep(3);

/// The S1 acquisition's state between machine actions.
pub struct S1Work {
    level: u16,
    rate_pair: [u32; 2],
    primary_pending: bool,
    selection_pending: bool,
    /// The latest selection request's program.
    program: Program,
    grid: AcquisitionGrid,
}

impl Default for S1Work { fn default() -> Self { Self::new() } }

impl S1Work {
    pub const fn new() -> Self {
        Self { level: 0, rate_pair: [0; 2], primary_pending: false, selection_pending: false, program: Program(0), grid: AcquisitionGrid([0; 1236]) }
    }

    /// Takes the record's level from its saved S1 exposure.
    pub fn initialize(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        let value = ports.exposure_value([view.s1_exposure, PipelineWord(0)]) as u16;
        let gain = ports.gain_offset(GAIN_STEP) as u16;
        self.level = value.wrapping_add(gain).wrapping_add(view.s1_level).wrapping_sub(view.s1_second);
        self.rate_pair = [u32::MAX; 2];
        ports.store_s1_level(record, self.level as i16);
        self.primary_pending = true;
        self.selection_pending = true;
    }

    /// Calc1 0x3d00 over the S1 sample with an empty grid.
    pub fn submit_primary(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        ports.prepare_s1_sample();
        let view = ports.record_view(record);
        if view.request == 6 { ports.request_mode_six_statistics(); }
        let sample = ports.s1_sample();
        let gain_offset = ports.gain_offset(GAIN_STEP) as i32;
        let gain_step = ports.gain_step_for(gain_offset);
        let reference_level = sample.levels[1].wrapping_sub(ports.gain_offset(gain_step) as u16);
        let value = ports.exposure_value(sample.exposure) as u16;
        let relative_level = sample.levels[0]
            .wrapping_sub(reference_level.wrapping_add(sample.offset_level))
            .wrapping_sub(view.decade)
            .wrapping_add(value);
        let correction = ports.published_exposure(record).correction;
        let request = AcquisitionRequest {
            mode: view.s1_mode,
            first: self.primary_pending,
            profile: acquisition_profile(&view, 0),
            sample: AcquisitionSample {
                complete: sample.status.complete(),
                exposure: sample.exposure[0].0,
                level: sample.levels[0],
                reference_level,
                gain_step: gain_step.0,
                offset_level: sample.offset_level,
                adjustment: 0,
                timing: sample.timing.0,
                halfword: sample.reference,
                extent: sample.extent.map(|word| word.0),
                size: sample.size.map(|word| word.0),
                origin: sample.origin.map(|word| word.0),
            },
            relative_level,
            configuration: ports.indexed_configuration(view.selection, view.adjustment, 0),
            level_category: ports.level_category(sample.levels[0] as i16, view.selection, view.adjustment),
            correction,
            aspect: ports.timing_offset(sample.timing),
            decade: view.decade,
        };
        ports.request_acquisition(Branch::S1, &request, &AcquisitionGrid([0; 1236]));
    }

    /// Takes the result's level and, under the S1 rate, hands it to the rate layer and the
    /// record's rate hook.
    pub fn collect_primary(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) {
        self.primary_pending = false;
        let Some(result) = ports.acquisition_result(completed) else { return };
        ports.store_s1_level(record, result.levels.base as i16);
        let settings = ports.acquisition_settings();
        if settings.s1_rate & 3 != 0 && ports.record_view(record).rate_enabled == 1 {
            let rate = ports.rate_record(record);
            ports.store_rate_selection(record, 1);
            if settings.s1_rate & 2 != 0 { ports.rate_pair(record, result.address, 1, &mut self.rate_pair); }
            ports.rate_flag(record, 1);
            if settings.draws_rate { ports.draw_rate(result.address, 1); }
            if settings.rate_status { ports.draw_rate_status(rate.level, rate.word, 1); }
            let (position, feature) = ports.rate_position(record);
            let reference = ports.selected_grid_reference(record, GridReference::default());
            ports.notify_rate(record, &RateNotice {
                position,
                feature,
                levels: [reference.level as i16, reference.second as i16],
                word: reference.word,
            });
        }
        let mut block = ports.acquisition_block(record);
        block.eligible = 1;
        ports.store_acquisition_block(record, &block);
        ports.release_result(ReleasedResult::Acquisition(result.address));
    }

    /// Calc1 0x3d01 with the window and a setting the shot restores from its saved reference.
    pub fn submit_secondary(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        let mut request = SelectionRequest::default();
        fill_selection(&mut request, &view, view.s1_mode, ports);
        request.offset_mode = ports.has_valid_offset(view.selection, view.s1_mode, view.lens_halfword as i16);
        request.program_word = view.program_word[0];
        if view.program_word[0] == 0 { ports.store_program_word(record, [0, 0]); }
        let settings = ports.acquisition_settings();
        request.rate_flag = settings.rate & 3 != 0 && view.rate_enabled != 0;
        let sample = ports.s1_sample();
        let current = ports.position_pair(record, PositionSlot::Current);
        ports.store_lens(record, ResourceIndex(current[0]));
        request.summary_level = self.level;
        let published = ports.published_exposure(record);
        request.lens = published.lens.0;
        request.exposure = sample.exposure[0].0;
        request.gain_step = published.gain_step.0;
        request.correction = published.correction;
        request.aspect = ports.timing_offset(sample.timing);
        let pair = ports.position_pair(record, PositionSlot::Lens);
        request.s1 = SelectionS1 {
            byte: view.s1_byte,
            halfword: view.s1_halfword,
            margin: selection::margin(record, ports),
            position: ports.lens_position(pair, current[0]),
        };
        fill_limits(&mut request, &view, ports);
        request.resource_level = ports.relative_exposure(PipelineWord(published.lens.0)) as u16;
        let program = ports.select_program(&ProgramInputs {
            selection: request.selection,
            adjustment: request.adjustment,
            kind: request.kind,
            mode: request.mode,
            word: request.program_word,
            flag: request.offset_mode,
            program_flag: request.program_flag,
            option: 0,
            policy: false,
            percent: 0,
            bytes: [1, 0],
        }).unwrap_or(self.program);
        self.program = program;
        request.program = program.0;
        request.first = self.selection_pending;
        if request.selection == 0 { ports.refresh_hardware_state(); }
        let inputs = GridInputs {
            timing: sample.timing,
            lens_word: ports.lens_word(record, Some(published.lens)),
            positions: ports.change_positions(record),
            window: view.window,
        };
        ports.fill_grid(&inputs, &mut self.grid);
        request.eligible = 1;
        selection::store_records(record, Branch::S1, &request, &self.grid, ports);
        request.window = true;
        let lens = ports.published_exposure(record).lens;
        ports.restore_selection_setting(Branch::S1, lens);
        let margin = selection::margin(record, ports);
        ports.store_restored_setting(Branch::S1, &RestoredSetting {
            margin,
            reference: request.reference,
            lens_halfword: request.lens_halfword,
            lens_reference: request.lens_reference,
        });
        ports.clear_selection_reuse(record);
        ports.copy_reference_backup(Branch::S1);
        request.shot_copy = true;
        request.recording = true;
        ports.selection_memory().set_preset(LensPreset { word: request.lens_reference, halfwords: [request.lens_halfword, request.reference] });
        if settings.traces { ports.trace(TraceMark::SelectionRequest); }
        ports.request_selection(Branch::S1, &request);
    }

    /// Hands the result's shot records to the shot and publishes the exposure the shot saved.
    pub fn restore(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) {
        self.selection_pending = false;
        let Some(result) = ports.selection_result(completed) else { return };
        let selection = ports.selection_record(record);
        if result.shot_data {
            ports.write_saved_settings(result.shot_record);
            if selection::changed(record, &selection, &result, ports) {
                ports.copy_selection_to_saved(result.saved_record);
                ports.restore_selection_if_enabled(record, result.position_word);
                if ports.selection_memory().value_changed() { ports.apply_selection_value_change(); }
            } else {
                ports.save_selection(result.position_word, &result.save, result.position);
            }
            let saved = ports.saved_exposure(result.saved_record);
            ports.publish_saved_exposure(record, &saved);
            ports.store_restore_levels(record, result.table_reference, result.restore_levels);
        }
        let shot = result.shot_data.then_some((result.shot_record, result.saved_record));
        ports.release_result(ReleasedResult::Selection { result: result.address, shot });
    }

    /// Takes the result into the record's selection and publishes it.
    pub fn publish_selection(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) {
        let Some(result) = ports.selection_result(completed) else { return };
        let mut selection = ports.selection_record(record);
        if selection::changed(record, &selection, &result, ports) {
            selection::take_result(record, &mut selection, &result, true, ports);
            ports.store_position_pair(record, PositionSlot::Selected, [selection.position, selection.word]);
        } else {
            selection::take_result(record, &mut selection, &result, false, ports);
        }
        ports.selection_memory().save_preset();
        ports.store_position_pair(record, PositionSlot::Reported, [result.position_word, result.position]);
        selection::publish(record, Publication::Restored, selection, ports);
        ports.release_result(ReleasedResult::Selection { result: result.address, shot: None });
    }

    /// Posts the result's last candidate under each program, at the S1 sample's timing, to imgdc.
    pub fn publish_focus(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) {
        let Some(result) = ports.selection_result(completed) else { return };
        let sample = ports.s1_sample();
        let view = ports.record_view(record);
        let gain_step = ports.acquisition_gain_step(result.adjustment);
        let decade = ports.kind_decade(result.kind);
        let program = super::exposure::chosen_program(record, &ProgramInputs {
            selection: view.selection,
            adjustment: result.adjustment,
            kind: result.kind,
            mode: view.s1_mode,
            word: result.program_word,
            flag: result.program_flag,
            program_flag: view.program_flag(),
            option: 0,
            policy: false,
            percent: 0,
            bytes: [1, 0],
        }, ports);
        ports.store_program(record, Program(program.0 & 0xff));
        let gain = ports.gain_offset(gain_step) as u16;
        let entry = |program: &xm1_saclac_interface::round_acquisition::SelectionProgram| SelectionEntry {
            timing: sample.timing.0,
            code: sample.code,
            selected: sample.flags,
            gain_step: gain_step.0,
            exposure: program.exposure,
            level: program.levels[3],
            second: program.first[3].wrapping_add(gain),
            offset: program.second[3],
            word: 0,
            reference: result.table_reference,
            decade,
        };
        ports.post_focus_table(&FocusTable { primary: entry(&result.primary), secondary: entry(&result.secondary) });
        ports.release_result(ReleasedResult::Selection { result: result.address, shot: None });
    }

    /// Hands the record's lens reference to the selection slot for the tracking modes and forgets
    /// the controllers' value change.
    pub fn stop(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        if matches!(view.s1_mode, 46 | 47 | 54 | 56) { ports.store_slot_word(record, view.lens_reference); }
        ports.selection_memory().clear_value_changed();
    }
}
