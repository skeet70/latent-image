#![forbid(unsafe_code)]

//! The round's record effects carried out against the profile records, the
//! property store, the imaging pipeline and the image peer.

use crate::pipeline::{Exposure, Pipeline, Statistics};
use crate::planner::{Cadence, Mode, Work};
use crate::profile::{IntervalHistory, IntervalTracking, ProfileRecords, ProfileSelection, PublishedResult};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceMark { WindowRequest }

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
}

pub trait Ports: ProfileRecords + Settings + Pipeline + Exposure + Power + Trace + replies::Effects {
    fn request(&mut self, request: Request);
    fn record_measurement_history(&mut self, record: RecordHandle);
}

pub fn apply(effect: RecordEffect, record: RecordHandle, ports: &mut impl Ports) {
    match effect {
        RecordEffect::ResetSelection => ports.detach_capture_buffer(record),
        RecordEffect::ClearSelection => {
            ports.restart_measurement_history(record);
            clear_intervals(record, ports);
        }
        RecordEffect::FinishSelectionClear(client) => finish_selection_clear(client, record, ports),
        RecordEffect::Geometry => geometry(record, ports),
        RecordEffect::CollectReady => collect(record, ports),
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

fn collect(record: RecordHandle, ports: &mut impl Ports) {
    let in_use = ports.statistics_in_use();
    if in_use.primary {
        ports.capture(Statistics::Primary);
        if ports.profile(record).mode.six() { ports.request_mode_six_statistics(); }
    }
    if in_use.search { ports.capture(Statistics::Search); }
    if in_use.auxiliary { ports.capture(Statistics::Auxiliary); }
    ports.record_measurement_history(record);
}

fn record_interval(record: RecordHandle, ports: &mut impl Ports) {
    let Some(interval) = ports.frame_interval() else { return };
    let mut history = ports.interval_history();
    history.record(interval);
    ports.set_interval_history(record, history);
}

fn clear_intervals(record: RecordHandle, ports: &mut impl Ports) {
    ports.set_interval_history(record, IntervalHistory::default());
}
