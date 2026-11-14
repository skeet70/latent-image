#![forbid(unsafe_code)]

use crate::{delay_message, timer_message, DelayMessage, TaskPorts};
use core::cell::Cell;
use xm1_kernel::{Backend, Caller, Header, Lockable, MessageQueue, PostError, QueueConfig, QueueId, SemaphoreWord, Task, TaskBackend, TaskConfig, TaskContext, TaskId, Timeout};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Service { Delay, Timer }

pub trait QueuePorts<C: Copy>: TaskPorts<C> {
    fn received(&mut self, header: Header, payload: &[u8]);
}

pub struct QueueDriver<C: Copy> {
    service: Service,
    queue: MessageQueue,
    callback: Cell<Option<C>>,
    last_delay_message: Cell<DelayMessage>,
}
impl<C: Copy> QueueDriver<C> {
    pub const fn new(service: Service, queue: QueueId, last_delay_message: DelayMessage) -> Self {
        Self { service, queue: MessageQueue::stock(queue), callback: Cell::new(None), last_delay_message: Cell::new(last_delay_message) }
    }
    pub fn set_callback(&self, callback: Option<C>) { self.callback.set(callback); }

    pub fn receive<B: Backend>(&self, cx: &TaskContext<'_, B>, ports: &mut impl QueuePorts<C>) {
        let received = self.queue.receive(cx, Timeout::Forever);
        let receive_failed = received.is_err();
        if let Ok(message) = received {
            ports.received(message.header(), message.payload());
            self.last_delay_message.set(if message.header().operation == 0x1600 { DelayMessage::Tick } else { DelayMessage::Unsupported });
        }
        match self.service {
            Service::Delay => delay_message(self.last_delay_message.get(), self.callback.get(), receive_failed, ports),
            Service::Timer => timer_message(self.callback.get(), ports),
        }
    }

    pub fn notify<B: Backend>(&self, cx: &impl Caller<B>) -> Result<(), PostError> {
        let (operation, sender) = match self.service {
            Service::Delay => (0x1600, cx.kernel().backend().current_task().map(|task| task.get()).unwrap_or(0)),
            Service::Timer => (0x1900, self.queue.id().get()),
        };
        self.queue.post(cx, Header { operation, sender, destination: self.queue.id().get() }, &[])
    }
}

pub struct ServiceTask { queue: MessageQueue, task: Task }
impl ServiceTask {
    pub const fn stock(queue: QueueId, task: TaskId) -> Self {
        Self { queue: MessageQueue::stock(queue), task: Task::stock(task) }
    }
    pub fn create<B: TaskBackend>(cx: &TaskContext<'_, B>, queue: QueueId, task: TaskConfig) -> Result<Self, xm1_kernel::Error> {
        let queue = MessageQueue::create(cx, queue, QueueConfig::new(48, 12).ok_or(xm1_kernel::Error::Parameter)?)?;
        let task = Task::create(cx, task)?;
        Ok(Self { queue: queue.handle(), task: task.handle() })
    }
    pub fn delete<B: TaskBackend>(&self, cx: &TaskContext<'_, B>) -> Result<(), xm1_kernel::Error> {
        self.queue.delete(cx)?;
        self.task.delete(cx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayWaitError<E> { Creation(SemaphoreWord), Schedule(E), Wait(xm1_kernel::Error) }

pub struct DelayWaiter { semaphore: Cell<Option<SemaphoreWord>> }
impl DelayWaiter {
    pub const fn new() -> Self { Self { semaphore: Cell::new(None) } }
    pub fn semaphore(&self) -> Option<SemaphoreWord> { self.semaphore.get() }
    pub fn wait<B: Backend, E>(&self, cx: &mut TaskContext<'_, B>, schedule: impl FnOnce() -> Result<(), E>) -> Result<(), DelayWaitError<E>> {
        let cached = {
            let _lock = cx.lock_cores();
            self.semaphore.get().filter(|word| word.word() != 0)
        };
        let (semaphore, creation_failed) = match cached {
            Some(word) => (word, false),
            None => {
                let word = SemaphoreWord::create_binary(cx);
                {
                    let _lock = cx.lock_cores();
                    self.semaphore.set(Some(word));
                }
                (word, !word.created())
            }
        };
        let _ = semaphore.try_take(cx);
        if creation_failed { return Err(DelayWaitError::Creation(semaphore)); }
        let _ = cx.kernel().backend().dispatch_disable();
        let scheduled = schedule();
        let _ = cx.kernel().backend().dispatch_enable();
        scheduled.map_err(DelayWaitError::Schedule)?;
        semaphore.wait(cx, Timeout::Forever).map_err(DelayWaitError::Wait)
    }
}
