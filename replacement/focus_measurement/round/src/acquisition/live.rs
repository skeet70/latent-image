#![forbid(unsafe_code)]

//! The live acquisition: the exposure it starts from, calc1's primary evaluation and secondary
//! selection over the primary statistics, the selection table it publishes to imgdc, and the
//! statistics it hands on when it stops.

use crate::auxiliary::ResourceIndex;
use crate::pipeline::{PipelineWord, Statistics, TimingWord};
use crate::record::TraceMark;
use xm1_saclac_interface::round_acquisition::{
    AcquisitionProfile, AcquisitionRequest, AcquisitionResult, AcquisitionSample, ChangeNotice, RestoredSetting,
    SelectionEntry, SelectionRequest, SelectionResult, SelectionTable,
};
use xm1_saclac_interface::{Completed, RecordHandle};

use super::exposure::{self, ExposureSetting, ExposureSummary};
use super::ports::{
    AcquisitionMode, AcquisitionSettings, ExposureRecordUpdate, GridInputs, ModeInputs, PositionSlot, Ports, Program, ProgramChoice,
    ProgramInputs, Range, RangeInputs, RecordView, Release, ReleasedResult, SnapshotRelease, Start, StatisticsBuffer,
    TargetInputs,
};
use super::selection::{self, LensPreset, Publication};
use super::statistics::{classify, offset_values, release_offset, StatisticsSet, VALUES};
use super::Branch;
use crate::profile::{ProfileSelection, StatisticsInUse};
use crate::stock::profile_selection_byte;

/// Extra statistics the release buffer holds.
pub const EXTRA_CAPACITY: usize = 552;

/// The lens, mode and position words whose change the acquisition reports to imgdc.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ChangeRecord {
    category: u32,
    mode_is_one: u32,
    lens: u32,
    unbounded: u32,
    pair: [u32; 2],
    kind: u32,
}

/// What the latest selection request's selection gate decided and left for the next.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Guard {
    eligible: u32,
    reuse: u32,
    lens: ResourceIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Gate { Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Refresh { Started, Released }

/// The live acquisition's state between machine actions.
pub struct LiveWork {
    summary: ExposureSummary,
    statistics: StatisticsSet,
    change: ChangeRecord,
    selection: SelectionRequest,
    guard: Guard,
    program_changed: bool,
    recording_selection: bool,
    /// The percent and policy of the latest selection request's program.
    program_tail: (u16, bool),
    timing: TimingWord,
    changed: bool,
}

impl Default for LiveWork { fn default() -> Self { Self::new() } }

impl LiveWork {
    pub const fn new() -> Self {
        Self {
            summary: ExposureSummary {
                base: 0,
                exposure: PipelineWord(0),
                level: 0,
                second: 0,
                offset: 0,
                aspect: 0,
                correction: 0,
                timing: TimingWord(0),
                code: 0,
                selected: 0,
                rate_pair: [0; 2],
            },
            statistics: StatisticsSet {
                buffers: [StatisticsBuffer(0); 4],
                exposure: PipelineWord(0),
                level: 0,
                offset: 0,
                aspect: 0,
                halfword: 0,
                gain_offset: 0,
                correction: 0,
                count: 0,
                selected: 0,
            },
            change: ChangeRecord { category: 0, mode_is_one: 0, lens: 0, unbounded: 0, pair: [0; 2], kind: 0 },
            selection: empty_selection(),
            guard: Guard { eligible: 0, reuse: 0, lens: ResourceIndex(0) },
            program_changed: false,
            recording_selection: false,
            program_tail: (0, false),
            timing: TimingWord(0),
            changed: false,
        }
    }

    /// The timing the acquisition last settled on; planning reads its frame interval.
    pub const fn timing(&self) -> TimingWord { self.timing }

    /// Whether the latest secondary collection changed the selection table.
    pub const fn changed(&self) -> bool { self.changed }

    pub fn initialize(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        match view.start {
            Start::Restore => self.restore(record, &view, ports),
            Start::Fresh => self.fresh(record, &view, ports),
            Start::Resume => self.resume(record, &view, ports),
        }
    }

    pub fn enable(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let in_use = ports.statistics_in_use();
        ports.set_statistics_in_use(record, StatisticsInUse { primary: true, ..in_use });
    }

    fn fresh(&mut self, record: RecordHandle, view: &RecordView, ports: &mut impl Ports) {
        let lens = ports.current_lens();
        ports.store_lens(record, lens);
        let program = exposure::chosen_program(record, &exposure::initial_program(view, view.kind), ports);
        let lens = ports.published_exposure(record).lens;
        let gain_step = ports.acquisition_gain_step(view.adjustment);
        let gain = ports.gain_offset(gain_step) as u16;
        let top = view.decade.wrapping_add(10u16.wrapping_mul(u16::from(ports.acquisition_settings().top_steps)));
        let wide = view.selection == ProfileSelection::Record && view.mode.wide();
        let target = ports.program_target(&TargetInputs {
            program,
            reference: view.reference,
            lens,
            level: top as i16,
            gain_offset: gain as i16,
            lens_reference: wide.then_some(view.lens_reference),
            gain_level: gain,
            offset: 0,
        });
        let range = ports.program_range(&RangeInputs {
            program,
            exposure: target.exposure,
            level: target.level,
            first: target.second,
            second: target.offset,
            adjustment: 0,
            mode: view.kind,
            argument: 0,
            forced: view.forced,
            primary: 0,
            lens: lens.0,
            alternate: 0,
            request: view.request,
        });
        let percent = ports.program_percent(program);
        self.settle(target.base, target.exposure, &range, percent, ports);
        exposure::publish(record, view, &ExposureSetting {
            exposure: target.exposure,
            level: range.level,
            second: range.first,
            reference: view.reference,
            decade: view.decade,
            offset: range.second,
            correction: percent,
            gain_step,
            timing: range.timing,
            code: range.code,
            selected: range.selected,
            adjustment: target.adjustment_flag,
            second_exposure: PipelineWord(lens.0),
        }, ports);
        ports.store_selection_reference(record, 0);
        ports.store_reference(record, view.reference);
        ports.store_program_kind(record, 0);
        self.program_changed = false;
        self.publish_initial_levels(record, view, target.base, target.exposure, ports);
        let current = ports.current_lens();
        self.change = self.change_record(record, view, current, ports);
        self.change.unbounded = ports.lens_word(record, None);
        self.reset_guard(record, view, ports);
    }

    fn restore(&mut self, record: RecordHandle, view: &RecordView, ports: &mut impl Ports) {
        let lens = ports.current_lens();
        let gain_step = ports.acquisition_gain_step(view.adjustment);
        let published = ports.published_exposure(record);
        let base_gain = ports.gain_offset(published.gain_step) as u16;
        let slot = ports.selection_slot(record);
        let value = ports.exposure_value([published.exposure, PipelineWord(lens.0)]) as u16;
        let start_level = published.level
            .wrapping_sub(published.second.wrapping_add(published.offset))
            .wrapping_add(value)
            .wrapping_sub(published.aspect);
        let kind = if view.selection == ProfileSelection::Record && ports.adjustment_words().controllers.0 == 1 && view.kind.wrapping_sub(1) <= 1 {
            view.kind_override
        } else {
            view.kind
        };
        let program = exposure::chosen_program(record, &exposure::initial_program(view, kind), ports);
        let range_inputs = |exposure, level, first, second, adjustment| RangeInputs {
            program,
            exposure,
            level,
            first,
            second,
            adjustment,
            mode: kind,
            argument: 0,
            forced: view.forced,
            primary: 0,
            lens: lens.0,
            alternate: 0,
            request: view.request,
        };
        let first = ports.program_range(&range_inputs(published.exposure, published.level, published.second, published.offset, published.aspect as i16));
        let percent = ports.program_percent(program);
        exposure::publish(record, view, &ExposureSetting {
            exposure: published.exposure,
            level: first.level,
            second: first.first,
            reference: view.reference,
            decade: view.decade,
            offset: first.second,
            correction: percent,
            gain_step,
            timing: first.timing,
            code: first.code,
            selected: first.selected,
            adjustment: 0,
            second_exposure: PipelineWord(lens.0),
        }, ports);
        let aspect = ports.published_exposure(record).aspect;
        let level = ports.offset_position(start_level as i16, view.reference, 1).wrapping_sub(aspect);
        let gain = ports.gain_offset(gain_step) as u16;
        let wide = ports.program_adjustment(program) == 1 || ports.program_range_limit(program) == 46;
        let target = ports.program_target(&TargetInputs {
            program,
            reference: view.reference,
            lens,
            level: level as i16,
            gain_offset: base_gain as i16,
            lens_reference: wide.then_some(view.lens_reference),
            gain_level: gain,
            offset: first.second,
        });
        let step = ports.gain_step_for(i32::from(target.gain_level as i16));
        let range = ports.program_range(&range_inputs(target.exposure, target.level, target.second, target.offset, target.adjustment as i16));
        exposure::publish(record, view, &ExposureSetting {
            exposure: target.exposure,
            level: range.level,
            second: range.first,
            reference: view.reference,
            decade: view.decade,
            offset: range.second,
            correction: percent,
            gain_step: step,
            timing: range.timing,
            code: range.code,
            selected: range.selected,
            adjustment: target.adjustment_flag,
            second_exposure: PipelineWord(lens.0),
        }, ports);
        self.settle(target.base, target.exposure, &range, percent, ports);
        ports.store_selection_reference(record, 0);
        let current = ports.current_lens();
        ports.store_lens(record, current);
        ports.store_reference(record, view.reference);
        let recording = ports.adjustment_words().recording.0;
        self.publish_initial_levels(record, view, target.base, target.exposure, ports);
        self.reset_guard(record, view, ports);

        let mut selection = ports.selection_record(record);
        selection.position = view.block_word;
        selection.word = slot.word;
        selection.level = slot.level;
        selection.levels = slot.levels;
        selection.limit_level = slot.limit_level;
        selection.flags = [slot.flags[0], if matches!(slot.kind, 1 | 2) { slot.kind } else { 0 }, if slot.kind == 3 { 3 } else { 0 }, slot.flags[1]];
        selection.tracked = 1;
        ports.store_selection_record(record, &selection);
        let preset = selection::record_preset(view);
        ports.with_selection(|memory| {
            memory.set_preset(preset);
            memory.save_preset();
        });
        ports.notify_selection(record, &super::ports::SelectionNotice {
            kind: slot.kind,
            level: slot.level as i16,
            word: slot.word,
            limit_level: slot.limit_level as i16,
            flags: slot.flags,
            recording,
            levels: slot.levels.map(|level| level as i16),
            position: view.block_word,
        });
    }

    fn resume(&mut self, record: RecordHandle, view: &RecordView, ports: &mut impl Ports) {
        let lens = ports.current_lens();
        let published = ports.published_exposure(record);
        let level = published.reference_level.wrapping_sub(published.aspect);
        let gain_step = ports.acquisition_gain_step(view.adjustment);
        let base_gain = ports.gain_offset(published.gain_step) as u16;
        let program = exposure::chosen_program(record, &exposure::initial_program(view, view.kind), ports);
        let gain = ports.gain_offset(gain_step) as u16;
        let wide = ports.program_adjustment(program) == 1 || ports.program_range_limit(program) == 46;
        let target = ports.program_target(&TargetInputs {
            program,
            reference: view.reference,
            lens,
            level: level as i16,
            gain_offset: base_gain as i16,
            lens_reference: wide.then_some(view.lens_reference),
            gain_level: gain,
            offset: 0,
        });
        let step = ports.gain_step_for(i32::from(target.gain_level as i16));
        let range = ports.program_range(&RangeInputs {
            program,
            exposure: target.exposure,
            level: target.level,
            first: target.second,
            second: target.offset,
            adjustment: target.adjustment as i16,
            mode: view.kind,
            argument: 0,
            forced: view.forced,
            primary: 0,
            lens: lens.0,
            alternate: 0,
            request: view.request,
        });
        let percent = ports.program_percent(program);
        self.settle(target.base, target.exposure, &range, percent, ports);
        exposure::publish(record, view, &ExposureSetting {
            exposure: target.exposure,
            level: range.level,
            second: range.first,
            reference: view.reference,
            decade: view.decade,
            offset: range.second,
            correction: percent,
            gain_step: step,
            timing: range.timing,
            code: range.code,
            selected: range.selected,
            adjustment: target.adjustment_flag,
            second_exposure: PipelineWord(lens.0),
        }, ports);
        ports.store_selection_reference(record, 0);
        let current = ports.current_lens();
        ports.store_lens(record, current);
        self.publish_initial_levels(record, view, target.base, target.exposure, ports);
        self.reset_guard(record, view, ports);
    }

    fn settle(&mut self, base: u16, exposure: PipelineWord, range: &Range, correction: u16, ports: &mut impl Ports) {
        self.summary = ExposureSummary {
            base,
            exposure,
            level: range.level,
            second: range.first,
            offset: range.second,
            aspect: ports.timing_offset(range.timing),
            correction,
            timing: range.timing,
            code: range.code,
            selected: range.selected,
            rate_pair: ExposureSummary::UNSET_RATE_PAIR,
        };
        self.timing = range.timing;
    }

    fn publish_initial_levels(&mut self, record: RecordHandle, view: &RecordView, base: u16, exposure: PipelineWord, ports: &mut impl Ports) {
        let (primary, secondary) = exposure::initial_levels(base);
        exposure::publish_primary(record, view, &primary, Branch::Live, ports);
        exposure::publish_selection(record, view, &secondary, ports);
        if exposure::keeps_exposure_record(view) {
            let lens = ports.current_lens();
            let index = ports.collapse_index(exposure);
            ports.update_exposure_record(record, ExposureRecordUpdate::Context { lens, index });
        }
    }

    /// Forgets the latest selection's gate, clears the grid selection and restarts the record's block.
    fn reset_guard(&mut self, record: RecordHandle, view: &RecordView, ports: &mut impl Ports) {
        self.guard = Guard { eligible: 0, reuse: 0, lens: ports.current_lens() };
        ports.clear_grid_selection(record);
        self.refresh_block(record, view, Refresh::Started, ports);
    }

    fn refresh_block(&mut self, record: RecordHandle, view: &RecordView, refresh: Refresh, ports: &mut impl Ports) {
        let mut block = ports.acquisition_block(record);
        let summary = self.summary;
        match refresh {
            Refresh::Started => {
                block.collected = 0;
                block.selected = 0;
                block.eligible = 0;
                block.lens = ports.current_lens();
                let gain_step = ports.published_exposure(record).gain_step;
                self.statistics = StatisticsSet {
                    buffers: [crate::stock::UNMEASURED_STATISTICS; 4],
                    exposure: summary.exposure,
                    level: summary.level,
                    offset: summary.offset.wrapping_add(summary.second),
                    aspect: summary.aspect,
                    halfword: 0,
                    gain_offset: ports.gain_offset(gain_step) as u16,
                    correction: summary.correction,
                    count: 0,
                    selected: self.statistics.selected,
                };
            }
            Refresh::Released => {
                block.lens = ports.published_exposure(record).lens;
                if view.profile_flags[0] != 0 || view.restored != 0 {
                    block.collected = 0;
                    block.selected = 0;
                }
            }
        }
        let published = ports.published_exposure(record);
        let gain = ports.gain_offset(published.gain_step) as u16;
        let levels = published.primary_levels;
        block.exposure = summary.exposure;
        block.level = summary.level;
        block.gain_offset = gain;
        block.offset = summary.offset.wrapping_add(summary.second);
        block.aspect = summary.aspect;
        block.levels = [levels[0], levels[1], levels[3], levels[2]].map(|level| level.wrapping_sub(gain));
        block.reference = published.reference;
        block.halfword = 0;
        block.selection_reference = published.selection_reference;
        ports.store_acquisition_block(record, &block);
    }

    /// Classes the primary statistics about the acquisition level and moves all four buffers by
    /// the configured release offset.
    pub fn release(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        let settings = ports.acquisition_settings();
        self.refresh_block(record, &view, Refresh::Released, ports);
        let mut block = ports.acquisition_block(record);
        let set = self.statistics;
        let value = ports.exposure_value([set.exposure, PipelineWord(block.lens.0)]) as u16;
        let shift = set.aspect.wrapping_sub(set.correction);
        let level = set.level.wrapping_sub(set.offset).wrapping_sub(shift).wrapping_add(value);
        let primary = ports.statistics_buffer(set.buffers[0]);
        let classification = classify(&primary, level, &settings.release);
        block.classes = crate::stock::RELEASE_CLASSES;
        block.weights = classification.weights;
        let configured = release_offset(&settings.release, view.adjustment, view.class_profile());
        block.release_offset = configured;
        let offset = shift.wrapping_sub(configured);
        let mut offsets = [[0u16; VALUES]; 4];
        offsets[0] = offset_values(&primary, offset);
        for (moved, &buffer) in offsets[1..].iter_mut().zip(&set.buffers[1..]) {
            *moved = offset_values(&ports.statistics_buffer(buffer), offset);
        }
        block.offsets = crate::stock::RELEASE_OFFSETS;
        let mut extra = [0u16; EXTRA_CAPACITY];
        let snapshot = if settings.snapshot_grid {
            let count = usize::from(set.count as u16).min(EXTRA_CAPACITY);
            let values = &mut extra[..count];
            ports.extra_statistics(values);
            for value in values.iter_mut() { *value = value.wrapping_add(offset); }
            block.extra = crate::stock::RELEASE_EXTRA;
            Some(SnapshotRelease { extra: &extra[..count], copy: crate::stock::RELEASE_COPY })
        } else {
            None
        };
        ports.store_release(&Release { classes: &classification.classes, offsets: &offsets, snapshot });
        block.copy = crate::stock::RELEASE_COPY;
        block.release_halfword = view.release_halfword;
        ports.store_acquisition_block(record, &block);
        if ports.adjustment_words().recording.0 == 0 { selection::clear(record, ports); }
    }

    /// Calc1 0x3d00 over the primary snapshot after publishing its exposure.
    pub fn submit_primary(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        let settings = ports.acquisition_settings();
        let snapshot = ports.snapshot(Statistics::Primary);
        let gain = ports.gain_offset(snapshot.gain_step) as u16;
        let reference_level = snapshot.levels[1].wrapping_sub(gain);
        let value = ports.exposure_value(snapshot.exposure) as u16;
        let mut relative_level = snapshot.levels[0]
            .wrapping_sub(view.decade.wrapping_add(reference_level))
            .wrapping_sub(snapshot.offset_level)
            .wrapping_add(value);
        if snapshot.adjustment.0 == 1 { relative_level = relative_level.wrapping_sub(100); }
        let correction = ports.published_exposure(record).correction;
        let configuration = ports.indexed_configuration(view.selection, view.adjustment, 0);
        let level_category = ports.level_category(snapshot.levels[0] as i16, view.selection, view.adjustment);
        exposure::publish(record, &view, &ExposureSetting {
            exposure: snapshot.exposure[0],
            level: snapshot.levels[0],
            second: reference_level,
            reference: snapshot.reference,
            decade: view.decade,
            offset: snapshot.offset_level,
            correction,
            gain_step: snapshot.gain_step,
            timing: snapshot.timing,
            code: snapshot.code,
            selected: snapshot.flags,
            adjustment: snapshot.adjustment.0,
            second_exposure: snapshot.exposure[1],
        }, ports);
        let lens = ResourceIndex(snapshot.exposure[1].0);
        ports.store_lens(record, lens);
        let aspect = ports.published_exposure(record).aspect;
        if exposure::keeps_exposure_record(&view) {
            let index = ports.collapse_index(snapshot.exposure[0]);
            ports.update_exposure_record(record, ExposureRecordUpdate::Context { lens, index });
        }
        let first = ports.acquisition_block(record).collected == 0;
        let count = ports.grid_count(view.selection, view.mode, view.adjustment, view.grid_kind);
        ports.set_grid_count(Branch::Live, count);
        if count == 0 {
            ports.clear_grid_lead(Branch::Live);
        } else if settings.snapshot_grid {
            ports.snapshot_grid(Branch::Live, Statistics::Primary);
        } else {
            let inputs = GridInputs {
                timing: snapshot.timing,
                lens_word: ports.lens_word(record, Some(lens)),
                positions: ports.change_positions(record),
                window: view.window,
            };
            ports.set_grid_inputs(Branch::Live, &inputs);
            ports.fill_grid(Branch::Live, &inputs);
        }
        let grid_configuration = ports.indexed_configuration(view.selection, view.adjustment, 1);
        ports.set_grid_configuration(Branch::Live, grid_configuration);
        let request = AcquisitionRequest {
            mode: view.mode.0,
            first,
            profile: acquisition_profile(&view, view.halfword),
            sample: AcquisitionSample {
                complete: snapshot.status.complete(),
                exposure: snapshot.exposure[0].0,
                level: snapshot.levels[0],
                reference_level,
                gain_step: snapshot.gain_step.0,
                offset_level: snapshot.offset_level,
                adjustment: snapshot.adjustment.0,
                timing: snapshot.timing.0,
                halfword: snapshot.reference,
                extent: snapshot.extent.map(|word| word.0),
                size: snapshot.size.map(|word| word.0),
                origin: snapshot.origin.map(|word| word.0),
            },
            relative_level,
            configuration,
            level_category,
            correction,
            aspect,
            decade: view.decade,
        };
        if settings.traces { ports.trace(TraceMark::AcquisitionRequest); }
        ports.request_acquisition(Branch::Live, &request, super::ports::EvaluationGrid::Current);
    }

    /// Publishes calc1's primary result, takes its statistics and gates the next selection.
    pub fn collect_primary(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) {
        let Some(result) = ports.acquisition_result(completed) else { return };
        let view = ports.record_view(record);
        let settings = ports.acquisition_settings();
        exposure::publish_primary(record, &view, &result.levels, Branch::Live, ports);
        let lens = ports.published_exposure(record).lens;
        let resource = ports.relative_exposure(PipelineWord(lens.0)) as u16;
        let gain_step = ports.acquisition_gain_step(view.adjustment);
        let gain = ports.gain_offset(gain_step) as u16;
        ports.set_pipeline_reference(result.levels.base.wrapping_sub(result.correction).wrapping_add(resource).wrapping_sub(gain) as i16);
        if result.statistics_valid { self.statistics = statistics_of(&result, self.statistics.selected); }
        ports.copy_rate_grid_source(StatisticsBuffer(result.statistics[0].0));
        if settings.rate & 3 != 0 && view.rate_enabled == 1 && result.statistics_valid {
            let rate = ports.rate_record(record);
            ports.store_rate_selection(record, 0);
            if settings.rate & 2 != 0 {
                ports.rate_pair(record, result.address, 0, &mut self.summary.rate_pair);
                ports.clear_rate_configuration(record);
            }
            ports.rate_flag(record, 0);
            if settings.draws_rate { ports.draw_rate(result.address, 0); }
            if settings.rate_status { ports.draw_rate_status(rate.level, rate.word, 0); }
        }
        self.guard.eligible = self.gate(&view, &settings, Gate::Primary, ports);
        let mut block = ports.acquisition_block(record);
        block.collected = 1;
        block.eligible = if settings.suppresses_eligible { 0 } else { self.guard.eligible };
        block.reused = self.statistics.selected != 0 && self.guard.lens == ports.current_lens();
        ports.store_acquisition_block(record, &block);
        ports.release_result(ReleasedResult::Acquisition(result.address));
        self.report_change(record, &view, ports);
    }

    fn change_record(&self, record: RecordHandle, view: &RecordView, lens: ResourceIndex, ports: &mut impl Ports) -> ChangeRecord {
        ChangeRecord {
            category: view.change_category,
            mode_is_one: u32::from(ports.adjustment_words().mode.0 == 1),
            lens: ports.lens_word(record, Some(lens)),
            unbounded: ports.lens_word(record, None),
            pair: view.pair,
            kind: view.category,
        }
    }

    /// Posts imgdc 0x3f2d when a kept word differs.
    ///
    /// Suspected stock defect, kept because it sets how often imgdc receives the notice: X-M1
    /// 1.50 0xf354f4 keeps the mode word where the unbounded lens word belongs, so the next
    /// collection posts again unless the two are equal.
    fn report_change(&mut self, record: RecordHandle, view: &RecordView, ports: &mut impl Ports) {
        let lens = ports.published_exposure(record).lens;
        let now = self.change_record(record, view, lens, ports);
        if now != self.change {
            let positions = ports.change_positions(record);
            ports.post_change_notice(&ChangeNotice {
                category: now.category,
                mode_is_one: now.mode_is_one,
                lens: now.lens,
                positions,
                unbounded: now.unbounded,
                pair: now.pair,
                kind: now.kind,
            });
        }
        self.change = ChangeRecord { unbounded: now.mode_is_one, ..now };
    }

    /// Whether the latest primary statistics let calc1 reuse its selection.
    fn gate(&self, view: &RecordView, settings: &AcquisitionSettings, gate: Gate, ports: &mut impl Ports) -> u32 {
        if !settings.gate { return 0; }
        if gate == Gate::Primary && self.guard.reuse != 1 { return 0; }
        let lead = ports.grid_lead(Branch::Live);
        let modes = ModeInputs {
            grid_kind: view.grid_kind,
            grid_byte: lead.first_byte,
            selection: view.selection,
            updated_position: view.updated_position,
            category: view.category,
            mode_flag: view.mode_flag,
            change_category: view.change_category,
            grid_flag: lead.flag,
        };
        let both = ports.both_modes(&modes);
        if ports.five_parameter_adjustment(view.selection, view.updated_position, view.mode_flag, both, lead.first_word) == 0 { return 0; }
        if view.selection == ProfileSelection::Record || view.s1_mode.refuses_reuse() || self.guard.lens != ports.current_lens() { return 0; }
        if gate == Gate::Primary && view.reuse_blocked == 1 { return 0; }
        u32::from(view.allows_reuse())
    }

    /// Calc1 0x3d01 over the latest primary result, with the window and setting it names.
    ///
    /// The first program selection reads the percent and policy of the previous request's
    /// program: X-M1 1.50 0xf373f4 enters 0xf77c8c with the request block's halfword at +0x24
    /// and word at +0x20 not yet written (0xf37508, 0xf37538 write them for the second
    /// selection), and its profile 0xf76414 reads the halfword when the selection byte is 0 and
    /// the option word is 1.
    pub fn submit_secondary(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let view = ports.record_view(record);
        let settings = ports.acquisition_settings();
        let mut request = self.selection;
        fill_selection(&mut request, &view, view.mode, ports);
        request.seventh_word = view.seventh_word;
        request.offset_mode = 0;
        request.program_word = view.program_word[0];
        if view.program_word[0] == 0 { ports.store_program_word(record, [0, 0]); }
        request.rate_flag = settings.rate & 3 != 0 && view.rate_enabled != 0;
        let snapshot = ports.snapshot(Statistics::Primary);
        let gain = ports.gain_offset(snapshot.gain_step) as u16;
        let published = ports.published_exposure(record);
        exposure::publish(record, &view, &ExposureSetting {
            exposure: snapshot.exposure[0],
            level: snapshot.levels[0],
            second: snapshot.levels[1].wrapping_sub(gain),
            reference: snapshot.reference,
            decade: view.decade,
            offset: snapshot.offset_level,
            correction: published.correction,
            gain_step: snapshot.gain_step,
            timing: snapshot.timing,
            code: snapshot.code,
            selected: snapshot.flags,
            adjustment: snapshot.adjustment.0,
            second_exposure: PipelineWord(published.lens.0),
        }, ports);
        let current = ports.current_lens();
        ports.store_lens(record, current);
        let published = ports.published_exposure(record);
        request.summary_level = self.summary.base;
        request.lens = published.lens.0;
        request.exposure = published.exposure.0;
        request.gain_step = published.gain_step.0;
        request.correction = self.summary.correction;
        request.aspect = self.summary.aspect;
        fill_limits(&mut request, &view, view.mode, ports);
        request.adjustment_class = ports.adjustment_class(snapshot.adjustment.0, view.reference as i16);
        request.resource_level = ports.relative_exposure(PipelineWord(published.lens.0)) as u16;

        let mut inputs = ProgramInputs {
            selection: view.selection,
            adjustment: request.adjustment,
            kind: request.kind,
            mode: view.mode,
            word: request.program_word,
            flag: request.offset_mode,
            program_flag: request.program_flag,
            option: request.option,
            policy: self.program_tail.1,
            percent: self.program_tail.0,
            bytes: [0, 2],
        };
        let kind = if self.program_changed {
            ports.program_kind(record)
        } else {
            let stored = ports.stored_program(record);
            let fresh = exposure::chosen_program(record, &inputs, ports);
            let kept = ports.program_secondary_gate(stored) == 1;
            if kept == (ports.program_secondary_gate(fresh) == 1) {
                let kind = u32::from(kept);
                ports.store_program_kind(record, kind);
                kind
            } else {
                ports.store_program_kind(record, 2);
                self.program_changed = true;
                2
            }
        };
        request.program_changed = u32::from(self.program_changed);
        let bound = match kind {
            1 => request.lens_reference,
            2 => request.exposure,
            _ => {
                let stored = ports.stored_program(record);
                ports.program_lower_bound(stored, request.lens)
            }
        };
        let (percent, policy) = self.program_percent_and_policy(record, bound, request.option, ports);
        inputs.percent = percent;
        inputs.policy = policy;
        self.program_tail = (percent, policy);
        let program = ports.select_program(&inputs).unwrap_or(Program(request.program));
        request.program = program.0;
        let stored = ports.stored_program(record);
        let (old, new) = (ports.program_scale(stored), ports.program_scale(Program(program.0 & 0xff)));
        request.scale_change = (old == 4 && new == 3) || (old == 3 && new == 4);
        request.first = ports.acquisition_block(record).selected == 0;
        if view.selection == ProfileSelection::Unselected { ports.refresh_hardware_state(); }
        request.eligible = self.gate(&view, &settings, Gate::Secondary, ports);
        let recording = ports.adjustment_words().recording.0 == 1;
        request.recording = recording;
        self.recording_selection = recording;
        request.shot_copy = false;
        if request.eligible != 0 || recording {
            selection::store_records(record, &view, &settings, Branch::Live, &request, ports);
            request.window = true;
            if recording {
                let lens = ports.published_exposure(record).lens;
                ports.restore_selection_setting(Branch::Live, lens);
                let margin = selection::margin(&view, ports);
                ports.store_restored_setting(Branch::Live, &RestoredSetting {
                    margin,
                    reference: request.reference,
                    lens_halfword: request.lens_halfword,
                    lens_reference: request.lens_reference,
                });
                ports.copy_reference_backup(Branch::Live);
                request.shot_copy = true;
            }
            if request.eligible != 0 {
                self.guard.reuse = 1;
                ports.clear_controller_reuse();
            }
        }
        self.guard.lens = ports.current_lens();
        if recording {
            ports.with_selection(|memory| memory.set_preset(LensPreset { word: request.lens_reference, halfwords: [request.lens_halfword, request.reference] }));
        }
        if settings.traces { ports.trace(TraceMark::SelectionRequest); }
        self.selection = request;
        ports.request_selection(Branch::Live, &request);
    }

    fn program_percent_and_policy(&self, record: RecordHandle, bound: u32, option: u32, ports: &mut impl Ports) -> (u16, bool) {
        let stored = ports.stored_program(record);
        let published = ports.published_exposure(record);
        let percent = ports.profile_percent(stored, published.combined, published.lens, bound);
        let policy = ports.program_policy(stored);
        (percent, option == 1 && policy.wrapping_sub(1) <= 1)
    }

    /// Takes calc1's selection result into the record and publishes the selection table when it
    /// differs from the published exposure; returns whether it did.
    pub fn collect_secondary(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) -> bool {
        let Some(result) = ports.selection_result(completed) else {
            self.changed = false;
            return false;
        };
        let view = ports.record_view(record);
        self.take_selection(record, &view, &result, ports);
        self.publish_selection(record, &view, &result, ports);
        self.changed = self.publish_table(record, &view, &result, ports);
        let shot = result.shot_data.then_some((result.shot_record, result.saved_record));
        ports.release_result(ReleasedResult::Selection { result: result.address, shot });
        self.changed
    }

    fn take_selection(&mut self, record: RecordHandle, view: &RecordView, result: &SelectionResult, ports: &mut impl Ports) {
        let selection = ports.selection_record(record);
        exposure::publish_selection(record, view, &result.levels, ports);
        self.statistics.selected = result.selected;
        if result.shot_data || self.recording_selection {
            ports.write_saved_settings(result.shot_record);
            if !self.recording_selection || selection::changed(record, view, &selection, result, ports) {
                ports.copy_selection_to_saved(result.saved_record);
            } else {
                let pair = ports.position_pair(record, PositionSlot::Lens);
                let lens_position = ports.lens_position(pair, result.position_word);
                ports.save_selection(result.position_word, &result.save, lens_position);
            }
            ports.store_restore_levels(record, result.table_reference, result.restore_levels);
        }
        let echo = ports.setting_echo(Branch::Live);
        let mut block = ports.acquisition_block(record);
        block.selected = 1;
        block.setting_ready = echo.ready;
        block.setting_position = echo.updated_position;
        block.setting_halfword = echo.position_halfword;
        block.grid_word = ports.grid_lead(Branch::Live).first_word;
        block.shot_span = if result.shot_data {
            let levels = ports.shot_record_levels(result.shot_record);
            levels.last.wrapping_sub(levels.first)
        } else {
            0
        };
        block.setting_shot_word = echo.shot_word;
        ports.store_acquisition_block(record, &block);
    }

    fn publish_selection(&mut self, record: RecordHandle, view: &RecordView, result: &SelectionResult, ports: &mut impl Ports) {
        let mut selection = ports.selection_record(record);
        if ports.adjustment_words().recording.0 == 1 {
            if selection::changed(record, view, &selection, result, ports) {
                selection::take_result(record, &mut selection, result, true, ports);
                ports.store_position_pair(record, PositionSlot::Selected, [selection.position, selection.word]);
            }
            ports.with_selection(|memory| memory.save_preset());
        } else {
            selection::clear(record, ports);
            selection::take_result(record, &mut selection, result, true, ports);
        }
        selection::publish(record, view, Publication::Collected, selection, ports);
    }

    /// Ranges each of the result's four candidates under the newly selected program and, when the
    /// last one differs from the published exposure, posts the table; the summary takes the last.
    ///
    /// The ranges' levels stay in the round; X-M1 1.50 0xf35ff4 and 0xf360d4 have 0xf7a0cc write
    /// them back into calc1's result record, which nothing reads before calc1's next result.
    fn publish_table(&mut self, record: RecordHandle, view: &RecordView, result: &SelectionResult, ports: &mut impl Ports) -> bool {
        let published = ports.published_exposure(record);
        let refreshed = ports.refreshed_position();
        let gain_step = ports.acquisition_gain_step(result.adjustment);
        let decade = ports.kind_decade(result.kind);
        if !result.keeps_program { self.program_changed = false; }
        let bound = match ports.program_kind(record) {
            1 => view.lens_reference,
            2 => refreshed,
            _ => {
                let stored = ports.stored_program(record);
                ports.program_lower_bound(stored, published.lens.0)
            }
        };
        let (percent, policy) = self.program_percent_and_policy(record, bound, result.primary_only, ports);
        let program = exposure::chosen_program(record, &ProgramInputs {
            selection: view.selection,
            adjustment: result.adjustment,
            kind: result.kind,
            mode: view.mode,
            word: result.program_word,
            flag: result.program_flag,
            program_flag: view.program_flag(),
            option: result.primary_only,
            policy,
            percent,
            bytes: [0, 2],
        }, ports);
        ports.store_program(record, Program(program.0 & 0xff));
        let gain = ports.gain_offset(gain_step) as u16;
        let entry = |range: &Range, exposure: u32| SelectionEntry {
            timing: range.timing.0,
            code: range.code,
            selected: range.selected,
            gain_step: gain_step.0,
            exposure,
            level: range.level,
            second: range.first.wrapping_add(gain),
            offset: range.second,
            word: result.table_word,
            reference: result.table_reference,
            decade,
        };
        let mut table = SelectionTable::default();
        let mut last_primary = Range::default();
        let mut last_secondary = Range::default();
        for index in 0..4 {
            let range = |program_values: &xm1_saclac_interface::round_acquisition::SelectionProgram, alternate| RangeInputs {
                program,
                exposure: PipelineWord(program_values.exposure),
                level: program_values.levels[index],
                first: program_values.first[index],
                second: program_values.second[index],
                adjustment: program_values.adjustment[index] as i16,
                mode: result.kind,
                argument: result.program_argument,
                forced: view.forced,
                primary: result.primary_only,
                lens: published.lens.0,
                alternate,
                request: view.request,
            };
            last_primary = ports.program_range(&range(&result.primary, result.alternate));
            last_secondary = ports.program_range(&range(&result.secondary, 0));
            table.secondary[index] = entry(&last_secondary, result.secondary.exposure);
            table.primary[index] = if result.primary_only != 0 { table.secondary[index] } else { entry(&last_primary, result.primary.exposure) };
        }
        let last = table.primary[3];
        let changed = result.primary.exposure != refreshed
            || last_primary.level != published.level
            || last_primary.first != published.second
            || !(published.gain_step == gain_step && last_primary.second == published.offset)
            || result.table_reference != published.reference
            || result.table_word != published.adjustment
            || TimingWord(last.timing) != published.timing
            || last_secondary.code != published.code
            || last_secondary.selected != published.selected;
        let exposure = PipelineWord(last.exposure);
        let second = last.second.wrapping_sub(gain);
        let value = ports.exposure_value([exposure, PipelineWord(published.lens.0)]) as u16;
        let correction = ports.published_exposure(record).correction;
        self.summary = ExposureSummary {
            base: last.level.wrapping_sub(last.offset.wrapping_add(second)).wrapping_add(value),
            exposure,
            level: last.level,
            second,
            offset: last.offset,
            aspect: ports.timing_offset(TimingWord(last.timing)),
            correction,
            timing: TimingWord(last.timing),
            code: last.code,
            selected: last.selected,
            rate_pair: self.summary.rate_pair,
        };
        self.timing = self.summary.timing;
        if result.primary_only != 0 {
            let choice = table.secondary[3];
            ports.store_program_choice(record, &ProgramChoice {
                word: choice.exposure,
                level: table.secondary[0].level,
                timing: TimingWord(choice.timing),
                code: choice.code,
                selected: choice.selected,
            });
        }
        table.levels = [result.levels.levels[1], result.levels.levels[2], result.levels.zeroed[0]];
        if changed { ports.post_selection_table(&table); }
        changed
    }
}

/// The profile values both acquisitions' 0x3d00 requests carry.
pub(crate) fn acquisition_profile(view: &RecordView, halfword: u16) -> AcquisitionProfile {
    AcquisitionProfile {
        halfword,
        updated_position: view.updated_position,
        word: view.word,
        reference: view.reference,
        selection: profile_selection_byte(view.selection),
        adjustment: view.adjustment,
        area: view.area,
        category: view.category,
        ratio: view.ratio,
        window: view.window,
    }
}

/// The record values both acquisitions' 0x3d01 requests carry.
pub(crate) fn fill_selection(request: &mut SelectionRequest, view: &RecordView, mode: AcquisitionMode, ports: &mut impl Ports) {
    request.mode = mode.0;
    request.halfword = view.halfword;
    request.third_word = view.third_word;
    request.zero_word = view.zero_word;
    request.mode_word = view.mode_word;
    request.byte = view.byte;
    request.second_halfword = view.second_halfword;
    request.selection_word = view.selection_word;
    request.selection = u32::from(profile_selection_byte(view.selection));
    request.reference = view.reference;
    request.restore_adjustment = view.restore_adjustment;
    request.lens_halfword = view.lens_halfword;
    request.lens_reference = view.lens_reference;
    request.limit_halfwords = view.halfwords;
    request.converted_level = view.converted_level as u16;
    request.converted_step = ports.gain_step_for(view.converted_level as i32).0;
    request.second_word = view.second_word;
    request.mode_flag = view.mode_flag;
    request.request = view.request;
    request.ninth_word = view.ninth_word;
    request.signed_word = view.signed_word;
    request.fourth_word = view.fourth_word;
    request.fifth_word = view.fifth_word;
    request.focus_word = view.focus_word;
    request.word = view.word;
    request.decade = view.decade;
    request.adjustment = view.adjustment;
    request.kind = view.kind;
    request.program_flag = view.program_flag();
    request.pair_word = view.pair_word;
    request.area = view.area_halfwords();
    request.unrestored = view.signed_word == 0;
    request.eighth_word = view.eighth_word;
    request.option = if view.category == 6 && view.forced == 0 { ports.adjustment_option() } else { 0 };
}

/// The grid count, the calc library's limits and the rate mode both 0x3d01 requests carry.
pub(crate) fn fill_limits(request: &mut SelectionRequest, view: &RecordView, mode: AcquisitionMode, ports: &mut impl Ports) {
    request.grid_count = ports.grid_count(view.selection, mode, view.adjustment, view.grid_kind);
    request.limits = ports.selection_limits(view.selection, view.adjustment, request.grid_count, request.request);
    request.rate_mode = ports.rate_mode(request.mode, request.halfword, request.third_word.0, request.zero_word.0);
}

fn statistics_of(result: &AcquisitionResult, selected: u32) -> StatisticsSet {
    StatisticsSet {
        buffers: result.statistics.map(|address| StatisticsBuffer(address.0)),
        exposure: PipelineWord(result.exposure),
        level: result.level,
        offset: result.offset.wrapping_add(result.second),
        aspect: result.aspect,
        halfword: result.halfword,
        gain_offset: result.gain_offset,
        correction: result.correction,
        count: result.count,
        selected,
    }
}

const fn empty_selection() -> SelectionRequest {
    use xm1_saclac_interface::round_acquisition::{SelectionLimits, SelectionS1};
    SelectionRequest {
        first: false, mode: 0, halfword: 0, third_word: crate::profile::RetainedWord(0), zero_word: crate::profile::RetainedWord(0), mode_word: 0, byte: 0, second_halfword: 0,
        lens: 0, converted_step: 0, converted_level: 0, reference: 0, restore_adjustment: 0, selection_word: 0,
        selection: 0, exposure: 0, summary_level: 0, limits: SelectionLimits { step: 0, lower: 0, upper: 0, byte: 0 },
        lens_halfword: 0, lens_reference: 0, limit_halfwords: [0; 2], gain_step: 0, unrestored: false, correction: 0,
        aspect: 0, decade: 0, adjustment_class: 0, eighth_word: crate::profile::RetainedWord(0), grid_count: 0, second_word: crate::profile::RetainedWord(0), adjustment: 0,
        signed_word: 0, seventh_word: crate::profile::RetainedWord(0), fourth_word: crate::profile::RetainedWord(0), fifth_word: crate::profile::RetainedWord(0), focus_word: 0, word: 0, rate_mode: 0,
        resource_level: 0, mode_flag: 0, kind: 0, ninth_word: crate::profile::RetainedWord(0), option: 0, area: [0; 3], pair_word: 0, program_word: 0,
        rate_flag: false, program: 0, request: 0, eligible: 0, window: false, shot_copy: false, scale_change: false,
        program_changed: 0, program_flag: 0, offset_mode: 0, s1: SelectionS1 { halfword: 0, margin: 0, byte: 0, position: 0 },
        recording: false,
    }
}
