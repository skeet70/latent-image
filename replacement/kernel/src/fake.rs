//! A scripted backend for host tests.

use crate::{Backend, Error, FlagBits, Form, Header, IrqState, MessageBlock, PostError, QueueConfig, QueueId, SemaphoreId, TaskId, Timeout};
use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    CurrentTask,
    InHandler,
    IrqMask,
    IrqRestore(u32),
    CoresLock,
    CoresUnlock(u32),
    Wait { id: SemaphoreId, timeout: u32 },
    Signal { id: SemaphoreId, form: Form },
    Post { id: QueueId, form: Form, header: Header, payload: Vec<u8> },
    Receive { id: QueueId, timeout: u32 },
    QueueCreate { id: QueueId, config: QueueConfig },
    QueueDelete(QueueId),
    SemaphoreCreateBinary,
    SemaphoreDelete(SemaphoreId),
    TryTake(SemaphoreId),
    FlagsWait { id: TaskId, bits: FlagBits, timeout: u32 },
    FlagsClear(TaskId),
    FlagsSet { id: TaskId, bits: FlagBits, form: Form },
    DispatchDisable,
    DispatchEnable,
}

pub struct Fake {
    task: Option<TaskId>,
    handler: bool,
    irq: u32,
    cores: u32,
    wait: Result<(), Error>,
    signal: Result<(), Error>,
    calls: RefCell<Vec<Call>>,
    messages: RefCell<std::collections::VecDeque<Result<FakeMessage, Error>>>,
    posted: Result<(), PostError>,
    created: Result<SemaphoreId, Error>,
    flags: Result<FlagBits, Error>,
}

impl Fake {
    pub fn new() -> Self {
        Self {
            task: None,
            handler: false,
            irq: 0,
            cores: 0,
            wait: Ok(()),
            signal: Ok(()),
            calls: RefCell::new(Vec::new()),
            messages: RefCell::new(std::collections::VecDeque::new()),
            posted: Ok(()),
            created: Ok(SemaphoreId::new(169).unwrap()),
            flags: Ok(FlagBits(0)),
        }
    }

    pub fn with_task(task: TaskId) -> Self { Self { task: Some(task), ..Self::new() } }
    pub fn in_handler(mut self) -> Self { self.handler = true; self }
    pub fn irq_state(mut self, state: u32) -> Self { self.irq = state; self }
    pub fn cores_state(mut self, state: u32) -> Self { self.cores = state; self }
    pub fn wait_result(mut self, result: Result<(), Error>) -> Self { self.wait = result; self }
    pub fn signal_result(mut self, result: Result<(), Error>) -> Self { self.signal = result; self }
    pub fn calls(&self) -> Vec<Call> { self.calls.borrow().clone() }
    pub fn receive_result(self, message: Result<FakeMessage, Error>) -> Self { self.messages.borrow_mut().push_back(message); self }
    pub fn post_result(mut self, result: Result<(), PostError>) -> Self { self.posted = result; self }
    pub fn create_result(mut self, result: Result<SemaphoreId, Error>) -> Self { self.created = result; self }
    pub fn flags_result(mut self, result: Result<FlagBits, Error>) -> Self { self.flags = result; self }

    fn record(&self, call: Call) { self.calls.borrow_mut().push(call); }
}

impl Default for Fake {
    fn default() -> Self { Self::new() }
}

impl crate::sealed::Sealed for Fake {}

impl Backend for Fake {
    type Message = FakeMessage;
    fn current_task(&self) -> Option<TaskId> {
        self.record(Call::CurrentTask);
        self.task
    }
    fn in_handler(&self) -> bool {
        self.record(Call::InHandler);
        self.handler
    }
    fn irq_mask(&self) -> IrqState {
        self.record(Call::IrqMask);
        IrqState::new(self.irq)
    }
    fn irq_restore(&self, saved: IrqState) { self.record(Call::IrqRestore(saved.word())); }
    fn cores_lock(&self) -> IrqState {
        self.record(Call::CoresLock);
        IrqState::new(self.cores)
    }
    fn cores_unlock(&self, saved: IrqState) { self.record(Call::CoresUnlock(saved.word())); }
    fn semaphore_wait(&self, id: SemaphoreId, timeout: Timeout) -> Result<(), Error> {
        self.record(Call::Wait { id, timeout: timeout.word() });
        self.wait
    }
    fn semaphore_signal(&self, id: SemaphoreId, form: Form) -> Result<(), Error> {
        self.record(Call::Signal { id, form });
        self.signal
    }
    fn message_post(&self, id: QueueId, form: Form, header: Header, payload: &[u8]) -> Result<(), PostError> {
        self.record(Call::Post { id, form, header, payload: payload.to_vec() }); self.posted
    }
    fn message_receive(&self, id: QueueId, timeout: Timeout) -> Result<Self::Message, Error> {
        self.record(Call::Receive { id, timeout: timeout.word() });
        self.messages.borrow_mut().pop_front().unwrap_or(Err(Error::TimedOut))
    }
    fn queue_create(&self, id: QueueId, config: QueueConfig) -> Result<(), Error> { self.record(Call::QueueCreate { id, config }); self.wait }
    fn queue_delete(&self, id: QueueId) -> Result<(), Error> { self.record(Call::QueueDelete(id)); self.signal }
    fn semaphore_create_binary(&self) -> Result<SemaphoreId, Error> { self.record(Call::SemaphoreCreateBinary); self.created }
    fn semaphore_delete(&self, id: SemaphoreId) -> Result<(), Error> { self.record(Call::SemaphoreDelete(id)); self.signal }
    fn semaphore_try_take(&self, id: SemaphoreId) -> Result<(), Error> { self.record(Call::TryTake(id)); self.wait }
    fn flags_wait_any(&self, id: TaskId, bits: FlagBits, timeout: Timeout) -> Result<FlagBits, Error> {
        self.record(Call::FlagsWait { id, bits, timeout: timeout.word() }); self.flags
    }
    fn flags_clear(&self, id: TaskId) -> Result<(), Error> { self.record(Call::FlagsClear(id)); self.signal }
    fn flags_set(&self, id: TaskId, bits: FlagBits, form: Form) -> Result<(), Error> { self.record(Call::FlagsSet { id, bits, form }); self.signal }
    fn dispatch_disable(&self) -> Result<(), Error> { self.record(Call::DispatchDisable); self.wait }
    fn dispatch_enable(&self) -> Result<(), Error> { self.record(Call::DispatchEnable); self.signal }
}

pub struct FakeMessage {
    pub header: Header,
    pub payload: Vec<u8>,
    pub releases: std::rc::Rc<std::cell::Cell<usize>>,
}
impl MessageBlock for FakeMessage {
    fn header(&self) -> Header { self.header }
    fn payload(&self) -> &[u8] { &self.payload }
}
impl Drop for FakeMessage {
    fn drop(&mut self) { self.releases.set(self.releases.get() + 1); }
}
