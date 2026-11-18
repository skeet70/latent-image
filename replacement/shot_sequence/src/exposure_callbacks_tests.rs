use crate::exposure_callbacks::{correction_completed, evaluation_completed, statistics_completed, CallbackPort, CallbackTrace};
use crate::exposure_work::ExposureWork;
use std::vec::Vec;
use xm1_saclac_interface::exposure::{Correction, MeasuredEntry, ReleaseStatus, CORRECTION_HALVES};
use xm1_saclac_interface::stock::exposure::{self as wire, CORRECTION_BYTES};
use xm1_saclac_interface::stock::parameters;
use xm1_saclac_interface::{Completed, Completion};

#[derive(Debug, Eq, PartialEq)]
enum Seen { Trace(CallbackTrace), Post(Completion, Vec<u8>) }

#[derive(Default)]
struct Port(Vec<Seen>);

impl CallbackPort for Port {
    fn trace(&mut self, trace: CallbackTrace) { self.0.push(Seen::Trace(trace)); }
    fn post(&mut self, completed: Completed<'_>) { self.0.push(Seen::Post(completed.completion, completed.parameters.bytes().to_vec())); }
}

fn correction(seed: u16) -> [u8; CORRECTION_BYTES] { wire::correction_payload(&Correction(core::array::from_fn(|index| seed + index as u16))) }

#[test]
fn a_measured_statistics_entry_posts_0x4d_and_any_other_status_posts_0x4c() {
    let mut port = Port::default();
    statistics_completed(3, 7, &mut port);
    statistics_completed(1, 7, &mut port);
    let [Seen::Post(Completion::Image4d, measured), Seen::Post(Completion::Image4c, released)] = &port.0[..] else { panic!("{:?}", port.0) };
    assert_eq!(wire::measured(measured), Some(MeasuredEntry(7)));
    assert_eq!(wire::release(released), Some(ReleaseStatus(1)));
}

#[test]
fn an_evaluation_is_traced_then_posted_with_its_bytes() {
    let mut port = Port::default();
    let evaluation: [u8; 16] = core::array::from_fn(|index| index as u8 + 1);
    evaluation_completed(&evaluation, &mut port);
    assert_eq!(port.0, [Seen::Trace(CallbackTrace::EvaluationDelivered), Seen::Post(Completion::Calculation8e, evaluation.to_vec())]);
}

#[test]
fn a_missing_correction_posts_0x8f_empty_and_the_step_keeps_the_previous_one() {
    let mut port = Port::default();
    correction_completed(Some(&correction(100)), &mut port);
    correction_completed(None, &mut port);
    assert_eq!(port.0[0], Seen::Trace(CallbackTrace::CorrectionDelivered));
    let posted: Vec<&Vec<u8>> = port.0.iter().filter_map(|seen| match seen { Seen::Post(Completion::Calculation8f, payload) => Some(payload), _ => None }).collect();
    assert_eq!(posted.iter().map(|payload| payload.len()).collect::<Vec<_>>(), [CORRECTION_BYTES, 0]);
    let mut work = ExposureWork::default();
    for payload in posted {
        work.receive(Completed { completion: Completion::Calculation8f, parameters: parameters(payload) });
        assert_eq!(work.correction().half(CORRECTION_HALVES - 1), 100 + CORRECTION_HALVES as u16 - 1);
    }
}
