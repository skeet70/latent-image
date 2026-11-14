#![forbid(unsafe_code)]

use crate::{Backend, Error, Task, TaskContext, TaskId};
use core::{ffi::CStr, num::NonZeroU32};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskPriority(NonZeroU32);
impl TaskPriority {
    pub const fn new(priority: u32) -> Option<Self> {
        match NonZeroU32::new(priority) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackBytes(NonZeroU32);
impl StackBytes {
    pub const fn new(bytes: u32) -> Option<Self> {
        match NonZeroU32::new(bytes) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskArgument(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskFlagPolicy { ClearAfterWait, PreserveAfterWait }

#[derive(Clone, Copy)]
pub struct TaskConfig {
    pub id: TaskId,
    pub name: &'static CStr,
    pub priority: TaskPriority,
    pub stack: StackBytes,
    pub entry: extern "C" fn(u32),
    pub argument: TaskArgument,
    pub flags: TaskFlagPolicy,
}

pub trait TaskBackend: Backend {
    fn task_create(&self, config: TaskConfig) -> Result<(), Error>;
    fn task_delete(&self, id: TaskId) -> Result<(), Error>;
}

#[must_use]
pub struct CreatedTask(Task);
impl CreatedTask {
    pub fn handle(&self) -> Task { self.0 }
    pub fn delete<B: TaskBackend>(self, cx: &TaskContext<'_, B>) -> Result<(), Error> {
        self.0.delete(cx)
    }
}
impl Task {
    pub fn create<B: TaskBackend>(cx: &TaskContext<'_, B>, config: TaskConfig) -> Result<CreatedTask, Error> {
        cx.kernel.backend.task_create(config)?;
        Ok(CreatedTask(Self::stock(config.id)))
    }
    pub fn delete<B: TaskBackend>(self, cx: &TaskContext<'_, B>) -> Result<(), Error> {
        cx.kernel.backend.task_delete(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fake::Fake, Kernel};

    extern "C" fn entry(_: u32) {}
    fn config(id: TaskId) -> TaskConfig {
        TaskConfig { id, name: c"timer", priority: TaskPriority::new(3).unwrap(), stack: StackBytes::new(3072).unwrap(), entry, argument: TaskArgument(0), flags: TaskFlagPolicy::ClearAfterWait }
    }

    #[test]
    fn created_task_can_be_deleted_and_recreated() {
        let kernel = Kernel::new(Fake::with_task(TaskId::new(1).unwrap()));
        let cx = kernel.task().unwrap();
        let id = TaskId::new(9).unwrap();
        let task = Task::create(&cx, config(id)).unwrap();
        assert_eq!(task.handle().id(), id);
        assert!(matches!(Task::create(&cx, config(id)), Err(Error::State)));
        task.delete(&cx).unwrap();
        assert!(Task::create(&cx, config(id)).is_ok());
    }

    #[test]
    fn creation_returns_backend_error_without_reserving_id() {
        let kernel = Kernel::new(Fake::with_task(TaskId::new(1).unwrap()).wait_result(Err(Error::Overflow)));
        let cx = kernel.task().unwrap();
        assert!(matches!(Task::create(&cx, config(TaskId::new(6).unwrap())), Err(Error::Overflow)));
    }
}
