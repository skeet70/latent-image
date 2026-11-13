use crate::scheduling::{Control, Failure, Lane, Lanes, Outcome, Ports, Schedule, Selection};
use std::vec::Vec;

#[derive(Default)]
struct Jobs { traffic: Vec<(Lane, Control)>, delayed: Lanes, fail: Option<Lane> }
impl Ports for Jobs {
    type Error = ();
    fn control(&mut self, lane: Lane, control: Control) -> Result<Outcome, ()> {
        self.traffic.push((lane, control));
        if self.fail == Some(lane) { return Err(()); }
        Ok(if self.delayed.contains(lane) { Outcome::Pending } else { Outcome::Complete })
    }
}

#[test]
fn acquisition_selection_enables_acquisition_and_window_in_order() {
    let mut schedule = Schedule::new();
    let mut jobs = Jobs::default();
    let selection = Selection { acquisition: true, ..Selection::default() };
    assert_eq!(schedule.initialize(selection.lanes(true), &mut jobs), Ok(Outcome::Pending));
    assert_eq!(jobs.traffic, [
        (Lane::Acquisition, Control::Enable), (Lane::Search, Control::Initialize),
        (Lane::Auxiliary, Control::Initialize), (Lane::Image, Control::Initialize),
        (Lane::Window, Control::Enable), (Lane::Source, Control::Initialize),
        (Lane::Bookkeeping, Control::Initialize),
    ]);
}

#[test]
fn stop_waits_for_each_delayed_lane_and_completes_in_either_order() {
    for completions in [[Lane::Acquisition, Lane::Image], [Lane::Image, Lane::Acquisition]] {
        let mut schedule = Schedule::new();
        let mut jobs = Jobs::default();
        jobs.delayed.insert(Lane::Acquisition);
        jobs.delayed.insert(Lane::Image);
        assert_eq!(schedule.stop(&mut jobs), Ok(Outcome::Pending));
        assert_eq!(schedule.completed(completions[0], Outcome::Complete), Outcome::Pending);
        assert_eq!(schedule.completed(completions[1], Outcome::Pending), Outcome::Pending);
        assert_eq!(schedule.completed(completions[1], Outcome::Complete), Outcome::Complete);
    }
}

#[test]
fn changes_keep_search_and_image_disabled_when_search_is_disallowed() {
    let mut schedule = Schedule::new();
    let mut jobs = Jobs::default();
    let selection = Selection { acquisition: true, search: true, auxiliary: true, image: true, source: true };
    assert_eq!(schedule.changes(selection, selection, true, false), Outcome::Pending);
    assert_eq!(schedule.enable_pending(&mut jobs), Ok(Outcome::Complete));
    assert_eq!(jobs.traffic, [(Lane::Acquisition, Control::Enable), (Lane::Auxiliary, Control::Enable), (Lane::Window, Control::Enable), (Lane::Source, Control::Enable)]);
}

#[test]
fn search_suspension_preserves_pending_enable_until_resume() {
    let mut schedule = Schedule::new();
    let mut jobs = Jobs::default();
    let selection = Selection { search: true, image: true, ..Selection::default() };
    schedule.changes(selection, selection, true, true);
    schedule.enable_search(false);
    assert_eq!(schedule.enable_pending(&mut jobs), Ok(Outcome::Pending));
    assert!(jobs.traffic.is_empty());
    schedule.enable_search(true);
    assert_eq!(schedule.enable_pending(&mut jobs), Ok(Outcome::Complete));
    assert_eq!(jobs.traffic, [(Lane::Search, Control::Enable), (Lane::Image, Control::Enable)]);
}

#[test]
fn empty_record_selection_drains_bookkeeping() {
    let mut schedule = Schedule::new();
    let mut jobs = Jobs::default();
    assert_eq!(schedule.changes(Selection::default(), Selection::default(), false, true), Outcome::Pending);
    assert_eq!(schedule.drain(&mut jobs), Ok(Outcome::Complete));
    assert_eq!(jobs.traffic, [(Lane::Bookkeeping, Control::Stop)]);
}

#[test]
fn initializing_stops_at_the_first_failed_lane() {
    let mut schedule = Schedule::new();
    let mut jobs = Jobs { fail: Some(Lane::Auxiliary), ..Jobs::default() };
    assert_eq!(schedule.initialize(Lanes::empty(), &mut jobs), Err(Failure::Port(())));
    assert_eq!(jobs.traffic.len(), 3);
    assert_eq!(jobs.traffic.last(), Some(&(Lane::Auxiliary, Control::Initialize)));
}

#[test]
fn initializing_does_not_treat_an_unexpected_pending_result_as_success() {
    let mut schedule = Schedule::new();
    let mut jobs = Jobs::default();
    jobs.delayed.insert(Lane::Acquisition);
    assert_eq!(schedule.initialize(Lanes::empty(), &mut jobs), Err(Failure::UnexpectedPending { lane: Lane::Acquisition, control: Control::Initialize }));
}
