//! Codec requests behind X-M1 1.50 stream setup at 0x006ebe78.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Request selection at 0x006e9c14, 0x006e9c84 and 0x006e9d20 is authenticated against image 4.

use xm1_power_lifecycle::audio_release::executor::{self, Action, ExecutorIo, ExecutorMemory, NodeAddress, OperationHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tick(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timeout(u32);
impl Timeout {
    pub const fn new(ticks: u32) -> Option<Self> {
        if ticks != 0 && ticks < 0x8000_0000 { Some(Self(ticks)) } else { None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { Start, Stop }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Rate { Default = 0, Hz16000 = 1, Hz11024 = 2, Hz11025 = 3, Hz48000 = 4 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum InputMode { Default = 0, Format3 = 1, Format4 = 2, Format5 = 3, Format6 = 4, Format8 = 5, Format9 = 6, Format10 = 7, Format11 = 8, Special = 9 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum OutputMode { Default = 0, Format1 = 1, Format8 = 2, Sound = 3, Playback = 4, Special = 5 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Channels { Primary = 0, Secondary = 1, Dual = 2 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gain(u8);
impl Gain {
    pub const fn new(value: u8) -> Option<Self> { if value <= 10 { Some(Self(value)) } else { None } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BankSelection { Primary, Alternate }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BankPhase { Idle, Active, Unavailable, Released }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    Initialize,
    Release,
    SelectBank { bank: BankSelection },
    Input { operation: Operation, mode: InputMode, rate: Rate },
    Output { operation: Operation, mode: OutputMode, gain: Gain, channels: Channels, rate: Rate },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Busy, NotInitialized, WrongRequest, Exhausted, Driver(i32), Action(Action) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion { Complete, Cancelled, TimedOut, Failed(Error) }

pub trait WorkerFields {
    fn initialize(&mut self);
    fn configure_input(&mut self, mode: InputMode, rate: Rate);
    fn configure_output(&mut self, mode: OutputMode, gain: Gain, channels: Channels, rate: Rate);
    fn alternate_bank(&self) -> bool;
    fn bank_phase(&self) -> BankPhase;
    fn select_bank(&mut self, bank: BankSelection, transition: bool);
    fn scheduled(&mut self, request: Request);
    fn completed(&mut self, request: Request);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage { Root, InputStopDefault }
#[derive(Clone, Copy)]
struct Pending { id: RequestId, handle: OperationHandle, started: Tick, timeout: Timeout, request: Request, stage: Stage }

pub struct AudioZoomActor {
    next: u32,
    initialized: bool,
    pending: Option<Pending>,
    completed: Option<(RequestId, Completion)>,
}
impl Default for AudioZoomActor { fn default() -> Self { Self::new() } }
impl AudioZoomActor {
    pub const fn new() -> Self { Self { next: 0, initialized: false, pending: None, completed: None } }
    pub fn adopt(worker: &impl WorkerFields) -> Self {
        Self { initialized: matches!(worker.bank_phase(), BankPhase::Idle | BankPhase::Active), ..Self::new() }
    }
    pub fn busy(&self) -> bool { self.pending.is_some() }

    pub fn submit(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, request: Request, now: Tick, timeout: Timeout) -> Result<RequestId, Error> {
        if self.pending.is_some() { return Err(Error::Busy); }
        if request == Request::Initialize && self.initialized { return Err(Error::Busy); }
        if !matches!(request, Request::Initialize | Request::SelectBank { .. }) && !self.initialized { return Err(Error::NotInitialized); }
        let next = self.next.checked_add(1).ok_or(Error::Exhausted)?;
        let root = match request {
            Request::Initialize => { worker.initialize(); NodeAddress(0x8291cc) },
            Request::Release => NodeAddress(0x7d1b34),
            Request::SelectBank { bank } => {
                let phase = worker.bank_phase();
                if phase == BankPhase::Unavailable { return Err(Error::Busy); }
                if phase == BankPhase::Released { return Err(Error::NotInitialized); }
                let alternate = bank == BankSelection::Alternate;
                let transition = phase == BankPhase::Active && worker.alternate_bank() != alternate;
                worker.select_bank(bank, transition);
                if !transition {
                    use xm1_power_lifecycle::audio_release::callbacks::AudioAction;
                    let notification = if phase == BankPhase::Active { AudioAction::BankNotifyUnchanged } else { AudioAction::BankNotify };
                    io.invoke(Action::Audio(notification)).map_err(Error::Action)?;
                    self.next = next;
                    let id = RequestId(next);
                    self.completed = Some((id, Completion::Complete));
                    return Ok(id);
                }
                NodeAddress(if alternate { 0x7d1d58 } else { 0x7d1538 })
            }
            Request::Input { operation, mode, rate } => {
                worker.configure_input(mode, rate);
                match operation {
                    Operation::Start => NodeAddress(if worker.alternate_bank() { 0x7d1b58 } else { 0x7d1c90 }),
                    Operation::Stop => NodeAddress(0x7d1e80),
                }
            }
            Request::Output { operation, mode, gain, channels, rate } => {
                if operation == Operation::Start { worker.configure_output(mode, gain, channels, rate); }
                NodeAddress(match (operation, channels, mode) {
                    (Operation::Start, Channels::Primary, OutputMode::Sound | OutputMode::Playback) => 0x7d1e14,
                    (Operation::Start, Channels::Primary, _) => 0x7d14d8,
                    (Operation::Start, Channels::Secondary, _) => 0x7d1454,
                    (Operation::Start, Channels::Dual, _) => 0x7d15b0,
                    (Operation::Stop, Channels::Primary, OutputMode::Sound | OutputMode::Playback) => 0x7d1b1c,
                    (Operation::Stop, Channels::Primary, _) => 0x7d1f1c,
                    (Operation::Stop, Channels::Secondary, _) => 0x7d1640,
                    (Operation::Stop, Channels::Dual, _) => 0x7d13fc,
                })
            }
        };
        let mut handle = OperationHandle(0);
        let status = executor::schedule(memory, io, root, &mut handle);
        if status.0 != 0 {
            if handle.0 != 0 { let _ = executor::cancel(memory, io, handle); }
            return Err(Error::Driver(status.0));
        }
        worker.scheduled(request);
        self.next = next;
        let id = RequestId(next);
        self.completed = None;
        self.pending = Some(Pending { id, handle, started: now, timeout, request, stage: Stage::Root });
        Ok(id)
    }

    /// Called once per executor timer tick, under the audio domain lock.
    pub fn drive(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, now: Tick) -> Result<(), Error> {
        if let Some(pending) = self.pending {
            if now.0.wrapping_sub(pending.started.0) >= pending.timeout.0 {
                self.retire(memory, io, pending, Completion::TimedOut)?;
            }
        }
        if let Err(action) = executor::tick(memory, io) {
            let error = Error::Action(action);
            if let Some(pending) = self.pending { self.retire(memory, io, pending, Completion::Failed(error))?; }
            return Err(error);
        }
        self.observe(memory, io, worker, now)
    }
    pub fn observe(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, now: Tick) -> Result<(), Error> {
        if let Some(pending) = self.pending {
            if !(0..10).any(|slot| memory.generation(slot) == pending.handle) {
                if matches!(pending.request, Request::Input { operation: Operation::Stop, .. }) && pending.stage == Stage::Root {
                    worker.configure_output(OutputMode::Default, Gain(0), Channels::Dual, Rate::Hz48000);
                    let mut handle = OperationHandle(0);
                    let status = executor::schedule(memory, io, NodeAddress(0x7d15b0), &mut handle);
                    if status.0 != 0 {
                        if handle.0 != 0 { let _ = executor::cancel(memory, io, handle); }
                        self.pending = None;
                        self.completed = Some((pending.id, Completion::Failed(Error::Driver(status.0))));
                        return Err(Error::Driver(status.0));
                    }
                    self.pending = Some(Pending { handle, stage: Stage::InputStopDefault, ..pending });
                    return Ok(());
                }
                worker.completed(pending.request);
                if !matches!(pending.request, Request::SelectBank { .. }) { self.initialized = pending.request != Request::Release; }
                self.completed = Some((pending.id, Completion::Complete));
                self.pending = None;
            } else if now.0.wrapping_sub(pending.started.0) >= pending.timeout.0 {
                self.retire(memory, io, pending, Completion::TimedOut)?;
            }
        }
        Ok(())
    }

    pub fn result(&self, id: RequestId) -> Result<Option<Completion>, Error> {
        if self.pending.is_some_and(|pending| pending.id == id) { return Ok(None); }
        match self.completed { Some((completed, result)) if completed == id => Ok(Some(result)), _ => Err(Error::WrongRequest) }
    }

    pub fn cancel(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, id: RequestId) -> Result<(), Error> {
        let pending = self.pending.filter(|pending| pending.id == id).ok_or(Error::WrongRequest)?;
        self.retire(memory, io, pending, Completion::Cancelled)
    }

    fn retire(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, pending: Pending, completion: Completion) -> Result<(), Error> {
        let status = executor::cancel(memory, io, pending.handle);
        if status.0 != 0 { return Err(Error::Driver(status.0)); }
        self.initialized = false;
        self.pending = None;
        self.completed = Some((pending.id, completion));
        Ok(())
    }
}

pub struct InstalledWorker<M> { pub memory: M }
#[derive(Clone, Copy)]
#[repr(u32)]
enum WorkerPhase { Initializing = 1, Idle = 2, Transition = 3, Output = 4, Input = 5, DefaultOutput = 8, InputStop = 10, Released = 12, BankTransition = 13 }
impl<M: xm1_power_lifecycle::audio_release::stream_construction::ConstructionMemory> InstalledWorker<M> {
    fn put(&mut self, address: u32, value: u32) { self.memory.put(xm1_power_lifecycle::audio_release::stream_construction::Address(address), value); }
    fn word(&self, address: u32) -> u32 { self.memory.word(xm1_power_lifecycle::audio_release::stream_construction::Address(address)) }
    fn phase(&mut self, phase: WorkerPhase) { self.put(0x8faef0, phase as u32); }
}
impl<M: xm1_power_lifecycle::audio_release::stream_construction::ConstructionMemory> WorkerFields for InstalledWorker<M> {
    fn initialize(&mut self) {
        self.memory.put_byte(xm1_power_lifecycle::audio_release::stream_construction::Address(0x8fc750), 0);
        self.put(0x8fc764, 0);
        self.put(0x8fc768, 0);
    }
    fn configure_input(&mut self, mode: InputMode, rate: Rate) { self.put(0x8fc75c, mode as u32); self.put(0x8fc760, rate as u32); }
    fn configure_output(&mut self, mode: OutputMode, gain: Gain, channels: Channels, rate: Rate) {
        self.memory.put_byte(xm1_power_lifecycle::audio_release::stream_construction::Address(0x8fc750), gain.0);
        self.put(0x8fc758, mode as u32);
        self.put(0x8fc760, rate as u32);
        self.put(0x8fc764, channels as u32);
    }
    fn alternate_bank(&self) -> bool { self.word(0x8fc768) == 1 }
    fn bank_phase(&self) -> BankPhase {
        match self.word(0x8faef0) { 2 | 4 | 8 => BankPhase::Idle, 5 | 11 => BankPhase::Active, 0 | 12 => BankPhase::Released, _ => BankPhase::Unavailable }
    }
    fn select_bank(&mut self, bank: BankSelection, transition: bool) {
        self.put(0x8fc768, u32::from(bank == BankSelection::Alternate));
        if transition {
            self.put(0x8fc774, 0);
            self.put(0x8fc778, self.word(0x8faef0));
            self.phase(WorkerPhase::BankTransition);
        }
    }
    fn scheduled(&mut self, request: Request) {
        if matches!(request, Request::SelectBank { .. }) { return; }
        self.put(0x8faf04, 0);
        self.phase(match request {
            Request::Initialize => WorkerPhase::Initializing,
            Request::Release => WorkerPhase::Released,
            Request::Input { operation: Operation::Stop, .. } => WorkerPhase::InputStop,
            _ => WorkerPhase::Transition,
        });
    }
    fn completed(&mut self, request: Request) {
        let phase = match request {
            Request::Initialize | Request::Output { operation: Operation::Stop, .. } => WorkerPhase::Idle,
            Request::Release => WorkerPhase::Released,
            Request::Input { operation: Operation::Start, .. } => WorkerPhase::Input,
            Request::Input { operation: Operation::Stop, .. } => WorkerPhase::DefaultOutput,
            Request::Output { operation: Operation::Start, .. } => WorkerPhase::Output,
            Request::SelectBank { .. } => return,
        };
        self.phase(phase);
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::InstalledWorker;
    use xm1_power_lifecycle::audio_release::stream_construction::native::NativeMemory;
    /// # Safety
    /// The caller exclusively owns the installed codec worker and executor domain, including timer ticks.
    pub unsafe fn worker() -> InstalledWorker<NativeMemory> { InstalledWorker { memory: NativeMemory } }
}

#[cfg(test)]
#[path = "audio_zoom_actor_tests.rs"]
mod tests;
