use crate::contrast::settings::{Setting, StepKind};
use crate::contrast::{LensIdentity, Step};
use crate::s1::{S1Completion, S1Record, S1Search, S1Snapshot};
use crate::stock::s1::{completion, s1_snapshot, S1_SNAPSHOT_BYTES};
use crate::test_platform::{Effect, Fake};
use std::{vec, vec::Vec};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, SearchControl as C};
use xm1_saclac_controllers::search::LensPosition;
use xm1_saclac_interface::search::{BridgeReport, BridgeStatus, LensClass, ResultAddress, S1DriveRequest, ScanEvaluation, SearchRecord};
use xm1_saclac_interface::stock::search::EVALUATION_BYTES;
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, RecordHandle};

const CLIMB_STEP: Step = Step { size: 9, rate: 5 };
const TRACK_STEP: Step = Step { size: 12, rate: 7 };
const SWEEP_STEP: Step = Step { size: 20, rate: 6 };
const RESULT_ADDRESS: [u8; 4] = [0x40, 0x0f, 0x6c, 0x01];
const MOVED: i32 = 0;
const DEFERRED: i32 = 21;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x5678) }

/// A record that runs the S1 search with the lens at -30 in a travel range of -200..=90, and
/// a retained focus that starts it tracking.
fn fake() -> Fake {
    let mut fake = Fake::new();
    fake.set(Setting::S1Search, 1);
    fake.set(Setting::ClimbStepLimit, 50);
    fake.set(Setting::WideRangeTens, 30);
    for setting in [Setting::ReturnRises, Setting::PeakReturns, Setting::ReturnStrikes] {
        fake.set(setting, 10);
    }
    fake.s1_record = S1Record { scale: 0x0003_0002, selection: 0, mode: 4, profile_class: 6, resource: 8, near: 500, far: 100, step: 3, timing: 11 };
    fake.zone_limits = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    fake.near = 50;
    fake.s1_snapshot = S1Snapshot { size: [640, 480], lens: LensIdentity(2), windows: [[1, 2], [3, 4]], extent: [5, 6], frame_mode: 0, pinned_setting: 7 };
    fake.steps = vec![(StepKind::Climb, CLIMB_STEP), (StepKind::Track, TRACK_STEP), (StepKind::Sweep, SWEEP_STEP)];
    fake
}

fn control(search: &mut S1Search, fake: &mut Fake, control: C) -> ChildOutcome {
    search.handle(ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }, fake)
}

fn complete(search: &mut S1Search, fake: &mut Fake, completion: Completion, payload: &[u8]) -> ChildOutcome {
    search.handle(ChildEvent::Completed { completed: Completed { completion, parameters: stock::parameters(payload) }, record: record() }, fake)
}

fn evaluated(search: &mut S1Search, fake: &mut Fake) -> ChildOutcome {
    let mut payload = [0u8; 8];
    payload[4..].copy_from_slice(&RESULT_ADDRESS);
    complete(search, fake, Completion::Calculation9d, &payload)
}

fn reported(search: &mut S1Search, fake: &mut Fake, status: i32, position: i16) -> ChildOutcome {
    let mut payload = [0u8; 12];
    payload[0..4].copy_from_slice(&status.to_le_bytes());
    payload[4..6].copy_from_slice(&position.to_le_bytes());
    payload[8..12].copy_from_slice(&2u32.to_le_bytes());
    complete(search, fake, Completion::Image5a, &payload)
}

fn drive(travel: i16, rate: u32, moves: bool, speed: u32) -> Effect {
    Effect::Drive(S1DriveRequest { travel, rate, moves, speed, lens_mode: 0 })
}

fn release() -> Effect { drive(0, 0, false, 2) }

fn evaluations(fake: &Fake) -> Vec<SearchRecord> {
    fake.traffic().into_iter().filter_map(|effect| if let Effect::Evaluate(record) = effect { Some(record) } else { None }).collect()
}

fn initialized(fake: &mut Fake) -> S1Search {
    let mut search = S1Search::new();
    assert_eq!(control(&mut search, fake, C::Initialize), ChildOutcome::Complete);
    search
}

#[test]
fn a_record_that_does_not_run_the_s1_search_leaves_it_idle() {
    let configurations: [fn(&mut Fake); 4] = [
        |fake| fake.set(Setting::S1Search, 0),
        |fake| fake.s1_record.selection = 1,
        |fake| fake.s1_record.mode = 3,
        |fake| fake.s1_record.mode = 7,
    ];
    for configure in configurations {
        let mut fake = fake();
        configure(&mut fake);
        let mut search = initialized(&mut fake);
        assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
        assert_eq!(control(&mut search, &mut fake, C::Conclude), ChildOutcome::Complete);
        assert_eq!(control(&mut search, &mut fake, C::Stop), ChildOutcome::Complete);
        assert!(fake.traffic().is_empty());
    }
}

#[test]
fn the_first_frame_goes_to_calc_with_the_records_range_and_the_frames_window() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
    let far = 500 - (100 - 30);
    assert_eq!(evaluations(&fake), [SearchRecord {
        first: false,
        scale: [2, 3],
        settled: true,
        level_matched: true,
        selection: 0,
        mode: 4,
        ratio: [0; 2],
        range: [50, far],
        range_offset: 100 - 30,
        extent: [5, 6],
        window: [1, 2],
        frame_mode: 0,
        scan: false,
        profile_class: 6,
        resource: 8,
        scan_evaluation: ScanEvaluation::Plain,
        size: [640, 480],
        timing: 11,
        decade: 0,
        correction: 0,
        limit: far,
    }]);
}

#[test]
fn a_frame_mode_1_frame_takes_the_second_window() {
    let mut fake = fake();
    fake.s1_snapshot.frame_mode = 1;
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    assert_eq!(evaluations(&fake).iter().map(|record| (record.window, record.frame_mode)).collect::<Vec<_>>(), [([3, 4], 1)]);
}

#[test]
fn one_evaluation_is_in_flight_and_its_result_drives_the_track_by_its_step() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
    assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
    assert_eq!(evaluations(&fake).len(), 1);
    assert_eq!(evaluated(&mut search, &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.results_read, [ResultAddress(0x016c_0f40)]);
    assert_eq!(fake.traffic().last(), Some(&drive(TRACK_STEP.size as i16, TRACK_STEP.rate, true, 2)));
}

#[test]
fn a_drive_asked_for_while_one_is_in_flight_is_dropped() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    evaluated(&mut search, &mut fake);
    control(&mut search, &mut fake, C::Start);
    evaluated(&mut search, &mut fake);
    let moved = drive(TRACK_STEP.size as i16, TRACK_STEP.rate, true, 2);
    assert!(matches!(fake.traffic().as_slice(), [Effect::Evaluate(_), drive, Effect::Evaluate(_)] if *drive == moved));
    assert_eq!(reported(&mut search, &mut fake, MOVED, -18), ChildOutcome::Pending);
    control(&mut search, &mut fake, C::Start);
    evaluated(&mut search, &mut fake);
    assert_eq!(fake.traffic().len(), 5);
    assert!(matches!(fake.traffic().last(), Some(Effect::Drive(request)) if request.moves));
}

#[test]
fn a_stop_waits_for_the_evaluation_in_flight_then_releases_the_lens() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    assert_eq!(control(&mut search, &mut fake, C::Stop), ChildOutcome::Pending);
    assert_eq!(fake.traffic().len(), 1);
    assert_eq!(evaluated(&mut search, &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.traffic().last(), Some(&release()));
    assert_eq!(reported(&mut search, &mut fake, MOVED, -30), ChildOutcome::Complete);
    assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
    assert_eq!(fake.traffic().len(), 2);
}

#[test]
fn a_cancel_while_the_lens_drives_waits_for_its_report_then_releases_and_parks() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    evaluated(&mut search, &mut fake);
    assert_eq!(control(&mut search, &mut fake, C::Cancel), ChildOutcome::Pending);
    assert_eq!(fake.traffic().len(), 2);
    assert_eq!(reported(&mut search, &mut fake, MOVED, -18), ChildOutcome::Pending);
    assert_eq!(fake.traffic().last(), Some(&drive(0, TRACK_STEP.rate, false, 2)));
    assert_eq!(reported(&mut search, &mut fake, MOVED, -18), ChildOutcome::Complete);
    assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
    assert_eq!(fake.traffic().len(), 3);
    assert_eq!(control(&mut search, &mut fake, C::Resume), ChildOutcome::Complete);
}

#[test]
fn before_the_first_frame_a_stop_or_conclude_ends_the_search_and_a_cancel_waits_for_resume() {
    for (end, after) in [(C::Stop, C::Initialize), (C::Conclude, C::Initialize), (C::Cancel, C::Resume)] {
        let mut fake = fake();
        let mut search = initialized(&mut fake);
        assert_eq!(control(&mut search, &mut fake, end), ChildOutcome::Complete);
        assert_eq!(control(&mut search, &mut fake, C::Start), ChildOutcome::Pending);
        assert_eq!(control(&mut search, &mut fake, after), ChildOutcome::Complete);
        assert!(fake.traffic().is_empty());
    }
}

#[test]
fn a_resumed_search_climbs_from_where_the_lens_is() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Cancel);
    fake.position = LensPosition(-40);
    assert_eq!(control(&mut search, &mut fake, C::Resume), ChildOutcome::Complete);
    control(&mut search, &mut fake, C::Start);
    evaluated(&mut search, &mut fake);
    assert_eq!(fake.traffic().last(), Some(&drive(0, CLIMB_STEP.rate, true, 2)));
}

#[test]
fn a_settled_climb_that_probes_at_once_sweeps_and_has_calc_evaluate_the_scan_pairs_too() {
    let mut fake = fake();
    fake.retained.spread = 2;
    fake.result[100..104].copy_from_slice(&1u32.to_le_bytes());
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    evaluated(&mut search, &mut fake);
    control(&mut search, &mut fake, C::Start);
    let scans: Vec<_> = evaluations(&fake).iter().map(|record| record.scan_evaluation).collect();
    assert_eq!(scans, [ScanEvaluation::Plain, ScanEvaluation::Scan]);
    assert_eq!(fake.traffic().len(), 2);
}

#[test]
fn a_conclude_drives_to_the_best_position_and_releases_the_lens_once_it_moved_there() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    assert_eq!(control(&mut search, &mut fake, C::Conclude), ChildOutcome::Pending);
    assert_eq!(evaluated(&mut search, &mut fake), ChildOutcome::Pending);
    assert_eq!(fake.traffic().last(), Some(&drive(0, 0, true, 0)));
    assert_eq!(reported(&mut search, &mut fake, DEFERRED, -30), ChildOutcome::Pending);
    assert_eq!(fake.traffic().last(), Some(&drive(0, 0, true, 0)));
    assert_eq!(reported(&mut search, &mut fake, MOVED, -30), ChildOutcome::Pending);
    assert_eq!(fake.traffic().last(), Some(&release()));
    assert_eq!(reported(&mut search, &mut fake, MOVED, -30), ChildOutcome::Complete);
    assert_eq!(fake.traffic().len(), 4);
}

#[test]
fn a_conclude_aims_back_into_the_travel_range_at_the_slow_speed() {
    let mut fake = fake();
    fake.position = LensPosition(150);
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    reported(&mut search, &mut fake, MOVED, 150);
    control(&mut search, &mut fake, C::Conclude);
    evaluated(&mut search, &mut fake);
    assert_eq!(fake.traffic().last(), Some(&drive(90 - 150, 0, true, 2)));
}

#[test]
fn events_out_of_place_take_the_fault_reaction() {
    let mut fake = fake();
    let mut search = S1Search::new();
    assert_eq!(reported(&mut search, &mut fake, MOVED, 0), ChildOutcome::Fault);
    assert_eq!(evaluated(&mut search, &mut fake), ChildOutcome::Fault);
    control(&mut search, &mut fake, C::Initialize);
    assert_eq!(control(&mut search, &mut fake, C::Initialize), ChildOutcome::Fault);
    assert_eq!(control(&mut search, &mut fake, C::Resume), ChildOutcome::Fault);
    control(&mut search, &mut fake, C::Cancel);
    assert_eq!(control(&mut search, &mut fake, C::Stop), ChildOutcome::Fault);
    assert_eq!(fake.traffic(), vec![Effect::Fault; 5]);
}

#[test]
fn other_completions_commands_and_short_payloads_are_not_the_searchs() {
    let mut fake = fake();
    let mut search = initialized(&mut fake);
    control(&mut search, &mut fake, C::Start);
    assert_eq!(complete(&mut search, &mut fake, Completion::Image59, &[0; 12]), ChildOutcome::Inactive);
    assert_eq!(complete(&mut search, &mut fake, Completion::Calculation9d, &[0; 4]), ChildOutcome::Inactive);
    assert_eq!(complete(&mut search, &mut fake, Completion::Image5a, &[0; 8]), ChildOutcome::Inactive);
    let command = Command { kind: CommandKind::Stop, parameters: stock::parameters(&[]) };
    assert_eq!(search.handle(ChildEvent::Command { command, record: record() }, &mut fake), ChildOutcome::Inactive);
    assert_eq!(fake.traffic().len(), 1);
    assert_eq!(evaluated(&mut search, &mut fake), ChildOutcome::Pending);
}

#[test]
fn the_s1_snapshot_is_read_at_its_offsets() {
    let mut bytes = [0u8; S1_SNAPSHOT_BYTES];
    let mut put = |offset: usize, value: u32| bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    for (offset, value) in [(0x00, 640), (0x04, 480), (0x28, 3), (0x2c, 11), (0x30, 12), (0x34, 21), (0x38, 22), (0x3c, 31), (0x40, 32), (0x44, 1), (0x4c, 0xffff_0007)] {
        put(offset, value);
    }
    assert_eq!(s1_snapshot(&bytes), S1Snapshot {
        size: [640, 480],
        lens: LensIdentity(3),
        windows: [[11, 12], [31, 32]],
        extent: [21, 22],
        frame_mode: 1,
        pinned_setting: 7,
    });
}

#[test]
fn the_completions_carry_calcs_result_and_imgdcs_report() {
    let mut result = [0u8; EVALUATION_BYTES];
    result[0x60..0x64].copy_from_slice(&9u32.to_le_bytes());
    let mut payload = [0u8; 8];
    payload[4..].copy_from_slice(&RESULT_ADDRESS);
    let mut read = Vec::new();
    let evaluation = completion(Completed { completion: Completion::Calculation9d, parameters: stock::parameters(&payload) }, |address| {
        read.push(address);
        result
    });
    assert_eq!(read, [ResultAddress(0x016c_0f40)]);
    assert!(matches!(evaluation, Some(S1Completion::Evaluated(evaluation)) if evaluation.reference == 9));
    let report = [0xeb, 0xff, 0xff, 0xff, 0xf6, 0xff, 0, 0, 2, 0, 0, 0];
    let reported = completion(Completed { completion: Completion::Image5a, parameters: stock::parameters(&report) }, |_| result);
    assert_eq!(reported, Some(S1Completion::LensReported(BridgeReport { status: BridgeStatus::Other(-21), position: -10, class: LensClass(2) })));
}
