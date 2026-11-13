use crate::acquisition::{Acquisition, Branch, Completion, Control, Effect, Pass, Work};
use crate::jobs::{InvalidTransition, Outcome};
use std::vec::Vec;

#[derive(Default)]
struct Steps { events: Vec<(Branch, Effect)>, changed: bool }
impl Work for Steps {
    fn effect(&mut self, branch: Branch, effect: Effect) { self.events.push((branch, effect)); }
    fn selection_changed(&mut self) -> bool { self.changed }
}

#[test]
fn s1_acquires_four_primary_passes_before_secondary_and_focus_publication() {
    let mut acquisition = Acquisition::new(Branch::S1);
    let mut steps = Steps::default();
    assert_eq!(acquisition.control(Control::Initialize, &mut steps), Ok(Outcome::Complete));
    for pass in [Pass::First, Pass::Second, Pass::Third, Pass::Final] {
        assert_eq!(acquisition.control(Control::Primary, &mut steps), Ok(Outcome::Pending));
        assert_eq!(steps.events.last(), Some(&(Branch::S1, Effect::SubmitPrimary(pass))));
        assert_eq!(acquisition.completed(Completion::Primary, &mut steps), Outcome::Pending);
    }
    assert_eq!(steps.events.last(), Some(&(Branch::S1, Effect::SubmitSecondary)));
    assert_eq!(acquisition.completed(Completion::Secondary, &mut steps), Outcome::Pending);
    assert_eq!(&steps.events[steps.events.len() - 3..], [(Branch::S1, Effect::Restore), (Branch::S1, Effect::PublishSelection), (Branch::S1, Effect::PublishFocus)]);
    assert_eq!(acquisition.control(Control::Primary, &mut steps), Ok(Outcome::Pending));
    assert_eq!(steps.events.last(), Some(&(Branch::S1, Effect::SubmitPrimary(Pass::First))));
}

#[test]
fn s1_stop_drains_the_pending_secondary_without_a_focus_table() {
    let mut acquisition = Acquisition::new(Branch::S1);
    let mut steps = Steps::default();
    acquisition.control(Control::Initialize, &mut steps).unwrap();
    for _ in 0..4 {
        acquisition.control(Control::Primary, &mut steps).unwrap();
        acquisition.completed(Completion::Primary, &mut steps);
    }
    assert_eq!(acquisition.control(Control::Stop, &mut steps), Ok(Outcome::Pending));
    let before = steps.events.len();
    assert_eq!(acquisition.completed(Completion::Secondary, &mut steps), Outcome::Complete);
    assert_eq!(&steps.events[before..], [(Branch::S1, Effect::Restore), (Branch::S1, Effect::PublishSelection), (Branch::S1, Effect::Release)]);
}

#[test]
fn live_acquisition_reports_a_changed_selection_after_its_secondary_collection() {
    let mut acquisition = Acquisition::new(Branch::Live);
    let mut steps = Steps { changed: true, ..Steps::default() };
    acquisition.control(Control::Enable, &mut steps).unwrap();
    assert_eq!(acquisition.control(Control::Secondary, &mut steps), Ok(Outcome::Pending));
    assert_eq!(acquisition.completed(Completion::Secondary, &mut steps), Outcome::SelectionUpdated);
    assert_eq!(steps.events.last(), Some(&(Branch::Live, Effect::CollectSecondary)));
    steps.changed = false;
    acquisition.control(Control::Secondary, &mut steps).unwrap();
    assert_eq!(acquisition.completed(Completion::Secondary, &mut steps), Outcome::NoWork);
}

#[test]
fn mismatched_completions_cannot_release_an_active_primary_job() {
    let mut acquisition = Acquisition::new(Branch::Live);
    let mut steps = Steps::default();
    acquisition.control(Control::Enable, &mut steps).unwrap();
    acquisition.control(Control::Primary, &mut steps).unwrap();
    let before = steps.events.clone();
    assert_eq!(acquisition.completed(Completion::Secondary, &mut steps), Outcome::Ignored);
    assert_eq!(steps.events, before);
    assert_eq!(acquisition.control(Control::Primary, &mut steps), Err(InvalidTransition));
}
