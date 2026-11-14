use crate::fakes::{Outbound, Ports};
use crate::history::MeasurementHistory;
use crate::lanes::Selection;
use crate::acquisition::ports::AcquisitionBlock;
use crate::auxiliary::{AuxiliaryPublication, ResourceIndex};
use crate::profile::{FrameInterval, GridCell, ProfileMode, IntervalHistory, IntervalTracking, ProfileSelection, PublishedResult, RetainedWord, StatisticsInUse};
use crate::pipeline::{GainStep, PipelineWord, SnapshotStatus, Statistics, TimingWord};
use crate::record::{self, Request, SearchMode};
use crate::round::{ParameterClient, RecordEffect, SearchStart};
use xm1_saclac_interface::round::{GeometryCalibration, GeometryPair, MeasurementGeometry, RetainedGeometryValue};
use xm1_saclac_interface::{stock, CommandKind, CompletionStatus, RecordHandle, ReplyTarget};
use std::vec::Vec;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }

fn apply(effect: RecordEffect, record: RecordHandle, ports: &mut Ports) { record::apply(effect, record, &mut MeasurementHistory::new(), ports) }

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
    let mut history = MeasurementHistory::new();
    record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
    assert!(history.started());
    ports.take();
    ports.records.intervals = IntervalHistory::new([FrameInterval(7); IntervalHistory::CAPACITY], 3);
    record::apply(RecordEffect::ClearSelection, record(), &mut history, &mut ports);
    assert!(!history.started());
    assert!(ports.records.intervals.intervals().is_empty());
    assert!(ports.take().is_empty());
}

#[test]
fn a_record_without_the_grant_keeps_the_history_started() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    let mut history = MeasurementHistory::new();
    record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
    ports.records.granted = false;
    record::apply(RecordEffect::ClearSelection, record(), &mut history, &mut ports);
    assert!(history.started());
}

#[test]
fn a_measurement_tick_enters_the_history_capture_with_the_focus_cell_levels() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.records.profile.focus_cell = GridCell { column: 2, row: 5 };
    ports.cell_levels = std::vec![(GridCell { column: 2, row: 5 }, [700, 650]), (GridCell { column: 3, row: 3 }, [1, 1])];
    ports.history.banks[0][0] = 0xa0;
    ports.history.banks[1][63] = 0xb1;
    ports.history.word = PipelineWord(0x51);
    ports.snapshot.levels = [400, 380];
    ports.snapshot.retained = [1, 2, 3, 4, 5, 6].map(PipelineWord);
    let mut history = MeasurementHistory::new();
    record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
    assert_eq!(ports.take(), [Outbound::Capture(Statistics::History)]);
    let entry = history.newest();
    assert!(entry.written());
    assert_eq!(entry.cell_levels(), [700, 650]);
    assert_eq!((entry.statistics().banks[0][0], entry.statistics().banks[1][63], entry.statistics().word), (0xa0, 0xb1, PipelineWord(0x51)));
    assert_eq!(entry.reference_level(), 380);
    assert_eq!(entry.retained(), [1, 2, 3, 4, 5, 6].map(PipelineWord));
}

#[test]
fn the_history_exposure_is_the_timing_offset_and_acquisition_level_less_the_window_level() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.timing_offset = 900;
    ports.records.acquisition_level = -30;
    ports.snapshot.levels = [400, 380];
    ports.snapshot.gain_step = GainStep(3);
    ports.gain_offsets[3] = 25;
    ports.exposure_value = -7;
    let mut history = MeasurementHistory::new();
    record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
    assert_eq!(history.newest().exposure(), 900 - 25 - 30 - (400 - 380) + 7);

    ports.timing_offset = 0;
    ports.snapshot.levels = [0, 0x8000];
    ports.records.acquisition_level = 0;
    ports.gain_offsets[3] = 0;
    ports.exposure_value = 0;
    record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
    assert_eq!(history.newest().exposure(), i16::MIN);
}

#[test]
fn an_incomplete_history_snapshot_leaves_the_history_alone() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    let mut history = MeasurementHistory::new();
    for status in [0, 1, 4, 0xfffa] {
        ports.snapshot.status = SnapshotStatus(status);
        record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
        assert_eq!(history, MeasurementHistory::new(), "status {status:#x}");
    }
    ports.snapshot.status = SnapshotStatus(0xfffd);
    ports.snapshot.timing = TimingWord(9);
    record::apply(RecordEffect::CollectReady, record(), &mut history, &mut ports);
    assert!(history.started());
}

/// A collected acquisition block and a published auxiliary curve, as the shared record holds them.
fn published(ports: &mut Ports) {
    ports.acquisition.block = AcquisitionBlock { lens: ResourceIndex(0x76d9c8), collected: 1, level: 0x123, ..AcquisitionBlock::default() };
    ports.auxiliary.publication = AuxiliaryPublication { curve: [1, 2, 3], levels: [4, 5, 6], published: true, curve_word: RetainedWord(7) };
}

#[test]
fn finishing_a_selection_clear_answers_the_client_and_invalidates_what_is_no_longer_offered() {
    let mut ports = Ports::new(Some(Selection { auxiliary: true, ..ACQUISITION }), ());
    published(&mut ports);
    let client = stock::optional_callback(0xf0a0);
    apply(RecordEffect::FinishSelectionClear(ParameterClient(client)), record(), &mut ports);
    let target = ReplyTarget::Command(CommandKind::Command17);
    assert_eq!(ports.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback: client.unwrap(), status: CompletionStatus(0), values: [0; 3] },
    ]);
    assert_eq!(ports.records.invalid, [PublishedResult::Search, PublishedResult::Source]);
    assert_eq!(ports.acquisition.block.collected, 1);
    assert!(ports.auxiliary.publication.published);
    assert!(!ports.records.focus_invalid);
}

#[test]
fn a_record_that_offers_nothing_clears_each_results_valid_word_and_the_focus_result() {
    let mut ports = Ports::new(None, ());
    published(&mut ports);
    apply(RecordEffect::FinishSelectionClear(ParameterClient(None)), record(), &mut ports);
    assert_eq!(ports.take(), [Outbound::ClearPending(ReplyTarget::Command(CommandKind::Command17))]);
    assert_eq!(ports.acquisition.block, AcquisitionBlock { lens: ResourceIndex(0x76d9c8), collected: 0, level: 0x123, ..AcquisitionBlock::default() });
    assert_eq!(ports.auxiliary.publication, AuxiliaryPublication { curve: [1, 2, 3], levels: [4, 5, 6], published: false, curve_word: RetainedWord(7) });
    assert_eq!(ports.records.invalid, [PublishedResult::Search, PublishedResult::Source]);
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
        Outbound::Capture(Statistics::History),
    ]);

    ports.records.in_use = StatisticsInUse { primary: true, search: true, auxiliary: false };
    ports.records.profile.mode = ProfileMode(6);
    apply(RecordEffect::CollectReady, record(), &mut ports);
    assert_eq!(ports.take(), [
        Outbound::Capture(Statistics::Primary),
        Outbound::ModeSixStatistics,
        Outbound::Capture(Statistics::Search),
        Outbound::Capture(Statistics::History),
    ]);
}

#[test]
fn the_mode_six_request_follows_the_primary_statistics_only() {
    let mut ports = Ports::new(Some(ACQUISITION), ());
    ports.records.in_use = StatisticsInUse { primary: false, search: true, auxiliary: false };
    ports.records.profile.mode = ProfileMode(6);
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
    ports.set_current_interval(Some(FrameInterval(33_333)));
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    assert!(ports.records.intervals.intervals().is_empty());

    ports.interval_history = true;
    ports.records.profile.interval_tracking = IntervalTracking::Record;
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    apply(RecordEffect::SelectionHistory, record(), &mut ports);
    assert_eq!(ports.records.intervals.intervals(), intervals(&[33_333, 33_333]));

    ports.set_current_interval(None);
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
    ports.set_current_interval(Some(FrameInterval(16_666)));
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
    ports.records.profile.selection = ProfileSelection::Other(2);
    apply(RecordEffect::SearchMode(SearchStart::Search), record(), &mut ports);
    apply(RecordEffect::SearchMode(SearchStart::Image), record(), &mut ports);
    assert_eq!(ports.take(), [
        Outbound::Request(Request::SearchMode(SearchMode::Search { profile_selected: false })),
        Outbound::Request(Request::SearchMode(SearchMode::Search { profile_selected: true })),
        Outbound::Request(Request::SearchMode(SearchMode::Image)),
    ]);
}
