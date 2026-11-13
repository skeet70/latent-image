#![forbid(unsafe_code)]

use crate::outcome::{fault, Outcome};
use crate::ports::{
    CalculationsEffect as CE, CalculationsJob, CalculationsQuery as CQ, Effect, LensMove, Ports, PrimaryEffect as PE, PrimaryQuery as PQ, Query, Reply,
    ReturnEffect as RE, ReturnQuery as RQ, SecondaryEffect as SE, SecondaryQuery as SQ, Signal,
};
use xm1_saclac_interface::{CommandKind, Completion, RecordHandle};

/// Lens return after a capture: calc 0x3c02 (0x90), then lensdc 0x4213 (0x3a) and 0x420b (0x35).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReturnState { Idle, Calculating, Absolute, Relative }

pub struct Return { state: ReturnState }

impl Return {
    pub const fn new() -> Self { Self { state: ReturnState::Idle } }

    pub fn start(&mut self, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.state != ReturnState::Idle { return fault(ports); }
        ports.effect(Effect::Return(RE::Begin(record)));
        ports.effect(Effect::Return(RE::PrepareRecords));
        ports.effect(Effect::Return(RE::MarkActive(true)));
        ports.effect(Effect::Return(RE::PrepareCalculation));
        ports.effect(Effect::Return(RE::AbsorbCalculation));
        ports.effect(Effect::Return(RE::ConcludeExposure));
        if ports.query(Query::Return(RQ::NeedsCalculation)) {
            self.state = ReturnState::Calculating;
            ports.effect(Effect::Return(RE::RequestCalculation));
            return Outcome::Pending;
        }
        self.moves(ports)
    }

    fn moves(&mut self, ports: &mut impl Ports) -> Outcome {
        ports.effect(Effect::Return(RE::PrepareMoves));
        if ports.query(Query::Return(RQ::NeedsAbsoluteMove)) {
            self.state = ReturnState::Absolute;
            ports.effect(Effect::Lens(LensMove::ReturnAbsolute));
            return Outcome::Pending;
        }
        self.done(ports)
    }

    fn done(&mut self, ports: &mut impl Ports) -> Outcome {
        self.state = ReturnState::Idle;
        ports.effect(Effect::Return(RE::MarkActive(false)));
        Outcome::Complete
    }

    pub fn signal(&mut self, signal: Signal, ports: &mut impl Ports) -> Outcome {
        let Signal::Completion(completion) = signal else { return Outcome::Unhandled };
        match (self.state, completion) {
            (ReturnState::Calculating, Completion::Calculation90) => {
                ports.effect(Effect::Return(RE::AbsorbCalculation));
                ports.effect(Effect::Return(RE::ConcludeExposure));
                self.moves(ports)
            }
            (ReturnState::Absolute, Completion::Lens3a) => {
                self.state = ReturnState::Relative;
                ports.effect(Effect::Return(RE::PrepareRelative));
                ports.effect(Effect::Lens(LensMove::ReturnRelative));
                Outcome::Pending
            }
            (ReturnState::Relative, Completion::Lens35) => self.done(ports),
            (_, Completion::Calculation90 | Completion::Lens3a | Completion::Lens35) => fault(ports),
            _ => Outcome::Unhandled,
        }
    }
}

/// Imgdc 0x3f46 (0x5b, 0x5c), calc 0x3c0d (0x94, 0x95) and imgdc 0x3f47 (0x5d).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecondaryState { Idle, Measuring, Calculating, CalculatedEarly, Refining, Rejected, Transferring }

pub struct Secondary { state: SecondaryState }

impl Secondary {
    pub const fn new() -> Self { Self { state: SecondaryState::Idle } }

    pub fn start(&mut self, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.state != SecondaryState::Idle { return fault(ports); }
        ports.effect(Effect::Secondary(SE::Begin(record)));
        ports.effect(Effect::Secondary(SE::ResetCount));
        ports.effect(Effect::Secondary(SE::Configure));
        ports.effect(Effect::Secondary(SE::Request));
        self.state = SecondaryState::Measuring;
        Outcome::Pending
    }

    pub fn signal(&mut self, signal: Signal, _record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        use SecondaryState::*;
        let Signal::Completion(completion) = signal else { return Outcome::Unhandled };
        match (self.state, completion) {
            (Measuring, Completion::Image5c) => {
                if !ports.query(Query::Secondary(SQ::Accepted)) {
                    self.state = Rejected;
                } else if ports.query(Query::Secondary(SQ::FirstPass)) {
                    ports.effect(Effect::Secondary(SE::PrepareFirst));
                } else if ports.query(Query::Secondary(SQ::Alternative)) {
                    self.state = Refining;
                    ports.effect(Effect::Secondary(SE::PrepareNext));
                } else {
                    self.state = Calculating;
                    ports.effect(Effect::Secondary(SE::RequestCalculation));
                }
                Outcome::Pending
            }
            (Rejected, Completion::Image5c) => Outcome::Pending,
            (Measuring | Rejected, Completion::Image5b) => {
                self.state = Idle;
                ports.effect(Effect::Secondary(SE::Absorb));
                ports.effect(Effect::Secondary(SE::AbsorbStatistics));
                Outcome::Complete
            }
            (Calculating, Completion::Image5b) => {
                self.state = CalculatedEarly;
                Outcome::Pending
            }
            (Refining, Completion::Image5b) => {
                ports.effect(Effect::Secondary(SE::AbsorbStatistics));
                self.transfer(ports)
            }
            (Calculating, Completion::Calculation94) => {
                ports.effect(Effect::Secondary(SE::AbsorbCalculation));
                self.state = Refining;
                Outcome::Pending
            }
            (CalculatedEarly, Completion::Calculation94) => {
                ports.effect(Effect::Secondary(SE::AbsorbCalculation));
                ports.effect(Effect::Secondary(SE::AbsorbStatistics));
                self.transfer(ports)
            }
            (Idle | Refining | Transferring, Completion::Calculation95) => {
                ports.effect(Effect::Secondary(SE::AbsorbSecondCalculation));
                Outcome::Pending
            }
            (Transferring, Completion::Image5d) => {
                self.state = Idle;
                Outcome::Complete
            }
            (_, Completion::Image5b | Completion::Image5c | Completion::Calculation94 | Completion::Calculation95 | Completion::Image5d) => fault(ports),
            _ => Outcome::Unhandled,
        }
    }

    fn transfer(&mut self, ports: &mut impl Ports) -> Outcome {
        if ports.query(Query::Secondary(SQ::TransferPending)) {
            self.state = SecondaryState::Transferring;
            ports.effect(Effect::Secondary(SE::RequestTransfer));
            Outcome::Pending
        } else {
            self.state = SecondaryState::Idle;
            ports.effect(Effect::Secondary(SE::Release));
            Outcome::Complete
        }
    }
}

/// Imgdc 0x3f49 (0x5e, 0x62), statistics 0x4005 (0x7d), imgdc 0x3f4e, lens moves (0x35, 0x3a)
/// and notifications 0x5f, 0x60, 0x61, 0x64.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrimaryState { Idle, Readout, Delivering, ReadoutThenLens, LensOnly, Settled, Displaying, Transferring }

pub struct Primary { state: PrimaryState, record: Option<RecordHandle> }

impl Primary {
    pub const fn new() -> Self { Self { state: PrimaryState::Idle, record: None } }

    pub fn start(&mut self, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        if self.state != PrimaryState::Idle { return fault(ports); }
        self.record = Some(record);
        ports.effect(Effect::Primary(PE::Begin(record)));
        ports.effect(Effect::Primary(PE::ResetFlags));
        ports.effect(Effect::Primary(PE::PrepareRecords));
        ports.effect(Effect::Primary(PE::PrepareTransfer));
        let image_first = ports.query(Query::Primary(PQ::ImageFirst));
        let variant = ports.query(Query::Primary(PQ::ImageVariant));
        self.state = match (image_first, variant) {
            (true, true) => PrimaryState::Readout,
            (true, false) => PrimaryState::Delivering,
            (false, true) => PrimaryState::ReadoutThenLens,
            (false, false) => PrimaryState::LensOnly,
        };
        if self.state == PrimaryState::LensOnly {
            if ports.query(Query::Primary(PQ::NeedsRelative)) {
                ports.effect(Effect::Primary(PE::PrepareRelative));
                ports.effect(Effect::Lens(LensMove::CaptureRelative));
            } else {
                ports.self_complete(LensMove::CaptureRelative.completion());
                if ports.query(Query::Primary(PQ::NeedsAbsolute)) {
                    ports.effect(Effect::Lens(LensMove::CaptureAbsolute));
                } else {
                    ports.self_complete(LensMove::CaptureAbsolute.completion());
                }
            }
        } else {
            ports.effect(Effect::Primary(PE::Request));
            if ports.query(Query::Primary(PQ::CommandCPending)) { ports.effect(Effect::Primary(PE::RequestCommandC)); }
            if ports.query(Query::Primary(PQ::Command28Pending)) { ports.effect(Effect::Primary(PE::RequestCommand28)); }
        }
        Outcome::Pending
    }

    pub fn command(&mut self, command: CommandKind, _record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        use PrimaryState::*;
        match (self.state, command) {
            (Idle, CommandKind::Command0c) => ports.effect(Effect::Primary(PE::MarkCommandC)),
            (Readout | Delivering | ReadoutThenLens, CommandKind::Command0c) => ports.effect(Effect::Primary(PE::RequestCommandC)),
            (LensOnly, CommandKind::Command0c) => {}
            (Idle, CommandKind::Command28) => ports.effect(Effect::Primary(PE::MarkCommand28)),
            (_, CommandKind::Command28) => ports.effect(Effect::Primary(PE::RequestCommand28)),
            (_, CommandKind::Command0c) => return fault(ports),
            _ => return Outcome::Unhandled,
        }
        Outcome::Pending
    }

    pub fn signal(&mut self, signal: Signal, record: RecordHandle, calculations: &mut Calculations, ports: &mut impl Ports) -> Outcome {
        use PrimaryState::*;
        let completion = match signal {
            Signal::Command(command @ (CommandKind::Command0c | CommandKind::Command28)) => return self.command(command, record, ports),
            Signal::Command(_) => {
                return match self.state {
                    Idle => Outcome::Unhandled,
                    Displaying | Transferring => fault(ports),
                    _ => calculations.signal(signal, record, ports),
                };
            }
            Signal::Completion(completion) => completion,
        };
        match (self.state, completion) {
            (Readout | Delivering | ReadoutThenLens | LensOnly | Settled, Completion::Image62) => {
                if ports.query(Query::Primary(PQ::StatisticsWanted)) { ports.effect(Effect::Primary(PE::RequestStatistics)); }
                Outcome::Pending
            }
            (Readout | ReadoutThenLens, Completion::Image61) => {
                if !ports.query(Query::Primary(PQ::BlockReady)) { return self.block(ports); }
                let outcome = calculations.signal(Signal::Completion(completion), record, ports);
                if outcome != Outcome::Complete { return outcome; }
                self.readout_done(ports)
            }
            (Delivering | LensOnly, Completion::Image61) => self.block(ports),
            (Readout, Completion::Image5f) => {
                ports.effect(Effect::Primary(PE::Output));
                Outcome::Pending
            }
            (Delivering, Completion::Image5f) => {
                ports.effect(Effect::Primary(PE::Output));
                ports.effect(Effect::Primary(PE::PublishProgress));
                ports.effect(Effect::Primary(PE::Account));
                Outcome::Pending
            }
            (Readout, Completion::Image60) => {
                ports.effect(Effect::Primary(PE::Output));
                self.state = ReadoutThenLens;
                Outcome::Pending
            }
            (Delivering, Completion::Image60) => {
                ports.effect(Effect::Primary(PE::Output));
                ports.effect(Effect::Primary(PE::Account));
                ports.effect(Effect::Primary(PE::PublishFinish));
                self.state = LensOnly;
                Outcome::Pending
            }
            (Readout | ReadoutThenLens | Settled, Completion::Image64) => {
                ports.effect(Effect::Primary(PE::Delivered));
                Outcome::Pending
            }
            (Delivering | LensOnly, Completion::Image64) => {
                ports.effect(Effect::Primary(PE::Delivered));
                ports.effect(Effect::Primary(PE::PublishProgress));
                Outcome::Pending
            }
            (Displaying, Completion::Image64) => {
                ports.effect(Effect::Primary(PE::Delivered));
                ports.effect(Effect::Primary(PE::PublishProgress));
                if ports.query(Query::Primary(PQ::TransferPending)) {
                    self.state = Transferring;
                    return Outcome::Pending;
                }
                self.state = Idle;
                ports.effect(Effect::Primary(PE::ClearCommandC));
                Outcome::Complete
            }
            (Readout | Delivering | Settled | Displaying | Transferring, Completion::Image5e) => self.abort_check(ports).unwrap_or(Outcome::Pending),
            (ReadoutThenLens, Completion::Image5e) => {
                ports.effect(Effect::Primary(PE::RestoreLens));
                if let Some(outcome) = self.abort_check(ports) { return outcome; }
                if !self.lens_moves(ports) { self.state = Settled; }
                Outcome::Pending
            }
            (LensOnly, Completion::Image5e) => {
                if let Some(outcome) = self.abort_check(ports) { return outcome; }
                if self.lens_moves(ports) { Outcome::Pending } else { self.settle(ports) }
            }
            (ReadoutThenLens, Completion::Lens3a) => {
                self.state = Settled;
                Outcome::Pending
            }
            (LensOnly, Completion::Lens3a) => self.settle(ports),
            (ReadoutThenLens, Completion::Lens35) => {
                if ports.query(Query::Primary(PQ::NeedsAbsolute)) {
                    ports.effect(Effect::Lens(LensMove::CaptureAbsolute));
                } else {
                    self.state = Settled;
                }
                Outcome::Pending
            }
            (LensOnly, Completion::Lens35) => {
                if ports.query(Query::Primary(PQ::NeedsAbsolute)) {
                    ports.effect(Effect::Lens(LensMove::CaptureAbsolute));
                    Outcome::Pending
                } else {
                    self.settle(ports)
                }
            }
            (Readout | ReadoutThenLens | Settled, Completion::Statistics7d) => {
                self.statistics_seen(ports);
                Outcome::Pending
            }
            (Delivering | LensOnly | Displaying, Completion::Statistics7d) => {
                self.statistics_seen(ports);
                ports.effect(Effect::Primary(PE::Account));
                Outcome::Pending
            }
            (Transferring, Completion::Statistics7d) => {
                self.statistics_seen(ports);
                ports.effect(Effect::Primary(PE::Account));
                self.complete(ports)
            }
            (_, Completion::Image5e | Completion::Image5f | Completion::Image60 | Completion::Image61 | Completion::Image62 | Completion::Image64 | Completion::Lens35 | Completion::Lens3a | Completion::Statistics7d) => fault(ports),
            (Idle, _) => Outcome::Unhandled,
            (Readout | ReadoutThenLens, _) => {
                let outcome = calculations.signal(signal, record, ports);
                if outcome != Outcome::Complete { return outcome; }
                self.readout_done(ports)
            }
            (Delivering | LensOnly, _) => calculations.signal(signal, record, ports),
            (Settled, _) => {
                let outcome = calculations.signal(signal, record, ports);
                if outcome != Outcome::Complete { return outcome; }
                ports.effect(Effect::Primary(PE::Account));
                ports.effect(Effect::Primary(PE::PublishFinish));
                if !ports.query(Query::Primary(PQ::DisplayEnabled)) { return self.transfer_or_complete(ports); }
                if !ports.query(Query::Primary(PQ::Delivered)) {
                    self.state = Displaying;
                    return Outcome::Pending;
                }
                ports.effect(Effect::Primary(PE::PublishProgress));
                self.transfer_or_complete(ports)
            }
            (Displaying | Transferring, _) => fault(ports),
        }
    }

    fn block(&mut self, ports: &mut impl Ports) -> Outcome {
        ports.effect(Effect::Primary(PE::Block));
        ports.effect(Effect::Primary(PE::Account));
        Outcome::Pending
    }

    fn readout_done(&mut self, ports: &mut impl Ports) -> Outcome {
        let lens_follows = self.state == PrimaryState::ReadoutThenLens;
        self.state = if lens_follows { PrimaryState::LensOnly } else { PrimaryState::Delivering };
        ports.effect(Effect::Primary(PE::PublishProgress));
        ports.effect(Effect::Primary(PE::Account));
        if lens_follows { ports.effect(Effect::Primary(PE::PublishFinish)); }
        Outcome::Pending
    }

    fn abort_check(&mut self, ports: &mut impl Ports) -> Option<Outcome> {
        let status = ports.capture_abort()?;
        ports.reply(Reply { command: CommandKind::Command07, record: self.record, status: status.0 });
        ports.effect(Effect::Primary(PE::Abort));
        Some(Outcome::Aborted)
    }

    /// Returns false when no lens move is needed.
    fn lens_moves(&mut self, ports: &mut impl Ports) -> bool {
        if ports.query(Query::Primary(PQ::NeedsRelative)) {
            ports.effect(Effect::Primary(PE::PrepareRelative));
            ports.effect(Effect::Lens(LensMove::CaptureRelative));
            true
        } else if ports.query(Query::Primary(PQ::NeedsAbsolute)) {
            ports.effect(Effect::Lens(LensMove::CaptureAbsolute));
            true
        } else {
            false
        }
    }

    fn settle(&mut self, ports: &mut impl Ports) -> Outcome {
        if ports.query(Query::Primary(PQ::DisplayEnabled)) && !ports.query(Query::Primary(PQ::Delivered)) {
            self.state = PrimaryState::Displaying;
            return Outcome::Pending;
        }
        self.transfer_or_complete(ports)
    }

    fn transfer_or_complete(&mut self, ports: &mut impl Ports) -> Outcome {
        if ports.query(Query::Primary(PQ::TransferPending)) {
            self.state = PrimaryState::Transferring;
            Outcome::Pending
        } else {
            self.complete(ports)
        }
    }

    fn statistics_seen(&mut self, ports: &mut impl Ports) {
        ports.effect(Effect::Primary(PE::MarkStatisticsSeen));
        ports.effect(Effect::Primary(PE::RequestDisplay));
    }

    fn complete(&mut self, ports: &mut impl Ports) -> Outcome {
        self.state = PrimaryState::Idle;
        ports.effect(Effect::Primary(PE::ClearCommandC));
        ports.effect(Effect::Primary(PE::ClearCommand28));
        Outcome::Complete
    }
}

/// Calc jobs 0x3c0f (0x99), 0x3c10 (0x97), 0x3c0e (0x98) and 0x3c11 (0x9a), offered in that order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CalculationsState { Idle, Requested, Tracked }

pub struct Calculations { state: CalculationsState }

impl Calculations {
    pub const fn new() -> Self { Self { state: CalculationsState::Idle } }

    pub fn signal(&mut self, signal: Signal, record: RecordHandle, ports: &mut impl Ports) -> Outcome {
        use CalculationsJob::*;
        use CalculationsState::*;
        let Signal::Completion(completion) = signal else { return Outcome::Unhandled };
        match (self.state, completion) {
            (Idle, Completion::Image61) => {
                ports.effect(Effect::Calculations(CE::Begin(record)));
                ports.effect(Effect::Calculations(CE::Configure));
                let requested = ports.query(Query::Calculations(CQ::Requested));
                self.state = if requested { Requested } else { Tracked };
                self.offer(&[First, Second, Third, Fourth], ports)
            }
            (Requested | Tracked, Completion::Image61) => Outcome::Pending,
            (Requested | Tracked, Completion::Calculation99) => {
                ports.effect(Effect::Calculations(CE::Absorb(First)));
                self.offer(&[Second, Third, Fourth], ports)
            }
            (Requested | Tracked, Completion::Calculation97) => {
                ports.effect(Effect::Calculations(CE::Absorb(Second)));
                self.offer(&[Third, Fourth], ports)
            }
            (Requested | Tracked, Completion::Calculation98) => {
                ports.effect(Effect::Calculations(CE::Absorb(Third)));
                self.offer(&[Fourth], ports)
            }
            (Requested | Tracked, Completion::Calculation9a) => {
                ports.effect(Effect::Calculations(CE::Absorb(Fourth)));
                self.state = Idle;
                Outcome::Complete
            }
            (_, Completion::Calculation99 | Completion::Calculation97 | Completion::Calculation98 | Completion::Calculation9a) => fault(ports),
            _ => Outcome::Unhandled,
        }
    }

    /// A tracked set waits for the first enabled job's completion without requesting it.
    fn offer(&mut self, jobs: &[CalculationsJob], ports: &mut impl Ports) -> Outcome {
        for &job in jobs {
            if ports.query(Query::Calculations(CQ::Enabled(job))) {
                if self.state == CalculationsState::Requested { ports.effect(Effect::Calculations(CE::Request(job))); }
                return Outcome::Pending;
            }
        }
        self.state = CalculationsState::Idle;
        Outcome::Complete
    }
}
