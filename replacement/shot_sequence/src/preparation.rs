#![forbid(unsafe_code)]

//! Leaf work of the half-press and live sequences: binding the record, preparing the records and
//! lens state the steps read, the half-press lens status and the live resource release.

use crate::boundary::{Adjustment, Client, ClientRef, Diagnostics, Dump, Frame, Hold, Lens, LensReading, LensReference, Platform, Publication, Records, Setting, Trace};
use crate::capture_boundary::ReferencePair;
use crate::ports::{HalfPressMode, HalfPressNotifications};
use crate::subjects::{self, Candidates, SubjectServices};
use crate::summary;
use crate::lens_return::ReturnPoint;
use xm1_saclac_interface::RecordHandle;

/// Notification and publication mask of the half-press parameters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HalfPressMask(pub u32);

impl HalfPressMask {
    const KEEP_AREA_SELECTION: u32 = 1;
    const KEEP_AREAS: u32 = 1 << 1;
    const FRAME_RELEASED: u32 = 1 << 2;
    const RELATIVE_LENS_DONE: u32 = 1 << 3;

    pub const fn is_empty(self) -> bool { self.0 == 0 }
    pub const fn publishes_area_selection(self) -> bool { self.0 & Self::KEEP_AREA_SELECTION == 0 }
    pub const fn publishes_areas(self) -> bool { self.0 & Self::KEEP_AREAS == 0 }
    pub const fn publishes_lens_target(self) -> bool { self.0 & Self::FRAME_RELEASED == 0 && self.0 & Self::RELATIVE_LENS_DONE != 0 }
    pub const fn notifications(self) -> HalfPressNotifications {
        HalfPressNotifications { frame_released: self.0 & Self::FRAME_RELEASED != 0, relative_lens_done: self.0 & Self::RELATIVE_LENS_DONE != 0 }
    }
}

/// The half-press command parameters as the client wrote them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HalfPressParameters {
    pub mode: HalfPressMode,
    pub option: u32,
    pub mask: HalfPressMask,
    /// The pipeline plan's option word.
    pub plan_option: u32,
    pub area_count: Option<ClientRef>,
    pub area_selection: u32,
    pub areas: Option<ClientRef>,
    pub lens_target: Option<ClientRef>,
    pub frame_slot: Option<ClientRef>,
    pub lens_reference: LensReference,
    /// The record's third reference pair when the selection source is 46, 47, 54 or 56.
    pub client_reference: ReferencePair,
}

/// What the frame header carries for the calculation task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub mode: HalfPressMode,
    pub mask: HalfPressMask,
    pub option: u32,
    pub lens_reference: LensReference,
}

/// Where a half press reads the lens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensStage { Pipeline, Conclusion }

/// The lens readings a half press keeps in the shot context.
pub trait LensReadings {
    /// Starts the readings with the one taken at the preparation.
    fn begin_lens_track(&mut self, reading: u32);
    /// `changed` is the lens change latch taken with the reading.
    fn record_lens(&mut self, stage: LensStage, reading: u32, changed: bool);
}

/// The shot context words a preparation writes; `P` is what building the selection list calls
/// outside the shot.
pub trait PreparationShot<P: ?Sized>: LensReadings {
    fn lens_reference(&mut self) -> LensReference;
    fn bind_lens_reference(&mut self, reference: LensReference);
    /// Builds the record's selection list from the half-press client's area count block, or for
    /// the live sequence.
    fn prepare_selection(&mut self, record: RecordHandle, area_count: Option<ClientRef>, live: bool, ports: &mut P);
    /// The words a preparation in `mode` (`None` for the live sequence) carries to the next.
    fn carry(&mut self, mode: Option<u32>);
}

/// The shot state the preparation writes and later steps read.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Prepared {
    pub record: Option<RecordHandle>,
    pub live_record: Option<RecordHandle>,
    pub parameters: Option<HalfPressParameters>,
    pub refinement_pass: bool,
    pub lens_control: u32,
    pub candidates: Candidates,
    pub level: u8,
    /// The preparation leaves it at 2.
    pub pipeline_gate: u32,
    pub lens_return: ReturnPoint,
}

pub trait Boundary: Records + Lens + Client + Diagnostics + Platform + SubjectServices {}
impl<T: Records + Lens + Client + Diagnostics + Platform + SubjectServices> Boundary for T {}

/// Takes the lens reading of `stage` with the change latch.
pub fn record_lens(stage: LensStage, shot: &mut impl LensReadings, lens: &mut impl Lens) {
    let changed = lens.take_change() != 0;
    let reading = lens.read(LensReading::Reference);
    shot.record_lens(stage, reading, changed);
}

impl Prepared {
    pub fn bind(&mut self, record: RecordHandle) { self.record = Some(record); }

    pub fn note_live_record(&mut self, record: RecordHandle) { self.live_record = Some(record); }

    pub fn set_refinement_pass(&mut self, refinement: bool) { self.refinement_pass = refinement; }

    pub fn has_work(&self) -> bool { self.parameters.is_some_and(|parameters| !parameters.mask.is_empty()) }

    pub fn two_pass_exposure(&self, records: &mut impl Records) -> bool { records.setting(Setting::ContextMode) == 0 }

    pub fn skips_lens_synchronization(&self, records: &mut impl Records) -> bool { records.setting(Setting::LensPolicy) != 0 }

    /// Every half press but a mode-6 one records the lens status.
    pub fn lens_status_pending(&self) -> bool { self.parameters.is_some_and(|parameters| parameters.mode.0 != 6) }

    pub fn release_live_resources(&mut self, platform: &mut impl Platform) { platform.hold(Hold::Preparation, false); }

    /// Prepares a half press (`Some` parameters) or the live sequence (`None`, with the live
    /// parameters' lens reference). The half-press synchronizations have run.
    pub fn prepare<P: Boundary>(&mut self, parameters: Option<HalfPressParameters>, live_reference: LensReference, shot: &mut impl PreparationShot<P>, boundary: &mut P) {
        boundary.trace(Trace::Prepare);
        self.parameters = parameters;
        let mode = parameters.map(|parameters| parameters.mode.0);
        let measuring = mode == Some(6);

        let mut result_kept = true;
        if !measuring {
            boundary.hold(Hold::Preparation, true);
            boundary.select_first_mode();
            shot.bind_lens_reference(parameters.map_or(live_reference, |parameters| parameters.lens_reference));
            boundary.take_change();
            let reading = boundary.read(LensReading::Reference);
            shot.begin_lens_track(reading);
            let lens_reference = shot.lens_reference();
            snapshot_lens(lens_reference, boundary);
            if let Some(target) = parameters.and_then(|parameters| parameters.lens_target) { write_lens_target(target, boundary); }
            if parameters.is_some() { result_kept = revalidate_results(lens_reference, boundary); }
            if let Some(record) = self.record {
                shot.prepare_selection(record, parameters.and_then(|parameters| parameters.area_count), parameters.is_none(), boundary);
            }
        }
        let lens_reference = shot.lens_reference();
        self.candidates.reset(parameters.is_some() && result_kept, lens_reference, boundary);
        if let Some(parameters) = parameters { subjects::await_selection(parameters.mode, boundary); }
        if !measuring {
            self.lens_control = boundary.read(LensReading::Control);
            boundary.clear_exposure_record();
            self.pipeline_gate = 2;
            self.level = boundary.level();
        }
        self.lens_return = ReturnPoint::default();

        match parameters {
            Some(parameters) => {
                if parameters.mode.0 == 2 { drop_result_unless_wide_area(boundary); }
                if !measuring { refine_result(parameters, boundary); }
                if !measuring {
                    if let Some(slot) = parameters.frame_slot { boundary.clear_frame_progress(slot); }
                }
            }
            None => {
                boundary.focus_result();
            }
        }
        boundary.reset_image_progress();
        if let Some(parameters) = parameters { publish_areas(parameters, boundary); }

        let frame = parameters.and_then(|parameters| announce_frame(parameters, boundary));
        if let Some(frame) = frame.filter(|_| !measuring) { dump_frame(frame, boundary); }
        if matches!(mode, Some(0 | 3 | 5)) { boundary.load_lens_capabilities(); }
        if boundary.setting(Setting::ContextMode) == 1 && parameters.is_some() && !measuring && boundary.setting(Setting::SelectionOption) != 0x39 {
            boundary.publish(Publication::CaptureFrame(frame));
        }
        boundary.publish(Publication::CaptureLink(0));
        shot.carry(mode);
    }
}

fn snapshot_lens(lens_reference: LensReference, boundary: &mut (impl Records + Lens)) {
    let mut snapshot = boundary.lens_snapshot();
    snapshot.current.position = boundary.read(LensReading::Position) as i16;
    snapshot.current.reference = lens_reference;
    snapshot.current.kind = boundary.read(LensReading::Kind);
    snapshot.current.state = boundary.read(LensReading::State);
    snapshot.held = snapshot.current;
    snapshot.marker = 0;
    boundary.store_lens_snapshot(&snapshot);
}

/// Results taken at another lens reference stop holding. Returns whether the focus result was
/// taken at `lens_reference`.
fn revalidate_results(lens_reference: LensReference, records: &mut impl Records) -> bool {
    let mut result = records.focus_result();
    let mut confirmation = records.focus_confirmation();
    let mut history = records.focus_history();
    let kept = result.reference == lens_reference;
    if !kept {
        result.valid = 0;
        result.kept = 0;
        records.store_focus_result(&result);
        history.valid = 0;
        records.store_focus_history(&history);
    }
    if confirmation.active != 0 && confirmation.reference != lens_reference {
        confirmation.active = 0;
        records.store_focus_confirmation(&confirmation);
    }
    kept
}

fn write_lens_target(target: ClientRef, boundary: &mut (impl Records + Lens + Client)) {
    let snapshot = boundary.lens_snapshot();
    let point = boundary.designated_point().unwrap_or(snapshot.current);
    let step = boundary.target_step(point);
    let control = boundary.read(LensReading::Control);
    let distance = boundary.target_distance(point, control);
    boundary.write_lens_target(target, point, step, distance);
}

fn drop_result_unless_wide_area(records: &mut impl Records) {
    let layout = records.setting(Setting::AreaLayout);
    if matches!((layout >> 16, layout & 0xffff), (4, 3) | (3, 2)) { return; }
    let mut result = records.focus_result();
    result.valid = 0;
    result.kept = 0;
    records.store_focus_result(&result);
}

fn refine_result(parameters: HalfPressParameters, records: &mut impl Records) {
    let mut result = records.focus_result();
    if parameters.mode.0 == 1 && records.setting(Setting::CaptureVariant) == 1 {
        result.refined = 0;
        records.store_focus_result(&result);
    }
    if !parameters.mask.publishes_areas() && result.extent != 128 {
        result.refined = 0;
        records.store_focus_result(&result);
    }
}

fn publish_areas(parameters: HalfPressParameters, boundary: &mut (impl Records + Client)) {
    if parameters.mask.publishes_area_selection() { boundary.publish(Publication::AreaSelection(parameters.area_selection)); }
    if parameters.mask.publishes_areas() {
        if let (Some(count), Some(areas)) = (parameters.area_count, parameters.areas) {
            let mut index = 0;
            while (index as u32) < boundary.area_count(count) {
                let word = boundary.area(areas, index);
                boundary.publish(Publication::Area { index, word });
                index += 1;
            }
        }
    }
    if parameters.mask.publishes_lens_target() {
        if let Some(target) = parameters.lens_target { boundary.publish(Publication::LensTargetArea(target)); }
    }
}

fn announce_frame(parameters: HalfPressParameters, boundary: &mut impl Client) -> Option<Frame> {
    let frame = boundary.frame(parameters.frame_slot?)?;
    let header = FrameHeader { mode: parameters.mode, mask: parameters.mask, option: parameters.option, lens_reference: parameters.lens_reference };
    boundary.announce_frame(frame, header);
    Some(frame)
}

fn dump_frame(frame: Frame, boundary: &mut impl Boundary) {
    let argument = boundary.setting(Setting::DumpArgument);
    if boundary.adjustment(Adjustment::StatisticsDump) & 0x80 != 0 { boundary.dump(Dump::Statistics, frame, argument); }
    boundary.dump(Dump::Frame, frame, 0);
    if (4..=6).contains(&boundary.setting(Setting::LensPolicy)) {
        boundary.dump(Dump::Lens, frame, 0);
        if boundary.calculation_context_mode() == 1 { boundary.dump_calculation(); }
    }
    let statistics = boundary.area_statistics();
    boundary.write_summary(frame, &summary::summarize(&statistics));
    if boundary.setting(Setting::ContextMode) == 1 { boundary.dump(Dump::Numbered, frame, 0); }
}
