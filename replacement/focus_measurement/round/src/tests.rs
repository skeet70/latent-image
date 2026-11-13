use crate::jobs::{Calculation, CalculationJob, CalculationPorts, Control, Effect, InvalidTransition, Outcome};
use std::vec::Vec;

#[derive(Default)]
struct Ports { effects: Vec<(Calculation, Effect)>, due: bool, changed: bool }
impl CalculationPorts for Ports {
    fn effect(&mut self, calculation: Calculation, effect: Effect) { self.effects.push((calculation, effect)); }
    fn source_due(&mut self) -> bool { self.due }
    fn result_changed(&mut self, _: Calculation) -> bool { self.changed }
}

#[test]
fn stop_waits_for_collection_before_releasing_the_record() {
    let mut job = CalculationJob::new(Calculation::Window);
    let mut ports = Ports::default();
    assert_eq!(job.control(Control::Enable, &mut ports), Ok(Outcome::Complete));
    assert_eq!(job.control(Control::Run, &mut ports), Ok(Outcome::Pending));
    assert_eq!(job.control(Control::Stop, &mut ports), Ok(Outcome::Pending));
    assert_eq!(ports.effects, [(Calculation::Window, Effect::Initialize), (Calculation::Window, Effect::Enable), (Calculation::Window, Effect::Submit)]);
    assert_eq!(job.completed(&mut ports), Outcome::Complete);
    assert_eq!(&ports.effects[3..], [(Calculation::Window, Effect::Collect), (Calculation::Window, Effect::Release)]);
    assert_eq!(job.control(Control::Run, &mut ports), Ok(Outcome::NoWork));
}

#[test]
fn repeated_rounds_reuse_the_enabled_job() {
    let mut job = CalculationJob::new(Calculation::Window);
    let mut ports = Ports::default();
    job.control(Control::Enable, &mut ports).unwrap();
    for _ in 0..3 {
        assert_eq!(job.control(Control::Run, &mut ports), Ok(Outcome::Pending));
        assert_eq!(job.completed(&mut ports), Outcome::NoWork);
    }
    assert_eq!(job.control(Control::Stop, &mut ports), Ok(Outcome::Complete));
    assert_eq!(ports.effects.iter().filter(|(_, effect)| *effect == Effect::Submit).count(), 3);
    assert_eq!(ports.effects.iter().filter(|(_, effect)| *effect == Effect::Release).count(), 1);
}

#[test]
fn a_source_tick_that_is_not_due_does_not_submit_a_job() {
    let mut job = CalculationJob::new(Calculation::Source);
    let mut ports = Ports::default();
    job.control(Control::Enable, &mut ports).unwrap();
    assert_eq!(job.control(Control::Run, &mut ports), Ok(Outcome::NoWork));
    assert_eq!(ports.effects.len(), 2);
    ports.due = true;
    assert_eq!(job.control(Control::Run, &mut ports), Ok(Outcome::Pending));
}

#[test]
fn busy_job_refuses_another_submission() {
    let mut job = CalculationJob::new(Calculation::Auxiliary);
    let mut ports = Ports::default();
    job.control(Control::Enable, &mut ports).unwrap();
    job.control(Control::Run, &mut ports).unwrap();
    let prior = ports.effects.clone();
    assert_eq!(job.control(Control::Run, &mut ports), Err(InvalidTransition));
    assert_eq!(ports.effects, prior);
}
