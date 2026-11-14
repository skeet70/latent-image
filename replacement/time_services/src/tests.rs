extern crate std;
use super::*;
use std::vec::Vec;
use core::cell::Cell;

#[derive(Default)]
struct Ports { calls: Vec<u8>, released: Vec<u8>, resolved: Vec<HandleWord>, live: [bool; 32], allocation_error: Option<Error> }
impl TimerPorts<u8> for Ports {
    fn allocate(&mut self, slot: SlotIndex) -> Result<TimerHandle, Error> {
        if let Some(error) = self.allocation_error { return Err(error); }
        self.live[usize::from(slot.get())] = true;
        Ok(TimerHandle::new(u32::from(slot.get()) + 1).unwrap())
    }
    fn resolve(&mut self, handle: HandleWord) -> Result<SlotIndex, Error> {
        self.resolved.push(handle);
        let index = handle.0.wrapping_sub(1);
        if index < 32 && self.live[index as usize] { Ok(SlotIndex::new(index as u8).unwrap()) } else { Err(Error::Backend(-30)) }
    }
    fn release(&mut self, slot: SlotIndex) { self.live[usize::from(slot.get())] = false; self.released.push(slot.get()); }
    fn invoke(&mut self, callback: u8) { self.calls.push(callback); }
}

#[test]
fn timers_preserve_expiry_order_and_repeat() {
    let timers = Timers::new();
    let mut ports = Ports::default();
    assert_eq!(timers.start(Milliseconds(0), Repeat::Once, Some(1), &mut ports), Err(Error::Uninitialized));
    timers.initialize().unwrap();
    timers.start(Milliseconds(0), Repeat::Once, Some(1), &mut ports).unwrap();
    timers.start(Milliseconds(5), Repeat::Periodic, Some(2), &mut ports).unwrap();
    timers.start(Milliseconds(1), Repeat::Once, Some(3), &mut ports).unwrap();
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [1]);
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [1, 3]);
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [1, 3, 2]);
    timers.tick(&mut ports);
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [1, 3, 2, 2]);
}

#[test]
fn cancellation_frees_capacity_without_firing() {
    let timers = Timers::new();
    let mut ports = Ports::default();
    timers.initialize().unwrap();
    let handle = timers.start(Milliseconds(0), Repeat::Once, Some(1), &mut ports).unwrap();
    for _ in 1..32 { timers.start(Milliseconds(0), Repeat::Once, None, &mut ports).unwrap(); }
    assert_eq!(timers.start(Milliseconds(0), Repeat::Once, None, &mut ports), Err(Error::Exhausted));
    timers.cancel(handle.into(), &mut ports);
    timers.start(Milliseconds(0), Repeat::Once, Some(2), &mut ports).unwrap();
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [2]);
}

#[test]
fn changing_reload_does_not_move_current_expiry() {
    let timers = Timers::new();
    let mut ports = Ports::default();
    timers.initialize().unwrap();
    let handle = timers.start(Milliseconds(0), Repeat::Periodic, Some(1), &mut ports).unwrap();
    timers.set_reload(handle.into(), Milliseconds(10), &mut ports).unwrap();
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [1]);
    for _ in 0..3 { timers.tick(&mut ports); }
    assert_eq!(ports.calls, [1]);
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [1, 1]);
}

#[test]
fn delay_completion_is_ordered_and_timer_stops_on_following_tick() {
    let delays = Delays::new();
    let first = DelayTask::new(1).unwrap();
    let last = DelayTask::new(104).unwrap();
    delays.begin(last, Milliseconds(0), false);
    delays.begin(first, Milliseconds(0), true);
    let mut completions = Vec::new();
    assert!(delays.tick(|task| completions.push(task.get())));
    assert_eq!(completions, [1, 104]);
    assert!(!delays.tick(|task| completions.push(task.get())));
    assert_eq!(completions, [1, 104]);
}

#[test]
fn running_delay_timer_adds_one_tick_for_nonzero_periods() {
    let delays = Delays::new();
    delays.begin(DelayTask::new(2).unwrap(), Milliseconds(1), true);
    let mut completions = Vec::new();
    assert!(delays.tick(|task| completions.push(task.get())));
    assert!(completions.is_empty());
    assert!(delays.tick(|task| completions.push(task.get())));
    assert_eq!(completions, [2]);
}

#[test]
fn periodic_callback_can_cancel_itself() {
    struct Reentrant<'a> { timers: &'a Timers<u8>, ports: Ports, handle: Option<TimerHandle> }
    impl TimerPorts<u8> for Reentrant<'_> {
        fn allocate(&mut self, slot: SlotIndex) -> Result<TimerHandle, Error> { self.ports.allocate(slot) }
        fn resolve(&mut self, handle: HandleWord) -> Result<SlotIndex, Error> { self.ports.resolve(handle) }
        fn release(&mut self, slot: SlotIndex) { self.ports.release(slot); }
        fn invoke(&mut self, callback: u8) {
            self.ports.invoke(callback);
            self.timers.cancel(self.handle.unwrap().into(), &mut self.ports);
        }
    }
    let timers = Timers::new();
    timers.initialize().unwrap();
    let mut ports = Reentrant { timers: &timers, ports: Ports::default(), handle: None };
    ports.handle = Some(timers.start(Milliseconds(0), Repeat::Periodic, Some(1), &mut ports).unwrap());
    timers.tick(&mut ports);
    timers.tick(&mut ports);
    assert_eq!(ports.ports.calls, [1]);
    assert_eq!(ports.ports.released, [0]);
}

#[derive(Default)]
struct TaskEffects { callbacks: Vec<u8>, faults: usize, received: Vec<xm1_kernel::Header> }
impl TaskPorts<u8> for TaskEffects {
    fn invoke(&mut self, callback: u8) { self.callbacks.push(callback); }
    fn fault(&mut self) { self.faults += 1; }
}
impl driver::QueuePorts<u8> for TaskEffects {
    fn received(&mut self, header: xm1_kernel::Header, _: &[u8]) { self.received.push(header); }
}

#[test]
fn delay_task_repeats_the_last_notification_after_receive_failure() {
    use xm1_kernel::{fake::{Fake, FakeMessage}, Header, Kernel, QueueId, TaskId};
    let releases = std::rc::Rc::new(Cell::new(0));
    let backend = Fake::with_task(TaskId::new(6).unwrap()).receive_result(Ok(FakeMessage {
        header: Header { operation: 0x1600, sender: 9, destination: 6 }, payload: Vec::new(), releases: releases.clone(),
    }));
    let kernel = Kernel::new(backend);
    let cx = kernel.task().unwrap();
    let driver = driver::QueueDriver::new(driver::Service::Delay, QueueId::new(6).unwrap(), DelayMessage::Unsupported);
    driver.set_callback(Some(7));
    let mut effects = TaskEffects::default();
    driver.receive(&cx, &mut effects);
    assert_eq!(effects.callbacks, [7]);
    assert_eq!(releases.get(), 1);
    driver.receive(&cx, &mut effects);
    assert_eq!(effects.callbacks, [7, 7]);
    assert_eq!(effects.received, [Header { operation: 0x1600, sender: 9, destination: 6 }]);
    assert_eq!(effects.faults, 1);
}

#[test]
fn timer_task_delivers_callback_even_when_receive_fails() {
    use xm1_kernel::{fake::Fake, Kernel, QueueId, TaskId};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(9).unwrap()));
    let cx = kernel.task().unwrap();
    let driver = driver::QueueDriver::new(driver::Service::Timer, QueueId::new(9).unwrap(), DelayMessage::Unsupported);
    driver.set_callback(Some(3));
    let mut effects = TaskEffects::default();
    driver.receive(&cx, &mut effects);
    assert_eq!(effects.callbacks, [3]);
    assert_eq!(effects.faults, 0);
}

#[test]
fn unsupported_delay_message_faults_without_delivering_tick() {
    use xm1_kernel::{fake::{Fake, FakeMessage}, Header, Kernel, QueueId, TaskId};
    let backend = Fake::with_task(TaskId::new(6).unwrap()).receive_result(Ok(FakeMessage {
        header: Header { operation: 0x1900, sender: 9, destination: 6 }, payload: Vec::new(), releases: std::rc::Rc::new(Cell::new(0)),
    }));
    let kernel = Kernel::new(backend);
    let driver = driver::QueueDriver::new(driver::Service::Delay, QueueId::new(6).unwrap(), DelayMessage::Unsupported);
    driver.set_callback(Some(3));
    let mut effects = TaskEffects::default();
    driver.receive(&kernel.task().unwrap(), &mut effects);
    assert!(effects.callbacks.is_empty());
    assert_eq!(effects.faults, 1);
}

#[test]
fn failed_handle_allocations_keep_their_slots_through_shutdown() {
    let timers = Timers::<u8>::new();
    timers.initialize().unwrap();
    let mut ports = Ports { allocation_error: Some(Error::Backend(-255)), ..Ports::default() };
    for _ in 0..31 { assert_eq!(timers.start(Milliseconds(1), Repeat::Once, None, &mut ports), Err(Error::Backend(-255))); }
    ports.allocation_error = None;
    timers.start(Milliseconds(1), Repeat::Once, None, &mut ports).unwrap();
    assert_eq!(timers.start(Milliseconds(1), Repeat::Once, None, &mut ports), Err(Error::Exhausted));
    timers.shutdown(&mut ports);
    assert_eq!(ports.released, [31]);
    timers.initialize().unwrap();
    assert!(timers.start(Milliseconds(1), Repeat::Once, None, &mut ports).is_ok());
    assert_eq!(timers.start(Milliseconds(1), Repeat::Once, None, &mut ports), Err(Error::Exhausted));
}

#[test]
fn a_zero_or_stale_word_still_reaches_the_resolver() {
    let timers = Timers::<u8>::new();
    timers.initialize().unwrap();
    let mut ports = Ports::default();
    timers.cancel(HandleWord(0), &mut ports);
    assert_eq!(timers.set_reload(HandleWord(7), Milliseconds(5), &mut ports), Err(Error::Backend(-30)));
    assert_eq!(ports.resolved, [HandleWord(0), HandleWord(7)]);
    assert!(ports.released.is_empty());
}

#[test]
fn recreated_delay_task_uses_the_retained_message_after_receive_failure() {
    use xm1_kernel::{fake::Fake, Kernel, QueueId, TaskId};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(6).unwrap()));
    let driver = driver::QueueDriver::new(driver::Service::Delay, QueueId::new(6).unwrap(), DelayMessage::Tick);
    driver.set_callback(Some(7));
    let mut effects = TaskEffects::default();
    driver.receive(&kernel.task().unwrap(), &mut effects);
    assert_eq!(effects.callbacks, [7]);
    assert_eq!(effects.faults, 1);
}

#[test]
fn releasing_a_zero_interrupt_word_asks_the_resolver_and_detaches_nothing() {
    use interrupts::{InterruptHandle, Line, Priority, Route, Source, Trigger};
    #[derive(Default)]
    struct Lines { resolved: Vec<HandleWord>, detached: Vec<u8>, released: Vec<u8> }
    impl interrupts::Ports<u8> for Lines {
        fn route(&mut self, _: Source) -> Result<Route, interrupts::Error> {
            Ok(Route { line: Line::new(5).unwrap(), priority: Priority::new(1).unwrap(), trigger: Trigger::Mode0 })
        }
        fn allocate(&mut self, line: Line) -> Result<InterruptHandle, interrupts::Error> { Ok(InterruptHandle::new(u32::from(line.get()) + 1).unwrap()) }
        fn configure(&mut self, _: Route, _: u8) -> Result<(), interrupts::Error> { Ok(()) }
        fn publish_source(&mut self, _: Line, _: Option<Source>) {}
        fn resolve(&mut self, handle: HandleWord) -> Result<Line, interrupts::Error> {
            self.resolved.push(handle);
            Line::new(handle.0.wrapping_sub(1)).filter(|_| handle.0 != 0).ok_or(interrupts::Error::InvalidHandle)
        }
        fn detach(&mut self, line: Line) -> Result<(), interrupts::Error> { self.detached.push(line.get()); Ok(()) }
        fn release(&mut self, line: Line) { self.released.push(line.get()); }
    }
    let mut lines = Lines::default();
    assert_eq!(interrupts::release(HandleWord(0), &mut lines).map_err(interrupts::Error::status), Err(-20));
    let handle = interrupts::attach(Source::new(9).unwrap(), 1, &mut lines).unwrap();
    assert_eq!(interrupts::release(HandleWord(handle.word()), &mut lines), Ok(()));
    assert_eq!(lines.resolved, [HandleWord(0), HandleWord(6)]);
    assert_eq!((lines.detached, lines.released), (std::vec![5], std::vec![5]));
}

#[test]
fn failed_delay_creation_is_retained_but_zero_is_retried() {
    use driver::{DelayWaitError, DelayWaiter};
    use xm1_kernel::{fake::Fake, Kernel, Semaphore, Status, TaskId};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(6).unwrap()).creation_responses([Status(-255), Status(7)]));
    let mut cx = kernel.task().unwrap();
    let waiter = DelayWaiter::new();
    let timers = Timers::new();
    timers.initialize().unwrap();
    let mut ports = Ports::default();
    assert!(matches!(waiter.wait(&mut cx, || timers.start(Milliseconds(0), Repeat::Once, Some(42), &mut ports).map(|_| ())), Err(DelayWaitError::Creation(_))));
    timers.tick(&mut ports);
    assert_eq!(ports.calls, []);
    assert_eq!(waiter.wait(&mut cx, || timers.start(Milliseconds(0), Repeat::Once, Some(42), &mut ports).map(|_| ())), Err(DelayWaitError::Wait(xm1_kernel::Error::InvalidId)));
    timers.tick(&mut ports);
    assert_eq!(ports.calls, [42]);
    assert_eq!(Semaphore::create_binary(&cx).unwrap().handle().id().get(), 7);

    let kernel = Kernel::new(Fake::with_task(TaskId::new(6).unwrap()).creation_responses([Status(0), Status(7)]));
    let mut cx = kernel.task().unwrap();
    let waiter = DelayWaiter::new();
    assert!(matches!(waiter.wait(&mut cx, || Ok::<(), ()>(())), Err(DelayWaitError::Creation(_))));
    assert_eq!(waiter.wait(&mut cx, || Ok::<(), ()>(())), Ok(()));
}

#[test]
fn delay_reenables_dispatch_after_a_failed_disable() {
    use driver::DelayWaiter;
    use xm1_kernel::{fake::Fake, Error as KernelError, Kernel, TaskId};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(6).unwrap()).initially_dispatch_disabled().fail_dispatch_disable_once(KernelError::Context));
    let mut cx = kernel.task().unwrap();
    assert_eq!(DelayWaiter::new().wait(&mut cx, || Ok::<(), ()>(())), Ok(()));
}

#[test]
fn failed_delay_schedule_still_reenables_dispatch() {
    use driver::{DelayWaitError, DelayWaiter};
    use xm1_kernel::{fake::Fake, Error as KernelError, Kernel, Semaphore, SemaphoreId, TaskId, Timeout};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(6).unwrap()).initially_dispatch_disabled().fail_dispatch_disable_once(KernelError::Context));
    let mut cx = kernel.task().unwrap();
    assert_eq!(DelayWaiter::new().wait(&mut cx, || Err(KernelError::Busy)), Err(DelayWaitError::Schedule(KernelError::Busy)));
    assert_eq!(Semaphore::stock(SemaphoreId::new(7).unwrap()).wait(&cx, Timeout::Forever), Ok(()));
}

fn service_config(id: xm1_kernel::TaskId) -> xm1_kernel::TaskConfig {
    use xm1_kernel::{StackBytes, TaskArgument, TaskConfig, TaskFlagPolicy, TaskPriority};
    extern "C" fn entry(_: u32) {}
    TaskConfig { id, name: c"service-fixture", priority: TaskPriority::new(3).unwrap(), stack: StackBytes::new(3072).unwrap(), entry, argument: TaskArgument(0), flags: TaskFlagPolicy::ClearAfterWait }
}

#[test]
fn service_teardown_works_without_a_local_creator() {
    use driver::ServiceTask;
    use xm1_kernel::{fake::Fake, Kernel, MessageQueue, QueueConfig, QueueId, Task, TaskId};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(1).unwrap()));
    let cx = kernel.task().unwrap();
    let queue = QueueId::new(9).unwrap();
    let task = TaskId::new(9).unwrap();
    MessageQueue::create(&cx, queue, QueueConfig::new(48, 12).unwrap()).unwrap();
    Task::create(&cx, service_config(task)).unwrap();
    ServiceTask::stock(queue, task).delete(&cx).unwrap();
    assert!(ServiceTask::create(&cx, queue, service_config(task)).is_ok());
}

#[test]
fn service_teardown_can_retry_after_a_queue_failure() {
    use driver::ServiceTask;
    use xm1_kernel::{fake::Fake, Error as KernelError, Kernel, QueueId, Task, TaskId};
    let kernel = Kernel::new(Fake::with_task(TaskId::new(1).unwrap()).fail_queue_delete_once(KernelError::Busy));
    let cx = kernel.task().unwrap();
    let queue = QueueId::new(9).unwrap();
    let task = TaskId::new(9).unwrap();
    let service = ServiceTask::create(&cx, queue, service_config(task)).unwrap();
    assert_eq!(service.delete(&cx), Err(KernelError::Busy));
    assert!(matches!(Task::create(&cx, service_config(task)), Err(KernelError::State)));
    assert_eq!(service.delete(&cx), Ok(()));
    assert!(ServiceTask::create(&cx, queue, service_config(task)).is_ok());
}
