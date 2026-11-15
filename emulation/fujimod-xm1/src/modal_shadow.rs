//! Exact retained-kernel IRQ-return certificate capture for the modal handoff.
use crate::{
    modal_certificate::{AuthenticatedIrqReturn, CausalContext, CertificateFacts, CertificateFailure, ProductionWrapper},
    modal_guard::InstalledGuard,
    modal_lifetime::{LifetimeReceipt, Word},
    modal_permit::{CoreId, EntryInstruction, Snapshot, STOCK_ENTRY, STOCK_RESUME},
    provenance::Guest,
    trace::InstructionCount,
};
use fujimod_emulator::{board::observation::{CoreIndex, InstructionPointEvent, PreservationWatch}, session::Session};
use fujimod_emulator_cpu::{HookRange, Register, arm,
    preservation::{ExecutionSource, PreservationEvidence, SavedFrame, shadow}};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    core: usize, vector: u32, bank_transfer: u32, status: u32, returning: u32,
    #[serde(default)]
    adapter: Adapter,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Adapter { #[default] Board, Context }


const EARLIER_IRQ_CAPACITY: usize = 16;
const IRQ_VECTOR_LR_SUBTRACT: u32 = 4;

#[derive(Clone, Copy, Serialize)]
struct TcmEvidence { address: u64, bytes: usize }

#[derive(Clone, Copy, Serialize)]
struct EarlierIrq {
    instructions: u64,
    address: u64,
    physical: Option<u64>,
    bytes: u32,
    progress: u64,
    pstate: u32,
    tcm: [Option<TcmEvidence>; 2],
    word: u32,
    cpsr: u32,
    irq_spsr: u32,
    sp: u32,
    lr: u32,
}

#[derive(Serialize)]
pub(crate) struct EarlierIrqReport {
    total: u64,
    dropped: u64,
    coverage_complete: bool,
    target_post_prefix_lr: Option<u32>,
    vector_lr_subtract: u32,
    matching_events: Vec<EarlierIrq>,
    raw_equal_wrong_phase_events: Vec<EarlierIrq>,
    normalization_underflow_events: Vec<EarlierIrq>,
    retained: Vec<EarlierIrq>,
}

#[derive(Clone, Copy, Debug)]
enum Failure { Entry, Missing, Counter, Certificate(CertificateFailure) }
struct State {
    wrapper: ProductionWrapper,
    started: u64,
    completed: u64,
    native_events: u64,
    failure: Option<Failure>,
    context: Option<CausalContext>,
    first_irq: Option<FrameEvidence>,
    earlier_irq: [Option<EarlierIrq>; EARLIER_IRQ_CAPACITY],
    earlier_irq_total: u64,
    earlier_irq_word: Option<u32>,
}
impl State {
    fn fail(&mut self, failure: Failure) { if self.failure.is_none() { self.failure = Some(failure); } }
    fn native(&mut self, evidence: PreservationEvidence) {
        if self.failure.is_some() { return; }
        let Some(events) = self.native_events.checked_add(1) else {
            self.fail(Failure::Counter);
            return;
        };
        self.native_events = events;
        if let Err(failure) = self.wrapper.observe(evidence) {
            self.fail(Failure::Certificate(failure));
        }
    }
}

#[derive(Clone, Copy)]
struct FrameEvidence {
    context: CausalContext,
    evidence: [Option<PreservationEvidence>; 4],
}

#[derive(Serialize)]
pub(crate) struct IrqReport { context: String, evidence: Vec<String> }

#[derive(Serialize)]
pub(crate) struct SourceReport {
    address: u64,
    physical: u64,
    word: u32,
    tcm: [Option<TcmEvidence>; 2],
}

#[derive(Serialize)]
pub(crate) struct CertificateReport {
    task: String,
    publication_count: usize,
    forward: u64,
    reverse: u64,
    retirement_source: SourceReport,
    status_source: SourceReport,
    return_source: SourceReport,
    old_svc_spsr: u32,
    entry_cpsr: u32,
    returned_cpsr: u32,
    frame_address: u64,
    frame_physical: u64,
    frame_words: [u32; 9],
}

#[derive(Serialize)]
pub(crate) struct Report {
    pub core: usize,
    pub source: Request,
    pub started: u64,
    pub completed: u64,
    pub native_events: u64,
    pub active: bool,
    pub failure: Option<String>,
    pub certificates_issued: u64,
    pub certificates_consumed: u64,
    pub task_identity_verified: bool,
    pub production_admission: bool,
    pub certificate: Option<CertificateReport>,
    pub context: Option<String>,
    pub native_evidence: Vec<String>,
    pub first_irq: Option<IrqReport>,
    pub earlier_irq_provenance: EarlierIrqReport,
}

pub(crate) struct Capture {
    request: Request,
    state: Rc<RefCell<State>>,
    watch: PreservationWatch,
}
impl Capture {
    pub(crate) fn new(request: Request, guard: &InstalledGuard, session: &Session) -> Result<Self, String> {
        if request.core >= 2 || request.status.checked_add(4) != Some(request.returning)
            || request.vector != 0x18 || request.status & 3 != 0 || request.bank_transfer & 3 != 0 {
            return Err("shadow source is not a bounded A32 exception-return plan".into());
        }
        let start = request.status.checked_sub(20).ok_or("shadow return prefix underflows")?;
        // Complete stock return prefix, not a mask inferred from the observed candidate delta.
        let prefix = [0xe8bd0002, 0xe10f2000, 0xe2022040, 0xe3c11040, 0xe1811002, 0xe169f001, 0xe8fdd00f];
        for (index, word) in prefix.into_iter().enumerate() {
            if guard.context_word(Guest(start + index as u32 * 4)) != Some(Word(word)) {
                return Err("shadow return prefix leaves the authenticated immutable loader image".into());
            }
        }
        let bank_start = request.bank_transfer.checked_sub(4).ok_or("shadow bank-transfer prefix underflows")?;
        // The word at 0x40218 computes the interrupted return PC as IRQ LR minus four.
        let bank_prefix = [0xe3a0d0d3, 0xe129f00d, 0xe92dd00f, 0xe3a000d2, 0xe129f000,
            0xe24e0004, 0xe14f1000, 0xe2012040, 0xe3822093, 0xe129f002, 0xe58d0018];
        for (index, word) in bank_prefix.into_iter().enumerate() {
            let address = bank_start.checked_add(index as u32 * 4).ok_or("shadow bank-transfer prefix overflows")?;
            if guard.context_word(Guest(address)) != Some(Word(word)) {
                return Err("shadow bank-transfer prefix leaves the authenticated immutable loader image".into());
            }
        }
        let state = Rc::new(RefCell::new(State {
            wrapper: ProductionWrapper::new(),
            started: 0,
            completed: 0,
            native_events: 0,
            failure: None,
            context: None,
            first_irq: None,
            earlier_irq: [None; EARLIER_IRQ_CAPACITY],
            earlier_irq_total: 0,
            earlier_irq_word: None,
        }));
        let observed = state.clone();
        let core = request.core;
        let callback = Rc::new(move |actual: CoreIndex, evidence| {
            if actual.0 != core { observed.borrow_mut().fail(Failure::Entry); }
            else { observed.borrow_mut().native(evidence); }
        });
        let range = HookRange::Inclusive { begin: u64::from(STOCK_ENTRY.0), end: u64::from(STOCK_ENTRY.0) };
        let watch = match request.adapter {
            Adapter::Board => session.board.preservation_evidence_begin(core, range, callback)?,
            Adapter::Context => session.board.observation_context().preservation_evidence_begin(core, range, callback)?,
        };
        Ok(Self { request, state, watch })
    }

    pub(crate) fn wants_task(&self, core: CoreId, pc: Guest) -> bool {
        core.0 == self.request.core && (pc == STOCK_ENTRY || pc == STOCK_RESUME)
    }

    pub(crate) fn task_identity_verified(&self) -> bool {
        let state = self.state.borrow();
        state.failure.is_none() && state.wrapper.consumed() != 0
            && state.wrapper.issued() == state.wrapper.consumed()
    }

    pub(crate) fn sites(&self) -> impl Iterator<Item = Guest> {
        std::iter::once(Guest(self.request.vector))
    }

    pub(crate) fn contains(&self, pc: Guest) -> bool { pc.0 == self.request.vector }

    pub(crate) fn observe_earlier_irq(&mut self, session: &Session, core: CoreId, pc: Guest,
        instructions: InstructionCount, point: InstructionPointEvent) -> Result<(), String> {
        if core.0 != self.request.core || !self.contains(pc) || point.core.0 != core.0
            || point.entry.address != u64::from(pc.0) || point.entry.physical != Some(u64::from(pc.0)) {
            return Err("earlier IRQ point leaves its configured core/vector/physical identity".into());
        }
        let register = |register| {
            u32::try_from(session.board.read_register(core.0, Register::Arm32(register))?)
                .map_err(|_| "earlier IRQ register exceeds32bits".to_owned())
        };
        let cpsr = register(arm::Register::Cpsr)?;
        let irq_spsr = register(arm::Register::Spsr)?;
        let sp = register(arm::Register::R(13))?;
        let lr = register(arm::Register::R(14))?;
        if point.entry.pstate != cpsr || cpsr & 0x1f != 0x12 || cpsr & 0x180 != 0x180 {
            return Err("earlier IRQ vector is not an A-masked IRQ entry".into());
        }
        let mut guest = [0; 4];
        let mut physical = [0; 4];
        session.board.read_memory(core.0, u64::from(pc.0), &mut guest)?;
        session.board.read_physical(u64::from(pc.0), &mut physical)?;
        if guest != physical { return Err("earlier IRQ vector guest and physical words differ".into()); }
        let word = u32::from_le_bytes(guest);
        let tcm = point.entry.tcm.map(|window| window.map(|window| TcmEvidence {
            address: window.address, bytes: window.bytes,
        }));
        let event = EarlierIrq { instructions: instructions.value(), address: point.entry.address,
            physical: point.entry.physical, bytes: point.entry.bytes, progress: point.entry.progress,
            pstate: point.entry.pstate, tcm, word, cpsr, irq_spsr, sp, lr };
        let mut state = self.state.borrow_mut();
        if state.failure.is_some() || state.first_irq.is_some() { return Ok(()); }
        if state.earlier_irq_word.is_some_and(|expected| expected != word) {
            return Err("earlier IRQ vector word changed before the authenticated IRQ chain".into());
        }
        state.earlier_irq_word = Some(word);
        let index = usize::try_from(state.earlier_irq_total % EARLIER_IRQ_CAPACITY as u64)
            .map_err(|_| "earlier IRQ index exceeds usize")?;
        state.earlier_irq[index] = Some(event);
        state.earlier_irq_total = state.earlier_irq_total.checked_add(1)
            .ok_or("earlier IRQ count overflow")?;
        Ok(())
    }

    pub(crate) fn observe(&mut self, snapshot: &Snapshot, point: Option<InstructionPointEvent>,
        lifetime: LifetimeReceipt) -> Result<Option<AuthenticatedIrqReturn>, String> {
        if snapshot.core.0 != self.request.core { return Ok(None); }
        let mut state = self.state.borrow_mut();
        if let Some(failure) = state.failure { return Err(format!("{failure:?}")); }
        if snapshot.pc == STOCK_ENTRY && snapshot.entry_instruction == EntryInstruction::OriginalPush {
            let Some(point) = point else {
                state.fail(Failure::Entry);
                return Err(format!("{:?}", Failure::Entry));
            };
            if point.core.0 != snapshot.core.0 || point.entry.address != u64::from(snapshot.pc.0)
                || point.entry.physical != Some(u64::from(snapshot.pc.0)) {
                state.fail(Failure::Entry);
                return Err(format!("{:?}", Failure::Entry));
            }
            let context = match CausalContext::from_observation(snapshot, lifetime) {
                Ok(context) => context,
                Err(failure) => {
                    state.fail(Failure::Certificate(failure));
                    return Err(format!("{failure:?}"));
                }
            };
            let source = |address, word| ExecutionSource {
                address,
                physical: u64::from(address),
                word,
                tcm: point.entry.tcm,
            };
            let stock = shadow::StockReturn {
                vector: self.request.vector,
                status: source(self.request.status, 0xe169f001),
                returning: source(self.request.returning, 0xe8fdd00f),
            };
            if let Err(failure) = state.wrapper.begin(snapshot, point.entry, context, stock) {
                state.fail(Failure::Certificate(failure));
                return Err(format!("{failure:?}"));
            }
            let Some(started) = state.started.checked_add(1) else {
                state.fail(Failure::Counter);
                return Err(format!("{:?}", Failure::Counter));
            };
            state.started = started;
            state.context = Some(context);
            Ok(None)
        } else if snapshot.pc == STOCK_RESUME && state.wrapper.active() {
            let context = match CausalContext::from_observation(snapshot, lifetime) {
                Ok(context) => context,
                Err(failure) => {
                    state.fail(Failure::Certificate(failure));
                    return Err(format!("{failure:?}"));
                }
            };
            let Some(frame) = snapshot.saved_stock_frame else {
                state.fail(Failure::Missing);
                return Err(format!("{:?}", Failure::Missing));
            };
            let frame = SavedFrame {
                address: snapshot.sp.0,
                physical: u64::from(snapshot.sp.0 & !0x40000000),
                words: frame.0,
            };
            let certificate = match state.wrapper.complete(snapshot, frame, context) {
                Ok(certificate) => certificate,
                Err(failure) => {
                    state.fail(Failure::Certificate(failure));
                    return Err(format!("{failure:?}"));
                }
            };
            let Some(completed) = state.completed.checked_add(1) else {
                state.fail(Failure::Counter);
                return Err(format!("{:?}", Failure::Counter));
            };
            state.completed = completed;
            if state.first_irq.is_none() && state.wrapper.evidence()
                .any(|event| matches!(event, PreservationEvidence::Entered(_))) {
                state.first_irq = Some(FrameEvidence {
                    context,
                    evidence: state.wrapper.evidence_array(),
                });
            }
            Ok(Some(certificate))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn consume_certificate(&mut self) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.wrapper.consume().map_err(|failure| {
            state.fail(Failure::Certificate(failure));
            format!("{failure:?}")
        })
    }

    pub(crate) fn finish(&mut self) -> Result<(), String> {
        {
            let mut state = self.state.borrow_mut();
            if let Err(failure) = state.wrapper.finish() {
                state.fail(Failure::Certificate(failure));
            }
            if let Some(failure) = state.failure { return Err(format!("{failure:?}")); }
        }
        if self.watch.remove().is_err() {
            self.state.borrow_mut().fail(Failure::Entry);
            return Err(format!("{:?}", Failure::Entry));
        }
        Ok(())
    }

    pub(crate) fn report(&self) -> Report {
        let state = self.state.borrow();
        let issued = state.wrapper.issued();
        let consumed = state.wrapper.consumed();
        let task_identity_verified = consumed != 0 && issued == consumed;
        let production_admission = state.failure.is_none() && task_identity_verified
            && !state.wrapper.active() && !state.wrapper.pending();
        Report {
            core: self.request.core,
            source: self.request,
            started: state.started,
            completed: state.completed,
            native_events: state.native_events,
            active: state.wrapper.active(),
            failure: state.failure.map(|failure| format!("{failure:?}")),
            certificates_issued: issued,
            certificates_consumed: consumed,
            task_identity_verified,
            production_admission,
            certificate: state.wrapper.last().map(certificate_report),
            context: state.context.map(|context| format!("{context:?}")),
            native_evidence: state.wrapper.evidence().map(|event| format!("{event:?}")).collect(),
            first_irq: state.first_irq.map(|frame| IrqReport {
                context: format!("{:?}", frame.context),
                evidence: frame.evidence.iter().flatten().map(|event| format!("{event:?}")).collect(),
            }),
            earlier_irq_provenance: earlier_irq_report(&state),
        }
    }
}

fn certificate_report(facts: CertificateFacts) -> CertificateReport {
    CertificateReport {
        task: format!("{:?}", facts.task),
        publication_count: facts.publication_count,
        forward: facts.forward,
        reverse: facts.reverse,
        retirement_source: source_report(facts.retirement_source),
        status_source: source_report(facts.status_source),
        return_source: source_report(facts.return_source),
        old_svc_spsr: facts.old_svc_spsr,
        entry_cpsr: facts.entry_cpsr,
        returned_cpsr: facts.returned_cpsr,
        frame_address: u64::from(facts.frame.address),
        frame_physical: facts.frame.physical,
        frame_words: facts.frame.words,
    }
}

fn source_report(source: ExecutionSource) -> SourceReport {
    SourceReport {
        address: u64::from(source.address),
        physical: source.physical,
        word: source.word,
        tcm: source.tcm.map(|entry| entry.map(|entry| TcmEvidence {
            address: entry.address,
            bytes: entry.bytes,
        })),
    }
}

fn earlier_irq_report(state: &State) -> EarlierIrqReport {
    let count = usize::try_from(state.earlier_irq_total.min(EARLIER_IRQ_CAPACITY as u64))
        .expect("bounded earlier IRQ count fits usize");
    let start = if state.earlier_irq_total > EARLIER_IRQ_CAPACITY as u64 {
        usize::try_from(state.earlier_irq_total % EARLIER_IRQ_CAPACITY as u64)
            .expect("bounded earlier IRQ index fits usize")
    } else { 0 };
    let retained = (0..count).filter_map(|offset| {
        state.earlier_irq[(start + offset) % EARLIER_IRQ_CAPACITY]
    }).collect::<Vec<_>>();
    let target_post_prefix_lr = state.first_irq.and_then(|frame| frame.evidence.into_iter().flatten()
        .find_map(|event| match event {
            PreservationEvidence::Retired(retired) => Some(retired.before.irq_lr),
            _ => None,
        }));
    let mut matching_events = Vec::new();
    let mut raw_equal_wrong_phase_events = Vec::new();
    let mut normalization_underflow_events = Vec::new();
    if let Some(target) = target_post_prefix_lr {
        for event in retained.iter().copied() {
            match event.lr.checked_sub(IRQ_VECTOR_LR_SUBTRACT) {
                Some(return_pc) if return_pc == target => matching_events.push(event),
                Some(_) if event.lr == target => raw_equal_wrong_phase_events.push(event),
                None => normalization_underflow_events.push(event),
                _ => {}
            }
        }
    }
    let dropped = state.earlier_irq_total.saturating_sub(EARLIER_IRQ_CAPACITY as u64);
    EarlierIrqReport { total: state.earlier_irq_total, dropped, coverage_complete: dropped == 0,
        target_post_prefix_lr, vector_lr_subtract: IRQ_VECTOR_LR_SUBTRACT, matching_events,
        raw_equal_wrong_phase_events, normalization_underflow_events, retained }
}

