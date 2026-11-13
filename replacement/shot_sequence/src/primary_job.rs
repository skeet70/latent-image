#![forbid(unsafe_code)]

//! Leaf work of the capture's primary job: the capture request (imgdc 0x3f49) and what its
//! completions leave in the record and the client's memory, the statistics request and the
//! template it leaves for capture settings, the display request, the lens moves after the frame,
//! the client's command-0xc and command-0x28 requests, and the client's replies.

use crate::calculations_work::{CaptureCommand, CaptureProcessing, Processing, ProcessingSelection};
use crate::capture_boundary::{
    CaptureAdjustment, CaptureClient, CaptureDiagnostics, CaptureParameters, CapturePeers, CapturePlatform, CaptureRecords, CaptureTrace,
    LensGeometry, LensMoves, MoveProfile, RecordField, RemapRequest, SettingResult, SlotField, SlotSelection,
};
use crate::boundary::{Lens, LensReading};
use crate::capture_control::CaptureControl;
use crate::capture_stock;
use crate::ports::{LensMove, PrimaryEffect, PrimaryQuery};
use crate::primary_boundary::{
    AreaFrame, AreaMode, Calibration, CalibrationSeed, CaptureCompletions, CaptureJobCalc, CaptureJobClient, CaptureJobRecords, CapturePipeline,
    CaptureProfiles, CaptureReplies, CaptureStatus, DeliveredList, DisplayArea, FactorRequest, GeometryWords, ProfileCalibration,
};
use crate::secondary_boundary::{CropGeometry, CropRequest, MeasurementCalc, MeasurementProfiles};
use crate::secondary_job::SecondaryResult;
use xm1_saclac_interface::capture_job::{
    AreaDelivery, CaptureEnd, CaptureOutput, CaptureRequest, CaptureStart, CaptureStatistics, CaptureWindow, DisplayRequest, SecondaryValues,
    StatisticsFrame, StatisticsProfile, TransferPoint,
};
use xm1_saclac_interface::capture_results::ReadoutPayload;
use xm1_saclac_interface::statistics::StatisticsSetting;
use xm1_saclac_interface::stock::{capture_job as payloads, capture_results};
use xm1_saclac_interface::{Completed, Completion, RecordHandle};

pub trait PrimaryBoundary:
    CaptureRecords
    + CaptureClient
    + CapturePlatform
    + CaptureDiagnostics
    + CapturePeers
    + Lens
    + LensGeometry
    + LensMoves
    + MeasurementCalc
    + MeasurementProfiles
    + CaptureCommand
    + CaptureProcessing
    + CaptureJobClient
    + CaptureJobRecords
    + CapturePipeline
    + ProfileCalibration
    + CaptureJobCalc
    + CaptureProfiles
    + CaptureReplies
{
}
impl<T> PrimaryBoundary for T where
    T: CaptureRecords
        + CaptureClient
        + CapturePlatform
        + CaptureDiagnostics
        + CapturePeers
        + Lens
        + LensGeometry
        + LensMoves
        + MeasurementCalc
        + MeasurementProfiles
        + CaptureCommand
        + CaptureProcessing
        + CaptureJobClient
        + CaptureJobRecords
        + CapturePipeline
        + ProfileCalibration
        + CaptureJobCalc
        + CaptureProfiles
        + CaptureReplies
{
}

/// The other capture steps' state the capture request carries.
#[derive(Clone, Copy)]
pub struct CaptureSteps<'a> {
    pub control: &'a CaptureControl,
    pub secondary: &'a SecondaryResult,
}

/// Subject source whose relative move goes between the record's slot positions.
const POSITIONED_SOURCE: u32 = 0x32;
/// Subject sources whose dump area stays unmarked.
const UNMARKED_SOURCES: core::ops::RangeInclusive<u32> = 0x35..=0x39;
/// Readout mode whose processing selection takes the variant byte.
const VARIANT_MODE: u32 = 3;
/// Readout mode the client keeps when the capture moves no lens.
const NO_MOVE_MODE: u32 = 2;
/// A slot reference that leaves the lens where it is.
const UNSET_REFERENCE: u32 = 0x80;
/// The record's second reference limit when it is unset.
const UNSET_LIMIT: u32 = 0xffff_8000;
/// The area selector's index for the delivered areas.
const AREA_INDEX: u8 = 1;
/// The layout of the second crop the sensor window is taken from.
const SENSOR_LAYOUT: [u16; 2] = [3, 4];
/// The calibration mode of the statistics' factors.
const FACTOR_MODE: u32 = 2;
/// The statistics window of a command without one: left, top, height and width.
const WHOLE_WINDOW: [u32; 4] = [0, 0, 480, 640];
/// The output scale is divided into this.
const SCALE_NUMERATOR: i32 = 0xff_ffff;

/// What the latest completion of each kind delivered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Received {
    start: Option<CaptureStart>,
    end: Option<CaptureEnd>,
    output: Option<CaptureOutput>,
    readout: Option<ReadoutPayload>,
    setting: Option<SettingResult>,
    delivery: Option<AreaDelivery>,
}

/// The primary job's state between a capture's job preparation and the next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryJob {
    record: Option<RecordHandle>,
    command_c: bool,
    command_28: bool,
    start: CaptureStart,
    calibration: Calibration,
    calibration_word: u32,
    output: CaptureOutput,
    delivered: bool,
    progress_published: bool,
    statistics_seen: bool,
    accounted: bool,
    relative: (i16, u32),
    transfer: TransferPoint,
    setting_template: StatisticsSetting,
    received: Received,
}

impl Default for PrimaryJob {
    fn default() -> Self { Self::new() }
}

impl PrimaryJob {
    pub fn new() -> Self {
        Self {
            record: None,
            command_c: false,
            command_28: false,
            start: CaptureStart::default(),
            calibration: Calibration::default(),
            calibration_word: 0,
            output: CaptureOutput::default(),
            delivered: false,
            progress_published: false,
            statistics_seen: false,
            accounted: false,
            relative: (0, 0),
            transfer: TransferPoint::default(),
            setting_template: StatisticsSetting::from_bytes([0; StatisticsSetting::BYTES]),
            received: Received::default(),
        }
    }

    pub fn effect(&mut self, effect: PrimaryEffect, parameters: &CaptureParameters, steps: CaptureSteps<'_>, boundary: &mut impl PrimaryBoundary) {
        match effect {
            PrimaryEffect::Begin(record) => self.record = Some(record),
            PrimaryEffect::ResetFlags => {
                self.delivered = false;
                self.progress_published = false;
                self.statistics_seen = false;
                self.accounted = false;
            }
            PrimaryEffect::PrepareRecords => self.prepare_dump(parameters, boundary),
            PrimaryEffect::PrepareTransfer => self.prepare_transfer(parameters, boundary),
            PrimaryEffect::Request => self.request(parameters, steps, boundary),
            PrimaryEffect::RequestCommandC => boundary.request_command_c(),
            PrimaryEffect::RequestCommand28 => boundary.request_command_28(),
            PrimaryEffect::MarkCommandC => self.command_c = true,
            PrimaryEffect::MarkCommand28 => self.command_28 = true,
            PrimaryEffect::ClearCommandC => self.command_c = false,
            PrimaryEffect::ClearCommand28 => self.command_28 = false,
            PrimaryEffect::PrepareRelative => self.prepare_relative(parameters, boundary),
            PrimaryEffect::RequestStatistics => self.request_statistics(parameters, boundary),
            PrimaryEffect::Output => self.absorb_output(parameters, boundary),
            PrimaryEffect::Delivered => {
                self.store_delivered_areas(parameters, boundary);
                self.delivered = true;
                self.dump_delivered_areas(parameters, boundary);
            }
            PrimaryEffect::Block => self.configure_without_jobs(boundary),
            PrimaryEffect::Account => self.account(parameters, boundary),
            PrimaryEffect::PublishProgress => self.publish_progress(boundary),
            PrimaryEffect::PublishFinish => self.reply(CaptureStatus::Finished, boundary),
            PrimaryEffect::RestoreLens => boundary.store_capture_end(parameters.selection, &self.received.end.unwrap_or_default()),
            PrimaryEffect::Abort => {
                boundary.monitor_device_commands(true);
                boundary.trace(CaptureTrace::Released);
            }
            PrimaryEffect::MarkStatisticsSeen => self.statistics_seen = true,
            PrimaryEffect::RequestDisplay => self.request_display(parameters, boundary),
        }
    }

    pub fn query(&mut self, query: PrimaryQuery, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) -> bool {
        let options = parameters.options;
        match query {
            PrimaryQuery::ImageFirst => options.reports_outputs() || parameters.reduced(),
            PrimaryQuery::ImageVariant => options.reports_readout(),
            PrimaryQuery::NeedsRelative => self.needs_relative(parameters, boundary),
            PrimaryQuery::NeedsAbsolute => self.needs_absolute(parameters, boundary),
            PrimaryQuery::CommandCPending => self.command_c,
            PrimaryQuery::Command28Pending => self.command_28,
            PrimaryQuery::StatisticsWanted => statistics_wanted(parameters),
            PrimaryQuery::BlockReady => {
                let source = self.received.readout.unwrap_or_default().source;
                boundary.readout_processing(source).is_some_and(|processing| capture_stock::processing_outputs(&processing) == capture_stock::OUTPUTS_REQUIRED)
            }
            PrimaryQuery::DisplayEnabled => options.displays(),
            PrimaryQuery::Delivered => self.delivered,
            PrimaryQuery::TransferPending => statistics_wanted(parameters) && !self.statistics_seen,
        }
    }

    /// Keeps what the 0x5e, 0x5f, 0x60, 0x61, 0x62, 0x64 and 0x7d completions deliver.
    pub fn receive(&mut self, completed: Completed<'_>) {
        let payload = completed.parameters.bytes();
        let received = &mut self.received;
        match completed.completion {
            Completion::Image62 => received.start = payloads::capture_start(payload).or(received.start),
            Completion::Image5e => received.end = payloads::capture_end(payload).or(received.end),
            Completion::Image5f | Completion::Image60 => received.output = payloads::capture_output(payload).or(received.output),
            Completion::Image61 => received.readout = capture_results::readout(payload).or(received.readout),
            Completion::Image64 => received.delivery = payloads::area_delivery(payload).or(received.delivery),
            Completion::Statistics7d => received.setting = capture_stock::setting_result(payload).or(received.setting),
            _ => {}
        }
    }

    /// Starts `CaptureRelative` or `CaptureAbsolute`; other moves are not the primary job's.
    pub fn lens_move(&self, lens_move: LensMove, boundary: &mut (impl LensMoves + CaptureDiagnostics)) {
        match lens_move {
            LensMove::CaptureAbsolute => {
                boundary.trace(CaptureTrace::CaptureAbsolute);
                boundary.absolute(self.transfer.reference, MoveProfile::Capture, lens_move.completion());
            }
            LensMove::CaptureRelative => {
                boundary.trace(CaptureTrace::CaptureRelative);
                boundary.relative(self.relative.0, self.relative.1, MoveProfile::Capture, lens_move.completion());
            }
            _ => {}
        }
    }

    /// The statistics record of the last capture that kept one; capture settings start from it.
    pub fn setting_template(&self) -> &StatisticsSetting { &self.setting_template }

    /// The last word of the output the job absorbed; the capture dump writes it.
    pub fn trailing_output_word(&self) -> u32 { self.output.trailing_word() }

    fn reply(&self, status: CaptureStatus, replies: &mut impl CaptureReplies) {
        if let Some(record) = self.record { replies.reply(record, status); }
    }

    /// Clears the job's part of the frame's dump and marks it when diagnostics ask for dumps.
    fn prepare_dump(&self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        if boundary.adjustment(CaptureAdjustment::Diagnostics) & 0x80 == 0 { return; }
        let frame = boundary.frame(parameters.sources);
        boundary.fill_frame(frame, capture_stock::PRIMARY_DUMP, capture_stock::PRIMARY_DUMP_BYTES, 0);
        if parameters.options.unmarked_dump() || parameters.reduced() { return; }
        let Some(record) = self.record else { return };
        let source = boundary.field(record, RecordField::SubjectSource);
        if UNMARKED_SOURCES.contains(&source) { return; }
        let (start, end) = capture_stock::PRIMARY_DUMP_MARKS;
        boundary.set_frame_word(frame, capture_stock::PRIMARY_DUMP, start);
        boundary.set_frame_word(frame, capture_stock::PRIMARY_DUMP_END, end);
    }

    /// The lens point after the frame: the selected slot's when the selection names a later slot
    /// with a lens reference, else the lens's own with the record's first slot.
    fn prepare_transfer(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        let selection = boundary.selection(parameters.selection);
        let active = SlotSelection { requested: selection.requested.wrapping_add(1), count: selection.count }.active();
        if active > 1 {
            let slot = boundary.slot(parameters.slots, active - 1);
            if slot.0 != 0 {
                let entry = boundary.slot_record(slot);
                if entry.reference() != UNSET_REFERENCE {
                    let [trim, _] = entry.trims();
                    self.transfer = TransferPoint {
                        reference: entry.reference(),
                        second_limit: entry.second_limit(),
                        trim,
                        offset: entry.offset(),
                        residual: entry.residual(),
                        step: entry.step(),
                    };
                    return;
                }
            }
        }
        let Some(record) = self.record else { return };
        let reference = boundary.read(LensReading::State);
        let slot = |boundary: &mut _, field| CaptureRecords::slot_field(boundary, record, 0, field) as i16;
        self.transfer = TransferPoint {
            reference,
            second_limit: slot(boundary, SlotField::SecondLimit),
            offset: slot(boundary, SlotField::Offset),
            trim: slot(boundary, SlotField::Trim),
            residual: slot(boundary, SlotField::Residual),
            step: slot(boundary, SlotField::Step),
        };
    }

    fn needs_relative(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) -> bool {
        let Some(record) = self.record else { return false };
        if boundary.field(record, RecordField::SubjectSource) == POSITIONED_SOURCE {
            boundary.slot_positions(record).count > 1
        } else {
            self.needs_absolute(parameters, boundary)
        }
    }

    fn needs_absolute(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) -> bool {
        boundary.readout_mode(parameters.selection) != NO_MOVE_MODE && boundary.read(LensReading::State) != self.transfer.reference
    }

    /// The relative move: between the requested slot's position and the one before it, or from
    /// the lens to the transfer point.
    fn prepare_relative(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        let Some(record) = self.record else { return };
        if boundary.field(record, RecordField::SubjectSource) == POSITIONED_SOURCE {
            let positions = boundary.slot_positions(record).positions;
            let requested = boundary.selection(parameters.selection).requested;
            let index = if requested.wrapping_sub(1) > 2 { 1 } else { usize::from(requested) };
            let delta = positions[index].position.wrapping_sub(positions[index - 1].position) as i16;
            self.relative = (delta, positions[index].speed);
        } else {
            let remap = boundary.field(record, RecordField::RemapMode) == 1;
            let position = boundary.read(LensReading::Position);
            let request = RemapRequest {
                position,
                reference: self.output.distance(),
                state: boundary.read(LensReading::State),
                target: self.transfer.reference,
                remap,
                kind: boundary.read(LensReading::Kind),
            };
            let delta = boundary.remap(request).wrapping_sub(position as i32) as i16;
            self.relative = (delta, boundary.read(LensReading::Kind));
        }
    }

    fn request(&mut self, parameters: &CaptureParameters, steps: CaptureSteps<'_>, boundary: &mut impl PrimaryBoundary) {
        self.start = CaptureStart::default();
        self.output = CaptureOutput::default();
        let Some(record) = self.record else { return };
        let field = |boundary: &mut _, field| CaptureRecords::field(boundary, record, field);
        let slot = |boundary: &mut _, field| CaptureRecords::slot_field(boundary, record, 0, field);
        let layout = field(boundary, RecordField::Layout);
        let source = boundary.source_words(parameters.sources);
        let selection = boundary.selection(parameters.selection);
        let selector = slot(boundary, SlotField::ProfileSelector);
        let kind = slot(boundary, SlotField::At0198) as u8;
        let display = boundary.display_layout(selector, kind);
        let evaluation = steps.secondary.evaluation();
        let request = CaptureRequest {
            source: source.first,
            source_words: source.words,
            source_option: source.option,
            source_byte: source.byte,
            windows: display.areas.map(|area| window_of(&area)),
            transfer: self.transfer,
            frame_words: [field(boundary, RecordField::FrameFirst), field(boundary, RecordField::FrameSecond)],
            layout: halves(layout),
            subject_source: field(boundary, RecordField::SubjectSource),
            resource_kind: field(boundary, RecordField::ResourceKind),
            subjects_locked: field(boundary, RecordField::SubjectsLocked),
            slot_reference: slot(boundary, SlotField::Reference),
            profile_selector: selector,
            secondary: SecondaryValues { measured: evaluation.measured(), primary: evaluation.primary(), primary_limit: evaluation.primary_limit() },
            options: parameters.options.0,
            mode: parameters.mode as u8,
            profile_class: field(boundary, RecordField::ProfileClass) as u16,
            slot_offset: slot(boundary, SlotField::Offset) as i16,
            slot_second_limit: slot(boundary, SlotField::SecondLimit) as i16,
            slot_trim: slot(boundary, SlotField::Trim) as i16,
            slot_residual: slot(boundary, SlotField::Residual) as i16,
            slot_step: slot(boundary, SlotField::Step) as i16,
            at_0184: slot(boundary, SlotField::At0184) as i16,
            at_0186: slot(boundary, SlotField::At0186) as i16,
            at_017a: slot(boundary, SlotField::At017a) as i16,
            secondary_mode: field(boundary, RecordField::SecondaryMode) as u8,
            secondary_latch: field(boundary, RecordField::SecondaryLatch) as u8,
            secondary_blocked: field(boundary, RecordField::SecondaryBlocked) as u8,
            secondary_enabled: steps.secondary.enabled(),
            held: steps.control.held(),
            secondary_pending: steps.control.secondary_pending(),
            at_0036: field(boundary, RecordField::At0036) as u8,
            at_0039: field(boundary, RecordField::At0039) as u8,
            at_003e: field(boundary, RecordField::At003e) as u8,
            at_0042: field(boundary, RecordField::At0042) as u8,
            at_004d: field(boundary, RecordField::At004d) as u8,
            at_0053: field(boundary, RecordField::At0053) as u8,
            at_006c: field(boundary, RecordField::At006c) as u8,
            at_0188: slot(boundary, SlotField::At0188) as u8,
            at_0198: kind,
            at_0208: slot(boundary, SlotField::At0208) as u8,
            paired: boundary.paired(selector),
            requested_slot: selection.requested,
            selection_byte: boundary.selection_option(parameters.selection),
            second_limit_unset: field(boundary, RecordField::ReferenceSecondLimit) == UNSET_LIMIT,
            slot_positions: boundary.slot_positions(record).count,
            subject_area: field(boundary, RecordField::SubjectArea) as u8,
        };
        let options = parameters.options;
        let completions = CaptureCompletions { outputs: options.reports_outputs() || parameters.reduced(), readout: options.reports_readout(), delivery: options.displays() };
        boundary.capture(&request, completions);
    }

    /// Builds the statistics record from the capture start, the calibration and the record,
    /// keeps it as the capture-setting template when the pipeline allows, and requests it.
    fn request_statistics(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        self.start = self.received.start.unwrap_or_default();
        let Some(record) = self.record else { return };
        let client = boundary.calibration_block(parameters.calibration);
        if parameters.options.default_calibration() {
            let seed = CalibrationSeed {
                record: self.record,
                base: boundary.record_calibration(record),
                pair: self.start.curve_pair,
                words: self.start.curve_words,
            };
            (self.calibration, self.calibration_word) = boundary.default_calibration(&seed);
        } else {
            self.calibration = client.calibration;
            self.calibration_word = client.word;
        }
        let field = |boundary: &mut _, field| CaptureRecords::field(boundary, record, field);
        let slot = |boundary: &mut _, field| CaptureRecords::slot_field(boundary, record, 0, field);
        let factors = FactorRequest {
            record_words: [field(boundary, RecordField::At0049), field(boundary, RecordField::At004a)],
            slot_reference: slot(boundary, SlotField::Reference),
            kind: field(boundary, RecordField::At003e),
            calibration: self.calibration,
            mode: FACTOR_MODE,
        };
        let calibrated = boundary.calibrate(&factors);
        let trim = slot(boundary, SlotField::Trim) as i16;
        let mut clamped = boundary.clamp_geometry(self.calibration_word, &calibrated, client.rate, trim);
        let step = slot(boundary, SlotField::Step) as i32;
        boundary.apply_positive(step, &mut clamped);
        self.calibration = boundary.clamp_output(&clamped);

        let (option_word, option) = boundary.statistics_option(record);
        let statistics = CaptureStatistics {
            frame: StatisticsFrame {
                record_word: field(boundary, RecordField::StatisticsWord),
                frame_words: [field(boundary, RecordField::FrameFirst), field(boundary, RecordField::FrameSecond)],
                layout: halves(field(boundary, RecordField::Layout)),
                flagged: self.start.flagged(),
                at_0198: slot(boundary, SlotField::At0198) as u8,
                selector: boundary.statistics_selector(parameters.statistics_selector),
            },
            window: if parameters.window.0 != 0 { boundary.statistics_window(parameters.window) } else { WHOLE_WINDOW },
            lens: [field(boundary, RecordField::At20c8), field(boundary, RecordField::At20b0), field(boundary, RecordField::At20b4)],
            profile: StatisticsProfile {
                record_words: [
                    field(boundary, RecordField::At0035),
                    field(boundary, RecordField::At0036),
                    field(boundary, RecordField::At0038),
                    slot(boundary, SlotField::At0208),
                    slot(boundary, SlotField::At0188),
                    field(boundary, RecordField::At0039),
                    field(boundary, RecordField::At003a),
                    field(boundary, RecordField::At003b),
                ],
                calibration_word: self.calibration_word,
                calibration: self.calibration.0,
                trim,
                second_limit: slot(boundary, SlotField::SecondLimit) as i16,
                option_word,
                option,
                at_003e: field(boundary, RecordField::At003e) as u8,
                record_byte: field(boundary, RecordField::StatisticsByte) as u8,
            },
        };
        let setting = payloads::statistics_setting(&statistics);
        let subject_source = field(boundary, RecordField::SubjectSource);
        let profile_class = field(boundary, RecordField::ProfileClass);
        if boundary.keeps_template(subject_source, parameters.options.0, parameters.mode as u8, profile_class) { self.setting_template = setting; }
        boundary.statistics_setting(&setting, Completion::Statistics7d);
    }

    /// Keeps the output and passes it on to the record's first slot, the source descriptor and
    /// the client's selection.
    fn absorb_output(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        let output = self.received.output.unwrap_or_default();
        self.output = output;
        let Some(record) = self.record else { return };
        let [first_half, second_half] = output.slot_halves();
        let [first_word, second_word] = output.slot_words();
        for (field, value) in [
            (SlotField::Reference, output.reference()),
            (SlotField::SecondLimit, i32::from(output.second_limit()) as u32),
            (SlotField::Trim, i32::from(output.trim()) as u32),
            (SlotField::At0186, i32::from(first_half) as u32),
            (SlotField::At0184, i32::from(second_half) as u32),
            (SlotField::At01b8, second_word),
            (SlotField::Distance, output.distance()),
            (SlotField::At0204, first_word),
        ] {
            boundary.set_slot_field(record, 0, field, value);
        }
        let (marker, value) = output.source();
        boundary.store_source_output(parameters.sources, marker, value);
        let offset = boundary.capture_offset(record);
        let scale = boundary.output_scale(output.scale_index(), output.distance(), output.reference(), output.scale_words());
        let rate = SCALE_NUMERATOR.wrapping_div(if scale == 0 { 1 } else { scale });
        boundary.store_output(parameters.selection, &output.selection(), offset, rate as u32);
    }

    /// Generates the delivered areas into the client's buffer when imgdc found any, else clears
    /// it, and stores the buffer's three lists in the record.
    fn store_delivered_areas(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        let Some(record) = self.record else { return };
        let buffer = parameters.areas;
        if self.received.delivery.is_some_and(|delivery| delivery.found) {
            self.generate_areas(record, buffer, boundary);
        } else {
            for list in [DeliveredList::Second, DeliveredList::First, DeliveredList::Third] { boundary.clear_area_list(buffer, list); }
        }
        let lists = [DeliveredList::Second, DeliveredList::First, DeliveredList::Third];
        let counts = lists.map(|list| boundary.area_count(buffer, list));
        for (list, count) in lists.iter().zip(counts) { boundary.store_delivered_count(record, *list, count); }
        for (list, count) in lists.iter().zip(counts) {
            for index in 0..capped(count) {
                let area = boundary.area(buffer, *list, index);
                boundary.store_delivered_area(record, *list, index, &area);
            }
        }
    }

    fn generate_areas(&self, record: RecordHandle, buffer: crate::boundary::ClientRef, boundary: &mut impl PrimaryBoundary) {
        let layout = halves(boundary.field(record, RecordField::Layout));
        let words = GeometryWords {
            offset: boundary.capture_offset(record),
            frame_words: [boundary.field(record, RecordField::FrameFirst), boundary.field(record, RecordField::FrameSecond)],
            layout,
        };
        let selector = boundary.area_selector(self.start.profile, AREA_INDEX);
        let request = CropRequest { selector, offset: words.offset, frame_words: words.frame_words, layout, full: false };
        let crop = boundary.crop_geometry(&request);
        let sensor_crop = boundary.crop_geometry(&CropRequest { layout: SENSOR_LAYOUT, ..request });
        let sensor = boundary.sensor_window(&sensor_crop);
        let frame = AreaFrame { crop: frame_of(&crop), sensor: [sensor[0].wrapping_sub(sensor_crop.origin[0]), sensor[1].wrapping_sub(sensor_crop.origin[1]), sensor[2], sensor[3]] };
        boundary.generate_areas(AreaMode::Second, selector, &frame, buffer);
        boundary.generate_areas(AreaMode::Third, selector, &frame, buffer);
        let corner = [frame.crop[2], frame.crop[3]];
        for index in 0..capped(boundary.area_count(buffer, DeliveredList::Second)) {
            let mut area = boundary.area(buffer, DeliveredList::Second, index);
            let mut rectangle = area.rectangle();
            let mut centre = area.centre();
            for axis in 0..2 {
                rectangle[axis] = rectangle[axis].wrapping_sub(corner[axis]);
                centre[axis] = centre[axis].wrapping_sub(corner[axis]);
            }
            area.set_rectangle(rectangle);
            area.set_centre(centre);
            boundary.set_area(buffer, DeliveredList::Second, index, &area);
        }
        boundary.frame_areas(&words, [frame.crop[4], frame.crop[5]], buffer);
    }

    /// Dumps the record's second delivered list and the measurement table when diagnostics ask
    /// for dumps.
    fn dump_delivered_areas(&self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        if boundary.adjustment(CaptureAdjustment::Diagnostics) & 0x80 == 0 { return; }
        let frame = boundary.frame(parameters.sources);
        let Some(record) = self.record else { return };
        let count = boundary.delivered_count(record, DeliveredList::Second);
        boundary.set_frame_word(frame, capture_stock::PRIMARY_DUMP_COUNT, count);
        for index in 0..count as u8 {
            let area = boundary.delivered_area(record, DeliveredList::Second, index);
            let word = |offset: usize| u32::from_le_bytes([area.0[offset], area.0[offset + 1], area.0[offset + 2], area.0[offset + 3]]);
            for (slot, offset) in capture_stock::DUMPED_AREA_WORDS.into_iter().enumerate() {
                boundary.set_frame_word(frame, capture_stock::primary_dump_area(index, slot), word(offset));
            }
            let half = u16::from_le_bytes([area.0[capture_stock::DUMPED_AREA_HALF], area.0[capture_stock::DUMPED_AREA_HALF + 1]]);
            boundary.set_frame_word(frame, capture_stock::primary_dump_area(index, 8), u32::from(half));
        }
        let selector = boundary.slot_field(record, 0, SlotField::ProfileSelector);
        let extent = boundary.profile_extent(selector, false);
        let [first, second] = capture_stock::PRIMARY_DUMP_EXTENT;
        boundary.set_frame_word(frame, first, extent[2]);
        boundary.set_frame_word(frame, second, extent[3]);
        for (index, value) in boundary.measurement_table().into_iter().enumerate() {
            if let Some(offset) = capture_stock::primary_dump_table(index) { boundary.set_frame_word(frame, offset, value); }
        }
    }

    /// Configures the capture pipeline's processing for the readout without the calculation jobs.
    fn configure_without_jobs(&self, boundary: &mut impl PrimaryBoundary) {
        let readout = self.received.readout.unwrap_or_default();
        boundary.publish_readout(readout.mode, readout.value);
        let variant_mode = readout.mode == VARIANT_MODE;
        let selection = ProcessingSelection { variant: if variant_mode { readout.variant } else { 0 }, variant_mode, mode: readout.mode, value: readout.value };
        let client = boundary.processing_word();
        let mut processing = Processing::default();
        boundary.configure(readout.source, false, &mut processing, client, 0, &selection);
    }

    /// Tells the client once that the capture is accounted for: every output made, the
    /// statistics seen when wanted and the pipeline's statistics ready when the command waits.
    fn account(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        if self.accounted { return; }
        let wanted = !statistics_wanted(parameters) || self.statistics_seen;
        let ready = !parameters.options.waits_for_statistics() || boundary.statistics_ready();
        if wanted && self.output.delivered() >= capture_stock::OUTPUTS_REQUIRED && ready {
            self.reply(CaptureStatus::Accounted, boundary);
            self.accounted = true;
        }
    }

    fn publish_progress(&mut self, boundary: &mut impl PrimaryBoundary) {
        if self.output.delivered() >= capture_stock::OUTPUTS_REQUIRED && self.delivered && !self.progress_published {
            self.reply(CaptureStatus::Progress, boundary);
            self.progress_published = true;
        }
    }

    /// Stores the statistics' setting result in the record and, when the command displays the
    /// frame, asks imgdc to display it at the result's extent.
    fn request_display(&mut self, parameters: &CaptureParameters, boundary: &mut impl PrimaryBoundary) {
        let Some(record) = self.record else { return };
        boundary.store_setting_result(record, &self.received.setting.unwrap_or_default());
        if parameters.options.displays() {
            let SettingResult(stored) = boundary.setting_result(record);
            let words = GeometryWords {
                offset: boundary.capture_offset(record),
                frame_words: [boundary.field(record, RecordField::FrameFirst), boundary.field(record, RecordField::FrameSecond)],
                layout: [0; 2],
            };
            let selector = boundary.slot_field(record, 0, SlotField::ProfileSelector);
            let pair = [stored[0], stored[1]];
            let extent = boundary.display_extent(selector, pair, &words);
            boundary.display(&DisplayRequest { setting_word: stored[3], setting_pair: pair, extent });
        }
        if parameters.options.reports_display() { self.reply(CaptureStatus::Displayed, boundary); }
    }
}

fn statistics_wanted(parameters: &CaptureParameters) -> bool { parameters.options.displays() || parameters.options.reports_display() }

fn halves(word: u32) -> [u16; 2] { [word as u16, (word >> 16) as u16] }

/// Areas of a delivered list the client's buffer and the record hold.
fn capped(count: u32) -> u8 { count.min(DeliveredList::CAPACITY as u32) as u8 }

fn window_of(area: &DisplayArea) -> CaptureWindow {
    let left = (area.extent[2] >> 1).wrapping_sub(area.window[1]).wrapping_add(area.extent[1]) as u16;
    let width = (area.window[2] as u16).wrapping_add(area.origin).wrapping_sub(left);
    CaptureWindow { left, width, half_height: (area.window[3] >> 1) as u16 }
}

fn frame_of(crop: &CropGeometry) -> [u32; 6] {
    [
        crop.frame[0],
        crop.frame[1],
        crop.window[0].wrapping_sub(crop.origin[0]),
        crop.window[1].wrapping_sub(crop.origin[1]),
        crop.window[2],
        crop.window[3],
    ]
}
