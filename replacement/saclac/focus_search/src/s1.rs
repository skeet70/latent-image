#![forbid(unsafe_code)]

//! The S1 search (machine 34), which the S1 controller runs during a half press. At most one
//! evaluation (calc 0x3c09) and one lens drive (imgdc 0x3f44) are in flight; a stop, cancel or
//! conclude waits for both before the lens is released or driven to the best position found.

mod climb;
mod search;
mod sweep;
mod track;

pub use search::{S1Record, S1Snapshot};

use crate::contrast::Platform;
use crate::intake::RecordReads;
use search::{S1Phase, Search, Stepping};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, SearchControl};
use xm1_saclac_interface::search::{BridgeReport, Evaluation, ResultAddress, S1DriveRequest, SearchRecord};
use xm1_saclac_interface::stock::search::EVALUATION_BYTES;

pub trait S1Ports: Platform + RecordReads {
    /// Stock's fault reaction (0xf46c20).
    fn fault(&mut self);
    /// calc 0x3c09; calc answers with completion 0x9d.
    fn evaluate(&mut self, record: SearchRecord);
    /// imgdc 0x3f44; imgdc answers with completion 0x5a.
    fn drive(&mut self, request: S1DriveRequest);
    /// The result calc left at `address`.
    fn evaluation_result(&mut self, address: ResultAddress) -> [u8; EVALUATION_BYTES];
    fn record(&mut self) -> S1Record;
    /// Stages the frame's statistics for calc (0xfeaacc(1)).
    fn stage_statistics(&mut self);
    /// What the capture pipeline recorded with the S1 frame (0xfeaadc(1)).
    fn frame_snapshot(&mut self) -> S1Snapshot;
    /// The record's near position (0x75a3f0).
    fn near_position(&mut self) -> i16;
    /// 0x750474.
    fn interval_tracking(&mut self) -> u32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S1Completion {
    /// Calc's evaluation of the frame sent at `Start` (0x9d).
    Evaluated(Evaluation),
    /// imgdc's report on the last drive (0x5a).
    LensReported(BridgeReport),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Idle,
    /// Initialized; the first frame starts this phase.
    Ready(Stepping),
    /// Cancelled before the first frame; waits for `Resume`.
    Parked,
    Searching(Stepping),
    /// The search concluded itself and drives to the best position.
    Concluded,
    /// A cancel waits for the evaluation and the drive in flight before releasing the lens.
    CancelDraining,
    CancelReleasing,
    StopDraining,
    StopReleasing,
    /// A conclude waits for the evaluation and the drive in flight.
    ConcludeDraining,
    /// A conclude arrived during the sweep, which runs until it ends itself.
    ConcludeSweeping,
    ConcludeDriving,
    ConcludeReleasing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Control(SearchControl),
    Completed(S1Completion),
}

pub struct S1Search {
    state: State,
    evaluating: bool,
    driving: bool,
    search: Search,
}

impl Default for S1Search {
    fn default() -> Self {
        Self::new()
    }
}

impl S1Search {
    pub const fn new() -> Self {
        Self { state: State::Idle, evaluating: false, driving: false, search: Search::new() }
    }

    /// `ChildRequest::S1Search`.
    pub fn handle(&mut self, event: ChildEvent<'_, SearchControl>, ports: &mut impl S1Ports) -> ChildOutcome {
        let event = match event {
            ChildEvent::Control { control, .. } => Event::Control(control),
            ChildEvent::Completed { completed, .. } => {
                match crate::stock::s1::completion(completed, |address| ports.evaluation_result(address)) {
                    Some(completion) => Event::Completed(completion),
                    None => return ChildOutcome::Inactive,
                }
            }
            ChildEvent::Command { .. } => return ChildOutcome::Inactive,
        };
        self.dispatch(event, ports)
    }

    fn dispatch(&mut self, event: Event, ports: &mut impl S1Ports) -> ChildOutcome {
        use Event::{Completed, Control};
        use S1Completion::{Evaluated, LensReported};
        use SearchControl as C;
        use State as S;
        match (self.state, event) {
            (S::Idle, Control(C::Initialize)) => {
                self.state = match self.search.initialize(ports) {
                    Some(start) => {
                        self.evaluating = false;
                        self.driving = false;
                        S::Ready(start)
                    }
                    None => S::Idle,
                };
                ChildOutcome::Complete
            }
            (S::Idle, Control(C::Stop | C::Cancel | C::Resume | C::Conclude)) => ChildOutcome::Complete,
            (S::Ready(_), Control(C::Stop | C::Conclude)) => self.settle(S::Idle),
            (S::Ready(_), Control(C::Cancel)) => self.settle(S::Parked),
            (S::Ready(start), Control(C::Start)) => {
                self.evaluate(ports);
                self.pend(S::Searching(start))
            }
            (S::Parked, Control(C::Resume)) => {
                self.search.restart(ports);
                self.settle(S::Ready(Stepping::Climb))
            }
            (S::Searching(_) | S::ConcludeSweeping, Control(C::Start)) => {
                self.evaluate(ports);
                ChildOutcome::Pending
            }
            (_, Control(C::Start)) => ChildOutcome::Pending,
            (S::Searching(_), Control(C::Stop)) => self.release_after(S::StopDraining, S::StopReleasing, ports),
            (S::Searching(_), Control(C::Cancel)) => self.release_after(S::CancelDraining, S::CancelReleasing, ports),
            (S::Searching(Stepping::Sweep), Control(C::Conclude)) => self.pend(S::ConcludeSweeping),
            (S::Searching(_), Control(C::Conclude)) => {
                if self.idle() {
                    self.conclude(ports);
                    self.pend(S::ConcludeDriving)
                } else {
                    self.pend(S::ConcludeDraining)
                }
            }
            (S::Concluded, Control(C::Stop)) => self.pend(S::StopDraining),
            (S::Concluded, Control(C::Cancel)) => self.pend(S::CancelDraining),
            (S::Concluded, Control(C::Conclude)) => self.pend(S::ConcludeDriving),

            (S::Searching(stepping), Completed(Evaluated(evaluation))) => {
                self.take_evaluation(&evaluation, ports);
                let next = self.search.step(stepping, ports);
                self.search.set_phase(next);
                match next.stepping() {
                    None => {
                        self.state = S::Concluded;
                        self.search.aim_at_best();
                        self.drive_on(ports);
                    }
                    Some(phase) if phase == stepping => self.drive_on(ports),
                    Some(phase) => self.state = S::Searching(phase),
                }
                ChildOutcome::Pending
            }
            (S::ConcludeSweeping, Completed(Evaluated(evaluation))) => {
                self.take_evaluation(&evaluation, ports);
                let next = self.search.step(Stepping::Sweep, ports);
                self.search.set_phase(next);
                if next != S1Phase::Sweep {
                    self.state = S::ConcludeDriving;
                    self.search.aim_at_best();
                }
                self.drive_on(ports);
                ChildOutcome::Pending
            }
            (S::CancelDraining, Completed(Evaluated(evaluation))) => {
                self.take_evaluation(&evaluation, ports);
                self.release_when_idle(S::CancelReleasing, ports)
            }
            (S::StopDraining, Completed(Evaluated(evaluation))) => {
                self.take_evaluation(&evaluation, ports);
                self.release_when_idle(S::StopReleasing, ports)
            }
            (S::ConcludeDraining, Completed(Evaluated(evaluation))) => {
                self.take_evaluation(&evaluation, ports);
                self.conclude_when_idle(ports)
            }

            (S::Searching(_) | S::ConcludeSweeping, Completed(LensReported(report))) => {
                self.take_report(report);
                ChildOutcome::Pending
            }
            (S::Concluded, Completed(LensReported(report))) => {
                self.take_report(report);
                if self.search.rearm() {
                    self.state = S::Searching(Stepping::Track);
                } else {
                    self.search.aim_at_best();
                    self.drive_on(ports);
                }
                ChildOutcome::Pending
            }
            (S::CancelDraining, Completed(LensReported(report))) => {
                self.take_report(report);
                self.release_when_idle(S::CancelReleasing, ports)
            }
            (S::StopDraining, Completed(LensReported(report))) => {
                self.take_report(report);
                self.release_when_idle(S::StopReleasing, ports)
            }
            (S::ConcludeDraining, Completed(LensReported(report))) => {
                self.take_report(report);
                self.conclude_when_idle(ports)
            }
            (S::CancelReleasing, Completed(LensReported(report))) => {
                self.take_report(report);
                self.settle(S::Parked)
            }
            (S::StopReleasing | S::ConcludeReleasing, Completed(LensReported(report))) => {
                self.take_report(report);
                self.search.finish();
                self.settle(S::Idle)
            }
            (S::ConcludeDriving, Completed(LensReported(report))) => {
                self.take_report(report);
                if self.search.rearm() {
                    self.state = S::ConcludeReleasing;
                    self.release(ports);
                } else {
                    self.search.aim_at_best();
                    self.drive_on(ports);
                }
                ChildOutcome::Pending
            }
            _ => {
                ports.fault();
                ChildOutcome::Fault
            }
        }
    }

    fn settle(&mut self, state: State) -> ChildOutcome {
        self.state = state;
        ChildOutcome::Complete
    }

    fn pend(&mut self, state: State) -> ChildOutcome {
        self.state = state;
        ChildOutcome::Pending
    }

    const fn idle(&self) -> bool {
        !self.evaluating && !self.driving
    }

    fn evaluate(&mut self, ports: &mut impl S1Ports) {
        if !self.evaluating {
            let record = self.search.frame(ports);
            self.evaluating = true;
            ports.evaluate(record);
        }
    }

    fn take_evaluation(&mut self, evaluation: &Evaluation, ports: &mut impl S1Ports) {
        self.evaluating = false;
        self.search.take_evaluation(evaluation, ports);
    }

    fn take_report(&mut self, report: BridgeReport) {
        self.driving = false;
        self.search.take_report(report);
    }

    /// Moves on by the step, pulled back into the travel range.
    fn drive_on(&mut self, ports: &mut impl S1Ports) {
        self.search.bound_step(ports);
        self.drive(true, ports);
    }

    fn release(&mut self, ports: &mut impl S1Ports) {
        self.drive(false, ports);
    }

    /// A drive asked for while another is in flight is dropped.
    fn drive(&mut self, moves: bool, ports: &mut impl S1Ports) {
        let request = self.search.drive_request(moves, ports);
        if !self.driving {
            self.driving = true;
            ports.drive(request);
        }
    }

    fn conclude(&mut self, ports: &mut impl S1Ports) {
        self.search.set_phase(S1Phase::Conclude);
        self.search.aim_at_best();
        self.drive_on(ports);
    }

    fn release_after(&mut self, draining: State, releasing: State, ports: &mut impl S1Ports) -> ChildOutcome {
        if self.idle() {
            self.release(ports);
            self.pend(releasing)
        } else {
            self.pend(draining)
        }
    }

    fn release_when_idle(&mut self, releasing: State, ports: &mut impl S1Ports) -> ChildOutcome {
        if self.idle() {
            self.state = releasing;
            self.release(ports);
        }
        ChildOutcome::Pending
    }

    fn conclude_when_idle(&mut self, ports: &mut impl S1Ports) -> ChildOutcome {
        if self.idle() {
            self.state = State::ConcludeDriving;
            self.conclude(ports);
        }
        ChildOutcome::Pending
    }
}
