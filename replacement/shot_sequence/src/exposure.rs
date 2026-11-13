#![forbid(unsafe_code)]

use crate::outcome::{fault, Outcome};
use crate::ports::{Branch, Effect, ExposureEffect as E, ExposureQuery as Q, Ports, Query};
use xm1_saclac_interface::{Completion, RecordHandle};

/// Statistics are imgdc 0x3f1a (0x4d measured, then 0x4c released), corrections calc 0x3c01
/// (0x8f) and evaluations calc 0x3c00 (0x8e).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    Statistics,
    Evaluating,
    CorrectionAndRelease,
    Correction,
    CorrectedAwaitingRelease,
    EvaluatingReleased,
    AwaitingEvaluation,
    AwaitingRelease,
}

pub struct Exposure { state: State }

impl Default for Exposure { fn default() -> Self { Self::new() } }

impl Exposure {
    pub const fn new() -> Self { Self { state: State::Idle } }

    pub fn start(&mut self, branch: Branch, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.state != State::Idle { return fault(ports); }
        ports.effect(Effect::Exposure(E::Begin { branch, record }));
        if ports.query(Query::Exposure(Q::CorrectionFirst)) {
            self.state = State::Correction;
            ports.effect(Effect::Exposure(E::RequestCorrection));
        } else {
            self.state = State::Statistics;
            ports.effect(Effect::Exposure(E::RequestStatistics));
        }
        Outcome::Pending
    }

    pub fn completed(&mut self, completion: Completion, ports: &mut impl Ports) -> Outcome {
        use Completion::{Calculation8e as Evaluated, Calculation8f as Corrected, Image4c as Released, Image4d as Measured};
        use State::*;
        match (self.state, completion) {
            (Statistics, Measured) => {
                if self.absorb_statistics(AwaitingRelease, ports) {
                    let index = ports.next_evaluation();
                    if !index.is_final() {
                        self.state = Evaluating;
                        ports.effect(Effect::Exposure(E::RequestEvaluation(index)));
                    }
                }
                Outcome::Pending
            }
            (Evaluating, Measured) => {
                self.absorb_statistics(EvaluatingReleased, ports);
                Outcome::Pending
            }
            (CorrectionAndRelease, Measured) => Outcome::Pending,
            (Evaluating, Evaluated) => {
                ports.effect(Effect::Exposure(E::AbsorbCalculation));
                if ports.query(Query::Exposure(Q::CorrectionNeeded)) {
                    self.state = CorrectionAndRelease;
                    ports.effect(Effect::Exposure(E::RequestStatisticsRelease));
                    ports.effect(Effect::Exposure(E::RequestCorrection));
                } else {
                    let index = ports.next_evaluation();
                    if index.is_final() {
                        self.state = Statistics;
                    } else {
                        ports.effect(Effect::Exposure(E::RequestEvaluation(index)));
                    }
                }
                Outcome::Pending
            }
            (Correction, Evaluated) => {
                ports.effect(Effect::Exposure(E::AbsorbCalculation));
                if ports.query(Query::Exposure(Q::CorrectionNeeded)) {
                    ports.effect(Effect::Exposure(E::RequestCorrection));
                    return Outcome::Pending;
                }
                let index = ports.next_evaluation();
                if index.is_final() { return self.conclude(ports); }
                ports.effect(Effect::Exposure(E::RequestEvaluation(index)));
                Outcome::Pending
            }
            (EvaluatingReleased, Evaluated) => {
                ports.effect(Effect::Exposure(E::AbsorbCalculation));
                if ports.query(Query::Exposure(Q::CorrectionNeeded)) {
                    self.state = Correction;
                    ports.effect(Effect::Exposure(E::RequestCorrection));
                } else {
                    self.state = AwaitingRelease;
                }
                Outcome::Pending
            }
            (AwaitingEvaluation, Evaluated) | (Correction, Corrected) => self.conclude(ports),
            (CorrectionAndRelease, Corrected) => {
                self.state = CorrectedAwaitingRelease;
                Outcome::Pending
            }
            (Statistics, Released) => {
                if ports.query(Query::Exposure(Q::ReleaseStatusAtMostOne)) && ports.query(Query::Exposure(Q::CorrectionNeeded)) {
                    self.state = Correction;
                    let index = ports.next_evaluation();
                    ports.effect(Effect::Exposure(E::RequestEvaluation(index)));
                    Outcome::Pending
                } else {
                    self.conclude(ports)
                }
            }
            (Evaluating, Released) => {
                self.state = if ports.query(Query::Exposure(Q::ReleaseStatusAtMostOne)) { Correction } else { EvaluatingReleased };
                Outcome::Pending
            }
            (CorrectionAndRelease, Released) => {
                self.state = Correction;
                Outcome::Pending
            }
            (EvaluatingReleased, Released) => {
                self.state = AwaitingEvaluation;
                Outcome::Pending
            }
            (CorrectedAwaitingRelease | AwaitingRelease, Released) => self.conclude(ports),
            (_, Measured | Evaluated | Corrected | Released) => fault(ports),
            _ => Outcome::Unhandled,
        }
    }

    /// Returns whether the statistics were accepted.
    fn absorb_statistics(&mut self, rejected: State, ports: &mut impl Ports) -> bool {
        ports.effect(Effect::Exposure(E::AbsorbStatistics));
        if ports.query(Query::Exposure(Q::StatisticsAccepted)) {
            ports.effect(Effect::Exposure(E::AbsorbStatisticsResult));
            true
        } else {
            self.state = rejected;
            ports.effect(Effect::Exposure(E::RequestStatisticsRelease));
            false
        }
    }

    fn conclude(&mut self, ports: &mut impl Ports) -> Outcome {
        self.state = State::Idle;
        ports.effect(Effect::Exposure(E::Conclude));
        Outcome::Complete
    }
}
