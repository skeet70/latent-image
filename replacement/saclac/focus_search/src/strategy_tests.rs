//! The contrast search at its strategy boundary: what it asks of the lens bridge for each
//! evaluation, and what it leaves in the record, the focus log and the debug display.

use crate::contrast::settings::{Setting, StepKind};
use crate::contrast::{BridgeDrive, ContrastSearch, Context, LensIdentity, LensMove, Phase, Step};
use crate::live::{LiveStrategy, Measurement};
use crate::test_platform::{Effect, Fake};
use std::vec;
use std::vec::Vec;
use xm1_saclac_controllers::search::{Direction, FocusSearch, LensPosition, SearchDecision, SearchInput, SearchStep, Travel};
use xm1_saclac_interface::search::{BridgePhase, BridgeReport, BridgeStatus, Evaluation, LensClass};

const DIRECTION_STEP: Step = Step { size: 10, rate: 3 };
const SCAN_STEP: Step = Step { size: 16, rate: 5 };
const RETURN_STEP: Step = Step { size: 6, rate: 7 };

/// The search with a lens that carries out each move and reports where it went.
struct Harness {
    search: ContrastSearch<Fake>,
    context: Context,
    lens: i16,
    /// The position the lens had reported when each evaluation arrived.
    reports: Vec<i16>,
}

impl Harness {
    fn new(configure: impl FnOnce(&mut Fake)) -> Self {
        let mut fake = Fake::new();
        fake.steps = vec![(StepKind::Direction, DIRECTION_STEP), (StepKind::Scan, SCAN_STEP), (StepKind::Return, RETURN_STEP)];
        configure(&mut fake);
        let lens = fake.position.0;
        Self { search: ContrastSearch::new(fake), context: Context::new(), lens, reports: Vec::new() }
    }

    fn fake(&mut self) -> &mut Fake { self.search.platform_mut() }

    fn input(&mut self, input: SearchInput<Measurement<Evaluation>>) -> SearchStep<BridgeDrive> {
        let SearchDecision::Next(step) = self.search.advance(&mut self.context, input) else {
            panic!("the stock search never ends");
        };
        step
    }

    fn begin(&mut self) -> SearchStep<BridgeDrive> { self.input(SearchInput::Begin) }

    /// Takes in a frame, evaluates it as `evaluation`, and lets the lens carry out a move.
    fn evaluate(&mut self, evaluation: Evaluation, bridge_busy: bool) -> SearchStep<BridgeDrive> {
        self.search.take_in(&mut self.context);
        self.reports.push(self.lens);
        let step = self.input(SearchInput::Sample(Measurement { evaluation, bridge_busy }));
        if let SearchStep::Travel { travel, drive } = step {
            if drive.moves {
                self.lens += signed(travel);
            }
            let report = BridgeReport { status: BridgeStatus::Moved, position: self.lens, class: LensClass(2) };
            self.search.lens_reported(&mut self.context, report);
        }
        step
    }

    fn sample(&mut self, contrast: u32) -> SearchStep<BridgeDrive> { self.evaluate(evaluation(contrast), false) }

    fn phases(&self) -> Vec<Phase> { self.search.platform().transitions().iter().map(|trace| trace.phase).collect() }

    /// Where the frame the next evaluation is for was taken: an evaluation belongs to the
    /// position reported two evaluations earlier (0xf44fec).
    fn frame_position(&self) -> i16 { self.reports.len().checked_sub(1).map_or(self.lens, |newest| self.reports[newest.saturating_sub(1)]) }
}

fn evaluation(contrast: u32) -> Evaluation { Evaluation { pairs: [[contrast, contrast / 2]; 3], settled: 1, ..Evaluation::default() } }

fn signed(travel: Travel) -> i16 {
    match travel.direction {
        Direction::Increasing => travel.steps as i16,
        Direction::Decreasing => -(travel.steps as i16),
    }
}

fn travel(step: SearchStep<BridgeDrive>) -> (i16, BridgeDrive) {
    match step {
        SearchStep::Travel { travel, drive } => (signed(travel), drive),
        other => panic!("expected a travel, got {other:?}"),
    }
}

/// A search that starts in direction search, traced, with the probe heading up.
fn direction_search(configure: impl FnOnce(&mut Fake)) -> Harness {
    let mut harness = Harness::new(|fake| {
        fake.retained.valid = 0;
        fake.tracing = true;
        fake.set(Setting::ProbeDirection, 8);
        fake.set(Setting::ScanDriveDisabled, 1);
        configure(fake);
    });
    harness.begin();
    harness
}

/// A stepped peak search that probed up, rose at its fourth sample and passed its peak; the
/// third sample (taken at the start position) was the best.
fn peaked(configure: impl FnOnce(&mut Fake)) -> Harness {
    let mut harness = direction_search(|fake| {
        fake.rose = true;
        fake.passed_after = 2;
        configure(fake);
    });
    for contrast in [100, 100, 100, 200, 150] {
        harness.sample(contrast);
    }
    harness
}

#[test]
fn a_paired_evaluation_takes_a_zero_first_sum_as_one() {
    let level = |paired: u32| {
        let mut harness = direction_search(|fake| fake.set(Setting::PairedEvaluation, paired));
        harness.evaluate(Evaluation { pairs: [[0, 70]; 3], settled: 1, ..Evaluation::default() }, false);
        harness.search.platform().transitions()[0].level
    };
    assert_eq!(level(1), 71);
    assert_eq!(level(0), 0);
}

#[test]
fn the_focus_log_takes_each_evaluation_while_it_is_on() {
    let mut harness = direction_search(|fake| fake.set(Setting::FocusLog, 2));
    harness.evaluate(Evaluation { pairs: [[100, 50], [30, 7], [0, 0]], settled: 1, ..Evaluation::default() }, false);
    harness.sample(100);
    let logs = harness.search.platform().logs();
    assert_eq!(logs.len(), 2);
    assert_eq!((logs[0].phase, logs[0].combined, logs[0].second), (13, 30, 7));
    assert_eq!(logs[1].phase, 14);

    let mut quiet = direction_search(|_| {});
    quiet.sample(100);
    assert!(quiet.search.platform().logs().is_empty());
}

fn shown(effects: &[Effect]) -> Vec<(Vec<u8>, u8, u8)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Debug(text, column, row) => Some((text.trim_ascii_end().to_vec(), *column, *row)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_debug_display_shows_the_phase_and_the_selected_rows_for_its_record_modes() {
    let mut harness = direction_search(|fake| {
        fake.set(Setting::DebugDisplay, 1);
        fake.set(Setting::DebugRowsFirst, 0b11);
        fake.record.mode = 5;
        fake.newest_log.lens = 3;
    });
    harness.sample(100);
    assert_eq!(
        shown(&harness.search.platform().effects),
        [
            (b"CAF".to_vec(), 1, 1),
            (b"DIRTION".to_vec(), 9, 1),
            (b"ZOOM".to_vec(), 1, 2),
            (b"3".to_vec(), 9, 2),
            (b"BV".to_vec(), 1, 3),
            (b"".to_vec(), 9, 3),
        ]
    );

    let mut other_mode = direction_search(|fake| {
        fake.set(Setting::DebugDisplay, 1);
        fake.record.mode = 3;
    });
    other_mode.sample(100);
    assert!(shown(&other_mode.search.platform().effects).is_empty());
}

#[test]
fn the_debug_display_refreshes_once_its_rate_of_evaluations_has_passed() {
    let mut harness = direction_search(|fake| {
        fake.set(Setting::DebugDisplay, 1);
        fake.set(Setting::DebugRate, 2);
        fake.record.mode = 4;
    });
    let refreshed: Vec<bool> = (0..5)
        .map(|_| {
            harness.fake().effects.clear();
            harness.sample(100);
            !shown(&harness.search.platform().effects).is_empty()
        })
        .collect();
    assert_eq!(refreshed, [false, false, true, false, true]);
}

#[test]
fn a_settled_first_evaluation_probes_for_the_peak_without_moving() {
    let mut harness = direction_search(|_| {});
    assert_eq!(harness.sample(100), SearchStep::Measure);
    assert_eq!(harness.phases(), [Phase::PeakSearch]);
    assert!(harness.search.platform().effects.contains(&Effect::ScanRefresh(true)));
}

#[test]
fn an_unsettled_first_evaluation_keeps_searching_for_a_direction() {
    let mut harness = direction_search(|_| {});
    let step = harness.evaluate(Evaluation { pairs: [[100, 50]; 3], ..Evaluation::default() }, false);
    assert_eq!(harness.phases(), [Phase::Direction]);
    assert_eq!(travel(step).0, 0);
}

#[test]
fn a_stepped_peak_search_moves_one_scan_step_per_evaluation_in_the_probe_direction() {
    for (probe, sign) in [(8, 1), (0, -1)] {
        let mut harness = direction_search(|fake| fake.set(Setting::ProbeDirection, probe));
        harness.sample(100);
        for _ in 0..2 {
            let (steps, drive) = travel(harness.sample(100));
            assert_eq!(steps, sign * SCAN_STEP.size as i16);
            assert_eq!((drive.phase, drive.scanning, drive.moves, drive.speed, drive.rate), (BridgePhase::PeakSearch, false, true, 2, SCAN_STEP.rate));
        }
    }
}

#[test]
fn a_peak_search_that_rose_and_passed_its_peak_drives_back_to_its_best_sample() {
    let mut harness = peaked(|_| {});
    assert_eq!(*harness.phases().last().unwrap(), Phase::Peak);
    let best = harness.reports[1];
    let (steps, drive) = travel(harness.sample(150));
    assert_eq!(harness.lens, best);
    assert!(steps < 0);
    assert_eq!(drive.phase, BridgePhase::Peak);
}

#[test]
fn at_the_best_sample_the_peak_holds_in_focus_and_keeps_the_lens() {
    let mut harness = peaked(|_| {});
    harness.sample(150);
    let (steps, drive) = travel(harness.sample(190));
    assert_eq!((steps, drive.moves, drive.speed), (0, false, 2));
    assert_eq!(*harness.phases().last().unwrap(), Phase::Focus);
    assert_eq!(harness.sample(190), SearchStep::Measure);
}

#[test]
fn focus_holds_while_the_contrast_stays_at_its_peak() {
    let mut harness = Harness::new(|fake| {
        fake.tracing = true;
        fake.set(Setting::StrikesMoving, 3);
    });
    harness.begin();
    for _ in 0..10 {
        assert_eq!(harness.sample(100), SearchStep::Measure);
    }
    assert!(harness.phases().iter().all(|phase| *phase == Phase::Focus));
}

#[test]
fn focus_starts_over_after_its_strike_limit_of_falls() {
    let mut harness = Harness::new(|fake| {
        fake.tracing = true;
        fake.set(Setting::StrikesMoving, 3);
    });
    harness.begin();
    for _ in 0..5 {
        harness.sample(100);
    }
    for _ in 0..3 {
        assert_eq!(harness.sample(40), SearchStep::Measure);
    }
    let phases = harness.phases();
    assert_eq!(phases[..7], [Phase::Focus; 7]);
    assert_eq!(phases[7], Phase::Direction);
}

#[test]
fn a_pinned_hold_that_drives_heads_for_the_lens_default_focus() {
    let mut harness = direction_search(|fake| {
        fake.set(Setting::HoldDrives, 1);
        fake.snapshot.pinned = 1;
        fake.snapshot.pacing = [1, 1];
    });
    assert_eq!(harness.sample(100), SearchStep::Measure);
    assert_eq!(harness.phases(), [Phase::FocusDrive]);
    let (steps, drive) = travel(harness.sample(100));
    assert_eq!(harness.lens, 45);
    assert_eq!((steps, drive.phase), (75, BridgePhase::FocusDrive));
}

/// A search that reached the peak and, with `PeakReturns` and `TrendOverride` set, returns.
fn returning(configure: impl FnOnce(&mut Fake)) -> Harness {
    let mut harness = peaked(|fake| {
        fake.set(Setting::PeakReturns, 1);
        fake.set(Setting::TrendOverride, 1);
        fake.set(Setting::ReturnRises, 9);
        configure(fake);
    });
    harness.sample(150);
    assert_eq!(harness.sample(190), SearchStep::Measure);
    assert_eq!(*harness.phases().last().unwrap(), Phase::Return);
    harness
}

#[test]
fn a_return_over_a_contrast_hill_drives_to_the_best_frame_it_saw() {
    let mut harness = returning(|fake| fake.set(Setting::ReturnStrikes, 9));
    let scene = |position: i16| 1000 - 10 * u32::from((position + 12).unsigned_abs());
    let mut best = (0, 0);
    for _ in 0..30 {
        if *harness.phases().last().unwrap() != Phase::Return {
            break;
        }
        let frame = harness.frame_position();
        if scene(frame) > best.0 {
            best = (scene(frame), frame);
        }
        harness.sample(scene(frame));
    }
    assert_eq!(*harness.phases().last().unwrap(), Phase::FocusDrive);
    let frame = harness.frame_position();
    let (_, drive) = travel(harness.sample(scene(frame)));
    assert_eq!(harness.lens, best.1);
    assert_eq!(drive.phase, BridgePhase::FocusDrive);
}

#[test]
fn a_return_that_passed_its_peaks_and_fell_far_drives_to_its_best_sample() {
    let mut harness = returning(|fake| fake.set(Setting::ReturnFallTens, 5));
    for contrast in [200, 210, 260] {
        harness.sample(contrast);
    }
    let best = harness.reports[harness.reports.len() - 3];
    assert_eq!(harness.sample(100), SearchStep::Measure);
    assert_eq!(*harness.phases().last().unwrap(), Phase::FocusDrive);
    let (_, drive) = travel(harness.sample(100));
    assert_eq!(harness.lens, best);
    assert_eq!(drive.phase, BridgePhase::FocusDrive);
}

#[test]
fn a_return_below_the_contrast_floor_starts_over_after_its_strikes() {
    let mut harness = returning(|fake| {
        fake.set(Setting::MotionReturn, 50);
        fake.set(Setting::ReturnStrikes, 2);
    });
    assert!(matches!(harness.sample(300), SearchStep::Travel { .. }));
    assert_eq!(harness.sample(310), SearchStep::Measure);
    assert_eq!(*harness.phases().last().unwrap(), Phase::Direction);
}

#[test]
fn a_paired_return_that_keeps_rising_searches_the_peak_again() {
    let mut harness = returning(|fake| {
        fake.set(Setting::PairedEvaluation, 1);
        fake.set(Setting::DirectionScale { band: 0, model_scaled: false }, 10);
        fake.set(Setting::ReturnRises, 2);
        fake.set(Setting::ReturnStrikes, 5);
    });
    assert!(matches!(harness.sample(200), SearchStep::Travel { .. }));
    assert_eq!(harness.sample(210), SearchStep::Measure);
    assert_eq!(*harness.phases().last().unwrap(), Phase::PeakSearch);
}

#[test]
fn a_focus_whose_selection_changed_hands_approaches_with_the_return_step() {
    let mut harness = Harness::new(|fake| {
        fake.tracing = true;
        fake.set(Setting::FocusApproaches, 1);
        fake.set(Setting::FocusDriveTurn, 1);
        fake.set(Setting::ReturnRises, 9);
        fake.snapshot.frame_mode = 1;
    });
    harness.begin();
    for _ in 0..4 {
        harness.sample(100);
    }
    assert_eq!(harness.sample(100), SearchStep::Measure);
    assert_eq!(*harness.phases().last().unwrap(), Phase::Approach);
    let (steps, drive) = travel(harness.sample(100));
    assert_eq!((steps.unsigned_abs(), drive.phase), (RETURN_STEP.size, BridgePhase::Approach));
}

/// A suspended search watching the scene, with a stepped peak search for large biases.
fn monitoring(configure: impl FnOnce(&mut Fake)) -> Harness {
    let mut harness = direction_search(|fake| {
        fake.set(Setting::MonitorRiseTens, 2);
        fake.lens_move = Some(LensMove::Ascending);
        configure(fake);
    });
    assert_eq!(harness.input(SearchInput::Suspend), SearchStep::Measure);
    harness
}

#[test]
fn the_monitor_keeps_the_lens_until_the_lens_identity_changes() {
    let mut harness = monitoring(|_| {});
    for _ in 0..3 {
        assert_eq!(harness.sample(100), SearchStep::Measure);
    }
    assert!(!harness.search.platform().effects.iter().any(|effect| matches!(effect, Effect::Bias(..))));
    assert!(harness.search.ready(&harness.context, false));
    assert!(!harness.search.ready(&harness.context, true));
}

#[test]
fn a_small_lens_bias_after_a_lens_move_is_followed_with_a_return() {
    let mut harness = monitoring(|fake| fake.bias = 5);
    harness.sample(100);
    harness.fake().lens = LensIdentity(1);
    harness.sample(100);
    let position = LensPosition(harness.fake().position.0);
    assert!(harness.search.platform().effects.contains(&Effect::Bias(LensIdentity(1), LensMove::Ascending, position)));
    let (steps, drive) = travel(harness.sample(100));
    assert_eq!((steps.unsigned_abs(), drive.phase), (RETURN_STEP.size, BridgePhase::Return));
}

#[test]
fn a_large_lens_bias_after_a_lens_move_is_followed_with_a_peak_search() {
    let mut harness = monitoring(|fake| fake.bias = -500);
    harness.sample(100);
    harness.fake().lens = LensIdentity(1);
    let (steps, drive) = travel(harness.sample(100));
    assert_eq!((steps, drive.phase), (-(SCAN_STEP.size as i16), BridgePhase::PeakSearch));
}

#[test]
fn a_lens_move_while_a_bridge_request_is_in_flight_stops_the_scan_drive() {
    let mut harness = monitoring(|fake| fake.bias = -500);
    harness.fake().lens = LensIdentity(1);
    harness.fake().effects.clear();
    harness.evaluate(evaluation(100), true);
    assert!(harness.search.platform().effects.contains(&Effect::ScanRefresh(false)));
}

/// A search whose peak search scans, entered after the probe.
fn scanning() -> Harness {
    let mut harness = direction_search(|fake| {
        fake.set(Setting::ScanDriveDisabled, 0);
        fake.record.stage.first_pass = 1;
    });
    harness.sample(100);
    harness
}

#[test]
fn a_scanning_peak_search_plans_its_drive_and_takes_frames_while_the_lens_moves() {
    let mut harness = scanning();
    assert!(!harness.search.ready(&harness.context, true));
    assert_eq!(harness.sample(100), SearchStep::Measure);
    assert!(harness.search.platform().effects.iter().any(|effect| matches!(effect, Effect::PlanScan(_))));
    assert!(harness.search.ready(&harness.context, true));
    let (_, drive) = travel(harness.sample(100));
    assert!(drive.scanning);
}

#[test]
fn cancelling_a_scanning_peak_search_stops_its_drive_and_interrupts_it() {
    let mut harness = scanning();
    harness.sample(100);
    harness.fake().effects.clear();
    assert_eq!(harness.input(SearchInput::Cancel), SearchStep::Interrupt);
    assert_eq!(harness.search.platform().effects, [Effect::ScanRefresh(false)]);
}

#[test]
fn cancelling_outside_a_scan_asks_nothing() {
    let mut harness = direction_search(|_| {});
    harness.fake().effects.clear();
    assert_eq!(harness.input(SearchInput::Cancel), SearchStep::Measure);
    assert!(harness.search.platform().effects.is_empty());
}

#[test]
fn releasing_the_lens_tells_the_bridge_to_keep_it() {
    let mut harness = direction_search(|_| {});
    let drive = harness.search.release(&harness.context);
    assert_eq!((drive.phase, drive.moves, drive.speed), (BridgePhase::Direction, false, 2));
    harness.fake().set(Setting::SlowBridge, 1);
    assert_eq!(harness.search.release(&harness.context).speed, 2);
}

#[test]
fn a_second_begin_is_accepted_only_in_direction_search_and_focus() {
    let harness = direction_search(|_| {});
    assert!(harness.search.accepts_begin(&harness.context));
    let mut probing = direction_search(|_| {});
    probing.sample(100);
    assert!(!probing.search.accepts_begin(&probing.context));
    let mut focus = Harness::new(|_| {});
    focus.begin();
    assert!(focus.search.accepts_begin(&focus.context));
}

#[test]
fn the_evaluation_record_marks_the_first_frame_of_a_search() {
    let mut harness = direction_search(|_| {});
    assert!(harness.search.take_in(&mut harness.context).first);
    harness.input(SearchInput::Sample(Measurement { evaluation: evaluation(100), bridge_busy: false }));
    assert!(!harness.search.take_in(&mut harness.context).first);
}

#[test]
fn fault_drain_resume_retains_the_scene_watch_without_a_stop_request() {
    use crate::live::{LaneEvent, LaneStatus, LiveSearch, Ports};
    use crate::requests::{BridgeMotion, Request};
    use xm1_saclac_interface::search::SearchRecord;

    #[derive(Debug, Eq, PartialEq)]
    enum Out { Evaluate, Travel, Stop, Release }
    #[derive(Default)]
    struct Peers(Vec<Out>);
    impl Ports<SearchRecord, BridgeDrive> for Peers {
        fn fault(&mut self) { panic!("unexpected lane fault"); }
        fn request(&mut self, request: Request<SearchRecord, BridgeDrive>) {
            self.0.push(match request {
                Request::Evaluate(_) => Out::Evaluate,
                Request::BridgeStop => Out::Stop,
                Request::Bridge { motion: BridgeMotion::Travel(_), .. } => Out::Travel,
                Request::Bridge { motion: BridgeMotion::Release, .. } => Out::Release,
            });
        }
    }
    let mut fake = Fake::new();
    fake.retained.valid = 0;
    fake.set(Setting::ProbeDirection, 8);
    fake.set(Setting::ScanDriveDisabled, 0);
    fake.record.stage.first_pass = 1;
    fake.steps = vec![(StepKind::Direction, DIRECTION_STEP), (StepKind::Scan, SCAN_STEP)];
    let mut lane = LiveSearch::new(ContrastSearch::new(fake), Context::new());
    let mut peers = Peers::default();
    assert_eq!(lane.handle(LaneEvent::Enable, &mut peers), LaneStatus::Complete);
    assert_eq!(lane.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
    assert_eq!(lane.handle(LaneEvent::Resume, &mut peers), LaneStatus::Complete);
    for _ in 0..3 {
        assert_eq!(lane.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
        assert_eq!(lane.handle(LaneEvent::Evaluated(evaluation(100)), &mut peers), LaneStatus::Idle);
    }
    assert_eq!(lane.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
    assert_eq!(lane.handle(LaneEvent::LensReported(BridgeReport {
        status: BridgeStatus::Fault, position: 0, class: LensClass(0),
    }), &mut peers), LaneStatus::Idle);
    assert_eq!(lane.handle(LaneEvent::Resume, &mut peers), LaneStatus::Pending);
    assert_eq!(peers.0, [Out::Evaluate, Out::Evaluate, Out::Evaluate, Out::Travel, Out::Evaluate]);
    assert_eq!(lane.handle(LaneEvent::Work, &mut peers), LaneStatus::Idle);
    assert_eq!(lane.handle(LaneEvent::Evaluated(evaluation(100)), &mut peers), LaneStatus::Complete);
    assert_eq!(lane.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
    assert_eq!(peers.0, [Out::Evaluate, Out::Evaluate, Out::Evaluate, Out::Travel, Out::Evaluate, Out::Evaluate]);
}

#[test]
fn pending_resume_uses_the_retained_return_or_peak_watch_after_leaving_the_monitor() {
    for (bias, stops) in [(5, false), (-500, true)] {
        let mut harness = monitoring(|fake| {
            fake.bias = bias;
            fake.set(Setting::ScanDriveDisabled, 0);
            fake.record.stage.first_pass = 1;
        });
        harness.sample(100);
        harness.fake().lens = LensIdentity(1);
        harness.sample(100);
        harness.sample(100);
        assert_eq!(harness.input(SearchInput::Resume), SearchStep::Measure);
        harness.fake().effects.clear();
        let step = harness.input(SearchInput::ResumePending);
        if stops {
            assert_eq!(harness.search.platform().effects, [Effect::ScanRefresh(false)]);
            assert_eq!(step, SearchStep::Interrupt);
        } else {
            assert!(harness.search.platform().effects.is_empty());
            assert_eq!(step, SearchStep::Measure);
        }
    }
}
