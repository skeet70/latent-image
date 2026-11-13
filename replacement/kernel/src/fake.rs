//! A scripted backend for host tests.

use crate::{Backend, Error, Form, IrqState, SemaphoreId, TaskId, Timeout};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    CurrentTask,
    InHandler,
    IrqMask,
    IrqRestore(u32),
    CoresLock,
    CoresUnlock(u32),
    Wait { id: SemaphoreId, timeout: u32 },
    Signal { id: SemaphoreId, form: Form },
}

pub struct Fake {
    task: Option<TaskId>,
    handler: bool,
    irq: u32,
    cores: u32,
    wait: Result<(), Error>,
    signal: Result<(), Error>,
    calls: RefCell<Vec<Call>>,
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
        }
    }

    pub fn with_task(task: TaskId) -> Self { Self { task: Some(task), ..Self::new() } }
    pub fn in_handler(mut self) -> Self { self.handler = true; self }
    pub fn irq_state(mut self, state: u32) -> Self { self.irq = state; self }
    pub fn cores_state(mut self, state: u32) -> Self { self.cores = state; self }
    pub fn wait_result(mut self, result: Result<(), Error>) -> Self { self.wait = result; self }
    pub fn signal_result(mut self, result: Result<(), Error>) -> Self { self.signal = result; self }
    pub fn calls(&self) -> Vec<Call> { self.calls.borrow().clone() }

    fn record(&self, call: Call) { self.calls.borrow_mut().push(call); }
}

impl Default for Fake {
    fn default() -> Self { Self::new() }
}

impl crate::sealed::Sealed for Fake {}

impl Backend for Fake {
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
}
