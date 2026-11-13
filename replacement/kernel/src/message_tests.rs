use crate::*;
use crate::fake::{Call, Fake, FakeMessage};
use std::{cell::Cell, rc::Rc};

fn task() -> TaskId { TaskId::new(43).unwrap() }
fn queue() -> MessageQueue { MessageQueue::stock(QueueId::new(43).unwrap()) }
fn header() -> Header { Header { operation: 0x3b06, sender: 3, destination: 43 } }

#[test]
fn received_payload_lives_until_the_message_is_released() {
    let releases = Rc::new(Cell::new(0));
    let kernel = Kernel::new(Fake::with_task(task()).receive_result(Ok(FakeMessage {
        header: header(), payload: vec![1, 2, 3, 4], releases: releases.clone(),
    })));
    let cx = kernel.task().unwrap();
    let message = queue().receive(&cx, Timeout::Forever).unwrap();
    assert_eq!(message.header(), header());
    assert_eq!(message.payload(), &[1, 2, 3, 4]);
    assert_eq!(releases.get(), 0);
    drop(message);
    assert_eq!(releases.get(), 1);
}

#[test]
fn a_failed_receive_exposes_no_previous_payload() {
    let kernel = Kernel::new(Fake::with_task(task()).receive_result(Err(Error::Released)));
    let cx = kernel.task().unwrap();
    assert!(matches!(queue().receive(&cx, Timeout::Forever), Err(Error::Released)));
}

#[test]
fn post_preserves_explicit_sender_destination_and_payload() {
    let kernel = Kernel::new(Fake::with_task(task()));
    let cx = kernel.task().unwrap();
    queue().post(&cx, header(), &[5, 6]).unwrap();
    assert!(kernel.backend().calls().contains(&Call::Post { id: queue().id(), form: Form::Task, header: header(), payload: vec![5, 6] }));
}

#[test]
fn interrupt_post_preserves_refusal() {
    let kernel = Kernel::new(Fake::new().in_handler().post_result(Err(PostError::NoBlock)));
    let Context::Interrupt(cx) = kernel.context() else { panic!("not interrupt context") };
    assert_eq!(queue().post(&cx, header(), &[]), Err(PostError::NoBlock));
    assert!(kernel.backend().calls().contains(&Call::Post { id: queue().id(), form: Form::Handler, header: header(), payload: vec![] }));
}

#[test]
fn queue_construction_returns_an_explicitly_deleted_owner() {
    let kernel = Kernel::new(Fake::with_task(task()));
    let cx = kernel.task().unwrap();
    let config = QueueConfig::new(48, 12).unwrap();
    let owned = MessageQueue::create(&cx, queue().id(), config).unwrap();
    assert_eq!(owned.handle(), queue());
    owned.delete(&cx).unwrap();
    assert!(kernel.backend().calls().contains(&Call::QueueCreate { id: queue().id(), config }));
    assert!(kernel.backend().calls().contains(&Call::QueueDelete(queue().id())));
}

#[test]
fn failed_construction_returns_the_kernel_error() {
    let kernel = Kernel::new(Fake::with_task(task()).wait_result(Err(Error::NotCreated)));
    let cx = kernel.task().unwrap();
    assert!(matches!(MessageQueue::create(&cx, queue().id(), QueueConfig::new(1, 12).unwrap()), Err(Error::NotCreated)));
}

#[test]
fn binary_semaphore_creation_and_poll_keep_the_allocated_id() {
    let id = SemaphoreId::new(200).unwrap();
    let kernel = Kernel::new(Fake::with_task(task()).create_result(Ok(id)));
    let cx = kernel.task().unwrap();
    let owned = Semaphore::create_binary(&cx).unwrap();
    assert_eq!(owned.handle().id(), id);
    owned.handle().try_take(&cx).unwrap();
    owned.delete(&cx).unwrap();
    assert!(kernel.backend().calls().contains(&Call::TryTake(id)));
    assert!(kernel.backend().calls().contains(&Call::SemaphoreDelete(id)));
}

#[test]
fn own_flag_wait_returns_the_whole_pattern() {
    let kernel = Kernel::new(Fake::with_task(task()).flags_result(Ok(FlagBits(0xa))));
    let cx = kernel.task().unwrap();
    assert_eq!(cx.wait_any(FlagBits(2), Timeout::Forever), Ok(FlagBits(0xa)));
    cx.clear_flags().unwrap();
    TaskFlags::stock(task()).set(&cx, FlagBits(4)).unwrap();
}

#[test]
fn dropping_the_dispatch_bracket_reenables_scheduling() {
    let kernel = Kernel::new(Fake::with_task(task()));
    let mut cx = kernel.task().unwrap();
    drop(cx.suspend_dispatch().unwrap());
    assert_eq!(kernel.backend().calls(), vec![Call::CurrentTask, Call::DispatchDisable, Call::DispatchEnable]);
}

#[test]
fn failed_disable_does_not_issue_an_enable() {
    let kernel = Kernel::new(Fake::with_task(task()).wait_result(Err(Error::Context)));
    let mut cx = kernel.task().unwrap();
    assert!(matches!(cx.suspend_dispatch(), Err(Error::Context)));
    assert!(!kernel.backend().calls().contains(&Call::DispatchEnable));
}

#[test]
fn queue_configuration_bounds_both_backing_allocations() {
    assert!(QueueConfig::new(0, 12).is_none());
    assert!(QueueConfig::new(1, 7).is_none());
    assert!(QueueConfig::new(1, u32::MAX).is_none());
    assert!(QueueConfig::new(u32::MAX, 12).is_none());
}
