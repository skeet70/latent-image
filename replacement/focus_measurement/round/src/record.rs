#![forbid(unsafe_code)]

//! The round's record effects carried out against the profile records, the
//! property store, the imaging pipeline and the image peer.

use crate::history::{HistoryEntry, MeasurementHistory};
use crate::pipeline::{Exposure, Pipeline, PipelineSnapshot, Statistics};
use crate::planner::{Cadence, Mode, Work};
use crate::profile::{FrameRate, IntervalHistory, IntervalTracking, ProfileRecords, ProfileSelection, PublishedResult, RetainedWord};
use crate::round::{ParameterClient, RecordEffect, SearchStart};
use xm1_saclac_controllers::replies;
use xm1_saclac_interface::round::{GeometryCalibration, MeasurementGeometry};
use xm1_saclac_interface::{CommandKind, CompletionStatus, RecordHandle, ReplyTarget};

/// When a suspended or resumed round suspends the search lane instead of
/// stopping and restarting it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlternateSearch { Never, Always, WithRecordSelection, WithoutSelection }

impl AlternateSearch {
    pub const fn applies(self, selection: ProfileSelection) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::WithRecordSelection => matches!(selection, ProfileSelection::Record),
            Self::WithoutSelection => matches!(selection, ProfileSelection::Unselected),
        }
    }
}

pub trait Settings {
    fn search_permitted(&mut self) -> bool;
    fn alternate_search(&mut self) -> AlternateSearch;
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence;
    /// The frame rate that separates the planner's two frame-rate cadences.
    fn frame_rate_threshold(&mut self) -> FrameRate;
    /// Whether the round keeps the frame-interval history at all.
    fn keeps_interval_history(&mut self) -> bool;
    fn geometry_calibration(&mut self) -> GeometryCalibration;
    /// Whether measurement requests leave trace marks.
    fn traces_measurement(&mut self) -> bool;
}

/// Requests or releases the power the window calculation needs.
pub trait Power {
    fn request_measurement_power(&mut self, held: bool);
}

/// The adjustment state's words other modules write and the round's requests carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdjustmentWords {
    /// One while the controllers' adjustment is set.
    pub controllers: RetainedWord,
    pub search: RetainedWord,
}

pub trait Adjustment {
    fn adjustment_words(&mut self) -> AdjustmentWords;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceMark { WindowRequest, AuxiliaryRequest }

pub trait Trace {
    fn trace(&mut self, mark: TraceMark);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchMode { Search { profile_selected: bool }, Image }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    /// Answered by the image peer with completion 0x45.
    SearchMode(SearchMode),
    Geometry(MeasurementGeometry),
    Window(crate::window::WindowRequest),
    Auxiliary(crate::auxiliary::AuxiliaryRequest),
}

pub trait Ports: ProfileRecords + Settings + Pipeline + Exposure + Power + Adjustment + Trace + replies::Effects {
    fn request(&mut self, request: Request);
}

pub fn apply(effect: RecordEffect, record: RecordHandle, history: &mut MeasurementHistory, ports: &mut impl Ports) {
    match effect {
        RecordEffect::ResetSelection => ports.detach_capture_buffer(record),
        RecordEffect::ClearSelection => {
            history.restart(ports.grants_measurement_history(record));
            clear_intervals(record, ports);
        }
        RecordEffect::FinishSelectionClear(client) => finish_selection_clear(client, record, ports),
        RecordEffect::Geometry => geometry(record, ports),
        RecordEffect::CollectReady => collect(record, history, ports),
        RecordEffect::SelectionHistory => {
            if !ports.keeps_interval_history() { return; }
            match ports.profile(record).interval_tracking {
                IntervalTracking::Record => record_interval(record, ports),
                IntervalTracking::Clear | IntervalTracking::Keep => clear_intervals(record, ports),
            }
        }
        RecordEffect::SearchMode(start) => {
            let mode = match start {
                SearchStart::Search => SearchMode::Search { profile_selected: ports.profile(record).selection != ProfileSelection::Unselected },
                SearchStart::Image => SearchMode::Image,
            };
            ports.request(Request::SearchMode(mode));
        }
        RecordEffect::Planned { primary_considered } => {
            if !primary_considered || !ports.keeps_interval_history() { return; }
            match ports.profile(record).interval_tracking {
                IntervalTracking::Record => record_interval(record, ports),
                IntervalTracking::Clear => clear_intervals(record, ports),
                IntervalTracking::Keep => {}
            }
        }
    }
}

fn finish_selection_clear(client: ParameterClient, record: RecordHandle, ports: &mut impl Ports) {
    replies::deliver(replies::Reply {
        target: ReplyTarget::Command(CommandKind::Command17),
        callback: client.0,
        status: CompletionStatus(0),
        values: xm1_saclac_interface::stock::parameters(&[]),
    }, ports);
    let offered = ports.offered();
    let kinds = offered.unwrap_or_default();
    for (offered, result) in [
        (kinds.acquisition, PublishedResult::Acquisition),
        (kinds.search, PublishedResult::Search),
        (kinds.auxiliary, PublishedResult::Auxiliary),
        (kinds.source, PublishedResult::Source),
    ] {
        if !offered { ports.invalidate(record, result); }
    }
    if offered.is_none() { ports.invalidate_focus_result(); }
}

fn geometry(record: RecordHandle, ports: &mut impl Ports) {
    let profile = ports.geometry(record);
    let calibration = ports.geometry_calibration();
    ports.request(Request::Geometry(MeasurementGeometry {
        resource: profile.resource,
        calibration,
        converted_profile: profile.converted_profile,
        first_parameter: profile.first_parameter,
        second_parameter: profile.second_parameter,
    }));
}

fn collect(record: RecordHandle, history: &mut MeasurementHistory, ports: &mut impl Ports) {
    let in_use = ports.statistics_in_use();
    if in_use.primary {
        ports.capture(Statistics::Primary);
        if ports.profile(record).mode.six() { ports.request_mode_six_statistics(); }
    }
    if in_use.search { ports.capture(Statistics::Search); }
    if in_use.auxiliary { ports.capture(Statistics::Auxiliary); }
    record_history(record, history, ports);
}

fn record_history(record: RecordHandle, history: &mut MeasurementHistory, ports: &mut impl Ports) {
    ports.capture(Statistics::History);
    let snapshot = ports.snapshot(Statistics::History);
    if !snapshot.status.complete() { return; }
    let cell = ports.profile(record).focus_cell;
    let statistics = ports.history_statistics(cell);
    let exposure = history_exposure(&snapshot, ports);
    let kept = ports.grants_measurement_history(record);
    history.record(HistoryEntry::new(&snapshot, statistics, exposure), kept);
}

/// The timing's offset and the acquisition's level, less the window level the snapshot gives.
fn history_exposure(snapshot: &PipelineSnapshot, ports: &mut impl Ports) -> i16 {
    let level = ports.acquisition_level() as u16;
    let exposure = ports.exposure_value(snapshot.exposure) as u16;
    let gain = ports.gain_offset(snapshot.gain_step) as u16;
    let timing = ports.timing_offset(snapshot.timing);
    let [first, second] = snapshot.levels;
    timing.wrapping_sub(gain).wrapping_add(level).wrapping_sub(first.wrapping_sub(second)).wrapping_sub(exposure) as i16
}

fn record_interval(record: RecordHandle, ports: &mut impl Ports) {
    let timing = ports.timing();
    let Some(interval) = ports.frame_interval(timing) else { return };
    let mut history = ports.interval_history();
    history.record(interval);
    ports.set_interval_history(record, history);
}

fn clear_intervals(record: RecordHandle, ports: &mut impl Ports) {
    ports.set_interval_history(record, IntervalHistory::default());
}
