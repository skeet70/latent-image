use crate::lanes::{Lane, Lanes};
use crate::planner::{classify_mode, Cadence, Mode, ModeInputs, Planner, Policy, SamplingBand, Slot, Work};

struct Always;
impl Policy for Always {
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: true, third: true, fourth: true } }
}
struct FirstOnly;
impl Policy for FirstOnly {
    fn cadence(&mut self, _: Work, _: Mode) -> Cadence { Cadence { first: true, second: false, third: false, fourth: false } }
}

#[test]
fn paired_schedule_orders_each_lane_and_joins_before_restarting() {
    let mut planner = Planner::new();
    let all = Lanes::all();
    let plan = planner.next(Mode::Default, all, &mut Always);
    assert_eq!((plan.first, plan.second, plan.primary_considered), (Some(Work::Window), Some(Work::PrimaryAcquisition), true));
    planner.submitted(Slot::First, Work::Window);
    planner.submitted(Slot::Second, Work::PrimaryAcquisition);
    let plan = planner.next(Mode::Default, all, &mut Always);
    assert_eq!((plan.first, plan.second, plan.cycle_complete), (None, None, false));
    planner.release(Slot::First);
    let plan = planner.next(Mode::Default, all, &mut Always);
    assert_eq!((plan.first, plan.second), (Some(Work::Search), None));
    planner.release(Slot::Second);
    let plan = planner.next(Mode::Default, all, &mut Always);
    assert_eq!((plan.first, plan.second), (Some(Work::Image), Some(Work::Auxiliary)));
    let plan = planner.next(Mode::Default, all, &mut Always);
    assert_eq!((plan.first, plan.second), (Some(Work::Source), None));
    assert!(planner.next(Mode::Default, all, &mut Always).cycle_complete);
    assert_eq!(planner.next(Mode::Default, all, &mut Always).first, Some(Work::Window));
}

#[test]
fn secondary_acquisition_runs_on_every_fourth_cycle_even_when_primary_is_filtered() {
    let mut planner = Planner::new();
    let enabled: Lanes = [Lane::Acquisition, Lane::Window].into_iter().collect();
    for (cycle, expected) in [Some(Work::PrimaryAcquisition), None, None, Some(Work::SecondaryAcquisition)].into_iter().enumerate() {
        let plan = planner.next(Mode::Default, enabled, &mut FirstOnly);
        assert_eq!(plan.first, Some(Work::Window));
        assert_eq!(plan.second, expected, "cycle {cycle}");
        assert!(planner.next(Mode::Default, enabled, &mut FirstOnly).cycle_complete);
    }
}

#[test]
fn disabled_acquisition_is_considered_without_starting_it() {
    let plan = Planner::new().next(Mode::Default, Lanes::empty(), &mut Always);
    assert!(plan.primary_considered);
    assert!(plan.cycle_complete);
    assert_eq!((plan.first, plan.second), (None, None));
}

#[test]
fn policy_changes_take_effect_only_at_cycle_boundaries() {
    struct Recorded(std::vec::Vec<Mode>);
    impl Policy for Recorded {
        fn cadence(&mut self, _: Work, mode: Mode) -> Cadence { self.0.push(mode); Always.cadence(Work::Search, mode) }
    }
    let mut planner = Planner::new();
    let mut policy = Recorded(std::vec::Vec::new());
    let enabled: Lanes = [Lane::Acquisition, Lane::Window, Lane::Search].into_iter().collect();
    planner.next(Mode::RecordSelected, enabled, &mut policy);
    planner.next(Mode::ProfileSelected, enabled, &mut policy);
    assert_eq!(policy.0, [Mode::RecordSelected, Mode::RecordSelected]);
    planner.next(Mode::ProfileSelected, enabled, &mut policy);
    planner.next(Mode::ProfileSelected, enabled, &mut policy);
    assert_eq!(policy.0.last(), Some(&Mode::ProfileSelected));
}

#[test]
fn paused_schedule_does_not_advance_or_consider_acquisition() {
    let mut planner = Planner::new();
    let plan = planner.next(Mode::Paused, Lanes::all(), &mut Always);
    assert_eq!((plan.first, plan.second, plan.primary_considered, plan.cycle_complete), (None, None, false, false));
    assert_eq!(planner.next(Mode::Default, Lanes::all(), &mut Always).first, Some(Work::Window));
}

#[test]
fn mode_classification_keeps_record_resource_and_profile_precedence() {
    let mut inputs = ModeInputs { record_selected: false, resource_selected: false, profile_selected: false, automatic_sampling: None };
    assert_eq!(classify_mode(inputs), Mode::Default);
    inputs.automatic_sampling = Some(SamplingBand::BelowThreshold);
    assert_eq!(classify_mode(inputs), Mode::BelowThreshold);
    inputs.automatic_sampling = Some(SamplingBand::AboveThreshold);
    assert_eq!(classify_mode(inputs), Mode::AboveThreshold);
    inputs.profile_selected = true;
    assert_eq!(classify_mode(inputs), Mode::ProfileSelected);
    inputs.resource_selected = true;
    assert_eq!(classify_mode(inputs), Mode::ResourceSelected);
    inputs.record_selected = true;
    assert_eq!(classify_mode(inputs), Mode::RecordSelected);
}
