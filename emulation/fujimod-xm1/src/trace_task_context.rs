//! Optional current-task facts sampled before a labelled instruction executes.
use crate::{modal_memory::{SampleMemory, TwoCoreTopology}, modal_permit::{CoreId, Cpsr},
    modal_task::{self, KernelId, MemoryError, TaskError, TaskId}, provenance::Guest};
use fujimod_emulator::board::observation::{InstructionPointEvent, ObservationContext};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Hook, HookId, HookRange, InstructionEntry, InstructionEntryId, Model, Register, arm};
use serde::{Deserialize, Serialize, Serializer};
use std::{cell::{Cell, RefCell}, ffi::c_void, num::NonZeroU32, rc::Rc};

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskCapture { #[default] Off, Logical }

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
pub struct ContextDepth(u32);

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
pub struct StackBytes(NonZeroU32);

#[derive(Clone, Copy, Debug, Serialize)]
pub struct TaskStack {
    #[serde(serialize_with = "guest")]
    base: Guest,
    bytes: StackBytes,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason { NoCurrentTask, InvalidTaskIdentity, UnavailableTaskMemory }

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TaskContext {
    Available {
        #[serde(serialize_with = "serialize_kernel")]
        kernel: KernelId,
        #[serde(serialize_with = "serialize_task_id")]
        task_id: TaskId,
        #[serde(serialize_with = "guest")]
        node: Guest,
        #[serde(serialize_with = "guest")]
        creation: Guest,
        stack: TaskStack,
        context_depth: ContextDepth,
        #[serde(serialize_with = "serialize_cpsr")]
        cpsr: Cpsr,
        #[serde(serialize_with = "guest")]
        sp: Guest,
    },
    Unavailable { reason: UnavailableReason },
}

fn guest<S: Serializer>(value: &Guest, serializer: S) -> Result<S::Ok, S::Error> { serializer.collect_str(value) }
fn serialize_kernel<S: Serializer>(value: &KernelId, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match value { KernelId::Primary => "a", KernelId::Secondary => "b" })
}
fn serialize_task_id<S: Serializer>(value: &TaskId, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_u16(value.get()) }
fn serialize_cpsr<S: Serializer>(value: &Cpsr, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_u32(value.0) }

struct Shared { access: ObservationContext, topology: TwoCoreTopology, failure: RefCell<Option<String>> }
impl Shared {
    fn fail(&self, error: String) { self.failure.borrow_mut().get_or_insert(error); }
    fn check(&self) -> Result<(), String> { self.failure.borrow().as_ref().map_or(Ok(()), |error| Err(error.clone())) }
    fn capture(&self, core: CoreId, entry: InstructionEntry) -> Result<TaskContext, String> {
        let register = |register| self.access.read_register(core.0, Register::Arm32(register))
            .and_then(|value| u32::try_from(value).map_err(|_| "logical context register exceeds32bits".into()));
        let cpsr = Cpsr(register(arm::Register::Cpsr)?);
        if cpsr.0 != entry.pstate { return Err("logical context execution state changed after entry".into()); }
        let sp = Guest(register(arm::Register::R(13))?);
        let memory = SampleMemory { access: &self.access, topology: &self.topology };
        match modal_task::sample_validated_context(&memory, core) {
            Ok(modal_task::ContextSample { identity, descriptor: words, context_depth: depth }) => {
                let bytes = NonZeroU32::new(words[4]).ok_or("logical context validated stack is empty")?;
                Ok(TaskContext::Available { kernel: identity.kernel, task_id: identity.id,
                    node: identity.node.address(), creation: identity.creation.address(),
                    stack: TaskStack { base: Guest(words[5]), bytes: StackBytes(bytes) }, context_depth: ContextDepth(depth), cpsr, sp })
            }
            Err(TaskError::NullCurrent) => Ok(TaskContext::Unavailable { reason: UnavailableReason::NoCurrentTask }),
            Err(TaskError::Memory(MemoryError::Read)) => Ok(TaskContext::Unavailable { reason: UnavailableReason::UnavailableTaskMemory }),
            Err(TaskError::Memory(error)) => Err(format!("logical context unsafe memory view: {error:?}")),
            Err(_) => Ok(TaskContext::Unavailable { reason: UnavailableReason::InvalidTaskIdentity }),
        }
    }
}

#[derive(Clone, Copy)]
struct Snapshot { entry: Option<InstructionEntryId>, context: Option<TaskContext> }
struct Callback {
    core: CoreId, shared: Rc<Shared>, targets: Vec<Guest>, skip: Cell<Option<Guest>>, pending: Cell<Option<Snapshot>>,
}
impl Callback {
    fn request_stop(&self) {
        self.shared.access.request_pause();
        if let Ok(cpu) = self.shared.access.cpu(self.core.0) { let _ = cpu.stop_before_instruction(); }
    }
    fn observe(&self, pc: u64) {
        let Ok(pc) = u32::try_from(pc) else {
            self.shared.fail("logical context instruction address exceeds32bits".into());
            self.request_stop();
            return;
        };
        let pc = Guest(pc);
        if self.skip.get() == Some(pc) { self.skip.set(None); return; }
        self.skip.set(Some(pc));
        if self.pending.get().is_some() {
            self.shared.fail("logical context has an unconsumed instruction entry".into());
            self.request_stop();
            return;
        }
        let entry = self.shared.access.cpu(self.core.0).ok().and_then(|cpu| cpu.instruction_entry());
        let result = self.shared.check().and_then(|()| {
            let entry = entry.filter(|entry| entry.address == u64::from(pc.0)).ok_or("logical context instruction entry is unavailable")?;
            self.shared.capture(self.core, entry)
        });
        let context = match result {
            Ok(context) => Some(context),
            Err(error) => {
                self.shared.fail(error);
                self.request_stop();
                None
            }
        };
        self.pending.set(Some(Snapshot { entry: entry.map(|entry| entry.id), context }));
    }
}
unsafe fn instruction(pc: u64, _: u32, data: *mut c_void) {
    // Callback storage remains boxed until every installed hook is removed.
    unsafe { &*data.cast::<Callback>() }.observe(pc);
}
struct Core { callback: Box<Callback>, hooks: Vec<HookId> }
enum PendingPolicy { Preserve, Discard }
pub(super) struct Observer { shared: Rc<Shared>, cores: Vec<Core> }
impl Observer {
    pub(super) fn new(session: &Session) -> Result<Self, String> {
        let access = session.board.observation_context();
        let topology = TwoCoreTopology::new(&access).map_err(|error| format!("logical context topology: {error:?}"))?;
        for core in 0..access.core_count() {
            let cpu = access.cpu(core)?;
            if cpu.model() != Model::CortexR5F || !cpu.features().instruction_progress || !cpu.features().architectural_exits {
                return Err("logical context requires counted R-profile instruction entries".into());
            }
        }
        let shared = Rc::new(Shared { access, topology, failure: RefCell::new(None) });
        let cores = (0..shared.access.core_count()).map(|core| Core {
            callback: Box::new(Callback { core: CoreId(core), shared: shared.clone(), targets: Vec::new(), skip: Cell::new(None), pending: Cell::new(None) }),
            hooks: Vec::new(),
        }).collect();
        Ok(Self { shared, cores })
    }
    pub(super) fn before_advance(&self, session: &Session) -> Result<(), String> {
        self.shared.check()?;
        if self.cores.iter().all(|core| core.hooks.is_empty()) { return Ok(()); }
        self.shared.topology.validate(session).map_err(|error| format!("logical context topology: {error:?}"))?;
        if session.board.mappings() != self.shared.access.mappings() { return Err("logical context mapping snapshot changed".into()); }
        Ok(())
    }
    pub(super) fn check(&self) -> Result<(), String> { self.shared.check() }
    pub(super) fn arm_core(&mut self, core: CoreId, addresses: &[u64]) -> Result<(), String> {
        self.remove_core(core, PendingPolicy::Preserve)?;
        self.shared.check()?;
        let observer = self.cores.get_mut(core.0).ok_or("logical context core is unavailable")?;
        let data = (&*observer.callback as *const Callback).cast_mut().cast();
        let cpu = self.shared.access.cpu(core.0)?;
        for &address in addresses {
            let target = u32::try_from(address).map(Guest).map_err(|_| "logical context site exceeds32bits".to_owned());
            let target = match target {
                Ok(target) => target,
                Err(error) => { self.shared.fail(error.clone()); return Err(error); }
            };
            if observer.callback.targets.last().is_some_and(|previous| *previous >= target) {
                let error = "logical context sites are not strictly ordered".to_owned();
                self.shared.fail(error.clone());
                return Err(error);
            }
            observer.callback.targets.push(target);
            let hook = unsafe { cpu.install_hook(Hook::Instruction { callback: instruction, data,
                range: HookRange::Inclusive { begin: address, end: address } }) };
            match hook {
                Ok(hook) => observer.hooks.push(hook),
                Err(error) => { self.shared.fail(error.clone()); return Err(error); }
            }
        }
        Ok(())
    }
    pub(super) fn discard_events(&self) {
        for observer in &self.cores { observer.callback.pending.set(None); }
    }
    pub(super) fn resume(&self, core: CoreId, candidate: Option<Guest>) {
        if let Some(observer) = self.cores.get(core.0) {
            observer.callback.skip.set(candidate.filter(|address| observer.callback.targets.binary_search(address).is_ok()));
        }
    }
    pub(super) fn take(&self, core: CoreId, event: Option<InstructionPointEvent>) -> Result<Option<TaskContext>, String> {
        let observer = self.cores.get(core.0).ok_or("logical context core is unavailable")?;
        let snapshot = observer.callback.pending.take();
        if let (Some(snapshot), Some(event)) = (snapshot, event) {
            if event.core.0 == core.0 && snapshot.entry == Some(event.entry.id) { return Ok(snapshot.context); }
        }
        if self.shared.failure.borrow().is_some() { return Ok(None); }
        let error = "logical context does not match the stopped instruction entry".to_owned();
        self.shared.fail(error.clone());
        Err(error)
    }
    fn remove_core(&mut self, core: CoreId, pending: PendingPolicy) -> Result<(), String> {
        let observer = self.cores.get_mut(core.0).ok_or("logical context core is unavailable")?;
        while let Some(&hook) = observer.hooks.last() {
            if let Err(error) = self.shared.access.cpu(core.0)?.remove_hook(hook) {
                self.shared.fail(error.clone());
                return Err(error);
            }
            observer.hooks.pop();
        }
        observer.callback.targets.clear();
        observer.callback.skip.set(None);
        if matches!(pending, PendingPolicy::Discard) { observer.callback.pending.set(None); }
        Ok(())
    }
    pub(super) fn disarm(&mut self) -> Result<(), String> {
        for core in 0..self.cores.len() { self.remove_core(CoreId(core), PendingPolicy::Discard)?; }
        Ok(())
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        for mut observer in self.cores.drain(..) {
            while let Some(&hook) = observer.hooks.last() {
                let removed = self.shared.access.cpu(observer.callback.core.0).and_then(|cpu| cpu.remove_hook(hook));
                if removed.is_err() {
                    self.shared.fail("logical context dropped with an unremoved hook".into());
                    std::mem::forget(observer.callback);
                    break;
                }
                observer.hooks.pop();
            }
        }
    }
}
