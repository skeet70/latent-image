#![forbid(unsafe_code)]

use crate::calculation::Calculation;
use crate::exposure::Exposure;
use crate::finish::Finish;
use crate::outcome::{fault, Outcome};
use crate::pipeline::Pipeline;
use crate::ports::{Branch, Effect, FinishKind, Ports, Query, Reply, SequenceEffect as S, Signal};
use xm1_saclac_interface::{BlockingRequest, CommandKind, CompletionStatus, RecordHandle};

#[derive(Default)]
pub struct Steps {
    pub exposure: Exposure,
    pub calculation: Calculation,
    pub pipeline: Pipeline,
    pub finish: Finish,
}

impl Steps {
    fn signal(&mut self, step: Step, signal: Signal, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        match signal {
            Signal::Completion(completion) => match step {
                Step::Exposure => self.exposure.completed(completion, ports),
                Step::Calculation => self.calculation.completed(completion, ports),
                Step::Pipeline => self.pipeline.completed(signal, record, &mut self.finish, ports),
                Step::Finish => self.finish.completed(signal, ports),
            },
            Signal::Command(_) => match step {
                Step::Pipeline => self.pipeline.completed(signal, record, &mut self.finish, ports),
                Step::Finish => self.finish.completed(signal, ports),
                Step::Exposure | Step::Calculation => Outcome::Unhandled,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step { Exposure, Calculation, Pipeline, Finish }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HalfPressStep { Idle, FirstExposure, FirstCalculation, Exposure, Calculation, Pipeline }

pub struct HalfPress { step: HalfPressStep, record: Option<RecordHandle> }

impl Default for HalfPress { fn default() -> Self { Self::new() } }

impl HalfPress {
    pub const fn new() -> Self { Self { step: HalfPressStep::Idle, record: None } }

    pub fn start(&mut self, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        if self.step != HalfPressStep::Idle { return fault(ports); }
        self.record = Some(record);
        ports.effect(Effect::Sequence(S::BindRecord(record)));
        let mode = ports.half_press_mode();
        if mode.synchronizes_adjustment() { ports.blocking(BlockingRequest::AdjustmentSynchronize); }
        if mode.synchronizes_lens() && !ports.query(Query::SkipLensSynchronization) { ports.blocking(BlockingRequest::LensSynchronize); }
        ports.effect(Effect::Sequence(S::PrepareRecords(Branch::HalfPress)));
        ports.effect(Effect::Sequence(S::SetRefinementPass(false)));
        if !ports.query(Query::HasWork) { return self.conclude(ports); }
        if mode.two_pass_exposure() && ports.query(Query::TwoPassExposure) {
            self.step = HalfPressStep::FirstExposure;
            ports.effect(Effect::Sequence(S::SetRefinementPass(true)));
        } else {
            self.step = HalfPressStep::Exposure;
        }
        steps.exposure.start(Branch::HalfPress, record, ports)
    }

    pub fn advance(&mut self, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        steps.pipeline.advance(record, ports)
    }

    pub fn command(&mut self, command: CommandKind, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        if command != CommandKind::Autofocus { return self.signal(Signal::Command(command), record, steps, ports); }
        if self.step != HalfPressStep::Idle { return fault(ports); }
        ports.reply(Reply { command, record: None, status: CompletionStatus(0) });
        Outcome::Pending
    }

    pub fn signal(&mut self, signal: Signal, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        use HalfPressStep::*;
        let step = match self.step {
            Idle => return Outcome::Unhandled,
            FirstExposure | Exposure => Step::Exposure,
            FirstCalculation | Calculation => Step::Calculation,
            Pipeline => Step::Pipeline,
        };
        let outcome = steps.signal(step, signal, record, ports);
        if outcome != Outcome::Complete { return outcome; }
        match self.step {
            FirstExposure => {
                self.step = FirstCalculation;
                steps.calculation.start(Branch::HalfPress, record, ports)
            }
            FirstCalculation => {
                self.step = Exposure;
                ports.effect(Effect::Sequence(S::SetRefinementPass(false)));
                steps.exposure.start(Branch::HalfPress, record, ports)
            }
            Exposure => {
                self.step = Calculation;
                steps.calculation.start(Branch::HalfPress, record, ports)
            }
            Calculation => {
                self.step = Pipeline;
                let outcome = steps.pipeline.start(record, &mut steps.finish, ports);
                if outcome == Outcome::Complete { self.conclude(ports) } else { outcome }
            }
            Pipeline | Idle => self.conclude(ports),
        }
    }

    fn conclude(&mut self, ports: &mut impl Ports) -> Outcome {
        self.step = HalfPressStep::Idle;
        if ports.query(Query::LensStatusPending) { ports.effect(Effect::Sequence(S::RecordLensStatus)); }
        ports.reply(Reply { command: CommandKind::HalfPress, record: self.record, status: CompletionStatus(0) });
        ports.effect(Effect::Sequence(S::Replied));
        Outcome::Complete
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveStep { Idle, Exposure, Calculation, Finish }

pub struct Live { step: LiveStep }

impl Default for Live { fn default() -> Self { Self::new() } }

impl Live {
    pub const fn new() -> Self { Self { step: LiveStep::Idle } }

    pub fn start(&mut self, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        ports.effect(Effect::Sequence(S::NoteLiveRecord(record)));
        if self.step != LiveStep::Idle { return fault(ports); }
        ports.effect(Effect::Sequence(S::BindRecord(record)));
        ports.effect(Effect::Sequence(S::PrepareRecords(Branch::Live)));
        let outcome = steps.exposure.start(Branch::Live, record, ports);
        self.continue_from(LiveStep::Exposure, outcome, record, steps, ports)
    }

    pub fn signal(&mut self, signal: Signal, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        ports.effect(Effect::Sequence(S::NoteLiveRecord(record)));
        let step = match self.step {
            LiveStep::Idle => return Outcome::Unhandled,
            LiveStep::Exposure => Step::Exposure,
            LiveStep::Calculation => Step::Calculation,
            LiveStep::Finish => Step::Finish,
        };
        let outcome = steps.signal(step, signal, record, ports);
        self.continue_from(self.step, outcome, record, steps, ports)
    }

    fn continue_from(&mut self, mut step: LiveStep, mut outcome: Outcome, record: RecordHandle, steps: &mut Steps, ports: &mut impl Ports) -> Outcome {
        loop {
            match outcome {
                Outcome::Complete => {}
                Outcome::Pending => { self.step = step; return outcome; }
                _ => return outcome,
            }
            match step {
                LiveStep::Exposure => {
                    step = LiveStep::Calculation;
                    outcome = steps.calculation.start(Branch::Live, record, ports);
                }
                LiveStep::Calculation => {
                    step = LiveStep::Finish;
                    outcome = steps.finish.start(FinishKind::Live, record, ports);
                }
                LiveStep::Finish | LiveStep::Idle => {
                    self.step = LiveStep::Idle;
                    ports.effect(Effect::Sequence(S::ReleaseLiveResources));
                    return Outcome::Complete;
                }
            }
        }
    }
}
