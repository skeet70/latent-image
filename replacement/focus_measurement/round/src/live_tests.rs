use crate::acquisition::Branch;
use crate::acquisition::ports::ReleasedResult;
use crate::fakes::{AcquisitionTraffic, Outbound, Ports, Published};
use crate::image::{ChannelCount, LensReport};
use crate::lanes::Selection;
use crate::live::{Effects, Measurement};
use crate::pipeline::{Statistics, TimingWord};
use crate::planner::{Cadence, Mode, Work};
use crate::profile::{FrameInterval, FrameRate, ProfileSelection, RetainedWord, WindowPosition, WindowStatus};
use crate::record::Request;
use crate::source::{ProcessingProfile, SegmentSettings, SourceProfile, SourceStrength};
use crate::window::WindowResult;
use crate::round::{LaneSignal, LaneStatus};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl};
use xm1_saclac_interface::round_image::{ImageArea, ImageArguments, ImageChannel, ImageLaneRequest};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, CompletionStatus, RecordHandle, ReplyTarget};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Traffic {
    Search(&'static str),
    Fault,
}

struct Live { traffic: Vec<Traffic> }

impl Live {
    fn take(&mut self) -> Vec<Traffic> { core::mem::take(&mut self.traffic) }
}

type Fake = Ports<Live>;

fn fake(available: Selection) -> Fake { Ports::new(Some(available), Live { traffic: Vec::new() }) }

fn acquisition_requests(outbound: &[Outbound]) -> usize {
    outbound.iter().filter(|traffic| matches!(traffic, Outbound::Acquisition(AcquisitionTraffic::Acquisition(Branch::Live, _)))).count()
}

impl Effects for Fake {
    fn fault(&mut self) { self.extra.traffic.push(Traffic::Fault); }
    fn search(&mut self, signal: LaneSignal<'_>, _: RecordHandle) -> LaneStatus {
        let (name, status) = match signal {
            LaneSignal::Work(_) => ("work", LaneStatus::Pending),
            LaneSignal::Initialize => ("initialize", LaneStatus::Complete),
            LaneSignal::Enable => ("enable", LaneStatus::Complete),
            LaneSignal::Stop => ("stop", LaneStatus::Complete),
            _ => ("other", LaneStatus::Complete),
        };
        self.extra.traffic.push(Traffic::Search(name));
        status
    }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }
fn control(control: MeasurementControl) -> ChildEvent<'static, MeasurementControl> {
    ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }
}
fn completion(completion: Completion) -> ChildEvent<'static, MeasurementControl> { finished(completion, &[]) }
fn finished(completion: Completion, parameters: &[u8]) -> ChildEvent<'_, MeasurementControl> {
    ChildEvent::Completed { completed: Completed { completion, parameters: stock::parameters(parameters) }, record: record() }
}

fn command(words: &[u8]) -> ChildEvent<'_, MeasurementControl> {
    ChildEvent::Command { command: Command { kind: CommandKind::Command14, parameters: stock::parameters(words) }, record: record() }
}

const ACQUISITION: Selection = Selection { acquisition: true, search: false, auxiliary: false, image: false, source: false };

#[test]
fn initialization_enables_acquisition_and_window_and_initializes_the_other_lanes() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    fake.image_client = Some(StockCallbackWord(0x00e0_1234));
    assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.extra.take(), [Traffic::Search("initialize")]);
    assert!(fake.records.in_use.primary);
    let outbound = fake.take();
    assert!(outbound.contains(&Outbound::NotifyImageClient(StockCallbackWord(0x00e0_1234))));
    assert!(outbound.iter().any(|traffic| matches!(traffic, Outbound::Acquisition(AcquisitionTraffic::Exposure(_)))));
    assert!(fake.records.in_use.primary && fake.records.in_use.search);
    assert!(fake.auxiliary.publication.published);
    assert_eq!((fake.source.published, fake.source.callbacks_enabled), (Some(Published::Withdrawn), Some(false)));
}

#[test]
fn a_round_offering_the_source_reads_it_after_the_window_and_publishes_the_average() {
    let mut measurement = Measurement::new();
    let mut fake = fake(Selection { source: true, ..ACQUISITION });
    fake.adjustment.controllers = RetainedWord(1);
    fake.source.profile = SourceProfile {
        measures_source: true,
        processing: ProcessingProfile { selection: ProfileSelection::Record, ..ProcessingProfile::default() },
        ..SourceProfile::default()
    };
    fake.source.settings.segments = [SegmentSettings { ring_length: 1, newest_weight: 0, gain: 128 }; 4];
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    assert!(matches!(fake.source.published, Some(Published::Fresh { .. })));
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    fake.take();
    measurement.handle(completion(Completion::Calculation8d), &mut fake);
    assert!(fake.take().iter().any(|traffic| matches!(traffic, Outbound::Request(Request::Source(_)))));
    let reading = [7, 0, 0, 0, 0, 0];
    let completed = Completed { completion: Completion::Calculation9a, parameters: stock::parameters(&reading) };
    measurement.handle(ChildEvent::Completed { completed, record: record() }, &mut fake);
    assert!(matches!(fake.source.published, Some(Published::Averaged { strength: SourceStrength(7), .. })));
}

#[test]
fn a_round_submits_window_and_primary_acquisition_and_collects_their_results() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
    let outbound = fake.take();
    assert_eq!(acquisition_requests(&outbound), 1);
    assert!(outbound.iter().any(|traffic| matches!(traffic, Outbound::Request(Request::Window(_)))));
    fake.window_result = Some(WindowResult { status: WindowStatus(5), position: WindowPosition([1, 2]) });
    assert_eq!(measurement.handle(completion(Completion::Calculation8d), &mut fake), ChildOutcome::Pending);
    assert_eq!((fake.records.window.status, fake.records.window_position), (WindowStatus(5), WindowPosition([1, 2])));
    let result = xm1_saclac_interface::calc::SharedAddress(0x0169_0000);
    fake.acquisition.acquisition_result = Some(xm1_saclac_interface::round_acquisition::AcquisitionResult { address: result, ..Default::default() });
    assert_eq!(measurement.handle(completion(Completion::Calculation87), &mut fake), ChildOutcome::Pending);
    assert!(fake.take().contains(&Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Acquisition(result)))));
    assert_eq!(fake.acquisition.block.collected, 1);
}

#[test]
fn stopping_a_round_releases_each_enabled_job() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    fake.take();
    assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut fake), ChildOutcome::Complete);
    assert_eq!(fake.extra.take(), [Traffic::Search("stop")]);
    assert_eq!(fake.take(), [Outbound::Acquisition(AcquisitionTraffic::Release)]);
}

#[test]
fn a_release_under_a_snapshot_grid_also_moves_the_extra_statistics_and_copies_the_primary_ones() {
    for snapshot_grid in [false, true] {
        let mut measurement = Measurement::new();
        let mut fake = fake(ACQUISITION);
        fake.acquisition.settings.snapshot_grid = snapshot_grid;
        fake.acquisition.extra[..4].copy_from_slice(&[10, 20, 30, 40]);
        measurement.handle(control(MeasurementControl::Initialize), &mut fake);
        measurement.handle(completion(Completion::Accessor4b), &mut fake);
        fake.acquisition.acquisition_result = Some(xm1_saclac_interface::round_acquisition::AcquisitionResult {
            address: xm1_saclac_interface::calc::SharedAddress(0x0169_0000),
            aspect: 7,
            correction: 2,
            count: 0x0001_0003,
            statistics_valid: true,
            ..Default::default()
        });
        measurement.handle(completion(Completion::Calculation87), &mut fake);
        measurement.handle(control(MeasurementControl::Stop), &mut fake);
        let [release] = &fake.acquisition.releases[..] else { panic!("one release") };
        // Three extra values, from the count's low halfword, each moved by the aspect less the correction.
        let copy = fake.acquisition.block.copy;
        assert_eq!(release.snapshot, snapshot_grid.then(|| (std::vec![15, 25, 35], copy)));
        assert_ne!(copy, crate::acquisition::ports::StatisticsBuffer(0));
    }
}

#[test]
fn an_image_command_asks_imgdc_and_replies_with_its_status() {
    let mut measurement = Measurement::new();
    let mut fake = fake(Selection { image: true, ..ACQUISITION });
    fake.channel_count = ChannelCount(8);
    fake.lens = LensReport { first: true, second: false, third: true };
    fake.image_area = |channel, area| ImageArea([area.0[0] + channel.0 as i16, -1]);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.extra.take();
    fake.take();
    let image = |channel: u32| -> Vec<u8> { [0xd00, 0x11, 0x22, channel, 9].iter().flat_map(|word: &u32| word.to_le_bytes()).collect() };
    let target = ReplyTarget::Command(CommandKind::Command14);
    let callback = stock::optional_callback(0xd00).unwrap();

    let words = image(5);
    assert_eq!(measurement.handle(command(&words), &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.take(), [
        Outbound::PrepareImageArea(ImageChannel(5), ImageArea([0, 0])),
        Outbound::Image(ImageLaneRequest::Image { arguments: ImageArguments([0x11, 0x22]), area: ImageArea([5, -1]), channel: ImageChannel(5) }),
    ]);
    measurement.handle(finished(Completion::Image47, &[1, 0, 0, 0, 0, 0, 0, 0]), &mut fake);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback, status: CompletionStatus(-94), values: [5, 9, 6] },
    ]);

    let words = image(8);
    measurement.handle(command(&words), &mut fake);
    assert_eq!(fake.take()[1], Outbound::Image(ImageLaneRequest::Image { arguments: ImageArguments([0x11, 0x22]), area: ImageArea([13, -1]), channel: ImageChannel(8) }));
    measurement.handle(finished(Completion::Image47, &[0; 8]), &mut fake);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback, status: CompletionStatus(0), values: [8, 0x80_0000, 6] },
    ]);
}

#[test]
fn a_measurement_tick_captures_the_statistics_in_use_and_records_the_history() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    fake.records.in_use.primary = true;
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    assert!(!fake.records.capture_attached);
    assert!(!measurement.history().started());
    fake.take();
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    let traffic = fake.take();
    assert_eq!(traffic[..3], [Outbound::Capture(Statistics::Primary), Outbound::Capture(Statistics::Search), Outbound::Capture(Statistics::History)]);
    assert!(matches!(traffic[3], Outbound::Request(Request::Window(_))));
    assert!(measurement.history().newest().written());
}

#[test]
fn a_parameter_change_with_nothing_to_change_answers_its_client() {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    let block: Vec<u8> = [0xf0a0u32, 1, 1].iter().flat_map(|word| word.to_le_bytes()).collect();
    let change = ChildEvent::Command { command: Command { kind: CommandKind::Command17, parameters: stock::parameters(&block) }, record: record() };
    assert_eq!(measurement.handle(change, &mut fake), ChildOutcome::Complete);
    let target = ReplyTarget::Command(CommandKind::Command17);
    assert_eq!(fake.take(), [
        Outbound::ClearPending(target),
        Outbound::Reply { target, callback: stock::optional_callback(0xf0a0).unwrap(), status: CompletionStatus(0), values: [0; 3] },
    ]);
}

fn primary_in(mode: Mode, work: Work, current: Mode) -> Cadence {
    let admitted = work != Work::PrimaryAcquisition || current == mode;
    Cadence { first: admitted, second: admitted, third: admitted, fourth: admitted }
}
fn fast(work: Work, mode: Mode) -> Cadence { primary_in(Mode::AboveThreshold, work, mode) }
fn slow(work: Work, mode: Mode) -> Cadence { primary_in(Mode::BelowThreshold, work, mode) }
fn by_record(work: Work, mode: Mode) -> Cadence { primary_in(Mode::RecordSelected, work, mode) }
fn by_profile(work: Work, mode: Mode) -> Cadence { primary_in(Mode::ProfileSelected, work, mode) }

/// A round over a profile that paces by frame rate, after `configure`; whether its first tick submits primary acquisition.
fn submits_primary(configure: impl FnOnce(&mut Fake)) -> bool {
    let mut measurement = Measurement::new();
    let mut fake = fake(ACQUISITION);
    fake.records.profile.frame_rate_cadence = true;
    fake.frame_rate_threshold = FrameRate(30);
    fake.acquisition.view.start = crate::acquisition::ports::Start::Fresh;
    fake.acquisition.range.timing = TimingWord(0x55);
    fake.set_current_interval(Some(FrameInterval(1_000)));
    configure(&mut fake);
    measurement.handle(control(MeasurementControl::Initialize), &mut fake);
    fake.take();
    measurement.handle(completion(Completion::Accessor4b), &mut fake);
    acquisition_requests(&fake.take()) == 1
}

#[test]
fn a_frame_rate_profile_paces_the_round_by_the_acquisitions_timing() {
    let paced = |interval: u32, cadence: fn(Work, Mode) -> Cadence| submits_primary(|fake| {
        fake.intervals.push((TimingWord(0x55), FrameInterval(interval)));
        fake.cadence = cadence;
    });
    assert!(paced(33_333, fast));
    assert!(!paced(40_000, fast));
    assert!(paced(40_000, slow));
    assert!(submits_primary(|fake| fake.cadence = slow));
}

#[test]
fn record_and_profile_selection_take_precedence_over_the_frame_rate() {
    assert!(submits_primary(|fake| { fake.records.profile.selection = ProfileSelection::Record; fake.cadence = by_record; }));
    assert!(submits_primary(|fake| { fake.records.profile.profile_cadence = true; fake.cadence = by_profile; }));
    assert!(!submits_primary(|fake| { fake.records.profile.profile_cadence = true; fake.cadence = by_record; }));
}

#[test]
fn each_live_start_publishes_its_exposure_and_initial_levels_before_requests() {
    use crate::acquisition::ports::{Start, ExposurePublication, ExposureRecordUpdate, Program};
    use crate::acquisition_fixtures as fixture;
    use crate::pipeline::{GainStep, PipelineWord};
    use xm1_saclac_interface::round_acquisition::{PrimaryLevels, SelectionLevels};
    for start in [Start::Restore, Start::Fresh, Start::Resume] {
        let mut measurement = Measurement::new();
        let mut fake = fake(ACQUISITION);
        fixture::configure(&mut fake);
        fake.acquisition.view.start = start;
        fake.acquisition.no_program = true;
        fake.acquisition.program = Program(0x56);
        fake.acquisition.published.reference_level = 1000;
        fake.acquisition.published.aspect = 9;
        assert_eq!(measurement.handle(control(MeasurementControl::Initialize), &mut fake), ChildOutcome::Complete);
        let traffic = fake.take();
        let publications: Vec<_> = traffic.into_iter().filter_map(|event| match event {
            Outbound::Acquisition(AcquisitionTraffic::Exposure(value)) => Some(value), _ => None,
        }).collect();
        let expected = ExposurePublication { exposure: PipelineWord(0x3030), level: 700, second: 100, reference: 100,
            decade: 200, offset: 20, correction: 0, gain_step: GainStep(3), timing: TimingWord(0x9090), code: 0x66,
            selected: 3, adjustment: 0, combined: 620, reference_level: 720, aspect: 7 };
        assert_eq!(publications.last(), Some(&expected));
        assert_eq!(publications.len(), if start == Start::Restore { 2 } else { 1 });
        assert_eq!(fake.acquisition.target_queries[0].program, Program(0x56));
        assert_eq!(fake.acquisition.target_queries[0].level, match start { Start::Fresh => 200, Start::Resume => 991, Start::Restore => 24 });
        assert_eq!(fake.acquisition.primary_levels, Some(PrimaryLevels { base: 600, levels: [600; 7], word: 0 }));
        assert_eq!(fake.acquisition.selection_levels, Some(SelectionLevels { base: 600, levels: [600; 3], reference: 600, tail: [600; 3], ..SelectionLevels::default() }));
        assert!(fake.acquisition.exposure_updates.contains(&ExposureRecordUpdate::Context { lens: crate::auxiliary::ResourceIndex(0x4040), index: 0x3030 }));
        assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
        let requests: Vec<_> = fake.take().into_iter().filter_map(|event| match event { Outbound::Acquisition(AcquisitionTraffic::Acquisition(Branch::Live, request)) => Some(request), _ => None }).collect();
        assert_eq!(requests, [fixture::primary_request(Branch::Live, true)]);
    }
}

#[test]
fn live_round_fourth_tick_selects_publishes_and_then_starts_a_new_primary() {
    use crate::acquisition::ports::*;
    use crate::acquisition_fixtures as fixture;
    use crate::fakes::ShotCall;
    use crate::pipeline::PipelineWord;
    use xm1_saclac_interface::round_acquisition::*;
    for recording in [false, true] {
        let mut measurement = Measurement::new();
        let mut fake = fake(ACQUISITION);
        fixture::configure(&mut fake);
        fake.adjustment.recording = RetainedWord(u32::from(recording));
        fake.cadence = |work, _| match work {
            Work::PrimaryAcquisition => Cadence { first: true, second: true, third: true, fourth: false },
            Work::SecondaryAcquisition => Cadence { first: false, second: false, third: false, fourth: true },
            _ => Cadence { first: true, second: true, third: true, fourth: true },
        };
        measurement.handle(control(MeasurementControl::Initialize), &mut fake);
        fake.take();
        fake.acquisition.acquisition_result = Some(fixture::primary_result());
        for phase in 0..4 {
            assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
            let traffic = fake.take();
            let requests: Vec<_> = traffic.iter().filter_map(|event| match event {
                Outbound::Acquisition(AcquisitionTraffic::Acquisition(Branch::Live, request)) => Some(*request), _ => None,
            }).collect();
            if phase < 3 {
                assert_eq!(requests, [fixture::primary_request(Branch::Live, phase == 0)]);
                assert!(!traffic.iter().any(|event| matches!(event, Outbound::Acquisition(AcquisitionTraffic::Selection(_, _)))));
                measurement.handle(completion(Completion::Calculation8d), &mut fake);
                assert_eq!(measurement.handle(completion(Completion::Calculation87), &mut fake), ChildOutcome::Pending);
                let traffic = fake.take();
                assert_eq!(fake.acquisition.primary_levels, Some(fixture::primary_result().levels));
                assert!(traffic.contains(&Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Acquisition(fixture::primary_result().address)))));
                assert_eq!(fake.acquisition.block.collected, 1);
                if phase > 0 {
                    assert!(traffic.contains(&Outbound::Acquisition(AcquisitionTraffic::ChangeNotice(ChangeNotice {
                        category: 31, mode_is_one: 0, lens: 0x2020, positions: [0, 0], unbounded: 0xffff, pair: [29, 30], kind: 3,
                    }))));
                }
            } else {
                assert!(requests.is_empty());
                let requests: Vec<_> = traffic.into_iter().filter_map(|event| match event {
                    Outbound::Acquisition(AcquisitionTraffic::Selection(Branch::Live, request)) => Some(request), _ => None,
                }).collect();
                assert_eq!(requests, [fixture::selection_request(Branch::Live, true, recording)]);
                if recording {
                    assert_eq!(fake.acquisition.restored_settings, [(Branch::Live, RestoredSetting { margin: 65488, reference: 100, lens_halfword: 25, lens_reference: 26 })]);
                    assert_eq!(fake.acquisition.settings_written, [(Branch::Live, SelectionSetting {
                        lens: 0x4040, shot_word: 0, margin: 65488, category: 0, one_mode: 0, context_word: 0,
                        selection: 1, updated_position: 44, position_halfword: 45, mode_word: 5, byte: 6, halfword: 7,
                        setting_word: 8, grid_kind: 9, focus_word: 10, fourth_word: RetainedWord(16), request: 22, rate_mode: 13,
                        word: 2, reference: 100, restore_adjustment: 47, selection_word: 24, lens_halfword: 25, lens_reference: 26,
                        halfwords: [27, 32, 28], eighth_word: RetainedWord(19), mode_rule: 14, category_word: 3, change_category: 31,
                        second_word: RetainedWord(15), mode_flag: 1, rate_flag: false, fifth_word: RetainedWord(17), ninth_word: RetainedWord(20),
                        profile_flags: [0, 2, 3, 4, 5, 6, 7, 8], tenth_word: RetainedWord(21), lens_limit: 51, lens_word_index: 33,
                        kind: 35, kind_override: 36, adjustment: 34, profile_index: 48, program_word: 38, restore_position: 40, hardware_ready: false,
                    })]);
                    assert_eq!(fake.acquisition.windows, [(Branch::Live, SelectionWindow {
                        kind: 2, selection: 1, updated_position: 44, position_halfword: 45, focus_word: 10, lens: 0x4040, lens_reference: 26,
                        mode_rule: 14, second_word: RetainedWord(15), mode_word: 5, request: 22, rate: 1,
                        relative_level: 65516, resource_level: 60, target_level: 1620, ratio: [49, 50], change_category: 31,
                        category: 3, limit_halfword: 27, ..SelectionWindow::default()
                    })]);
                } else {
                    assert!(fake.acquisition.windows.is_empty());
                    assert!(fake.acquisition.settings_written.is_empty());
                }
                let result = fixture::selection_result();
                fake.acquisition.selection_result = Some(result);
                fake.acquisition.range_passthrough = true;
                measurement.handle(completion(Completion::Calculation8d), &mut fake);
                assert_eq!(measurement.handle(completion(Completion::Calculation88), &mut fake), ChildOutcome::Measurement(xm1_saclac_controllers::children::MeasurementOutcome::Update { update: xm1_saclac_controllers::children::MeasurementUpdate::NoPeerRequest, restart: xm1_saclac_controllers::children::Restart::Pending }));
                let traffic = fake.take();
                let selected = SelectionNotice { kind: 3, level: 120, word: 0x8181, limit_level: 113, flags: [1, 1], recording: u32::from(recording), levels: [131, 132, 133, 134], position: 0x8282 };
                assert_eq!(fake.acquisition.selection, SelectionRecord { position: 0x8282, word: 0x8181, level: 120,
                    levels: [131, 132, 133, 134], target: 121, limit_level: 135, reference: 114, flags: [1, 2, 3, 1], limit: 129, tracked: 1 });
                assert_eq!(fake.acquisition.selection_levels, Some(result.levels));
                let candidates = |exposure, level, second, offset| SelectionEntry { timing: 0x9090, code: 0x66, selected: 3, gain_step: 3,
                    exposure, level, second, offset, word: 123, reference: 122, decade: 126 };
                let table = SelectionTable {
                    primary: [candidates(0x8000, 201, 235, 209), candidates(0x8000, 202, 236, 210), candidates(0x8000, 203, 237, 211), candidates(0x8000, 204, 238, 212)],
                    secondary: [candidates(0x9000, 301, 335, 309), candidates(0x9000, 302, 336, 310), candidates(0x9000, 303, 337, 311), candidates(0x9000, 304, 338, 312)], levels: [103, 104, 105],
                };
                assert_eq!(traffic, [Outbound::Acquisition(AcquisitionTraffic::Selected(selected)), Outbound::Acquisition(AcquisitionTraffic::SelectionTable(table)),
                    Outbound::Acquisition(AcquisitionTraffic::Released(ReleasedResult::Selection { result: result.address, shot: Some((result.shot_record, result.saved_record)) }))]);
                assert_eq!(fake.acquisition.restore_levels, Some((122, [118, 119])));
                assert!(fake.acquisition.shot_calls.ends_with(&[ShotCall::Write(result.shot_record), ShotCall::Copy(result.saved_record)]));
                assert_eq!((fake.acquisition.block.selected, fake.acquisition.block.setting_position, fake.acquisition.block.setting_halfword, fake.acquisition.block.setting_shot_word), (1, 0x81, 0x82, 0x83));
                assert_eq!(measurement.handle(completion(Completion::Accessor4b), &mut fake), ChildOutcome::Pending);
                let requests: Vec<_> = fake.take().into_iter().filter_map(|event| match event { Outbound::Acquisition(AcquisitionTraffic::Acquisition(_, request)) => Some(request), _ => None }).collect();
                let mut expected = fixture::primary_request(Branch::Live, false);
                expected.correction = 0;
                assert_eq!(requests, [expected]);
                assert_eq!(fake.acquisition.published.exposure, PipelineWord(0x1010));
            }
        }
        assert_eq!(fake.faults, 0);
    }
}

#[test]
fn release_classifies_boundaries_weights_each_side_and_moves_every_buffer() {
    use crate::acquisition::ports::*;
    use crate::acquisition_fixtures as fixture;
    use crate::fakes::ReleaseRecord;
    for (adjustment, class_profile, configured) in [(4, 7, -2i16), (34, 7, -3), (34, 1, -4)] {
        let mut measurement = Measurement::new();
        let mut fake = fake(ACQUISITION);
        fixture::configure(&mut fake);
        fake.acquisition.view.adjustment = adjustment;
        fake.acquisition.view.profile_flags[6] = class_profile;
        fake.acquisition.settings.snapshot_grid = true;
        fake.acquisition.settings.release = ReleaseSettings { bounds: [-1, -2, 1, 2], weights: [[2, 3], [5, 7], [11, 13], [17, 19]], offsets: [-2, -3, -4] };
        let mut primary = [915; 256];
        primary[..9].copy_from_slice(&[894, 895, 904, 905, 915, 925, 926, 935, 936]);
        fake.acquisition.statistics_banks = Some([primary, [100; 256], [200; 256], [300; 256]]);
        fake.acquisition.extra = core::array::from_fn(|index| index as u16);
        measurement.handle(control(MeasurementControl::Initialize), &mut fake);
        measurement.handle(completion(Completion::Accessor4b), &mut fake);
        let mut result = fixture::primary_result();
        result.count = 0x10000 + 553;
        fake.acquisition.acquisition_result = Some(result);
        measurement.handle(completion(Completion::Calculation8d), &mut fake);
        measurement.handle(completion(Completion::Calculation87), &mut fake);
        fake.take();
        assert_eq!(measurement.handle(control(MeasurementControl::Stop), &mut fake), ChildOutcome::Complete);
        assert_eq!(fake.take(), [Outbound::Acquisition(AcquisitionTraffic::Release)]);
        let mut classes = [0; 256];
        classes[..9].copy_from_slice(&[4, 2, 2, 0, 0, 0, 1, 1, 3]);
        let offset = 5u16.wrapping_sub(configured as u16);
        assert_eq!(fake.acquisition.releases, [ReleaseRecord { classes,
            offsets: [primary.map(|value| value.wrapping_add(offset)), [100 + offset; 256], [200 + offset; 256], [300 + offset; 256]],
            snapshot: Some(((0..552).map(|value| value + offset).collect(), crate::stock::RELEASE_COPY)),
        }]);
        assert_eq!(fake.acquisition.block.weights, [7, 17, 35, 53]);
        assert_eq!(fake.acquisition.block.release_offset, configured as u16);
        assert_eq!(fake.acquisition.block.release_halfword, 43);
        assert_eq!((fake.acquisition.block.classes, fake.acquisition.block.offsets, fake.acquisition.block.extra, fake.acquisition.block.copy),
            (crate::stock::RELEASE_CLASSES, crate::stock::RELEASE_OFFSETS, crate::stock::RELEASE_EXTRA, crate::stock::RELEASE_COPY));
        assert_eq!(fake.acquisition.selection, SelectionRecord::default());
    }
}
