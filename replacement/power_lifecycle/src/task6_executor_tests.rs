use super::*;
use crate::shutdown::TaskId;
#[derive(Default)]
struct Callbacks { executions: u32, failures: u32 }
impl Task6Callbacks for Callbacks {
    fn execute(&mut self) { self.executions += 1; }
    fn failure(&mut self) { self.failures += 1; }
}
struct Kernel { status: Result<(), KernelStatus>, posted: Option<(RequestId, TaskId, QueueId, usize)> }
impl ShutdownRtos for Kernel {
    fn current_task(&mut self) -> TaskId { TaskId(23) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        self.posted = Some((message.operation, message.sender, message.destination, message.payload.len()));
        self.status
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}
#[test]
fn irq_producer_posts_task6_without_executing_work() {
    let mut kernel = Kernel { status: Ok(()), posted: None };
    let mut callbacks = Callbacks::default();
    assert_eq!(notify(&mut kernel, &mut callbacks), Ok(()));
    assert_eq!(kernel.posted, Some((RequestId(0x1600), TaskId(23), QueueId(6), 0)));
    assert_eq!((callbacks.executions, callbacks.failures), (0, 0));
}
#[test]
fn queue_failure_reports_without_completing() {
    let mut kernel = Kernel { status: Err(KernelStatus(-62)), posted: None };
    let mut callbacks = Callbacks::default();
    assert_eq!(notify(&mut kernel, &mut callbacks), Err(KernelStatus(-62)));
    assert_eq!((callbacks.executions, callbacks.failures), (0, 1));
}
#[test]
fn receive_failure_still_dispatches_persistent_message() {
    let mut callbacks = Callbacks::default();
    consume(&mut callbacks, Err(KernelStatus(-50)), RequestId(0x1600));
    assert_eq!((callbacks.executions, callbacks.failures), (1, 1));
}
#[test]
fn unknown_command_reports_each_failed_stage() {
    let mut callbacks = Callbacks::default();
    consume(&mut callbacks, Err(KernelStatus(-18)), RequestId(0x1601));
    assert_eq!((callbacks.executions, callbacks.failures), (0, 2));
    consume(&mut callbacks, Ok(()), RequestId(0x1600));
    assert_eq!((callbacks.executions, callbacks.failures), (1, 2));
}
