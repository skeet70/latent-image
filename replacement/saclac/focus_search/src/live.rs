#![forbid(unsafe_code)]

//! The live contrast search, the round's search lane. At most one evaluation and one
//! lens-bridge request are in flight; both drain before the lane stops, suspends or resumes.

use crate::requests::{BridgeMotion, Request};
use xm1_saclac_controllers::search::{FocusSearch, LensPosition, SearchDecision, SearchInput, SearchStep, SearchTerminal};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeStatus {
    Moved,
    /// imgdc reports 21 from 0xff91dc; a release answered with it is asked again.
    Deferred,
    /// -94.
    Fault,
    Other(i32),
}

/// The lens table class imgdc reads through 0x76dad4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensClass(pub u32);

/// imgdc's answer to a bridge request (completion 0x46).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeReport {
    pub status: BridgeStatus,
    pub position: LensPosition,
    pub class: LensClass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Measurement<Frame, Evaluation> {
    /// What the intake took in when the evaluation was requested.
    pub frame: Frame,
    pub evaluation: Evaluation,
    /// The bridge's report since the previous measurement, when it reported a move.
    pub report: Option<BridgeReport>,
    pub bridge_busy: bool,
}

/// What the live search asks of a strategy beyond the slot.
pub trait LiveStrategy: FocusSearch<Sample = Measurement<Self::Frame, Self::Evaluation>> {
    type Frame;
    type Evaluation;

    fn ready(&mut self, context: &Self::Context, bridge_busy: bool) -> bool;
    /// What the bridge is told when the search hands the lens back.
    fn release(&mut self, context: &Self::Context) -> Self::Drive;
    /// Whether a second `Begin` while searching is accepted; stock faults on it except in
    /// its direction and focus states (m23 row 1).
    fn accepts_begin(&self, context: &Self::Context) -> bool;
}

pub trait Ports<Frame, Drive> {
    /// Stock's fault reaction (0xf46c20).
    fn fault(&mut self);
    /// Takes in the next frame (0xf40944) and posts its evaluation.
    fn evaluate(&mut self) -> Frame;
    fn request(&mut self, request: Request<Drive>);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneEvent<Evaluation> {
    Initialize,
    Enable,
    Stop,
    Suspend,
    Resume,
    /// A frame is ready to evaluate.
    Work,
    Evaluated(Evaluation),
    LensReported(BridgeReport),
    /// Anything else the round forwards.
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneStatus {
    Refused,
    Complete,
    Pending,
    Unhandled,
    Idle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Activity {
    Idle,
    /// After a bridge fault was released; only `Stop` leaves it.
    Halted,
    Searching,
    Suspending,
    Suspended,
    Resuming,
    /// A bridge fault arrived with an evaluation in flight.
    FaultDraining,
    FaultReleasing,
    Draining,
    Releasing,
}

/// The coordinator in front of the search (m33). Stock's other follower (m32) is entered
/// only with its initialization, which changes nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lane {
    Idle,
    Active,
    Stopping,
}

pub struct LiveSearch<S: LiveStrategy> {
    strategy: S,
    context: S::Context,
    lane: Lane,
    activity: Activity,
    evaluating: bool,
    bridging: bool,
    frame: Option<S::Frame>,
    report: Option<BridgeReport>,
}

impl<S: LiveStrategy> LiveSearch<S>
where
    S::Frame: Copy,
{
    pub fn new(strategy: S, context: S::Context) -> Self {
        Self {
            strategy,
            context,
            lane: Lane::Idle,
            activity: Activity::Idle,
            evaluating: false,
            bridging: false,
            frame: None,
            report: None,
        }
    }

    pub fn context(&self) -> &S::Context {
        &self.context
    }

    pub fn handle(&mut self, event: LaneEvent<S::Evaluation>, ports: &mut impl Ports<S::Frame, S::Drive>) -> LaneStatus {
        match (self.lane, event) {
            (Lane::Idle, LaneEvent::Initialize) => {
                self.search(LaneEvent::Initialize, ports);
                LaneStatus::Complete
            }
            (Lane::Idle, LaneEvent::Enable) => {
                self.lane = Lane::Active;
                self.search(LaneEvent::Enable, ports);
                LaneStatus::Complete
            }
            (Lane::Idle, LaneEvent::Stop) | (Lane::Active, LaneEvent::Enable) => LaneStatus::Complete,
            (Lane::Idle, _) => LaneStatus::Unhandled,
            (Lane::Active, LaneEvent::Initialize)
            | (Lane::Stopping, LaneEvent::Initialize | LaneEvent::Enable | LaneEvent::Stop) => {
                ports.fault();
                LaneStatus::Refused
            }
            (Lane::Active, LaneEvent::Stop) => {
                let status = self.search(LaneEvent::Stop, ports);
                self.lane = if status == LaneStatus::Complete { Lane::Idle } else { Lane::Stopping };
                status
            }
            (Lane::Active, event) => self.search(event, ports),
            (Lane::Stopping, event) => {
                let status = self.search(event, ports);
                if status == LaneStatus::Complete {
                    self.lane = Lane::Idle;
                }
                status
            }
        }
    }

    fn search(&mut self, event: LaneEvent<S::Evaluation>, ports: &mut impl Ports<S::Frame, S::Drive>) -> LaneStatus {
        use Activity as A;
        match (self.activity, event) {
            (_, LaneEvent::Other) => LaneStatus::Unhandled,

            (A::Idle, LaneEvent::Initialize) => {
                self.advance(SearchInput::Initialize, ports);
                LaneStatus::Complete
            }
            (A::Idle, LaneEvent::Enable) => {
                self.activity = A::Searching;
                self.advance(SearchInput::Begin, ports);
                LaneStatus::Complete
            }
            (A::Searching, LaneEvent::Enable) if self.strategy.accepts_begin(&self.context) => LaneStatus::Complete,
            (A::Idle | A::Halted, LaneEvent::Suspend | LaneEvent::Resume) | (A::Idle, LaneEvent::Stop) => LaneStatus::Complete,
            (A::Halted, LaneEvent::Stop) => {
                self.activity = A::Idle;
                LaneStatus::Complete
            }
            (_, LaneEvent::Initialize | LaneEvent::Enable) => self.refuse(ports),

            (A::Searching | A::Suspended, LaneEvent::Stop) => {
                if self.in_flight() {
                    self.activity = A::Draining;
                    self.advance(SearchInput::Cancel, ports);
                } else {
                    self.release(A::Releasing, ports);
                }
                LaneStatus::Pending
            }
            (A::FaultDraining, LaneEvent::Stop) => {
                self.activity = A::Draining;
                LaneStatus::Pending
            }
            (A::FaultReleasing, LaneEvent::Stop) => {
                self.activity = A::Releasing;
                LaneStatus::Pending
            }
            (_, LaneEvent::Stop) => self.refuse(ports),

            (A::Searching, LaneEvent::Suspend) => {
                if self.in_flight() {
                    self.activity = A::Suspending;
                    self.advance(SearchInput::Cancel, ports);
                    LaneStatus::Pending
                } else {
                    self.suspend(ports)
                }
            }
            (_, LaneEvent::Suspend) => self.refuse(ports),

            (A::Suspended | A::FaultDraining, LaneEvent::Resume) => {
                if self.in_flight() {
                    self.activity = A::Resuming;
                    self.advance(SearchInput::Cancel, ports);
                    LaneStatus::Pending
                } else {
                    self.resume(ports)
                }
            }
            (_, LaneEvent::Resume) => self.refuse(ports),

            (A::Searching | A::Suspended, LaneEvent::Work) => {
                if self.strategy.ready(&self.context, self.bridging) {
                    self.evaluating = true;
                    self.frame = Some(ports.evaluate());
                    LaneStatus::Pending
                } else {
                    LaneStatus::Idle
                }
            }
            (_, LaneEvent::Work) => LaneStatus::Idle,

            (A::Searching | A::Suspended, LaneEvent::Evaluated(evaluation)) => {
                self.evaluating = false;
                if let Some(frame) = self.frame {
                    let measurement = Measurement { frame, evaluation, report: self.report.take(), bridge_busy: self.bridging };
                    self.advance(SearchInput::Sample(measurement), ports);
                }
                LaneStatus::Idle
            }
            (A::Halted, LaneEvent::Evaluated(_)) => LaneStatus::Idle,
            (A::Suspending, LaneEvent::Evaluated(_)) => {
                self.evaluating = false;
                if self.bridging { LaneStatus::Idle } else { self.suspend(ports) }
            }
            (A::Resuming, LaneEvent::Evaluated(_)) => {
                self.evaluating = false;
                if self.bridging { LaneStatus::Idle } else { self.resume(ports) }
            }
            (A::FaultDraining, LaneEvent::Evaluated(_)) => {
                self.evaluating = false;
                self.release(A::FaultReleasing, ports);
                LaneStatus::Idle
            }
            (A::Draining, LaneEvent::Evaluated(_)) => {
                self.evaluating = false;
                if !self.bridging {
                    self.release(A::Releasing, ports);
                }
                LaneStatus::Pending
            }
            (_, LaneEvent::Evaluated(_)) => LaneStatus::Refused,

            (A::Searching | A::Suspended, LaneEvent::LensReported(report)) => {
                self.bridging = false;
                if report.status == BridgeStatus::Fault {
                    if self.evaluating {
                        self.activity = A::FaultDraining;
                    } else {
                        self.release(A::FaultReleasing, ports);
                    }
                } else {
                    self.report = Some(report);
                }
                LaneStatus::Idle
            }
            (A::Suspending, LaneEvent::LensReported(report)) => {
                self.bridging = false;
                self.report = Some(report);
                if self.evaluating { LaneStatus::Idle } else { self.suspend(ports) }
            }
            (A::Resuming, LaneEvent::LensReported(report)) => {
                self.bridging = false;
                self.report = Some(report);
                if self.evaluating { LaneStatus::Idle } else { self.resume(ports) }
            }
            (A::FaultReleasing, LaneEvent::LensReported(_)) => {
                self.bridging = false;
                self.activity = A::Halted;
                LaneStatus::Idle
            }
            (A::Draining, LaneEvent::LensReported(_)) => {
                self.bridging = false;
                if !self.evaluating {
                    self.release(A::Releasing, ports);
                }
                LaneStatus::Pending
            }
            (A::Releasing, LaneEvent::LensReported(report)) => {
                self.bridging = false;
                if report.status == BridgeStatus::Deferred {
                    self.release(A::Releasing, ports);
                    LaneStatus::Pending
                } else {
                    self.activity = A::Idle;
                    LaneStatus::Complete
                }
            }
            (_, LaneEvent::LensReported(_)) => LaneStatus::Refused,
        }
    }

    fn in_flight(&self) -> bool {
        self.evaluating || self.bridging
    }

    fn refuse(&mut self, ports: &mut impl Ports<S::Frame, S::Drive>) -> LaneStatus {
        ports.fault();
        LaneStatus::Refused
    }

    fn suspend(&mut self, ports: &mut impl Ports<S::Frame, S::Drive>) -> LaneStatus {
        self.activity = Activity::Suspended;
        self.advance(SearchInput::Suspend, ports);
        LaneStatus::Complete
    }

    fn resume(&mut self, ports: &mut impl Ports<S::Frame, S::Drive>) -> LaneStatus {
        self.activity = Activity::Searching;
        self.advance(SearchInput::Resume, ports);
        LaneStatus::Complete
    }

    fn release(&mut self, next: Activity, ports: &mut impl Ports<S::Frame, S::Drive>) {
        self.activity = next;
        let drive = self.strategy.release(&self.context);
        self.bridge(BridgeMotion::Release, drive, ports);
    }

    /// Stock drops a bridge request while another is in flight (0xf3e9cc).
    fn bridge(&mut self, motion: BridgeMotion, drive: S::Drive, ports: &mut impl Ports<S::Frame, S::Drive>) {
        if !self.bridging {
            self.bridging = true;
            ports.request(Request::Bridge { motion, drive });
        }
    }

    fn advance(&mut self, input: SearchInput<S::Sample>, ports: &mut impl Ports<S::Frame, S::Drive>) {
        match self.strategy.advance(&mut self.context, input) {
            SearchDecision::Next(SearchStep::Measure) => {}
            SearchDecision::Next(SearchStep::Travel { travel, drive }) => self.bridge(BridgeMotion::Travel(travel), drive, ports),
            SearchDecision::Next(SearchStep::Interrupt) => ports.request(Request::BridgeStop),
            // Stock's strategy never ends; one that does keeps the lens and watches the scene.
            SearchDecision::Terminal(SearchTerminal::Focused(_) | SearchTerminal::Failed | SearchTerminal::Cancelled) => {
                if self.activity == Activity::Searching {
                    self.activity = Activity::Suspended;
                }
            }
        }
    }
}
