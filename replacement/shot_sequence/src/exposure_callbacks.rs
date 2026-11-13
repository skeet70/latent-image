#![forbid(unsafe_code)]

//! The completion callbacks the exposure step hands its peers (X-M1 1.50 id5 0xf1eddc, 0xf1ee10,
//! 0xf1fc08). Each runs in the peer's task and posts its completion to queue 43 with the peer's
//! result as the payload, which `ExposureWork::receive` takes.

use xm1_saclac_interface::exposure::EVALUATION_BYTES;
use xm1_saclac_interface::stock::exposure::{self as wire, StatisticsResult, CORRECTION_BYTES};
use xm1_saclac_interface::stock::parameters;
use xm1_saclac_interface::{Completed, Completion};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackTrace { EvaluationDelivered, CorrectionDelivered }

/// Queue 43 and the trace as a callback reaches them from the peer's task.
pub trait CallbackPort {
    fn trace(&mut self, trace: CallbackTrace);
    fn post(&mut self, completed: Completed<'_>);
}

/// The statistics measurement's callback: status 3 delivers the measured entry (0x4d), any other
/// status the status the measurement released its buffers with (0x4c).
pub fn statistics_completed(status: u32, entry: u32, port: &mut impl CallbackPort) {
    match wire::statistics_result(status, entry) {
        StatisticsResult::Measured(entry) => port.post(Completed { completion: Completion::Image4d, parameters: parameters(&wire::measured_payload(entry)) }),
        StatisticsResult::Released(status) => port.post(Completed { completion: Completion::Image4c, parameters: parameters(&wire::release_payload(status)) }),
    }
}

/// Calc 0x3c00's callback with the entry's evaluation.
pub fn evaluation_completed(evaluation: &[u8; EVALUATION_BYTES], port: &mut impl CallbackPort) {
    port.trace(CallbackTrace::EvaluationDelivered);
    port.post(Completed { completion: Completion::Calculation8e, parameters: parameters(evaluation) });
}

/// Calc 0x3c01's callback; without a correction the step keeps its previous one.
pub fn correction_completed(correction: Option<&[u8; CORRECTION_BYTES]>, port: &mut impl CallbackPort) {
    port.trace(CallbackTrace::CorrectionDelivered);
    let payload: &[u8] = correction.map_or(&[], |correction| correction);
    port.post(Completed { completion: Completion::Calculation8f, parameters: parameters(payload) });
}
