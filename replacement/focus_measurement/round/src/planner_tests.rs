use crate::lanes::{Lane, Lanes};
use crate::planner::{classify_mode, Cadence, Mode, ModeInputs, Planner, Policy, SamplingBand, Slot, Work};
use crate::profile::{FrameInterval, FrameRate};

struct Always;
impl Policy for Always {
    fn mode(&mut self) -> Mode { Mode::Default }
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: true, third: true, fourth: true } }
}
struct FirstOnly;
impl Policy for FirstOnly {
    fn mode(&mut self) -> Mode { Mode::Default }
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: false, third: false, fourth: false } }
}

#[test]
fn paired_schedule_orders_each_lane_and_joins_before_restarting() {
    let mut planner = Planner::new();
    let all = Lanes::all();
    let plan = planner.next(all, &mut Always);
    assert_eq!((plan.first, plan.second, plan.primary_considered), (Some(Work::Window), Some(Work::PrimaryAcquisition), true));
    planner.submitted(Slot::First, Work::Window);
    planner.submitted(Slot::Second, Work::PrimaryAcquisition);
    let plan = planner.next(all, &mut Always);
    assert_eq!((plan.first, plan.second, plan.cycle_complete), (None, None, false));
    planner.release(Slot::First);
    let plan = planner.next(all, &mut Always);
    assert_eq!((plan.first, plan.second), (Some(Work::Search), None));
    planner.release(Slot::Second);
    let plan = planner.next(all, &mut Always);
    assert_eq!((plan.first, plan.second), (Some(Work::Image), Some(Work::Auxiliary)));
    let plan = planner.next(all, &mut Always);
    assert_eq!((plan.first, plan.second), (Some(Work::Source), None));
    assert!(planner.next(all, &mut Always).cycle_complete);
    assert_eq!(planner.next(all, &mut Always).first, Some(Work::Window));
}

#[test]
fn secondary_acquisition_runs_on_every_fourth_cycle_even_when_primary_is_filtered() {
    let mut planner = Planner::new();
    let enabled: Lanes = [Lane::Acquisition, Lane::Window].into_iter().collect();
    for (cycle, expected) in [Some(Work::PrimaryAcquisition), None, None, Some(Work::SecondaryAcquisition)].into_iter().enumerate() {
        let plan = planner.next(enabled, &mut FirstOnly);
        assert_eq!(plan.first, Some(Work::Window));
        assert_eq!(plan.second, expected, "cycle {cycle}");
        assert!(planner.next(enabled, &mut FirstOnly).cycle_complete);
    }
}

#[test]
fn disabled_acquisition_is_considered_without_starting_it() {
    let plan = Planner::new().next(Lanes::empty(), &mut Always);
    assert!(plan.primary_considered);
    assert!(plan.cycle_complete);
    assert_eq!((plan.first, plan.second), (None, None));
}

#[test]
fn the_mode_is_taken_only_at_cycle_boundaries() {
    struct Recorded { mode: Mode, asked: usize, seen: std::vec::Vec<Mode> }
    impl Policy for Recorded {
        fn mode(&mut self) -> Mode { self.asked += 1; self.mode }
        fn cadence(&mut self, _: Work, mode: Mode) -> Cadence { self.seen.push(mode); Always.cadence(Work::Search, mode) }
    }
    let mut planner = Planner::new();
    let mut policy = Recorded { mode: Mode::RecordSelected, asked: 0, seen: std::vec::Vec::new() };
    let enabled: Lanes = [Lane::Acquisition, Lane::Window, Lane::Search].into_iter().collect();
    planner.next(enabled, &mut policy);
    policy.mode = Mode::ProfileSelected;
    planner.next(enabled, &mut policy);
    assert_eq!((policy.asked, policy.seen.as_slice()), (1, [Mode::RecordSelected, Mode::RecordSelected].as_slice()));
    planner.next(enabled, &mut policy);
    planner.next(enabled, &mut policy);
    assert_eq!(policy.asked, 2);
    assert_eq!(policy.seen.last(), Some(&Mode::ProfileSelected));
}

#[test]
fn mode_classification_keeps_record_then_profile_precedence() {
    let mut inputs = ModeInputs { record_selected: false, profile_selected: false, automatic_sampling: None };
    assert_eq!(classify_mode(inputs), Mode::Default);
    inputs.automatic_sampling = Some(SamplingBand::BelowThreshold);
    assert_eq!(classify_mode(inputs), Mode::BelowThreshold);
    inputs.automatic_sampling = Some(SamplingBand::AboveThreshold);
    assert_eq!(classify_mode(inputs), Mode::AboveThreshold);
    inputs.profile_selected = true;
    assert_eq!(classify_mode(inputs), Mode::ProfileSelected);
    inputs.record_selected = true;
    assert_eq!(classify_mode(inputs), Mode::RecordSelected);
}

#[test]
fn a_frame_rate_at_the_threshold_samples_above_it() {
    assert_eq!(SamplingBand::of(FrameRate(30), FrameRate(30)), SamplingBand::AboveThreshold);
    assert_eq!(SamplingBand::of(FrameRate(29), FrameRate(30)), SamplingBand::BelowThreshold);
    assert_eq!(SamplingBand::of(FrameRate(0), FrameRate(0)), SamplingBand::AboveThreshold);
}

#[test]
fn the_frame_rate_is_a_million_over_the_interval_in_sixteen_bits() {
    assert_eq!(FrameInterval(33_333).rate(), FrameRate(30));
    assert_eq!(FrameInterval(0).rate(), FrameRate(0));
    assert_eq!(FrameInterval(1).rate(), FrameRate((1_000_000u32 & 0xffff) as u16));
}
