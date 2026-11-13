#![forbid(unsafe_code)]

use crate::acquisition::{self, Acquisition, Branch};
use crate::image::{self, ImageEffect, ImageJob, ImageOutcome, ImagePeer, ImageRequest, ImageSignal, Interruption, ReplyPorts};
use crate::jobs::{self, Calculation, CalculationJob};
use crate::lanes::{Lane, Selection};
use crate::planner::{Cadence, Mode, Work};
use crate::record::{self, Ports as RecordPorts};
use crate::window::{self, Window};
use crate::round::{Environment, LaneSignal, LaneStatus, RecordEffect, Round};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, MeasurementControl, MeasurementUpdate};
use xm1_saclac_interface::{CommandKind, Completed, Completion, RecordHandle};

/// Everything the live measurement asks of the records, its peers and the search slot.
pub trait Effects: RecordPorts + ReplyPorts + window::Ports {
    fn planning_mode(&mut self, record: RecordHandle) -> Mode;
    fn fault(&mut self);

    /// The contrast search lane, whose implementation is the AF-candidate slot.
    fn search(&mut self, signal: LaneSignal<'_>, record: RecordHandle) -> LaneStatus;
    fn calculation(&mut self, record: RecordHandle, calculation: Calculation, effect: jobs::Effect);
    fn source_available(&mut self, record: RecordHandle) -> bool;
    fn result_changed(&mut self, record: RecordHandle, calculation: Calculation) -> bool;
    fn acquisition(&mut self, record: RecordHandle, effect: acquisition::Effect);
    fn selection_changed(&mut self, record: RecordHandle) -> bool;
    fn image(&mut self, record: RecordHandle, request: ImagePeer<'_>);
    fn image_failed(&mut self, completed: Completed<'_>) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bookkeeping { Disabled, Enabled }

/// The live measurement child: the round and its lanes.
pub struct Measurement {
    round: Round,
    acquisition: Acquisition,
    auxiliary: CalculationJob,
    window: CalculationJob,
    window_work: Window,
    source: CalculationJob,
    image: ImageJob,
    bookkeeping: Bookkeeping,
}

impl Default for Measurement { fn default() -> Self { Self::new() } }

impl Measurement {
    pub const fn new() -> Self {
        Self {
            round: Round::new(),
            acquisition: Acquisition::new(Branch::Live),
            auxiliary: CalculationJob::new(Calculation::Auxiliary),
            window: CalculationJob::new(Calculation::Window),
            window_work: Window::new(),
            source: CalculationJob::new(Calculation::Source),
            image: ImageJob::new(),
            bookkeeping: Bookkeeping::Disabled,
        }
    }

    pub fn handle(&mut self, event: ChildEvent<'_, MeasurementControl>, effects: &mut impl Effects) -> ChildOutcome {
        let Self { round, acquisition, auxiliary, window, window_work, source, image, bookkeeping } = self;
        let mut lanes = Lanes { acquisition, auxiliary, window, window_work, source, image, bookkeeping, effects };
        round.handle(event, &mut lanes)
    }
}

struct Lanes<'a, E> {
    acquisition: &'a mut Acquisition,
    auxiliary: &'a mut CalculationJob,
    window: &'a mut CalculationJob,
    window_work: &'a mut Window,
    source: &'a mut CalculationJob,
    image: &'a mut ImageJob,
    bookkeeping: &'a mut Bookkeeping,
    effects: &'a mut E,
}

struct CalculationEffects<'a, 'c, E> {
    record: RecordHandle,
    completed: Option<Completed<'c>>,
    window: &'a mut Window,
    effects: &'a mut E,
}
impl<E: Effects> jobs::CalculationPorts for CalculationEffects<'_, '_, E> {
    fn effect(&mut self, calculation: Calculation, effect: jobs::Effect) {
        match calculation {
            Calculation::Window => self.window.effect(effect, self.record, self.completed, &mut *self.effects),
            Calculation::Auxiliary | Calculation::Source => self.effects.calculation(self.record, calculation, effect),
        }
    }
    fn source_available(&mut self) -> bool { self.effects.source_available(self.record) }
    fn result_changed(&mut self, calculation: Calculation) -> bool { self.effects.result_changed(self.record, calculation) }
}

struct AcquisitionEffects<'a, E> { record: RecordHandle, effects: &'a mut E }
impl<E: Effects> acquisition::Ports for AcquisitionEffects<'_, E> {
    fn effect(&mut self, _: Branch, effect: acquisition::Effect) { self.effects.acquisition(self.record, effect); }
    fn selection_changed(&mut self) -> bool { self.effects.selection_changed(self.record) }
}

impl<E: Effects> Lanes<'_, E> {
    fn calculation(&mut self, lane: Lane, signal: LaneSignal<'_>, record: RecordHandle) -> LaneStatus {
        let job = match lane { Lane::Auxiliary => &mut *self.auxiliary, Lane::Window => &mut *self.window, _ => &mut *self.source };
        let completed = if let LaneSignal::Completed(completed) = signal { Some(completed) } else { None };
        let mut ports = CalculationEffects { record, completed, window: &mut *self.window_work, effects: &mut *self.effects };
        let control = match signal {
            LaneSignal::Initialize => jobs::Control::Initialize,
            LaneSignal::Enable => jobs::Control::Enable,
            LaneSignal::Stop => jobs::Control::Stop,
            LaneSignal::Work(_) => jobs::Control::Run,
            LaneSignal::Completed(_) => return job_status(job.completed(&mut ports)),
            LaneSignal::Suspend | LaneSignal::Resume | LaneSignal::Command(_) => return LaneStatus::Unhandled,
        };
        match job.control(control, &mut ports) {
            Ok(outcome) => job_status(outcome),
            Err(_) => self.refuse(),
        }
    }

    fn acquisition(&mut self, signal: LaneSignal<'_>, record: RecordHandle) -> LaneStatus {
        let mut ports = AcquisitionEffects { record, effects: &mut *self.effects };
        let control = match signal {
            LaneSignal::Initialize => acquisition::Control::Initialize,
            LaneSignal::Enable => acquisition::Control::Enable,
            LaneSignal::Stop => acquisition::Control::Stop,
            LaneSignal::Work(Work::SecondaryAcquisition) => acquisition::Control::Secondary,
            LaneSignal::Work(_) => acquisition::Control::Primary,
            LaneSignal::Completed(completed) => {
                let completion = if completed.completion == Completion::Calculation88 { acquisition::Completion::Secondary } else { acquisition::Completion::Primary };
                return job_status(self.acquisition.completed(completion, &mut ports));
            }
            LaneSignal::Suspend | LaneSignal::Resume | LaneSignal::Command(_) => return LaneStatus::Unhandled,
        };
        match self.acquisition.control(control, &mut ports) {
            Ok(outcome) => job_status(outcome),
            Err(_) => self.refuse(),
        }
    }

    fn image(&mut self, signal: LaneSignal<'_>, record: RecordHandle) -> LaneStatus {
        let signal = match signal {
            LaneSignal::Initialize => ImageSignal::Initialize,
            LaneSignal::Enable => ImageSignal::Enable,
            LaneSignal::Stop => ImageSignal::Stop,
            LaneSignal::Work(_) => ImageSignal::Run,
            LaneSignal::Command(command) => match command.kind {
                CommandKind::Command14 => image_request(ImageRequest::Image, command.parameters),
                CommandKind::Command2b => image_request(ImageRequest::Snapshot, command.parameters),
                CommandKind::Command15 => interruption(Interruption::Cancel, command.parameters),
                CommandKind::Command16 => interruption(Interruption::Stop, command.parameters),
                _ => return LaneStatus::Unhandled,
            },
            LaneSignal::Completed(completed) => match completed.completion {
                Completion::Calculation8b => ImageSignal::Calculated,
                Completion::Image47 => ImageSignal::Imaged { kind: ImageRequest::Image, failed: self.effects.image_failed(completed) },
                _ => ImageSignal::Imaged { kind: ImageRequest::Snapshot, failed: self.effects.image_failed(completed) },
            },
            LaneSignal::Suspend | LaneSignal::Resume => return LaneStatus::Unhandled,
        };
        let effects = &mut *self.effects;
        match self.image.signal(signal, &mut |effect| match effect {
            ImageEffect::Peer(request) => effects.image(record, request),
            ImageEffect::Reply(reply) => image::deliver(reply, effects),
        }) {
            ImageOutcome::Ignored => LaneStatus::Refused,
            ImageOutcome::Complete => LaneStatus::Complete,
            ImageOutcome::Pending => LaneStatus::Pending,
            ImageOutcome::Idle => LaneStatus::Idle,
            ImageOutcome::Fault => self.refuse(),
        }
    }

    fn bookkeeping(&mut self, signal: LaneSignal<'_>) -> LaneStatus {
        match (*self.bookkeeping, signal) {
            (Bookkeeping::Disabled, LaneSignal::Initialize | LaneSignal::Stop) => LaneStatus::Complete,
            (Bookkeeping::Disabled, LaneSignal::Enable) => { *self.bookkeeping = Bookkeeping::Enabled; LaneStatus::Complete }
            (Bookkeeping::Enabled, LaneSignal::Stop) => { *self.bookkeeping = Bookkeeping::Disabled; LaneStatus::Complete }
            (_, LaneSignal::Initialize | LaneSignal::Enable) => self.refuse(),
            _ => LaneStatus::Unhandled,
        }
    }

    fn refuse(&mut self) -> LaneStatus {
        self.effects.fault();
        LaneStatus::Refused
    }
}

fn image_request(kind: ImageRequest, parameters: xm1_saclac_interface::Parameters<'_>) -> ImageSignal<'_> {
    ImageSignal::Request { kind, parameters, client: crate::stock::image_client(kind, parameters) }
}

fn interruption(interruption: Interruption, parameters: xm1_saclac_interface::Parameters<'_>) -> ImageSignal<'_> {
    ImageSignal::Interrupt { interruption, parameters, callback: crate::stock::interruption_callback(parameters) }
}

fn job_status(outcome: jobs::Outcome) -> LaneStatus {
    match outcome {
        jobs::Outcome::Complete => LaneStatus::Complete,
        jobs::Outcome::Pending => LaneStatus::Pending,
        jobs::Outcome::NoWork => LaneStatus::Idle,
        jobs::Outcome::Updated(Calculation::Source) => LaneStatus::Updated(MeasurementUpdate::Source),
        jobs::Outcome::Updated(_) => LaneStatus::Updated(MeasurementUpdate::Window),
        jobs::Outcome::SelectionUpdated => LaneStatus::Updated(MeasurementUpdate::NoPeerRequest),
        jobs::Outcome::Ignored => LaneStatus::Refused,
    }
}

impl<E: Effects> Environment for Lanes<'_, E> {
    fn signal(&mut self, lane: Lane, signal: LaneSignal<'_>, record: RecordHandle) -> LaneStatus {
        match lane {
            Lane::Acquisition => self.acquisition(signal, record),
            Lane::Search => self.effects.search(signal, record),
            Lane::Auxiliary | Lane::Window | Lane::Source => self.calculation(lane, signal, record),
            Lane::Image => self.image(signal, record),
            Lane::Bookkeeping => self.bookkeeping(signal),
        }
    }
    fn available(&mut self, _: RecordHandle) -> Option<Selection> { self.effects.offered() }
    fn search_permitted(&mut self) -> bool { self.effects.search_permitted() }
    fn alternate_search(&mut self, record: RecordHandle) -> bool {
        let rule = self.effects.alternate_search();
        rule.applies(self.effects.profile(record).selection)
    }
    fn autofocus_geometry(&mut self, record: RecordHandle) -> bool { self.effects.profile(record).geometry_on_autofocus }
    fn planning_mode(&mut self, record: RecordHandle) -> Mode { self.effects.planning_mode(record) }
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence { self.effects.cadence(work, mode) }
    fn record_effect(&mut self, record: RecordHandle, effect: RecordEffect) { record::apply(effect, record, &mut *self.effects) }
    fn fault(&mut self) { self.effects.fault() }
}
