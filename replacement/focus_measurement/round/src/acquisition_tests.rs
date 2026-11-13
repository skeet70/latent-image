use crate::acquisition::{Acquisition, Branch, Completion, Control, Effect, Pass, Ports};
use crate::jobs::{InvalidTransition, Outcome};
use std::vec::Vec;

#[derive(Default)]
struct Effects { events: Vec<(Branch, Effect)>, changed: bool }
impl Ports for Effects {
    fn effect(&mut self, branch: Branch, effect: Effect) { self.events.push((branch, effect)); }
    fn selection_changed(&mut self) -> bool { self.changed }
}

#[test]
fn s1_acquires_four_primary_passes_before_secondary_and_image_publication() {
    let mut acquisition = Acquisition::new(Branch::S1);
    let mut effects = Effects::default();
    assert_eq!(acquisition.control(Control::Initialize, &mut effects), Ok(Outcome::Complete));
    for pass in [Pass::First, Pass::Second, Pass::Third, Pass::Final] {
        assert_eq!(acquisition.control(Control::Primary, &mut effects), Ok(Outcome::Pending));
        assert_eq!(effects.events.last(), Some(&(Branch::S1, Effect::SubmitPrimary(pass))));
        assert_eq!(acquisition.completed(Completion::Primary, &mut effects), Outcome::Pending);
    }
    assert_eq!(effects.events.last(), Some(&(Branch::S1, Effect::SubmitSecondary)));
    assert_eq!(acquisition.completed(Completion::Secondary, &mut effects), Outcome::Pending);
    assert_eq!(&effects.events[effects.events.len() - 3..], [(Branch::S1, Effect::Restore), (Branch::S1, Effect::PublishFocus), (Branch::S1, Effect::PublishSecondary)]);
    assert_eq!(acquisition.control(Control::Primary, &mut effects), Ok(Outcome::Pending));
    assert_eq!(effects.events.last(), Some(&(Branch::S1, Effect::SubmitPrimary(Pass::First))));
}

#[test]
fn s1_stop_drains_the_pending_secondary_without_publishing_a_new_image() {
    let mut acquisition = Acquisition::new(Branch::S1);
    let mut effects = Effects::default();
    acquisition.control(Control::Initialize, &mut effects).unwrap();
    for _ in 0..4 {
        acquisition.control(Control::Primary, &mut effects).unwrap();
        acquisition.completed(Completion::Primary, &mut effects);
    }
    assert_eq!(acquisition.control(Control::Stop, &mut effects), Ok(Outcome::Pending));
    let before = effects.events.len();
    assert_eq!(acquisition.completed(Completion::Secondary, &mut effects), Outcome::Complete);
    assert_eq!(&effects.events[before..], [(Branch::S1, Effect::Restore), (Branch::S1, Effect::PublishFocus), (Branch::S1, Effect::Release)]);
}

#[test]
fn live_acquisition_reports_a_changed_selection_after_secondary_publication() {
    let mut acquisition = Acquisition::new(Branch::Live);
    let mut effects = Effects { changed: true, ..Effects::default() };
    acquisition.control(Control::Enable, &mut effects).unwrap();
    assert_eq!(acquisition.control(Control::Secondary, &mut effects), Ok(Outcome::Pending));
    assert_eq!(acquisition.completed(Completion::Secondary, &mut effects), Outcome::SelectionUpdated);
    assert_eq!(effects.events.last(), Some(&(Branch::Live, Effect::PublishSecondary)));
}

#[test]
fn mismatched_completions_cannot_release_an_active_primary_job() {
    let mut acquisition = Acquisition::new(Branch::Live);
    let mut effects = Effects::default();
    acquisition.control(Control::Enable, &mut effects).unwrap();
    acquisition.control(Control::Primary, &mut effects).unwrap();
    let before = effects.events.clone();
    assert_eq!(acquisition.completed(Completion::Secondary, &mut effects), Outcome::Ignored);
    assert_eq!(effects.events, before);
    assert_eq!(acquisition.control(Control::Primary, &mut effects), Err(InvalidTransition));
}
