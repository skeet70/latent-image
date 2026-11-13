use crate::fakes::{Outbound, Ports};
use crate::lanes::Selection;
use crate::profile::{FrameInterval, IntervalHistory, IntervalTracking, ProfileSelection, PublishedResult, StatisticsInUse};
use crate::record::{apply, Request, SearchMode, Statistics};
use crate::round::{ParameterClient, RecordEffect, SearchStart};
use xm1_saclac_interface::round::{GeometryCalibration, GeometryPair, MeasurementGeometry, RetainedGeometryValue};
use xm1_saclac_interface::{stock, CommandKind, CompletionStatus, RecordHandle, ReplyTarget};
use std::vec::Vec;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }

const ACQUISITION: Selection = Selection { acquisition: true, search: false, auxiliary: false, image: false, source: false };

fn intervals(values: &[u32]) -> Vec<FrameInterval> { values.iter().copied().map(FrameInterval).collect() }

#[test]
fn a_reset_detaches_the_capture_buffer_when_the_record_may_write_it() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    apply(RecordEffect::ResetSelection, record(), &mut ports);
    assert!(!ports.records.capture_attached);

    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.records.granted = false;
    apply(RecordEffect::ResetSelection, record(), &mut ports);
    assert!(ports.records.capture_attached);
}

#[test]
fn clearing_the_selection_restarts_the_measurement_history_and_forgets_the_intervals() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.records.intervals = IntervalHistory::new([FrameInterval(7); IntervalHistory::CAPACITY], 3);
    apply(RecordEffect::ClearSelection, record(), &mut ports);
    assert!(ports.records.history_restarted);
    assert!(ports.records.intervals.intervals().is_empty());
    assert!(ports.take().is_empty());
}

#[test]
fn finishing_a_selection_clear_answers_the_client_and_invalidates_what_is_no_longer_offered() {
    let mut ports = Ports::new(Some(Selection { auxiliary: true, ..ACQUISITION }), ());
    let client = stock::optional_callback(0xf0a0);
    apply(RecordEffect::FinishSelectionClear(ParameterClient(client)), record(), &mut ports);
    let target = ReplyTarget::Command(CommandKind::Command17);
    assert_eq!(ports.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback: client.unwrap(), status: CompletionStatus(0) },
    ]);
    assert_eq!(ports.records.invalid, [PublishedResult::Search, PublishedResult::Source]);
    assert!(!ports.records.focus_invalid);
}

#[test]
fn a_record_that_offers_nothing_also_invalidates_the_focus_result() {
    let mut ports = Ports::new(None, ());
    apply(RecordEffect::FinishSelectionClear(ParameterClient(None)), record(), &mut ports);
    assert_eq!(ports.take(), [Outbound::ClearPending(ReplyTarget::Command(CommandKind::Command17))]);
    assert_eq!(ports.records.invalid, [PublishedResult::Acquisition, PublishedResult::Search, PublishedResult::Auxiliary, PublishedResult::Source]);
    assert!(ports.records.focus_invalid);
}

#[test]
fn geometry_joins_the_profile_geometry_with_the_calibration_settings() {
    let mut ports = Ports::new(None, ());
    ports.records.geometry.resource = GeometryPair([640, 480]);
    ports.records.geometry.converted_profile = RetainedGeometryValue(3);
    ports.records.geometry.first_parameter = RetainedGeometryValue(4);
    ports.records.geometry.second_parameter = RetainedGeometryValue(5);
    ports.calibration = GeometryCalibration([1, 2, 3, 4]);
    apply(RecordEffect::Geometry, record(), &mut ports);
    assert_eq!(ports.take(), [Outbound::Request(Request::Geometry(MeasurementGeometry {
        resource: GeometryPair([640, 480]),
        calibration: GeometryCalibration([1, 2, 3, 4]),
        converted_profile: RetainedGeometryValue(3),
        first_parameter: RetainedGeometryValue(4),
        second_parameter: RetainedGeometryValue(5),
    }))]);
}

#[test]
fn a_measurement_tick_captures_the_statistics_in_use_then_records_the_history() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.records.in_use = StatisticsInUse { primary: true, search: false, auxiliary: true };
    apply(RecordEffect::CollectReady, record(), &mut ports);
    assert_eq!(ports.take(), [
        Outbound::Capture(Statistics::Primary),
        Outbound::Capture(Statistics::Auxiliary),
        Outbound::MeasurementHistory,
    ]);

    ports.records.in_use = StatisticsInUse { primary: true, search: true, auxiliary: false };
    ports.records.profile.mode_six = true;
    apply(RecordEffect::CollectReady, record(), &mut ports);
    assert_eq!(ports.take(), [
        Outbound::Capture(Statistics::Primary),
        Outbound::ModeSixStatistics,
        Outbound::Capture(Statistics::Search),
        Outbound::MeasurementHistory,
    ]);
}

#[test]
fn the_mode_six_request_follows_the_primary_statistics_only() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.records.in_use = StatisticsInUse { primary: false, search: true, auxiliary: false };
    ports.records.profile.mode_six = true;
    apply(RecordEffect::CollectReady, record(), &mut ports);
    assert!(!ports.take().contains(&Outbound::ModeSixStatistics));
}

#[test]
fn the_interval_history_keeps_the_last_ten_intervals() {
    let mut history = IntervalHistory::default();
    for value in 1..=12 { history.record(FrameInterval(value)); }
    assert_eq!(history.intervals(), intervals(&[3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
    assert_eq!(IntervalHistory::new([FrameInterval(1); IntervalHistory::CAPACITY], 40).intervals().len(), IntervalHistory::CAPACITY);
}

#[test]
fn a_running_tick_records_the_frame_interval_or_clears_the_history_by_profile() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.frame_interval = Some(FrameInterval(33_333));
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    assert!(ports.records.intervals.intervals().is_empty());

    ports.interval_history = true;
    ports.records.profile.interval_tracking = IntervalTracking::Record;
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    assert_eq!(ports.records.intervals.intervals(), intervals(&[33_333, 33_333]));

    ports.frame_interval = None;
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    assert_eq!(ports.records.intervals.intervals().len(), 2);

    ports.records.profile.interval_tracking = IntervalTracking::Keep;
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    assert!(ports.records.intervals.intervals().is_empty());
}

#[test]
fn a_plan_that_considered_primary_acquisition_updates_the_interval_history() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.interval_history = true;
    ports.frame_interval = Some(FrameInterval(16_666));
    ports.records.profile.interval_tracking = IntervalTracking::Record;
    apply(RecordEffect::Planned { primary_considered: false }, record(), &mut ports);
    assert!(ports.records.intervals.intervals().is_empty());
    apply(RecordEffect::Planned { primary_considered: true }, record(), &mut ports);
    assert_eq!(ports.records.intervals.intervals(), intervals(&[16_666]));

    ports.records.profile.interval_tracking = IntervalTracking::Keep;
    apply(RecordEffect::Planned { primary_considered: true }, record(), &mut ports);
    assert_eq!(ports.records.intervals.intervals(), intervals(&[16_666]));

    ports.records.profile.interval_tracking = IntervalTracking::Clear;
    apply(RecordEffect::Planned { primary_considered: true }, record(), &mut ports);
    assert!(ports.records.intervals.intervals().is_empty());
}

#[test]
fn the_search_mode_names_the_lane_and_whether_the_profile_selects() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    apply(RecordEffect::SearchMode(SearchStart::Search), record(), &mut ports);
    ports.records.profile.selection = ProfileSelection::Other;
    apply(RecordEffect::SearchMode(SearchStart::Search), record(), &mut ports);
    apply(RecordEffect::SearchMode(SearchStart::Image), record(), &mut ports);
    assert_eq!(ports.take(), [
        Outbound::Request(Request::SearchMode(SearchMode::Search { profile_selected: false })),
        Outbound::Request(Request::SearchMode(SearchMode::Search { profile_selected: true })),
        Outbound::Request(Request::SearchMode(SearchMode::Image)),
    ]);
}
