//! Exact one-use authorization for the authenticated retained-kernel IRQ return.
use crate::{
    modal_lifetime::{LifetimePhase, LifetimeReceipt},
    modal_permit::{CoreId, Cpsr, EntryInstruction, Snapshot, StockFrame, STOCK_ENTRY, STOCK_RESUME},
    modal_task::{TaskExecution, TaskIdentity},
    provenance::Guest,
};
use fujimod_emulator_cpu::{InstructionEntry, preservation::{
    ExceptionEntry, ExceptionReturn, ExecutionSource, InstructionRetirement, PreservationEvidence,
    SavedFrame, SavedStatusWrite, shadow,
}};

const EVIDENCE_CAPACITY: usize = 4;
const RETURN_CONTROL_MASK: u32 = 0xff00_00ff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CausalContext {
    task: TaskExecution,
    phase: LifetimePhase,
    publication_count: usize,
    forward: u64,
    reverse: u64,
    install_core: CoreId,
    rollback_core: CoreId,
}

impl CausalContext {
    pub fn from_observation(snapshot: &Snapshot, lifetime: LifetimeReceipt) -> Result<Self, CertificateFailure> {
        if lifetime.failure.is_some() || lifetime.pending_stores != 0
            || lifetime.phase != LifetimePhase::RollbackStored
            || lifetime.forward_writes != lifetime.publication_count as u64
            || lifetime.reverse_writes != lifetime.publication_count as u64
            || lifetime.install_core != Some(snapshot.core.0)
            || lifetime.rollback_core != Some(snapshot.core.0)
        {
            return Err(CertificateFailure::Publication);
        }
        let task = snapshot.task.map_err(|_| CertificateFailure::Task)?;
        task.validate_execution(snapshot.core, snapshot.sp, snapshot.cpsr)
            .map_err(|_| CertificateFailure::Task)?;
        Ok(Self {
            task,
            phase: lifetime.phase,
            publication_count: lifetime.publication_count,
            forward: lifetime.forward_writes,
            reverse: lifetime.reverse_writes,
            install_core: snapshot.core,
            rollback_core: snapshot.core,
        })
    }

    pub fn task(self) -> TaskExecution { self.task }
    pub fn phase(self) -> LifetimePhase { self.phase }
    pub fn publication_count(self) -> usize { self.publication_count }
    pub fn forward(self) -> u64 { self.forward }
    pub fn reverse(self) -> u64 { self.reverse }
    pub fn install_core(self) -> CoreId { self.install_core }
    pub fn rollback_core(self) -> CoreId { self.rollback_core }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateFailure {
    Task,
    Publication,
    Binding,
    Overlap,
    PacketOutsideActive,
    EvidenceCapacity,
    EvidenceShape,
    Missing,
    PendingConsumption,
    DuplicateConsumption,
    Destination,
    Native(shadow::Failure),
    Counter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoundState {
    core: CoreId,
    pc: Guest,
    registers: [u32; 13],
    sp: Guest,
    lr: Guest,
    cpsr: Cpsr,
    task: TaskExecution,
    frame: Option<StockFrame>,
}

impl BoundState {
    fn from_snapshot(snapshot: &Snapshot, task: TaskExecution) -> Self {
        Self {
            core: snapshot.core,
            pc: snapshot.pc,
            registers: snapshot.registers,
            sp: snapshot.sp,
            lr: snapshot.lr,
            cpsr: snapshot.cpsr,
            task,
            frame: snapshot.saved_stock_frame,
        }
    }

    fn matches(self, snapshot: &Snapshot) -> bool {
        snapshot.task == Ok(self.task)
            && self == Self::from_snapshot(snapshot, self.task)
    }
}

pub struct AuthenticatedIrqReturn {
    entry: BoundState,
    destination: BoundState,
    context: CausalContext,
    retirement: InstructionRetirement,
    exception: ExceptionEntry,
    saved_status: SavedStatusWrite,
    returned: ExceptionReturn,
}

impl AuthenticatedIrqReturn {
    pub(crate) fn matches_monitor_entry(
        &self,
        core: CoreId,
        registers: [u32; 13],
        sp: Guest,
        lr: Guest,
        cpsr: Cpsr,
        destination: &Snapshot,
    ) -> bool {
        self.entry.core == core
            && self.entry.registers == registers
            && self.entry.sp == sp
            && self.entry.lr == lr
            && self.entry.cpsr == cpsr
            && self.destination.matches(destination)
            && self.destination.frame == Some(StockFrame(self.retirement.frame.words))
            && self.context.task == self.entry.task
            && self.context.task == self.destination.task
            && self.retirement.frame.address == self.destination.sp.0
            && self.retirement.frame.physical == u64::from(self.destination.sp.0 & !0x4000_0000)
            && self.exception.execution == self.saved_status.exception
            && self.exception.execution == self.returned.exception
            && self.saved_status.after.spsr == self.returned.written_status
            && self.returned.after.cpsr
                == (self.exception.before.spsr & !RETURN_CONTROL_MASK)
                    | (self.exception.before.cpsr & RETURN_CONTROL_MASK)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CertificateFacts {
    pub task: TaskIdentity,
    pub publication_count: usize,
    pub forward: u64,
    pub reverse: u64,
    pub retirement_source: ExecutionSource,
    pub status_source: ExecutionSource,
    pub return_source: ExecutionSource,
    pub old_svc_spsr: u32,
    pub entry_cpsr: u32,
    pub returned_cpsr: u32,
    pub frame: SavedFrame,
}

struct Active {
    verifier: shadow::Shadow<CausalContext>,
    entry: BoundState,
}

pub struct ProductionWrapper {
    active: Option<Active>,
    pending: bool,
    evidence: [Option<PreservationEvidence>; EVIDENCE_CAPACITY],
    evidence_count: usize,
    issued: u64,
    consumed: u64,
    last: Option<CertificateFacts>,
    failure: Option<CertificateFailure>,
}

impl ProductionWrapper {
    pub fn new() -> Self {
        Self {
            active: None,
            pending: false,
            evidence: [None; EVIDENCE_CAPACITY],
            evidence_count: 0,
            issued: 0,
            consumed: 0,
            last: None,
            failure: None,
        }
    }

    pub fn failure(&self) -> Option<CertificateFailure> { self.failure }
    pub fn active(&self) -> bool { self.active.is_some() }
    pub fn pending(&self) -> bool { self.pending }
    pub fn issued(&self) -> u64 { self.issued }
    pub fn consumed(&self) -> u64 { self.consumed }
    pub fn last(&self) -> Option<CertificateFacts> { self.last }
    pub fn evidence(&self) -> impl Iterator<Item = PreservationEvidence> + '_ {
        self.evidence.iter().flatten().copied()
    }
    pub fn evidence_array(&self) -> [Option<PreservationEvidence>; EVIDENCE_CAPACITY] {
        self.evidence
    }

    fn reject<T>(&mut self, failure: CertificateFailure) -> Result<T, CertificateFailure> {
        let first = *self.failure.get_or_insert(failure);
        Err(first)
    }

    fn healthy(&self) -> Result<(), CertificateFailure> {
        self.failure.map_or(Ok(()), Err)
    }

    pub fn begin(
        &mut self,
        snapshot: &Snapshot,
        entry: InstructionEntry,
        context: CausalContext,
        stock: shadow::StockReturn,
    ) -> Result<(), CertificateFailure> {
        self.healthy()?;
        if self.active.is_some() { return self.reject(CertificateFailure::Overlap); }
        if self.pending { return self.reject(CertificateFailure::PendingConsumption); }
        if snapshot.pc != STOCK_ENTRY || snapshot.entry_instruction != EntryInstruction::OriginalPush
            || snapshot.saved_stock_frame.is_some() || snapshot.task != Ok(context.task)
        {
            return self.reject(CertificateFailure::Binding);
        }
        let mut registers = [0; 16];
        registers[..13].copy_from_slice(&snapshot.registers);
        registers[13] = snapshot.sp.0;
        registers[14] = snapshot.lr.0;
        registers[15] = snapshot.pc.0;
        let binding = shadow::Binding { entry, registers, cpsr: snapshot.cpsr.0, context };
        let verifier = match shadow::Shadow::new(binding, stock).map_err(CertificateFailure::Native) {
            Ok(verifier) => verifier,
            Err(failure) => return self.reject(failure),
        };
        self.evidence.fill(None);
        self.evidence_count = 0;
        self.active = Some(Active {
            verifier,
            entry: BoundState::from_snapshot(snapshot, context.task),
        });
        Ok(())
    }

    pub fn observe(&mut self, evidence: PreservationEvidence) -> Result<(), CertificateFailure> {
        self.healthy()?;
        if self.active.is_none() { return self.reject(CertificateFailure::PacketOutsideActive); }
        if self.evidence_count == EVIDENCE_CAPACITY {
            return self.reject(CertificateFailure::EvidenceCapacity);
        }
        let result = self.active.as_mut().expect("active wrapper checked above").verifier.observe(evidence)
            .map_err(CertificateFailure::Native);
        if let Err(failure) = result { return self.reject(failure); }
        self.evidence[self.evidence_count] = Some(evidence);
        self.evidence_count += 1;
        Ok(())
    }

    pub fn complete_handoff(
        &mut self,
        snapshot: &Snapshot,
        frame: SavedFrame,
        context: CausalContext,
    ) -> Result<Option<AuthenticatedIrqReturn>, CertificateFailure> {
        self.complete_inner(snapshot, frame, context, true)
    }

    pub fn complete(
        &mut self,
        snapshot: &Snapshot,
        frame: SavedFrame,
        context: CausalContext,
    ) -> Result<AuthenticatedIrqReturn, CertificateFailure> {
        match self.complete_inner(snapshot, frame, context, false)? {
            Some(certificate) => Ok(certificate),
            None => self.reject(CertificateFailure::EvidenceShape),
        }
    }

    fn complete_inner(
        &mut self,
        snapshot: &Snapshot,
        frame: SavedFrame,
        context: CausalContext,
        allow_unchanged: bool,
    ) -> Result<Option<AuthenticatedIrqReturn>, CertificateFailure> {
        self.healthy()?;
        if self.pending { return self.reject(CertificateFailure::PendingConsumption); }
        let Some(mut active) = self.active.take() else {
            return self.reject(CertificateFailure::Missing);
        };
        if snapshot.pc != STOCK_RESUME || snapshot.entry_instruction != EntryInstruction::NotEntry
            || snapshot.saved_stock_frame != Some(StockFrame(frame.words))
            || snapshot.task != Ok(context.task)
        {
            return self.reject(CertificateFailure::Destination);
        }
        if let Err(failure) = active.verifier
            .handoff(snapshot_registers(snapshot), snapshot.cpsr.0, frame, context)
            .and_then(|_| active.verifier.finish())
        {
            return self.reject(CertificateFailure::Native(failure));
        }
        if allow_unchanged && matches!(
            self.evidence,
            [Some(PreservationEvidence::Retired(_)), None, None, None]
        ) {
            return Ok(None);
        }
        let (retirement, exception, saved_status, returned) = match self.evidence {
            [Some(PreservationEvidence::Retired(retirement)),
             Some(PreservationEvidence::Entered(exception)),
             Some(PreservationEvidence::SavedStatus(saved_status)),
             Some(PreservationEvidence::Returned(returned))] => {
                (retirement, exception, saved_status, returned)
            }
            _ => return self.reject(CertificateFailure::EvidenceShape),
        };
        let returned_cpsr = (exception.before.spsr & !RETURN_CONTROL_MASK)
            | (exception.before.cpsr & RETURN_CONTROL_MASK);
        if returned.after.cpsr != returned_cpsr || returned.written_status != returned_cpsr
            || exception.execution != saved_status.exception
            || exception.execution != returned.exception
        {
            return self.reject(CertificateFailure::EvidenceShape);
        }
        if allow_unchanged && snapshot.cpsr == active.entry.cpsr {
            return Ok(None);
        }
        let destination = BoundState::from_snapshot(snapshot, context.task);
        let facts = CertificateFacts {
            task: context.task.identity(),
            publication_count: context.publication_count,
            forward: context.forward,
            reverse: context.reverse,
            retirement_source: retirement.source,
            status_source: saved_status.source,
            return_source: returned.source,
            old_svc_spsr: exception.before.spsr,
            entry_cpsr: exception.before.cpsr,
            returned_cpsr,
            frame,
        };
        let Some(issued) = self.issued.checked_add(1) else {
            return self.reject(CertificateFailure::Counter);
        };
        self.issued = issued;
        self.last = Some(facts);
        self.pending = true;
        Ok(Some(AuthenticatedIrqReturn {
            entry: active.entry,
            destination,
            context,
            retirement,
            exception,
            saved_status,
            returned,
        }))
    }

    pub fn consume(&mut self) -> Result<(), CertificateFailure> {
        self.healthy()?;
        if !self.pending { return self.reject(CertificateFailure::DuplicateConsumption); }
        let Some(consumed) = self.consumed.checked_add(1) else {
            return self.reject(CertificateFailure::Counter);
        };
        self.consumed = consumed;
        self.pending = false;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), CertificateFailure> {
        self.healthy()?;
        if self.active.is_some() { return self.reject(CertificateFailure::Missing); }
        if self.pending || self.issued != self.consumed {
            return self.reject(CertificateFailure::PendingConsumption);
        }
        Ok(())
    }
}

fn snapshot_registers(snapshot: &Snapshot) -> [u32; 16] {
    let mut registers = [0; 16];
    registers[..13].copy_from_slice(&snapshot.registers);
    registers[13] = snapshot.sp.0;
    registers[14] = snapshot.lr.0;
    registers[15] = snapshot.pc.0;
    registers
}
