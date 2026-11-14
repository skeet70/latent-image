#![forbid(unsafe_code)]

//! The selection both acquisitions keep: the lens preset a result is compared against, the
//! record's selection taken from a result and its publication, and the window and setting a
//! selection request names.

use crate::pipeline::{GainStep, PipelineWord};
use xm1_saclac_interface::round_acquisition::{SelectionRequest, SelectionResult, SelectionSetting, SelectionWindow};
use xm1_saclac_interface::RecordHandle;

use super::ports::{
    AcquisitionSettings, Branch, GridReference, ModeInputs, PositionInputs, PositionSlot, Ports, RecordView, SelectionNotice,
    SelectionRecord, SelectionSlot,
};

/// A lens position as the record reports it: a word and two halfwords.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensPreset {
    pub word: u32,
    pub halfwords: [u16; 2],
}

/// State the live and S1 acquisitions share.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionMemory {
    current: LensPreset,
    saved: LensPreset,
    value_changed: bool,
}

impl SelectionMemory {
    pub const fn new() -> Self {
        Self { current: LensPreset { word: 0, halfwords: [0; 2] }, saved: LensPreset { word: 0, halfwords: [0; 2] }, value_changed: false }
    }

    /// The controllers' report that the selection value changed; the next S1 restore applies it.
    pub fn mark_value_changed(&mut self) { self.value_changed = true; }

    pub const fn value_changed(&self) -> bool { self.value_changed }

    pub const fn presets(&self) -> (LensPreset, LensPreset) { (self.current, self.saved) }

    pub(crate) fn set_preset(&mut self, preset: LensPreset) { self.current = preset; }

    pub(crate) fn save_preset(&mut self) { self.saved = self.current; }

    pub(crate) fn clear_value_changed(&mut self) { self.value_changed = false; }

    fn clear_presets(&mut self) {
        self.current = LensPreset::default();
        self.saved = LensPreset::default();
    }
}

/// The record's current lens preset: lens reference, lens halfword and reference.
pub(crate) const fn record_preset(view: &RecordView) -> LensPreset {
    LensPreset { word: view.lens_reference, halfwords: [view.lens_halfword, view.reference] }
}

/// Clears the record's selection and both presets.
pub(crate) fn clear(record: RecordHandle, ports: &mut impl Ports) {
    ports.selection_memory().clear_presets();
    ports.store_selection_record(record, &SelectionRecord::default());
}

/// Whether a result's position replaces the record's selection.
pub(crate) fn changed(record: RecordHandle, view: &RecordView, selection: &SelectionRecord, result: &SelectionResult, ports: &mut impl Ports) -> bool {
    if selection.tracked == 0 { return true; }
    let mode = view.s1_mode;
    let reported = ports.mapped_value(result.position, result.position_word);
    let stored = ports.mapped_value(selection.word, selection.position);
    if mode.tracks_position() && stored != reported { return true; }
    let (current, saved) = ports.selection_memory().presets();
    if mode.follows_lens_halfword() && saved.halfwords[0] != current.halfwords[0] { return true; }
    if mode.follows_reference() && saved.halfwords[1] != current.halfwords[1] { return true; }
    let pair = ports.position_pair(record, PositionSlot::Lens);
    let lensed = ports.lens_position(pair, result.position_word);
    let mapped = ports.mapped_value(lensed, result.position_word);
    stored != reported && mapped == reported
}

/// Takes the result's position, and with `full` its levels and flags, into the record's selection.
pub(crate) fn take_result(record: RecordHandle, selection: &mut SelectionRecord, result: &SelectionResult, full: bool, ports: &mut impl Ports) {
    selection.position = result.position_word;
    selection.word = result.position;
    if full {
        selection.level = result.halfwords[0];
        selection.levels = result.tail;
        selection.target = result.halfwords[1];
        selection.limit_level = result.level_halfword;
        selection.reference = result.levels.reference;
        selection.flags = [result.words[1], result.words[0], result.word, result.words[2]];
        selection.limit = result.limit;
        selection.tracked = result.position_flag;
    }
    ports.store_selection_record(record, selection);
}

/// Where a published selection comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Publication {
    /// The S1 acquisition restored it; the slot and the shot's grid reference take it.
    Restored,
    /// The live acquisition collected it; the hook gets its level clamped to the lens.
    Collected,
}

/// Publishes the record's selection and enters the record's selection hook.
pub(crate) fn publish(record: RecordHandle, view: &RecordView, publication: Publication, mut selection: SelectionRecord, ports: &mut impl Ports) {
    let first = selection.flags[0] & 1;
    let kind = if selection.flags[2] & 3 != 0 { 3 } else if selection.flags[1] & 2 != 0 { 2 } else { selection.flags[1] & 1 };
    let last = selection.flags[3] & 1;
    let (level, recording) = match publication {
        Publication::Restored => {
            let level = selection.limit_level as i16;
            store_selection_byte(record, selection.limit, ports);
            ports.publish_selection_slot(record, &SelectionSlot {
                word: selection.word,
                level: selection.level,
                kind,
                limit_level: selection.limit_level,
                flags: [first, last],
                levels: selection.levels,
            });
            take_grid_reference(record, view, &mut selection, ports);
            (level, 1)
        }
        Publication::Collected => {
            let raised = selection.reference.wrapping_add(view.converted_level as u16).wrapping_sub(view.restore_adjustment);
            let lens = ports.published_exposure(record).lens;
            let level = ports.clamp_selection_difference(view.mode, lens, raised as i16, view.lens_halfword, view.lens_reference, selection.target as i16);
            store_selection_byte(record, selection.limit, ports);
            let recording = ports.adjustment_words().recording.0;
            if recording == 1 { ports.store_position_pair(record, PositionSlot::Reported, [selection.position, selection.word]); }
            (level, recording)
        }
    };
    ports.notify_selection(record, &SelectionNotice {
        kind,
        level: selection.level as i16,
        word: selection.word,
        limit_level: level,
        flags: [first, last],
        recording,
        levels: selection.levels.map(|level| level as i16),
        position: selection.position,
    });
}

fn store_selection_byte(record: RecordHandle, byte: u8, ports: &mut impl Ports) {
    let mut block = ports.acquisition_block(record);
    block.selection_byte = byte;
    ports.store_acquisition_block(record, &block);
}

/// Lets the shot's grid reference adjust the selection's level and levels when the S1 rate applies.
fn take_grid_reference(record: RecordHandle, view: &RecordView, selection: &mut SelectionRecord, ports: &mut impl Ports) {
    if ports.acquisition_settings().s1_rate & 3 == 0 || view.rate_enabled != 1 { return; }
    let reference = ports.selected_grid_reference(record, GridReference { level: selection.level, second: 0, word: 0, levels: selection.levels });
    selection.level = reference.level;
    selection.levels = reference.levels;
}

/// The shot's reference level against the record's reference, offset by the record block's.
pub(crate) fn margin(view: &RecordView, ports: &mut impl Ports) -> u16 {
    ports.shot_reference_level().wrapping_sub(view.reference.wrapping_sub(view.block_reference))
}

/// Builds the window and setting a selection request names and stores them where calc1 reads them.
pub(crate) fn store_records(
    record: RecordHandle,
    view: &RecordView,
    settings: &AcquisitionSettings,
    branch: Branch,
    request: &SelectionRequest,
    ports: &mut impl Ports,
) {
    let lens = ports.published_exposure(record).lens;
    let (shot_word, margin) = if branch == Branch::S1 || ports.adjustment_words().recording.0 == 1 {
        (ports.shot_selection_word(), margin(view, ports))
    } else {
        (128, 0)
    };
    let category = ports.selection_mode_category(request.mode_word);
    let position = ports.capture_selection_position(branch, &PositionInputs {
        word: request.eighth_word.0,
        level: request.signed_word as i16,
        updated_position: view.updated_position,
        position_halfword: view.position_halfword,
        mode_word: request.mode_word,
        byte: request.byte,
        halfword: request.second_halfword,
    });
    let lead = ports.grid_lead(branch);
    let modes = ModeInputs {
        grid_kind: view.grid_kind,
        grid_byte: lead.first_byte,
        selection: view.selection,
        updated_position: view.updated_position,
        category: view.category,
        mode_flag: request.mode_flag,
        change_category: view.change_category,
        grid_flag: lead.flag,
    };
    let one_mode = ports.mode_one(&modes);
    let shot = ports.prepare_selection_defaults(record);
    let rate_mode = if request.mode_flag != 0 && !ports.allows_unflagged_selection(view.updated_position, request.halfword) && request.third_word.0 <= 1 {
        6
    } else {
        request.third_word.0
    };
    let mode_rule = if request.mode_flag != 0 && view.updated_position == 48 { 1 } else { request.zero_word.0 };
    let setting = SelectionSetting {
        lens: lens.0,
        shot_word,
        margin,
        category,
        one_mode,
        context_word: shot.word,
        selection: request.selection,
        updated_position: view.updated_position,
        position_halfword: view.position_halfword,
        mode_word: request.mode_word,
        byte: request.byte,
        halfword: request.second_halfword,
        setting_word: view.setting_word,
        grid_kind: view.grid_kind,
        focus_word: request.focus_word,
        fourth_word: request.fourth_word,
        request: request.request,
        rate_mode,
        word: request.word,
        reference: request.reference,
        restore_adjustment: view.restore_adjustment,
        selection_word: request.selection_word,
        lens_halfword: request.lens_halfword,
        lens_reference: request.lens_reference,
        halfwords: [request.limit_halfwords[0], request.signed_word, request.limit_halfwords[1]],
        eighth_word: request.eighth_word,
        mode_rule,
        category_word: view.category,
        change_category: view.change_category,
        second_word: request.second_word,
        mode_flag: request.mode_flag,
        rate_flag: request.rate_flag,
        fifth_word: request.fifth_word,
        ninth_word: request.ninth_word,
        profile_flags: view.profile_flags,
        tenth_word: view.tenth_word,
        lens_limit: view.lens_limit,
        lens_word_index: view.lens_word_index,
        kind: request.kind,
        kind_override: view.kind_override,
        adjustment: request.adjustment,
        profile_index: view.profile_index,
        program_word: request.program_word,
        restore_position: view.restore_position,
        hardware_ready: ports.hardware_ready(),
    };
    ports.store_selection_setting(branch, &setting);

    let correction = ports.published_exposure(record).correction;
    let gain = ports.gain_offset(GainStep(request.gain_step)) as u16;
    let relative_level = position.level.wrapping_sub(gain).wrapping_sub(correction);
    let exposure = ports.exposure_value([PipelineWord(request.exposure), PipelineWord(lens.0)]) as u16;
    let rate = if one_mode != 0 && settings.rate_divisor {
        match ports.rate_divisor(view.updated_position, view.change_category, lens, lead.halfword) {
            0 => 1,
            divisor => 10_000 / divisor,
        }
    } else {
        1
    };
    let window = SelectionWindow {
        kind: match view.word { 1 => 1, 2 => 2, _ => 0 },
        selection: request.selection,
        updated_position: view.updated_position,
        position_halfword: view.position_halfword,
        focus_word: request.focus_word,
        lens: lens.0,
        lens_reference: request.lens_reference,
        mode_rule,
        second_word: request.second_word,
        mode_word: request.mode_word,
        request: request.request,
        clock: if view.detail_flag == 1 { ports.clock_word() } else { 0 },
        rate,
        context: shot.words,
        context_byte: shot.byte,
        position_level: position.level,
        relative_level,
        resource_level: ports.relative_exposure(PipelineWord(lens.0)) as u16,
        rate_limit: ports.rate_limit(view.selection, view.updated_position, view.position_halfword),
        rate_mode: ports.rate_mode(view.updated_position, view.position_halfword, rate_mode, mode_rule),
        target_level: relative_level.wrapping_add(1600).wrapping_add(exposure),
        ratio: view.ratio,
        converted: ports.gain_step_for(i32::from(request.converted_level as i16)) == GainStep(2),
        both_modes: ports.both_modes(&modes),
        one_mode,
        context_word: shot.word,
        change_category: view.change_category,
        category: view.category,
        position_flag: position.flag == 1,
        limit_halfword: request.limit_halfwords[0],
    };
    ports.store_selection_window(branch, &window);
}
