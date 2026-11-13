#![forbid(unsafe_code)]

//! Leaf work of the capture controller: the record binding, the statistics setup and the capture
//! setting, the decisions between lens return, secondary and primary job, the job preparation of
//! the record and the client's slots, and the release after a capture.

use crate::boundary::{Frame, Lens, LensReading};
use crate::capture_boundary::{
    AreaReference, CalcLibrary, CalculationWord, CaptureAdjustment, CaptureClient, CaptureDiagnostics, CaptureParameters, CapturePeers,
    CapturePlatform, CaptureRecords, CaptureShared, CaptureTrace, FrameTable, LensGeometry, LensSample, RecordField, SettingResult,
    SlotField,
};
use crate::capture_stock;
use crate::lens_return::{ReturnPoint, SavedSlots, SAVED_SLOTS};
use crate::subjects::Candidates;
use xm1_saclac_interface::area_measurement::AreaEntry;
use xm1_saclac_interface::statistics::{SettingOverrides, StatisticsSetting};
use xm1_saclac_interface::{Completed, Completion, RecordHandle};

/// The resource kind that compares the lens with the record's fifth reference.
const COMPARING_KIND: u32 = 2;
/// `SecondaryMode` value that enables and also blocks the secondary job.
const SECONDARY_MODE: u32 = 7;
/// The second limit of a slot when the record leaves it unset.
const UNSET_SECOND_LIMIT: u32 = 0xffff_8000;
const DEFAULT_SECOND_LIMIT: i32 = -500;

pub trait ControlBoundary:
    CaptureRecords + CaptureClient + Lens + LensGeometry + CalcLibrary + CapturePeers + CaptureShared + CapturePlatform + CaptureDiagnostics
{
}
impl<T> ControlBoundary for T where
    T: CaptureRecords + CaptureClient + Lens + LensGeometry + CalcLibrary + CapturePeers + CaptureShared + CapturePlatform + CaptureDiagnostics
{
}

/// The capture controller's own state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureControl {
    record: Option<RecordHandle>,
    held: bool,
    secondary_pending: bool,
    setting_result: SettingResult,
    /// The statistics record of the last primary job; a capture setting starts from it.
    pub setting_template: StatisticsSetting,
}

impl Default for CaptureControl {
    fn default() -> Self { Self::new() }
}

impl CaptureControl {
    pub const fn new() -> Self {
        Self {
            record: None,
            held: false,
            secondary_pending: false,
            setting_result: SettingResult([0; 4]),
            setting_template: StatisticsSetting::from_bytes([0; StatisticsSetting::BYTES]),
        }
    }

    pub fn bind(&mut self, record: RecordHandle) { self.record = Some(record); }

    pub fn record(&self) -> Option<RecordHandle> { self.record }

    /// Cleared when a job is prepared; the primary job reads it.
    pub fn held(&self) -> bool { self.held }

    /// Set when a job is prepared and cleared when the secondary job starts; the primary job
    /// reads it.
    pub fn secondary_pending(&self) -> bool { self.secondary_pending }

    pub fn clear_secondary(&mut self) { self.secondary_pending = false; }

    /// Keeps the setting result the 0x85 completion delivers.
    pub fn receive(&mut self, completed: Completed<'_>) {
        if completed.completion == Completion::Statistics85 {
            if let Some(result) = capture_stock::setting_result(completed.parameters.bytes()) { self.setting_result = result; }
        }
    }

    pub fn request_statistics_setup(&self, parameters: &CaptureParameters, peers: &mut impl CapturePeers) {
        peers.statistics_setup(parameters.options.statistics_setup(), Completion::Statistics83);
    }

    pub fn prepare_setting(&self, platform: &mut impl CapturePlatform) { platform.prepare_setting(); }

    pub fn request_setting(&self, overrides: &SettingOverrides, peers: &mut impl CapturePeers) {
        let mut setting = self.setting_template;
        setting.apply(overrides);
        peers.statistics_setting(&setting, Completion::Statistics85);
    }

    pub fn commit_setting(&self, boundary: &mut (impl CaptureRecords + CapturePlatform)) {
        if let Some(record) = self.record { boundary.store_setting_result(record, &self.setting_result); }
        boundary.refresh_setting();
    }

    /// Drops the focus result's validity and refinement before a job.
    pub fn prepare_record(&self, records: &mut impl CaptureRecords) {
        let Some(record) = self.record else { return };
        let mut result = records.focus_result(record);
        result.valid = 0;
        result.refined = 0;
        records.store_focus_result(record, &result);
    }

    pub fn release(&self, boundary: &mut (impl CapturePlatform + CaptureDiagnostics)) {
        boundary.monitor_device_commands(true);
        boundary.trace(CaptureTrace::Released);
    }

    /// Decides the lens return and leaves `point` for it: the lens left the record's fifth
    /// reference (kind 2), a half press asked for it, or a slot's reference changes at the current
    /// lens reference. Only a capture of the first slot as requested and selected, not reduced,
    /// decides or touches `point`.
    pub fn needs_lens_return(&self, parameters: &CaptureParameters, point: &mut ReturnPoint, boundary: &mut impl ControlBoundary) -> bool {
        let requested = boundary.take_return_request();
        let Some(record) = self.record else { return false };
        let selection = boundary.selection(parameters.selection);
        if boundary.active_slot(selection) != 1 || selection.requested != 1 || parameters.reduced() { return false; }
        let slots = selection.count.min(SAVED_SLOTS as u8);

        let mut saved = SavedSlots::default();
        for slot in 0..slots {
            let handle = boundary.slot(parameters.slots, slot);
            saved.references[usize::from(slot)] = boundary.slot_reference(handle);
        }
        let base = boundary.reference_pair(record, 0);
        let lens = boundary.read(LensReading::Reference);
        let mut needed = requested;
        let mut armed = false;
        if boundary.field(record, RecordField::ResourceKind) == COMPARING_KIND {
            armed = boundary.reference_pair(record, 4).reference != lens;
            needed |= armed;
        }
        point.pending = false;
        point.armed = armed;

        let mut keep = false;
        for slot in 0..slots {
            let handle = boundary.slot(parameters.slots, slot);
            saved.offsets[usize::from(slot)] = boundary.slot_offset(handle);
        }
        if !needed {
            for slot in 0..slots {
                let index = usize::from(slot);
                let reference = saved.references[index];
                let converted = boundary.convert(base.reference, reference, lens);
                if boundary.mapped(reference, base.reference) != boundary.mapped(converted, lens) {
                    keep = false;
                    needed = true;
                    break;
                }
                if converted != reference {
                    keep = true;
                    saved.references[index] = converted;
                    saved.offsets[index] = boundary.compensated(converted, lens);
                    needed = true;
                }
            }
        }
        point.saved = keep.then_some(saved);
        needed
    }

    pub fn secondary_job(&self, parameters: &CaptureParameters, boundary: &mut impl ControlBoundary) -> bool {
        if parameters.reduced() { return false; }
        if boundary.adjustment(CaptureAdjustment::Diagnostics) & 1 != 0 {
            return secondary_style_enables(boundary.adjustment(CaptureAdjustment::SecondaryStyle));
        }
        let Some(record) = self.record else { return false };
        if boundary.field(record, RecordField::SecondaryBlocked) == 1 { return false; }
        boundary.secondary_enabled() && boundary.field(record, RecordField::SecondaryMode) != SECONDARY_MODE
    }

    pub fn prepare_job(&mut self, parameters: &CaptureParameters, candidates: &mut Candidates, boundary: &mut impl ControlBoundary) {
        boundary.trace(CaptureTrace::PrepareJob);
        let Some(record) = self.record else { return };
        let selection = boundary.selection(parameters.selection);
        let active = boundary.active_slot(selection);
        let active_handle = boundary.slot(parameters.slots, active.wrapping_sub(1));
        if parameters.options.loads_profile() {
            boundary.load_profile(record, parameters.profile);
            boundary.select_slot(record, active_handle);
        } else if parameters.options.selects_slot() {
            boundary.select_slot(record, active_handle);
        }

        let first_active = active == 1;
        if boundary.field(record, RecordField::SlotMode) == 1 && first_active {
            self.refresh_slot_rates(record, selection.count, parameters, boundary);
        }
        if boundary.has_work() && selection.requested == 1 && !parameters.reduced() {
            let reference = boundary.area_reference(record);
            let frame = boundary.frame(parameters.sources);
            if self.refresh_subjects(record, &reference, frame, candidates, boundary) {
                copy_slot_entries(record, selection.count, parameters, boundary);
            }
            let kind = boundary.field(record, RecordField::SubjectKind);
            let result = if kind == 1 {
                let entry = boundary.slot_entry(record, 0);
                boundary.slot_rate(entry.rate_active(), entry.rate_profile())
            } else {
                kind
            };
            boundary.store_selection_result(parameters.selection, result);
        }
        if first_active {
            for slot in selection.indices() {
                let mut entry = boundary.slot_entry(record, slot);
                let distance = entry.offset().wrapping_add(entry.trims()[0]).wrapping_add(entry.trims()[1]);
                let split = boundary.split_distance(distance);
                entry.set_step(split.excess);
                entry.set_residual(split.kept.wrapping_sub(entry.offset()));
                boundary.set_slot_field(record, slot, SlotField::Step, i32::from(split.excess) as u32);
                boundary.set_slot_field(record, slot, SlotField::Residual, i32::from(entry.residual()) as u32);
                let target = boundary.slot(parameters.slots, slot);
                boundary.store_slot_entry(target, &entry);
            }
        }

        self.held = false;
        self.secondary_pending = true;
        if !parameters.reduced() {
            self.prepare_secondary(record, parameters, boundary);
        }
        if parameters.options.defaults_second_limit() && boundary.field(record, RecordField::ReferenceSecondLimit) == UNSET_SECOND_LIMIT {
            boundary.set_slot_field(record, 0, SlotField::SecondLimit, DEFAULT_SECOND_LIMIT as u32);
        }
        let dumps = dumps(boundary);
        if dumps {
            let frame = boundary.frame(parameters.sources);
            boundary.export_record_snapshot(frame);
            boundary.export_joint_tables(frame);
            boundary.export_pipeline(frame);
        }
        boundary.monitor_device_commands(false);
        boundary.store_selection_hook(parameters.selection);
        boundary.set_field(record, RecordField::JobPrepared, 1);
        self.prepare_selection_dump(record, parameters, dumps, boundary);
    }

    fn refresh_slot_rates(&self, record: RecordHandle, count: u8, parameters: &CaptureParameters, boundary: &mut impl ControlBoundary) {
        let sample = LensSample {
            reference: boundary.read(LensReading::Reference),
            state: boundary.read(LensReading::State),
            position: boundary.read(LensReading::Position) as u16,
            kind: boundary.read(LensReading::Kind),
        };
        let control = boundary.read(LensReading::Control);
        let rate = boundary.selection_rate(record, sample, control);
        let direction = boundary.selection_direction(sample);
        for slot in 0..3 { boundary.set_slot_field(record, slot, SlotField::Rate, rate); }
        for slot in 0..3 { boundary.set_slot_field(record, slot, SlotField::Direction, direction); }
        copy_slot_entries(record, count, parameters, boundary);
    }

    /// Replaces the capture's candidates when the areas moved since the half press; returns
    /// whether they were replaced.
    fn refresh_subjects(&self, record: RecordHandle, reference: &AreaReference, frame: Frame, candidates: &mut Candidates, boundary: &mut impl ControlBoundary) -> bool {
        if boundary.calculation_word(CalculationWord::SubjectRefresh) == 0 { return false; }
        if boundary.adjustment(CaptureAdjustment::Subjects) & 3 == 0 { return false; }
        let kind = boundary.field(record, RecordField::SubjectKind);
        let source = boundary.field(record, RecordField::SubjectSource);
        let area = boundary.field(record, RecordField::SubjectArea);
        boundary.classify_subjects(reference, kind, source, area);
        let moved = boundary.moved_areas() != 0;
        if !moved && !dumps(boundary) { return false; }
        boundary.update_capture_selection(record);
        boundary.export_selection(frame);
        if !moved { return false; }
        if boundary.field(record, RecordField::SubjectsLocked) != 0 { return true; }
        let primary = boundary.slot_entry(record, 0).primary_subjects();
        candidates.lists[3] = boundary.subject_source(primary);
        if boundary.field(record, RecordField::KeepsAllSubjects) == 0 { candidates.keep_common(4); }
        boundary.publish_subjects(record, &candidates.lists[3]);
        true
    }

    fn prepare_secondary(&self, record: RecordHandle, parameters: &CaptureParameters, boundary: &mut impl ControlBoundary) {
        boundary.reset_secondary();
        boundary.set_slot_field(record, 0, SlotField::ItemState, 0);
        boundary.store_item(record, 0, &AreaEntry::EMPTY);
        boundary.set_slot_field(record, 0, SlotField::State, 0);
        if boundary.adjustment(CaptureAdjustment::SlotLock) & 1 != 0 { boundary.set_field(record, RecordField::SlotLock, 1); }
        if boundary.slot_field(record, 0, SlotField::SecondaryFirst) != 0
            && boundary.slot_field(record, 0, SlotField::SecondarySecond) != 0
            && (boundary.device_status_bit()
                || boundary.field(record, RecordField::SecondaryMode) == SECONDARY_MODE
                || boundary.field(record, RecordField::SecondaryLatch) == 1)
        {
            boundary.set_secondary_enabled(true);
        }
        if boundary.adjustment(CaptureAdjustment::Diagnostics) & 1 != 0 {
            let style = boundary.adjustment(CaptureAdjustment::SecondaryStyle);
            let (record_style, enabled) = match style {
                1 | 4..=7 => (0, true),
                3 => (7, true),
                _ => (6, false),
            };
            boundary.set_field(record, RecordField::SecondaryStyle, record_style);
            boundary.set_secondary_enabled(enabled);
        }
        let value = if boundary.field(record, RecordField::SecondaryLatch) == 1 { boundary.device_value() } else { 0 };
        boundary.set_slot_field(record, 0, SlotField::DeviceValue, value);
        if dumps(boundary) {
            let frame = boundary.frame(parameters.sources);
            let kept = capture_stock::TRACE_KEPT.map(|offset| boundary.frame_word(frame, offset));
            boundary.fill_frame(frame, capture_stock::TRACE_AREA, capture_stock::TRACE_AREA_BYTES, 0);
            for (offset, value) in capture_stock::TRACE_KEPT.into_iter().zip(kept) { boundary.set_frame_word(frame, offset, value); }
        }
    }

    /// Readies the selection map area of the frame's dump.
    fn prepare_selection_dump(&self, record: RecordHandle, parameters: &CaptureParameters, dumps: bool, boundary: &mut impl ControlBoundary) {
        let frame = boundary.frame(parameters.sources);
        let present = boundary.field(record, RecordField::MapPresent) == 1;
        if !dumps { return; }
        if !boundary.dump_armed() && present {
            boundary.prepare_dump_area(frame, capture_stock::MAP_AREA);
            self.mark_map(frame, boundary);
            boundary.set_dump_armed(true);
        }
        if boundary.adjustment(CaptureAdjustment::Subjects) == 0 {
            boundary.fill_frame(frame, capture_stock::MAP_TABLE, capture_stock::MAP_TABLE_BYTES, 0);
            for (offset, bytes) in capture_stock::MAP_UNSET { boundary.fill_frame(frame, offset, bytes, 0xff); }
        } else if present {
            boundary.copy_table(frame, capture_stock::MAP_TABLE, FrameTable::SelectionMap);
        } else {
            boundary.fill_frame(frame, capture_stock::MAP_AREA, capture_stock::MAP_AREA_BYTES, 0);
            self.mark_map(frame, boundary);
            boundary.set_dump_armed(false);
        }
    }

    fn mark_map(&self, frame: Frame, client: &mut impl CaptureClient) {
        let (start, end) = capture_stock::MAP_MARKS;
        client.set_frame_word(frame, capture_stock::MAP_AREA, start);
        client.set_frame_word(frame, capture_stock::MAP_END, end);
    }
}

fn dumps(platform: &mut impl CapturePlatform) -> bool { platform.adjustment(CaptureAdjustment::Diagnostics) & 0x80 != 0 }

/// Styles 1 and 4..=7 run the secondary job when the diagnostics choose it.
const fn secondary_style_enables(style: u8) -> bool { matches!(style, 1 | 4..=7) }

fn copy_slot_entries(record: RecordHandle, count: u8, parameters: &CaptureParameters, boundary: &mut (impl CaptureRecords + CaptureClient)) {
    for slot in 0..count {
        let entry = boundary.slot_entry(record, slot);
        let target = boundary.slot(parameters.slots, slot);
        boundary.store_slot_entry(target, &entry);
    }
}
