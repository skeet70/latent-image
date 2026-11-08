use crate::admission::{admit, Entry, Facts, Params, Records, Request, Service};
use crate::create::{create_data_queue, delete_data_queue, DataQueueCreate, QueueAttributes};
use crate::env::{retry_masked, Attempt, HandlerContext};
use crate::id::{own_data_queue_slot, resolve_data_queue, DataQueueId, EncodedId, EntryDomain};
use crate::layout::{
    DataQueueSlot, DataQueueWait, Domain, DomainId, GuestAddress, Task, TaskDefinition, TaskSlot,
    TaskState,
};
use crate::ops::{
    force_send, receive, resume_receive, resume_send, send, send_poll, send_timeout, ReceiveBlocking,
    ReceiveResume, SendBlocking, SendResume, RECEIVE_STATE, SEND_STATE,
};
use crate::ring::{Capacity, OutWord, Payload, RingBase};
use crate::sim::Sim;
use crate::status::{ObjectStatus, WaitOutcome};
use crate::timeout::Timeout;

const RING: GuestAddress = GuestAddress(0x4000_0000);
const DEFINITION_BASE: u32 = 0x0096_c000;

fn task(slot: u32) -> Task {
    Domain::A.words().task(TaskSlot::new(slot).unwrap())
}

fn definition(slot: u32) -> TaskDefinition {
    TaskDefinition(GuestAddress(DEFINITION_BASE + slot * 0x40))
}

fn queue(slot: u32) -> DataQueueId {
    DataQueueId {
        domain: Domain::A,
        slot: DataQueueSlot::new(slot, 120).unwrap(),
    }
}

fn wait(slot: u32) -> DataQueueWait {
    Domain::A.words().data_queue_wait(TaskSlot::new(slot).unwrap())
}

fn timer(slot: u32) -> GuestAddress {
    Domain::A.words().data_queue_timer(TaskSlot::new(slot).unwrap())
}

fn context() -> crate::layout::KernelContext {
    Domain::A.words().context(Domain::A)
}

fn running(sim: &Sim) -> u32 {
    sim.word(GuestAddress(sim.word(context().running())))
}

fn set_selected(sim: &Sim, value: u32) {
    let cell = GuestAddress(sim.word(context().selected()));
    sim.set(cell, value);
}

fn make_ready(sim: &Sim, slot: u32, priority: u8) {
    let task = task(slot);
    let head = Domain::A.words().ready_head(crate::layout::Priority::new(priority as u32).unwrap());
    sim.set_byte(task.state(), TaskState::READY.bits());
    sim.set_byte(task.priority(), priority);
    sim.set(task.next(), head.0 .0);
    sim.set(task.prev(), head.0 .0);
    sim.set(head.next(), task.0 .0);
    sim.set(head.prev(), task.0 .0);
    let bitmap = GuestAddress(sim.word(context().ready_bitmap()));
    sim.set(bitmap, sim.word(bitmap) | (1 << priority));
}

fn install_tasks(sim: &Sim) {
    for slot in 1..=4 {
        let task = task(slot);
        let definition = definition(slot);
        sim.set(task.next(), task.0 .0);
        sim.set(task.prev(), task.0 .0);
        sim.set(task.definition(), definition.0 .0);
        sim.set(task.domain(), DomainId::A.word());
        sim.set_byte(task.state(), TaskState::DORMANT.bits());
        sim.set_byte(task.priority(), slot as u8);
        sim.set(task.wait(), 0);
        sim.set(task.saved_sp(), 0x0095_9800);
        sim.set(task.resume_pc(), 0x0095_9200);
        sim.set(definition.priority(), slot);
        let [low, high] = definition.initial_sp_terms();
        sim.set(low, 0x0095_9700);
        sim.set(high, 0x100);
    }
    make_ready(sim, 1, 0);
    sim.set_running(Domain::A, task(1));
    set_selected(sim, task(1).0 .0);
}

fn install_queue(sim: &Sim, slot: u32, attributes: u32, capacity: u32, base: GuestAddress) {
    let w = Domain::A.words();
    let config = w.data_queue_config(DataQueueSlot::new(slot, 120).unwrap());
    sim.set(config.attributes(), attributes);
    sim.set(config.capacity(), capacity);
    sim.set(config.ring_base(), base.0);
    sim.set(config.exists(), 1);
    let control = w.data_queue_control(DataQueueSlot::new(slot, 120).unwrap());
    sim.set(control.sender_queue().next(), control.0 .0);
    sim.set(control.sender_queue().prev(), control.0 .0);
    sim.set(control.receivers().next(), control.receivers().0 .0);
    sim.set(control.receivers().prev(), control.receivers().0 .0);
    sim.set(control.count(), 0);
    sim.set(control.read(), 0);
    sim.set(control.write(), 0);
}

fn fixture() -> Sim {
    let sim = Sim::new(Domain::A);
    sim.install(Domain::A, true);
    sim.install(Domain::B, true);
    install_tasks(&sim);
    sim
}

fn entry(handler: bool, masked: bool, dispatch: u32) -> Entry {
    Entry {
        handler_context: handler as u32,
        irq_masked: masked,
        dispatch_enabled: dispatch,
    }
}

fn request(service: Service, id: EncodedId, facts: Facts, params: Params) -> Request {
    Request {
        service,
        entry_domain: Domain::A,
        entry: entry(false, false, 1),
        id,
        records: Records { data_queues: 120 },
        facts,
        params,
    }
}

#[test]
fn create_writes_the_record_and_publishes_exists_last() {
    let sim = fixture();
    let id = queue(1);
    let w = Domain::A.words();
    let config = w.data_queue_config(id.slot);
    let control = w.data_queue_control(id.slot);
    sim.set(control.creation(), 0xdead_beef);
    sim.clear_log();
    create_data_queue(
        &sim,
        id,
        DataQueueCreate {
            attributes: QueueAttributes(1),
            capacity: Capacity(2),
            ring: RingBase(RING),
        },
    )
    .unwrap();
    assert_eq!(sim.word(config.attributes()), 1);
    assert_eq!(sim.word(config.capacity()), 2);
    assert_eq!(sim.word(config.ring_base()), RING.0);
    assert_eq!(sim.word(config.exists()), 1);
    assert_eq!(sim.word(control.creation()), 0xdead_beef);
    assert_eq!(sim.word(control.sender_queue().next()), control.0 .0);
    assert_eq!(sim.word(control.sender_queue().prev()), control.0 .0);
    assert_eq!(sim.word(control.receivers().next()), control.receivers().0 .0);
    assert_eq!(sim.word(control.receivers().prev()), control.receivers().0 .0);
    assert_eq!(sim.log().last().copied(), Some(config.exists().0));
}

#[test]
fn create_refuses_an_existing_record_without_a_write() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    sim.clear_log();
    assert_eq!(
        create_data_queue(
            &sim,
            id,
            DataQueueCreate {
                attributes: QueueAttributes(0),
                capacity: Capacity(1),
                ring: RingBase(RING),
            },
        ),
        Err(ObjectStatus::AlreadyExists)
    );
    assert!(sim.log().is_empty());
}

#[test]
fn ring_appends_pops_and_wraps() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 2, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    assert_eq!(
        send(&sim, EntryDomain::new(Domain::A), id, Payload(1)),
        Ok(SendBlocking::Ready(WaitOutcome::Satisfied(())))
    );
    assert_eq!(
        send(&sim, EntryDomain::new(Domain::A), id, Payload(2)),
        Ok(SendBlocking::Ready(WaitOutcome::Satisfied(())))
    );
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(1))))
    );
    assert_eq!(
        send(&sim, EntryDomain::new(Domain::A), id, Payload(3)),
        Ok(SendBlocking::Ready(WaitOutcome::Satisfied(())))
    );
    assert_eq!(sim.word(out.0), 1);
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(2))))
    );
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(3))))
    );
    assert_eq!(sim.word(out.0), 3);
}

#[test]
fn a_full_ring_refuses_a_polling_send_without_a_write() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    send(&sim, EntryDomain::new(Domain::A), id, Payload(0xaa)).unwrap();
    sim.clear_log();
    assert_eq!(
        send_poll(&sim, id, Payload(0xbb), false),
        Ok(WaitOutcome::TimedOut)
    );
    assert_eq!(sim.word(RING), 0xaa);
    assert!(sim.log().is_empty());
}

#[test]
fn force_send_drops_the_oldest_word() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 2, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    send(&sim, EntryDomain::new(Domain::A), id, Payload(1)).unwrap();
    send(&sim, EntryDomain::new(Domain::A), id, Payload(2)).unwrap();
    force_send(&sim, id, Payload(3), false).unwrap();
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(2))))
    );
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(3))))
    );
}

#[test]
fn force_send_refuses_a_zero_capacity_ring() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 0, RING);
    assert_eq!(
        force_send(&sim, queue(1), Payload(1), false),
        Err(ObjectStatus::Capacity)
    );
}

#[test]
fn a_waiting_receiver_takes_the_sent_word() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let pending = match receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Forever) {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    assert_eq!(sim.task_state(task(2)), RECEIVE_STATE.bits());
    sim.set_running(Domain::A, task(1));
    assert_eq!(
        send(&sim, EntryDomain::new(Domain::A), id, Payload(0x1234)),
        Ok(SendBlocking::Ready(WaitOutcome::Satisfied(())))
    );
    assert_eq!(sim.word(wait(2).data()), 0x1234);
    assert_eq!(sim.word(wait(2).base().result()), 0);
    assert_eq!(sim.task_state(task(2)), TaskState::READY.bits());
    assert_eq!(
        sim.word(Domain::A.words().data_queue_control(id.slot).receivers().next()),
        Domain::A.words().data_queue_control(id.slot).receivers().0 .0
    );
    assert_eq!(
        resume_receive(&sim, pending),
        ReceiveResume::Completed(WaitOutcome::Satisfied(Payload(0x1234)))
    );
    assert_eq!(sim.word(out.0), 0x1234);
}

#[test]
fn a_waiting_sender_refills_the_ring_after_a_receive() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    send(&sim, EntryDomain::new(Domain::A), id, Payload(0xaaaa)).unwrap();
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let pending = match send(&sim, EntryDomain::new(Domain::A), id, Payload(0xbbbb)) {
        Ok(SendBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    assert_eq!(sim.task_state(task(2)), SEND_STATE.bits());
    sim.set_running(Domain::A, task(1));
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(0xaaaa))))
    );
    assert_eq!(sim.word(out.0), 0xaaaa);
    assert_eq!(sim.word(RING), 0xbbbb);
    assert_eq!(sim.task_state(task(2)), TaskState::READY.bits());
    assert_eq!(
        resume_send(&sim, pending),
        SendResume::Completed(WaitOutcome::Satisfied(()))
    );
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(0xbbbb))))
    );
}

#[test]
fn a_timed_sender_expires_with_its_armed_timer() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    send(&sim, EntryDomain::new(Domain::A), id, Payload(1)).unwrap();
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let pending = match send_timeout(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        Payload(2),
        Timeout::Units(5),
    ) {
        Ok(SendBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    assert!(sim.armed(timer(2)));
    assert_eq!(sim.deadline(timer(2)), Some(5));
    sim.complete(task(2), wait(2), -50);
    assert!(!sim.armed(timer(2)));
    assert_eq!(
        resume_send(&sim, pending),
        SendResume::Completed(WaitOutcome::TimedOut)
    );
}

#[test]
fn delete_releases_a_blocked_receiver_and_a_blocked_sender() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    install_queue(&sim, 2, 0, 1, RING);
    let out = OutWord(GuestAddress(0x4000_1000));

    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let receiver = match receive(&sim, EntryDomain::new(Domain::A), queue(1), out, Timeout::Forever) {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };

    send(&sim, EntryDomain::new(Domain::A), queue(2), Payload(1)).unwrap();
    make_ready(&sim, 3, 3);
    sim.set_running(Domain::A, task(3));
    let sender = match send(&sim, EntryDomain::new(Domain::A), queue(2), Payload(2)) {
        Ok(SendBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };

    sim.set_running(Domain::A, task(1));
    delete_data_queue(&sim, queue(1), HandlerContext::Task).unwrap();
    delete_data_queue(&sim, queue(2), HandlerContext::Task).unwrap();
    assert_eq!(
        resume_receive(&sim, receiver),
        ReceiveResume::Completed(WaitOutcome::Released)
    );
    assert_eq!(
        resume_send(&sim, sender),
        SendResume::Completed(WaitOutcome::Released)
    );
    assert_eq!(sim.word(out.0), 0);
}

#[test]
fn delete_cancels_a_timed_waiter() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let pending = match receive(
        &sim,
        EntryDomain::new(Domain::A),
        queue(1),
        out,
        Timeout::Units(5),
    ) {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    assert!(sim.armed(timer(2)));
    sim.set_running(Domain::A, task(1));
    delete_data_queue(&sim, queue(1), HandlerContext::Task).unwrap();
    assert!(!sim.armed(timer(2)));
    assert_eq!(
        resume_receive(&sim, pending),
        ReceiveResume::Completed(WaitOutcome::Released)
    );
}

#[test]
fn a_handler_delete_defers_the_released_waiter_dispatch() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 1, 3);
    make_ready(&sim, 2, 0);
    set_selected(&sim, task(1).0 .0);
    sim.set_running(Domain::A, task(2));
    let pending = match receive(&sim, EntryDomain::new(Domain::A), queue(1), out, Timeout::Forever)
    {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    sim.set_running(Domain::A, task(3));
    delete_data_queue(&sim, queue(1), HandlerContext::Handler).unwrap();
    assert_eq!(sim.delayed(), 1);
    assert_eq!(running(&sim), task(3).0 .0);
    assert_eq!(
        resume_receive(&sim, pending),
        ReceiveResume::Completed(WaitOutcome::Released)
    );
}

#[test]
fn a_task_delete_dispatches_the_released_waiter() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 1, 3);
    make_ready(&sim, 2, 0);
    set_selected(&sim, task(1).0 .0);
    sim.set_running(Domain::A, task(2));
    let _ = match receive(&sim, EntryDomain::new(Domain::A), queue(1), out, Timeout::Forever) {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    sim.set_running(Domain::A, task(3));
    delete_data_queue(&sim, queue(1), HandlerContext::Task).unwrap();
    assert_eq!(sim.delayed(), 0);
    assert_eq!(running(&sim), task(2).0 .0);
}

#[test]
fn a_timeout_that_completed_first_keeps_its_result_across_delete() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let pending = match receive(
        &sim,
        EntryDomain::new(Domain::A),
        queue(1),
        out,
        Timeout::Units(5),
    ) {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    sim.complete(task(2), wait(2), -50);
    sim.set_running(Domain::A, task(1));
    delete_data_queue(&sim, queue(1), HandlerContext::Task).unwrap();
    assert_eq!(
        resume_receive(&sim, pending),
        ReceiveResume::Completed(WaitOutcome::TimedOut)
    );
    assert_eq!(sim.wait_result(wait(2)), -50);
}

#[test]
fn senders_are_ordered_by_priority_and_keep_arrival_order_on_ties() {
    let sim = fixture();
    install_queue(&sim, 1, 1, 1, RING);
    let id = queue(1);
    send(&sim, EntryDomain::new(Domain::A), id, Payload(0)).unwrap();

    for (slot, payload, priority) in [(2, 0x20, 5), (3, 0x30, 3), (4, 0x40, 5)] {
        make_ready(&sim, slot, priority);
        sim.set_running(Domain::A, task(slot));
        match send(&sim, EntryDomain::new(Domain::A), id, Payload(payload)) {
            Ok(SendBlocking::Pending(_)) => {}
            other => panic!("{other:?}"),
        }
    }
    sim.set_running(Domain::A, task(1));
    let control = Domain::A.words().data_queue_control(id.slot);
    assert_eq!(sim.word(control.sender_queue().next()), task(3).0 .0);
    let out = OutWord(GuestAddress(0x4000_1000));
    receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll).unwrap();
    assert_eq!(sim.task_state(task(3)), TaskState::READY.bits());
    receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll).unwrap();
    assert_eq!(sim.task_state(task(2)), TaskState::READY.bits());
    receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll).unwrap();
    assert_eq!(sim.task_state(task(4)), TaskState::READY.bits());
}

#[test]
fn receivers_are_released_in_arrival_order() {
    let sim = fixture();
    install_queue(&sim, 1, 1, 1, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    for slot in [2, 3] {
        make_ready(&sim, slot, if slot == 2 { 3 } else { 5 });
        sim.set_running(Domain::A, task(slot));
        match receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Forever) {
            Ok(ReceiveBlocking::Pending(_)) => {}
            other => panic!("{other:?}"),
        }
    }
    sim.set_running(Domain::A, task(1));
    send(&sim, EntryDomain::new(Domain::A), id, Payload(1)).unwrap();
    assert_eq!(sim.word(wait(2).data()), 1);
    assert_eq!(sim.task_state(task(2)), TaskState::READY.bits());
    assert_eq!(sim.task_state(task(3)), RECEIVE_STATE.bits());
    send(&sim, EntryDomain::new(Domain::A), id, Payload(2)).unwrap();
    assert_eq!(sim.word(wait(3).data()), 2);
    assert_eq!(sim.task_state(task(3)), TaskState::READY.bits());
}

#[test]
fn delete_resets_the_stock_record_values() {
    let sim = fixture();
    install_queue(&sim, 1, 7, 4, GuestAddress(0x4000_2000));
    let id = queue(1);
    let w = Domain::A.words();
    let config = w.data_queue_config(id.slot);
    let control = w.data_queue_control(id.slot);
    delete_data_queue(&sim, id, HandlerContext::Task).unwrap();
    assert_eq!(sim.word(config.attributes()), 0);
    assert_eq!(sim.word(config.capacity()), 1);
    assert_eq!(sim.word(config.ring_base()), w.data_queue_placeholder.0);
    assert_eq!(sim.word(config.exists()), 0);
    assert_eq!(sim.word(control.sender_queue().next()), control.0 .0);
    assert_eq!(sim.word(control.receivers().next()), control.receivers().0 .0);
    assert_eq!(sim.word(control.count()), 0);
    assert_eq!(sim.word(control.read()), 0);
    assert_eq!(sim.word(control.write()), 0);
}

#[test]
fn resolve_uses_the_class_and_signed_index() {
    let entry = EntryDomain::new(Domain::A);
    assert_eq!(
        resolve_data_queue(entry, EncodedId::of(0, 1), 120),
        Ok(DataQueueId {
            domain: Domain::A,
            slot: DataQueueSlot::new(1, 120).unwrap()
        })
    );
    assert_eq!(
        resolve_data_queue(entry, EncodedId::of(2, 3), 120),
        Ok(DataQueueId {
            domain: Domain::B,
            slot: DataQueueSlot::new(3, 120).unwrap()
        })
    );
    assert_eq!(
        resolve_data_queue(entry, EncodedId::of(3, 1), 120),
        Err(ObjectStatus::InvalidClass)
    );
    assert_eq!(
        resolve_data_queue(entry, EncodedId::of(0, 0), 120),
        Err(ObjectStatus::InvalidIndex)
    );
    assert_eq!(
        resolve_data_queue(entry, EncodedId::of(0, 121), 120),
        Err(ObjectStatus::InvalidIndex)
    );
    assert_eq!(
        own_data_queue_slot(EncodedId::of(0x7000, 2), 120),
        Ok(DataQueueSlot::new(2, 120).unwrap())
    );
}

#[test]
fn admission_refuses_in_stock_order() {
    let facts = Facts {
        exists: true,
        attributes: 0,
        capacity: 2,
    };
    let mut request = request(Service::Receive, EncodedId::of(0, 1), facts, Params { timeout: None });
    request.entry = entry(true, false, 1);
    assert_eq!(admit(&request), Err(ObjectStatus::Context));

    request.entry = entry(false, true, 1);
    assert_eq!(admit(&request), Err(ObjectStatus::Context));

    request.entry = entry(false, false, 0);
    assert_eq!(admit(&request), Err(ObjectStatus::Context));

    let mut timed = request.clone_for_test(Service::ReceiveTimeout, EncodedId::of(3, 1));
    timed.entry = entry(false, false, 1);
    timed.params.timeout = Some(-2);
    timed.facts.exists = false;
    assert_eq!(admit(&timed), Err(ObjectStatus::InvalidClass));

    timed.id = EncodedId::of(0, 1);
    assert_eq!(admit(&timed), Err(ObjectStatus::Param));

    timed.params.timeout = Some(0);
    assert_eq!(admit(&timed), Err(ObjectStatus::NotCreated));

    let mut force = request.clone_for_test(Service::ForceSend, EncodedId::of(0, 1));
    force.facts.capacity = 0;
    assert_eq!(admit(&force), Err(ObjectStatus::Capacity));

    let mut handler_force =
        request.clone_for_test(Service::ForceSendHandler, EncodedId::of(2, 1));
    handler_force.entry = entry(true, false, 1);
    handler_force.facts.exists = false;
    assert_eq!(admit(&handler_force), Err(ObjectStatus::InvalidIndex));

    let mut send = request.clone_for_test(Service::Send, EncodedId::of(0, 1));
    send.entry = entry(true, false, 1);
    assert_eq!(admit(&send), Err(ObjectStatus::Context));
}

/// Named counterexample `handler-context-blocking-send`: the stock body checks no context and
/// would block the handler task on the ISR stack; admission refuses it with E_CTX.
#[test]
fn a_handler_send_is_refused() {
    let facts = Facts {
        exists: true,
        attributes: 0,
        capacity: 2,
    };
    let mut request = request(Service::Send, EncodedId::of(0, 1), facts, Params { timeout: None });
    request.entry = entry(true, false, 1);
    assert_eq!(admit(&request), Err(ObjectStatus::Context));
    request.entry = entry(false, true, 1);
    assert_eq!(admit(&request), Err(ObjectStatus::Context));
    request.entry = entry(false, false, 0);
    assert_eq!(admit(&request), Err(ObjectStatus::Context));
    request.entry = entry(false, false, 1);
    assert!(admit(&request).is_ok());
}

impl Request {
    fn clone_for_test(&self, service: Service, id: EncodedId) -> Request {
        Request {
            service,
            entry_domain: self.entry_domain,
            entry: self.entry,
            id,
            records: self.records,
            facts: self.facts,
            params: self.params,
        }
    }
}

#[test]
fn the_wait_states_match_the_stock_bits() {
    assert_eq!(SEND_STATE.bits(), 0x32);
    assert_eq!(RECEIVE_STATE.bits(), 0x12);
    assert_eq!(TaskState::WAIT.bits(), 0x02);
    assert_eq!(TaskState::OBJECT_QUEUED.bits(), 0x10);
    assert_eq!(TaskState::OWNER_LOCK_QUEUED.bits(), 0x20);
}

#[test]
fn force_send_on_a_capacity_one_ring_replaces_the_word() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    send(&sim, EntryDomain::new(Domain::A), id, Payload(1)).unwrap();
    force_send(&sim, id, Payload(2), false).unwrap();
    assert_eq!(sim.word(RING), 2);
    let control = Domain::A.words().data_queue_control(id.slot);
    assert_eq!(sim.word(control.read()), 0);
    assert_eq!(sim.word(control.write()), 0);
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(2))))
    );
}

#[test]
fn force_send_at_the_last_slot_wraps_both_indices() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 2, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    send(&sim, EntryDomain::new(Domain::A), id, Payload(1)).unwrap();
    send(&sim, EntryDomain::new(Domain::A), id, Payload(2)).unwrap();
    receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll).unwrap();
    send(&sim, EntryDomain::new(Domain::A), id, Payload(3)).unwrap();
    force_send(&sim, id, Payload(4), false).unwrap();
    let control = Domain::A.words().data_queue_control(id.slot);
    assert_eq!(sim.word(control.read()), 0);
    assert_eq!(sim.word(control.write()), 0);
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(3))))
    );
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Poll),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(Payload(4))))
    );
}

#[test]
fn a_pending_irq_wakes_the_suspended_caller_only_when_the_retry_reopens_i() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    sim.suspend_pending_irq(task(2));
    sim.set_interrupts(0x40);
    let mut attempts = 0;
    let outcome = retry_masked(&sim, || {
        attempts += 1;
        if attempts > 4 {
            return Ok(Attempt::Done(ReceiveBlocking::Ready(WaitOutcome::TimedOut)));
        }
        match receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Forever) {
            Ok(ReceiveBlocking::Retry) => Ok(Attempt::Retry),
            Ok(outcome) => Ok(Attempt::Done(outcome)),
            Err(status) => Err(status),
        }
    })
    .unwrap();
    assert_eq!(attempts, 2);
    let pending = match outcome {
        ReceiveBlocking::Pending(pending) => pending,
        other => panic!("{other:?}"),
    };
    assert_eq!(sim.task_state(task(2)), RECEIVE_STATE.bits());
    sim.set(wait(2).data(), 0x5a5a);
    sim.complete(task(2), wait(2), 0);
    assert_eq!(
        resume_receive(&sim, pending),
        ReceiveResume::Completed(WaitOutcome::Satisfied(Payload(0x5a5a)))
    );
    assert_eq!(sim.word(out.0), 0x5a5a);
    assert_eq!(sim.interrupts(), 0x40);
}

#[test]
fn resume_receive_restores_the_caller_mask() {
    let sim = fixture();
    install_queue(&sim, 1, 0, 1, RING);
    let id = queue(1);
    let out = OutWord(GuestAddress(0x4000_1000));
    make_ready(&sim, 2, 3);
    sim.set_running(Domain::A, task(2));
    let pending = match receive(&sim, EntryDomain::new(Domain::A), id, out, Timeout::Forever) {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("{other:?}"),
    };
    sim.set_running(Domain::A, task(1));
    send(&sim, EntryDomain::new(Domain::A), id, Payload(0x55)).unwrap();
    sim.set_interrupts(0x40);
    assert_eq!(
        resume_receive(&sim, pending),
        ReceiveResume::Completed(WaitOutcome::Satisfied(Payload(0x55)))
    );
    assert_eq!(sim.interrupts(), 0x40);
    assert_eq!(sim.word(out.0), 0x55);
}
