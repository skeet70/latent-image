//! Passive late-preservation proposal. No result admits a production handoff.
use crate::{modal_guard::InstalledGuard, modal_lifetime::{LifetimePhase, LifetimeReceipt, Word},
    modal_permit::{CoreId, EntryInstruction, Snapshot, STOCK_ENTRY, STOCK_RESUME},
    modal_task::TaskExecution, provenance::Guest, trace::InstructionCount};
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
    spsr: u32,
    sp: u32,
    lr: u32,
}

#[derive(Serialize)]
pub(crate) struct EarlierIrqReport {
    total: u64,
    dropped: u64,
    coverage_complete: bool,
    target_irq_lr: Option<u32>,
    matching_events: Vec<EarlierIrq>,
    retained: Vec<EarlierIrq>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Context {
    task: TaskExecution,
    phase: LifetimePhase,
    forward: u64,
    reverse: u64,
    install_core: Option<usize>,
    rollback_core: Option<usize>,
}
impl Context {
    fn new(snapshot: &Snapshot, lifetime: LifetimeReceipt) -> Result<Self, Failure> {
        if lifetime.failure.is_some() || lifetime.pending_stores != 0 { return Err(Failure::Publication); }
        let task = snapshot.task.map_err(|_| Failure::Task)?;
        task.validate_execution(snapshot.core, snapshot.sp, snapshot.cpsr).map_err(|_| Failure::Task)?;
        Ok(Self { task, phase: lifetime.phase, forward: lifetime.forward_writes,
            reverse: lifetime.reverse_writes, install_core: lifetime.install_core, rollback_core: lifetime.rollback_core })
    }
}

#[derive(Clone, Copy, Debug)]
enum Failure { Task, Publication, Entry, Overlap, Missing, Counter, Native(shadow::Failure) }
struct State {
    active: Option<shadow::Shadow<Context>>,
    started: u64,
    completed: u64,
    native_events: u64,
    failure: Option<Failure>,
    evidence: [Option<PreservationEvidence>; 16],
    evidence_count: usize,
    context: Option<Context>,
    first_irq: Option<FrameEvidence>,
    earlier_irq: [Option<EarlierIrq>; EARLIER_IRQ_CAPACITY],
    earlier_irq_total: u64,
    earlier_irq_word: Option<u32>,
}
impl State {
    fn fail(&mut self, failure: Failure) { if self.failure.is_none() { self.failure = Some(failure); } }
    fn native(&mut self, evidence: PreservationEvidence) {
        if self.failure.is_some() || self.active.is_none() { return; }
        let Some(events) = self.native_events.checked_add(1) else { self.fail(Failure::Counter); return; };
        self.native_events = events;
        if self.evidence_count == self.evidence.len() { self.fail(Failure::Counter); return; }
        self.evidence[self.evidence_count] = Some(evidence);
        self.evidence_count += 1;
        if let Some(active) = &mut self.active {
            if let Err(failure) = active.observe(evidence) { self.fail(Failure::Native(failure)); }
        }
    }
}

#[derive(Clone, Copy)]
struct FrameEvidence {
    context: Context,
    evidence: [Option<PreservationEvidence>; 16],
}

#[derive(Serialize)]
pub(crate) struct IrqReport { context: String, evidence: Vec<String> }

#[derive(Serialize)]
pub(crate) struct Report {
    pub core: usize,
    pub source: Request,
    pub started: u64,
    pub completed: u64,
    pub native_events: u64,
    pub active: bool,
    pub failure: Option<String>,
    pub production_admission: bool,
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
        let bank_prefix = [0xe3a0d0d3, 0xe129f00d, 0xe92dd00f, 0xe3a000d2, 0xe129f000,
            0xe24e0004, 0xe14f1000, 0xe2012040, 0xe3822093, 0xe129f002, 0xe58d0018];
        for (index, word) in bank_prefix.into_iter().enumerate() {
            let address = bank_start.checked_add(index as u32 * 4).ok_or("shadow bank-transfer prefix overflows")?;
            if guard.context_word(Guest(address)) != Some(Word(word)) {
                return Err("shadow bank-transfer prefix leaves the authenticated immutable loader image".into());
            }
        }
        let state = Rc::new(RefCell::new(State { active: None, started: 0, completed: 0, native_events: 0, failure: None,
            evidence: [None; 16], evidence_count: 0, context: None, first_irq: None,
            earlier_irq: [None; EARLIER_IRQ_CAPACITY], earlier_irq_total: 0, earlier_irq_word: None }));
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
        let spsr = register(arm::Register::Spsr)?;
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
            pstate: point.entry.pstate, tcm, word, cpsr, spsr, sp, lr };
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

    pub(crate) fn observe(&mut self, snapshot: &Snapshot, point: Option<InstructionPointEvent>, lifetime: LifetimeReceipt) {
        if snapshot.core.0 != self.request.core { return; }
        let mut state = self.state.borrow_mut();
        if state.failure.is_some() { return; }
        if snapshot.pc == STOCK_ENTRY && snapshot.entry_instruction == EntryInstruction::OriginalPush {
            if state.active.is_some() { state.fail(Failure::Overlap); return; }
            let Some(point) = point else { state.fail(Failure::Entry); return; };
            if point.core.0 != snapshot.core.0 || point.entry.address != u64::from(snapshot.pc.0)
                || point.entry.physical != Some(u64::from(snapshot.pc.0)) {
                state.fail(Failure::Entry); return;
            }
            let context = match Context::new(snapshot, lifetime) {
                Ok(context) => context, Err(failure) => { state.fail(failure); return; }
            };
            let registers = registers(snapshot);
            let source = |address, word| ExecutionSource { address, physical: u64::from(address), word, tcm: point.entry.tcm };
            let stock = shadow::StockReturn { vector: self.request.vector,
                status: source(self.request.status, 0xe169f001), returning: source(self.request.returning, 0xe8fdd00f) };
            match shadow::Shadow::new(shadow::Binding { entry: point.entry, registers, cpsr: snapshot.cpsr.0, context }, stock) {
                Ok(active) => {
                    let Some(started) = state.started.checked_add(1) else { state.fail(Failure::Counter); return; };
                    state.started = started;
                    state.active = Some(active);
                    state.context = Some(context);
                    state.evidence.fill(None);
                    state.evidence_count = 0;
                }
                Err(failure) => state.fail(Failure::Native(failure)),
            }
        } else if snapshot.pc == STOCK_RESUME && state.active.is_some() {
            let context = match Context::new(snapshot, lifetime) {
                Ok(context) => context, Err(failure) => { state.fail(failure); return; }
            };
            let Some(frame) = snapshot.saved_stock_frame else { state.fail(Failure::Missing); return; };
            // Task attribution supplies the physical stack extent independently of the native packet.
            let physical = u64::from(snapshot.sp.0 & !0x40000000);
            let frame = SavedFrame { address: snapshot.sp.0, physical, words: frame.0 };
            let mut active = state.active.take().expect("active shadow checked above");
            if let Err(failure) = active.handoff(registers(snapshot), snapshot.cpsr.0, frame, context).and_then(|_| active.finish()) {
                state.fail(Failure::Native(failure)); return;
            }
            let Some(completed) = state.completed.checked_add(1) else { state.fail(Failure::Counter); return; };
            state.completed = completed;
            if state.first_irq.is_none() && state.evidence.iter().flatten()
                .any(|event| matches!(event, PreservationEvidence::Entered(_))) {
                state.first_irq = Some(FrameEvidence { context, evidence: state.evidence });
            }
        }
    }

    pub(crate) fn finish(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.active.is_some() { state.fail(Failure::Missing); }
        drop(state);
        if self.watch.remove().is_err() { self.state.borrow_mut().fail(Failure::Entry); }
    }

    pub(crate) fn report(&self) -> Report {
        let state = self.state.borrow();
        Report { core: self.request.core, source: self.request, started: state.started, completed: state.completed,
            native_events: state.native_events, active: state.active.is_some(),
            failure: state.failure.map(|failure| format!("{failure:?}")), production_admission: false,
            context: state.context.map(|context| format!("{context:?}")),
            native_evidence: state.evidence.iter().flatten().map(|event| format!("{event:?}")).collect(),
            first_irq: state.first_irq.map(|frame| IrqReport { context: format!("{:?}", frame.context),
                evidence: frame.evidence.iter().flatten().map(|event| format!("{event:?}")).collect() }),
            earlier_irq_provenance: earlier_irq_report(&state) }
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
    let target_irq_lr = state.first_irq.and_then(|frame| frame.evidence.into_iter().flatten()
        .find_map(|event| match event {
            PreservationEvidence::Retired(retired) => Some(retired.before.irq_lr),
            _ => None,
        }));
    let matching_events = target_irq_lr.map(|target| retained.iter().copied()
        .filter(|event| event.lr == target).collect()).unwrap_or_default();
    let dropped = state.earlier_irq_total.saturating_sub(EARLIER_IRQ_CAPACITY as u64);
    EarlierIrqReport { total: state.earlier_irq_total, dropped, coverage_complete: dropped == 0,
        target_irq_lr, matching_events, retained }
}

fn registers(snapshot: &Snapshot) -> [u32; 16] {
    let mut registers = [0; 16];
    registers[..13].copy_from_slice(&snapshot.registers);
    registers[13] = snapshot.sp.0; registers[14] = snapshot.lr.0; registers[15] = snapshot.pc.0;
    registers
}
