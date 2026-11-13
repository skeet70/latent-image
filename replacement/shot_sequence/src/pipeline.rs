#![forbid(unsafe_code)]

use crate::finish::Finish;
use crate::outcome::{fault, Outcome};
use crate::ports::{Effect, FinishKind, PipelineEffect as P, PipelinePhase, PipelineQuery as Q, PipelineRequest as R, Ports, Query, Reply, Signal};
use xm1_saclac_interface::{CommandKind, Completion, CompletionStatus, RecordHandle};

pub const FRAME_RELEASED_STATUS: CompletionStatus = CompletionStatus(5);

/// Each stage names the requests it waits for; `Cancelled*` stages follow an advance event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Idle,
    Image1d,
    Calc03,
    Image1e,
    Image1f,
    Calc04,
    CancelledBeforeFrame,
    CancelledFrame,
    CancelledCalc04,
    Image21,
    Image21AfterCalc04,
    Image21Retry,
    Calc05,
    Image22,
    Statistics03,
    FinishLens,
    Commit,
    FinishLast,
}

pub struct Pipeline { stage: Stage, active: bool, record: Option<RecordHandle> }

impl Default for Pipeline { fn default() -> Self { Self::new() } }

impl Pipeline {
    pub const fn new() -> Self { Self { stage: Stage::Idle, active: false, record: None } }

    pub fn start(&mut self, record: RecordHandle, finish: &mut Finish, ports: &mut impl Ports) -> Outcome {
        if self.active { return fault(ports); }
        if ports.query(Query::LensStatusPending) { ports.effect(Effect::Pipeline(P::RecordLensStatus)); }
        ports.effect(Effect::Pipeline(P::BindRecord(record)));
        let outcome = self.begin(record, finish, ports);
        if outcome != Outcome::Complete { self.active = true; }
        outcome
    }

    pub fn advance(&mut self, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        use Stage::*;
        match self.stage {
            Idle => ports.effect(Effect::Pipeline(P::MarkRestart)),
            Image1d | Calc03 | Image1e => {
                if ports.query(Query::Pipeline(Q::SkipLensFinish)) { Self::configure(record, false, ports); }
                self.stage = CancelledBeforeFrame;
            }
            Image1f => { self.request(R::Image21, ports); self.stage = CancelledFrame; }
            Calc04 => { self.request(R::Image21, ports); self.stage = CancelledCalc04; }
            CancelledBeforeFrame | CancelledFrame | CancelledCalc04 => return fault(ports),
            Image21Retry => self.stage = CancelledFrame,
            Calc05 => self.stage = CancelledCalc04,
            Image21 | Image21AfterCalc04 | Image22 | Statistics03 | FinishLens | Commit | FinishLast => {}
        }
        Outcome::Pending
    }

    pub fn completed(&mut self, signal: Signal, record: RecordHandle, finish: &mut Finish, ports: &mut impl Ports) -> Outcome {
        if !self.active { return Outcome::Unhandled; }
        let outcome = match signal {
            Signal::Completion(completion) => self.step(completion, record, finish, ports),
            Signal::Command(_) => self.finishing(signal, finish, ports),
        };
        if outcome == Outcome::Complete { self.active = false; }
        outcome
    }

    fn begin(&mut self, record: RecordHandle, finish: &mut Finish, ports: &mut impl Ports) -> Outcome {
        self.record = Some(record);
        ports.effect(Effect::Pipeline(P::Prepare(record)));
        if ports.query(Query::Pipeline(Q::FinishOnly)) {
            self.stage = if ports.query(Query::Pipeline(Q::CaptureIdle)) { Stage::FinishLast } else { Stage::FinishLens };
            return finish.start(FinishKind::HalfPressLens, record, ports);
        }
        if !ports.query(Query::Pipeline(Q::FrameReady)) {
            return self.release_and_finish(record, true, false, finish, ports);
        }
        if ports.query(Query::Pipeline(Q::Restarted)) {
            return self.release_and_finish(record, false, true, finish, ports);
        }
        if ports.query(Query::Pipeline(Q::NeedsRaw)) {
            self.stage = Stage::Image1d;
            self.request(R::Image1d, ports);
        } else {
            self.start_frame(ports);
        }
        Outcome::Pending
    }

    fn release_and_finish(&mut self, record: RecordHandle, first: bool, clear_ready: bool, finish: &mut Finish, ports: &mut impl Ports) -> Outcome {
        let immediate = ports.query(Query::Pipeline(Q::SkipLensFinish));
        self.stage = if immediate { Stage::Idle } else { Stage::FinishLast };
        if clear_ready { ports.effect(Effect::Pipeline(P::ClearFrameReady)); }
        Self::configure(record, first, ports);
        self.release_frame(ports);
        finish.start(if immediate { FinishKind::HalfPressImmediate } else { FinishKind::HalfPressLens }, record, ports)
    }

    fn start_frame(&mut self, ports: &mut impl Ports) {
        self.stage = Stage::Image1e;
        ports.effect(Effect::Pipeline(P::StartFrame));
        self.request(R::Image1e, ports);
        ports.effect(Effect::Pipeline(P::PrepareFrame));
        ports.effect(Effect::Pipeline(P::ArmFrame));
        ports.effect(Effect::Pipeline(P::SynchronizeFrame));
        self.request(R::Image1f, ports);
    }

    fn configure(record: RecordHandle, first: bool, ports: &mut impl Ports) {
        ports.effect(Effect::Pipeline(P::Configure { record, first }));
    }

    fn release_frame(&mut self, ports: &mut impl Ports) {
        ports.effect(Effect::Pipeline(P::ReleaseFrame));
        if ports.half_press_notifications().frame_released {
            ports.reply(Reply { command: CommandKind::HalfPress, record: self.record, status: FRAME_RELEASED_STATUS });
        }
        ports.effect(Effect::Pipeline(P::FrameReleased));
    }

    fn output(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        self.stage = Stage::Image22;
        ports.effect(Effect::Pipeline(P::MarkFrameReady));
        Self::configure(record, false, ports);
        self.release_frame(ports);
        self.request(R::Image22, ports);
    }

    fn request(&mut self, request: R, ports: &mut impl Ports) { ports.effect(Effect::Pipeline(P::Request(request))); }

    fn upload_done(ports: &mut impl Ports) {
        ports.effect(Effect::Pipeline(P::SetStatisticsPending(false)));
        ports.effect(Effect::Pipeline(P::SetUploadDone(true)));
    }

    fn step(&mut self, completion: Completion, record: RecordHandle, finish: &mut Finish, ports: &mut impl Ports) -> Outcome {
        use Completion::*;
        use Stage::*;
        match (self.stage, completion) {
            (Image1e, Image4f) => {
                if ports.query(Query::Pipeline(Q::StatisticsReady)) {
                    ports.effect(Effect::Pipeline(P::SetStatisticsPending(true)));
                    self.request(R::Statistics03, ports);
                }
                self.stage = Image1f;
            }
            (CancelledBeforeFrame, Image4f) => { self.request(R::Image21, ports); self.stage = CancelledFrame; }
            (Image1f, Image50) => {
                let skip = ports.query(Query::Pipeline(Q::ToneRequested)) || ports.query(Query::Pipeline(Q::ToneSkipped));
                if skip {
                    self.stage = Image21;
                    self.request(R::Image21, ports);
                    ports.effect(Effect::Pipeline(P::SetPhase(PipelinePhase::Second)));
                } else {
                    self.stage = Calc04;
                    self.request(R::Calc04, ports);
                }
            }
            (Calc04, Image50) => {
                self.request(R::Image21, ports);
                ports.effect(Effect::Pipeline(P::SetPhase(PipelinePhase::First)));
                self.stage = Image21AfterCalc04;
            }
            (CancelledFrame | CancelledCalc04 | Image21 | Image21AfterCalc04 | Image21Retry | Calc05, Image50) => {}
            (Calc04, Calculation92) => {
                if ports.query(Query::Pipeline(Q::ToneCalculationFinal)) {
                    self.stage = Image21Retry;
                    self.request(R::Image21, ports);
                } else {
                    self.stage = Image1f;
                    self.request(R::Image20, ports);
                }
            }
            (CancelledCalc04, Calculation92 | Calculation93) => {
                if ports.query(Query::Pipeline(Q::ToneResultSet)) { self.stage = CancelledFrame; } else { self.output(record, ports); }
            }
            (Image21AfterCalc04, Calculation92) => {
                if ports.query(Query::Pipeline(Q::ToneResultSet)) { self.stage = Image21; } else { self.output(record, ports); }
            }
            (Calc05, Calculation92) => self.request(R::Calc05, ports),
            (Image1f | Image21Retry, Image51) => {
                let gradation = ports.query(Query::Pipeline(Q::GradationNeeded));
                ports.effect(Effect::Pipeline(P::SetToneResult(false)));
                if gradation {
                    self.stage = Calc05;
                    self.request(R::Calc05, ports);
                } else {
                    self.output(record, ports);
                    ports.effect(Effect::Pipeline(P::SetPhase(PipelinePhase::Third)));
                }
            }
            (Calc04, Image51) => {
                let gradation = ports.query(Query::Pipeline(Q::GradationNeeded));
                ports.effect(Effect::Pipeline(P::SetToneResult(false)));
                if gradation {
                    self.stage = Calc05;
                } else {
                    self.stage = Image21AfterCalc04;
                    ports.effect(Effect::Pipeline(P::SetPhase(PipelinePhase::Third)));
                }
            }
            (CancelledFrame | Image21, Image51) => {
                ports.effect(Effect::Pipeline(P::SetToneResult(false)));
                self.output(record, ports);
            }
            (CancelledCalc04 | Image21AfterCalc04, Image51) => ports.effect(Effect::Pipeline(P::SetToneResult(false))),
            (Calc05, Calculation93) => {
                ports.effect(Effect::Pipeline(P::PrepareFrame));
                if ports.query(Query::Pipeline(Q::ToneRestart)) {
                    self.stage = Image1f;
                    ports.effect(Effect::Pipeline(P::ResumeFrame));
                    ports.effect(Effect::Pipeline(P::ReleaseFrameLocks));
                    ports.effect(Effect::Pipeline(P::ArmFrame));
                    ports.effect(Effect::Pipeline(P::SynchronizeFrame));
                    self.request(R::Image1f, ports);
                } else {
                    self.stage = Image22;
                    ports.effect(Effect::Pipeline(P::PrepareFallback));
                    self.release_frame(ports);
                    self.request(R::Image22, ports);
                }
            }
            (Image1f | Calc04 | CancelledFrame | CancelledCalc04 | Image21 | Image21AfterCalc04 | Image21Retry | Calc05 | Image22 | FinishLens, Statistics7a) => Self::upload_done(ports),
            (Statistics03, Statistics7a) => {
                self.stage = Idle;
                Self::upload_done(ports);
                return finish.start(FinishKind::HalfPressImmediate, record, ports);
            }
            (Commit, Statistics7a) => {
                Self::upload_done(ports);
                self.request(R::Statistics07, ports);
            }
            (Commit, Statistics6b) => {
                ports.effect(Effect::Pipeline(P::SetUploadDone(false)));
                self.request(R::Image23, ports);
            }
            (Image22, Image52) => {
                ports.effect(Effect::Pipeline(P::ResumeFrame));
                ports.effect(Effect::Pipeline(P::ReleaseFrameLocks));
                if !ports.query(Query::Pipeline(Q::SkipLensFinish)) {
                    self.stage = FinishLens;
                    return finish.start(FinishKind::HalfPressLens, record, ports);
                }
                if ports.query(Query::Pipeline(Q::StatisticsPending)) {
                    self.stage = Statistics03;
                } else {
                    self.stage = Idle;
                    return finish.start(FinishKind::HalfPressImmediate, record, ports);
                }
            }
            (Commit, Image53) => {
                if ports.query(Query::Pipeline(Q::FinishDone)) {
                    self.stage = Idle;
                    ports.effect(Effect::Pipeline(P::Reset));
                    return Outcome::Complete;
                }
                self.stage = FinishLast;
            }
            (Image1d, Image4e) => {
                if ports.query(Query::Pipeline(Q::RawCalculationNeeded)) {
                    self.stage = Calc03;
                    self.request(R::Calc03, ports);
                } else {
                    return self.release_and_finish(record, false, true, finish, ports);
                }
            }
            (CancelledBeforeFrame, Image4e | Calculation91) => return self.release_and_finish(record, false, true, finish, ports),
            (Calc03, Calculation91) => {
                ports.effect(Effect::Pipeline(P::ConcludeRaw));
                self.start_frame(ports);
            }
            (_, Image4e | Image4f | Image50 | Image51 | Image52 | Image53 | Calculation91 | Calculation92 | Calculation93 | Statistics6b | Statistics7a) => return fault(ports),
            (FinishLens | Commit | FinishLast, _) => return self.finishing(Signal::Completion(completion), finish, ports),
            _ => return Outcome::Unhandled,
        }
        Outcome::Pending
    }

    fn finishing(&mut self, signal: Signal, finish: &mut Finish, ports: &mut impl Ports) -> Outcome {
        match self.stage {
            Stage::FinishLens => {
                let outcome = finish.completed(signal, ports);
                if outcome == Outcome::Complete { ports.effect(Effect::Pipeline(P::SetFinishDone)); }
                if matches!(outcome, Outcome::Complete | Outcome::Progress) {
                    self.stage = Stage::Commit;
                    if ports.query(Query::Pipeline(Q::UploadDone)) {
                        self.request(R::Statistics07, ports);
                    } else if !ports.query(Query::Pipeline(Q::StatisticsPending)) {
                        self.request(R::Image23, ports);
                    }
                }
                Outcome::Pending
            }
            Stage::Commit => {
                if finish.completed(signal, ports) == Outcome::Complete { ports.effect(Effect::Pipeline(P::SetFinishDone)); }
                Outcome::Pending
            }
            Stage::FinishLast => {
                if finish.completed(signal, ports) == Outcome::Complete {
                    self.stage = Stage::Idle;
                    ports.effect(Effect::Pipeline(P::Reset));
                    return Outcome::Complete;
                }
                Outcome::Pending
            }
            _ => Outcome::Unhandled,
        }
    }
}
