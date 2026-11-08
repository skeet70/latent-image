use crate::admission::{admit, require_created, Entry, Request, Service};
use crate::block::{poll, resume, timed_wait, wait, SemaphoreBlocking, SemaphoreResume};
use crate::count::{Count, Maximum, SemaphoreAttributes};
use crate::create::{create_semaphore, SemaphoreCreate};
use crate::id::{EncodedId, EntryDomain, SemaphoreId};
use crate::layout::*;
use crate::signal::signal;
use crate::wake::WakeMode;
use crate::sim::Sim;
use crate::status::{SemaphoreStatus, WaitOutcome};
use crate::teardown::delete_semaphore;
use crate::timeout::Timeout;

const DEFINITIONS: u32 = 0x0096_c000;

struct Fixture {
    sim: Sim,
    entry: Domain,
}

impl Fixture {
    fn new(entry: Domain) -> Self {
        let sim = Sim::new(entry);
        for owner in [Domain::A, Domain::B] {
            sim.install(owner, true);
            for slot in 1..=3u32 {
                let task = owner.words().task(TaskSlot::new(slot).unwrap());
                let index = if owner == Domain::B { 120 } else { 0 } + slot - 1;
                let definition = GuestAddress(DEFINITIONS + index * 0x40);
                sim.set(task.next(), task.0 .0);
                sim.set(task.prev(), task.0 .0);
                sim.set(task.definition(), definition.0);
                sim.set(task.domain(), owner.id().word());
                sim.set_byte(task.state(), TaskState::DORMANT.bits());
                sim.set_byte(task.priority(), if slot == 1 { 10 } else { slot as u8 });
                sim.set(task.wait(), 0);
                sim.set(definition.offset(0x0c), 10);
                sim.set(definition.offset(0x20), slot);
            }
        }
        Self { sim, entry }
    }

    fn task(&self, owner: Domain, slot: u32) -> Task {
        owner.words().task(TaskSlot::new(slot).unwrap())
    }

    fn ready(&self, owner: Domain, slot: u32) {
        let task = self.task(owner, slot);
        let priority = Priority::new(self.priority(owner, slot)).unwrap();
        let head = owner.words().ready_head(priority);
        self.sim.set(head.next(), task.0 .0);
        self.sim.set(head.prev(), task.0 .0);
        self.sim.set(task.next(), head.0 .0);
        self.sim.set(task.prev(), head.0 .0);
        self.sim.set_byte(task.state(), TaskState::READY.bits());
        let bitmap = GuestAddress(self.sim.word(owner.words().context(owner).ready_bitmap()));
        self.sim.set(bitmap, self.sim.word(bitmap) | priority.bit());
    }

    fn running(&self, owner: Domain, slot: u32) {
        self.ready(owner, slot);
        self.sim.set_running(owner, self.task(owner, slot));
        self.sim.set(
            GuestAddress(self.sim.word(owner.words().context(owner).selected())),
            self.task(owner, slot).0 .0,
        );
    }

    fn priority(&self, owner: Domain, slot: u32) -> u32 {
        let task = self.task(owner, slot);
        let GuestAddress(at) = task.priority();
        (self.sim.word(GuestAddress(at & !3)) >> (8 * (at & 3))) as u32
    }

    fn set_priority(&self, owner: Domain, slot: u32, priority: u8) {
        self.sim.set_byte(self.task(owner, slot).priority(), priority);
    }

    fn object(&self, domain: Domain, slot: u32) -> (SemaphoreConfig, SemaphoreControl) {
        let words = domain.words();
        let slot = SemaphoreSlot::new(slot, SemaphoreSlot::COUNT).unwrap();
        (words.semaphore_config(slot), words.semaphore_control(slot))
    }

    fn create(&self, domain: Domain, slot: u32, attributes: u32, initial: u32, maximum: u32) {
        let (config, control) = self.object(domain, slot);
        self.sim.set(config.attributes(), attributes);
        self.sim.set(config.initial(), initial);
        self.sim.set(config.maximum(), maximum);
        self.sim.set(config.exists(), 1);
        self.sim.set(control.queue().next(), control.0 .0);
        self.sim.set(control.queue().prev(), control.0 .0);
        self.sim.set(control.queue().config(), config.0 .0);
        self.sim.set(control.count(), initial);
    }

    fn wait_block(&self, owner: Domain, slot: u32) -> WaitBlock {
        owner.words().semaphore_wait(TaskSlot::new(slot).unwrap())
    }

    fn queue_waiter(&self, owner: Domain, slot: u32, domain: Domain, semaphore: u32) {
        let task = self.task(owner, slot);
        let wait = self.wait_block(owner, slot);
        let (_, control) = self.object(domain, semaphore);
        self.sim.set_byte(
            task.state(),
            TaskState::WAIT
                .union(TaskState::OBJECT_QUEUED)
                .union(TaskState::OWNER_LOCK_QUEUED)
                .bits(),
        );
        self.sim.set(task.wait(), wait.0 .0);
        let prev = self.sim.word(control.queue().prev());
        self.sim.set(task.prev(), prev);
        self.sim.set(task.next(), control.0 .0);
        self.sim.set(GuestAddress(prev), task.0 .0);
        self.sim.set(control.queue().prev(), task.0 .0);
        self.sim.set(wait.result(), 0);
        self.sim.set(wait.queue(), control.0 .0);
        self.sim.set(wait.owner(), domain.id().word());
    }

    fn id(&self, domain: Domain, slot: u32) -> SemaphoreId {
        SemaphoreId {
            domain,
            slot: SemaphoreSlot::new(slot, SemaphoreSlot::COUNT).unwrap(),
        }
    }

    fn entry(&self) -> EntryDomain {
        EntryDomain::new(self.entry)
    }

    fn clock(&self, domain: Domain, value: u32) {
        let words = domain.words();
        self.sim.set(GuestAddress(self.sim.word(words.tick_now)), value);
    }
}

fn count(fixture: &Fixture, domain: Domain, slot: u32) -> u32 {
    let (_, control) = fixture.object(domain, slot);
    fixture.sim.word(control.count())
}

fn exists(fixture: &Fixture, domain: Domain, slot: u32) -> u32 {
    let (config, _) = fixture.object(domain, slot);
    fixture.sim.word(config.exists())
}

fn state(fixture: &Fixture, owner: Domain, slot: u32) -> u8 {
    let GuestAddress(at) = fixture.task(owner, slot).state();
    (fixture.sim.word(GuestAddress(at & !3)) >> (8 * (at & 3))) as u8
}

fn result(fixture: &Fixture, owner: Domain, slot: u32) -> i32 {
    fixture.sim.word(fixture.wait_block(owner, slot).result()) as i32
}

fn create_one(fixture: &Fixture, attributes: u32, initial: u32, maximum: u32) -> Result<(), SemaphoreStatus> {
    create_semaphore(&fixture.sim, fixture.id(Domain::A, 1), || SemaphoreCreate {
        attributes: SemaphoreAttributes(attributes),
        initial: Count::from_word(initial),
        maximum: Maximum::from_word(maximum),
    })
}

#[test]
fn create_writes_the_record_and_the_count() {
    let fixture = Fixture::new(Domain::A);
    create_one(&fixture, 1, 3, 5).unwrap();
    let (config, control) = fixture.object(Domain::A, 1);
    assert_eq!(fixture.sim.word(config.attributes()), 1);
    assert_eq!(fixture.sim.word(config.initial()), 3);
    assert_eq!(fixture.sim.word(config.maximum()), 5);
    assert_eq!(fixture.sim.word(config.exists()), 1);
    assert_eq!(fixture.sim.word(control.count()), 3);
    assert_eq!(create_one(&fixture, 0, 9, 9), Err(SemaphoreStatus::AlreadyExists));
    assert_eq!(fixture.sim.word(config.attributes()), 1);
    assert_eq!(fixture.sim.word(control.count()), 3);
}

#[test]
fn create_and_delete_refuse_an_id_outside_the_table() {
    let fixture = Fixture::new(Domain::A);
    let outside = |index: i32| {
        admit(
            &Request {
                service: Service::Create,
                entry: None,
                entry_domain: fixture.entry(),
                id: EncodedId(index),
            },
            |_| SemaphoreSlot::COUNT,
        )
    };
    assert_eq!(outside(251), Err(SemaphoreStatus::InvalidId));
    assert_eq!(outside(0), Err(SemaphoreStatus::InvalidId));
    assert_eq!(outside(-1), Err(SemaphoreStatus::InvalidId));
    assert!(outside(250).is_ok());
}

#[test]
fn delete_resets_the_record_and_the_count() {
    let fixture = Fixture::new(Domain::A);
    fixture.create(Domain::A, 1, 1, 4, 6);
    delete_semaphore(&fixture.sim, fixture.id(Domain::A, 1)).unwrap();
    let (config, control) = fixture.object(Domain::A, 1);
    assert_eq!(fixture.sim.word(config.attributes()), 0);
    assert_eq!(fixture.sim.word(config.initial()), 1);
    assert_eq!(fixture.sim.word(config.maximum()), 1);
    assert_eq!(fixture.sim.word(config.exists()), 0);
    assert_eq!(fixture.sim.word(control.count()), 1);
    assert!(fixture.sim.dispatches().is_empty());
    assert_eq!(require_created(exists(&fixture, Domain::A, 1)), Err(SemaphoreStatus::NotCreated));
}

#[test]
fn delete_releases_every_waiter_with_released() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    fixture.queue_waiter(Domain::A, 2, Domain::A, 1);
    fixture.queue_waiter(Domain::A, 3, Domain::A, 1);
    delete_semaphore(&fixture.sim, fixture.id(Domain::A, 1)).unwrap();
    for slot in [2, 3] {
        assert_eq!(state(&fixture, Domain::A, slot), TaskState::READY.bits());
        assert_eq!(result(&fixture, Domain::A, slot), -49);
    }
    let (_, control) = fixture.object(Domain::A, 1);
    assert_eq!(fixture.sim.word(control.queue().next()), control.0 .0);
    assert_eq!(fixture.sim.word(control.queue().prev()), control.0 .0);
    assert_eq!(fixture.sim.dispatches(), vec![0]);
}

#[test]
fn signal_increments_below_the_maximum() {
    let fixture = Fixture::new(Domain::A);
    fixture.create(Domain::A, 1, 0, 0, 2);
    signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Task).unwrap();
    assert_eq!(count(&fixture, Domain::A, 1), 1);
}

#[test]
fn signal_at_the_maximum_is_overflow_without_a_write() {
    let fixture = Fixture::new(Domain::A);
    fixture.create(Domain::A, 1, 0, 2, 2);
    assert_eq!(
        signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Task),
        Err(SemaphoreStatus::Overflow)
    );
    assert_eq!(count(&fixture, Domain::A, 1), 2);
}

#[test]
fn signal_wakes_the_first_waiter_without_changing_the_count() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 2);
    fixture.queue_waiter(Domain::A, 2, Domain::A, 1);
    signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Task).unwrap();
    assert_eq!(count(&fixture, Domain::A, 1), 0);
    assert_eq!(state(&fixture, Domain::A, 2), TaskState::READY.bits());
    assert_eq!(result(&fixture, Domain::A, 2), 0);
    assert_eq!(fixture.sim.dispatches(), vec![-35]);
}

#[test]
fn handler_signal_defers_the_dispatch() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 2);
    fixture.queue_waiter(Domain::A, 2, Domain::A, 1);
    signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Handler).unwrap();
    assert!(fixture.sim.dispatches().is_empty());
    assert_eq!(fixture.sim.delayed(), 1);
    assert_eq!(state(&fixture, Domain::A, 2), TaskState::READY.bits());
}

#[test]
fn a_peer_waiter_is_rung_not_dispatched_locally() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.running(Domain::B, 1);
    fixture.create(Domain::A, 1, 0, 0, 2);
    fixture.queue_waiter(Domain::B, 2, Domain::A, 1);
    signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Task).unwrap();
    assert!(fixture.sim.doorbell(1));
    assert!(!fixture.sim.doorbell(0));
    assert!(fixture.sim.dispatches().is_empty());
    assert_eq!(state(&fixture, Domain::B, 2), TaskState::READY.bits());
}

#[test]
fn wait_takes_a_count() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 2, 2);
    assert_eq!(
        wait(&fixture.sim, fixture.entry(), fixture.id(Domain::A, 1)),
        Ok(SemaphoreBlocking::Ready(WaitOutcome::Acquired))
    );
    assert_eq!(count(&fixture, Domain::A, 1), 1);
    assert!(fixture.sim.dispatches().is_empty());
}

#[test]
fn wait_blocks_and_a_signal_resumes_it() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    let blocking = wait(&fixture.sim, fixture.entry(), fixture.id(Domain::A, 1)).unwrap();
    let pending = match blocking {
        SemaphoreBlocking::Pending(pending) => pending,
        other => panic!("expected a pending wait, got {other:?}"),
    };
    assert_eq!(state(&fixture, Domain::A, 1) & TaskState::WAIT.bits(), TaskState::WAIT.bits());
    assert_eq!(result(&fixture, Domain::A, 1), 0);
    let (_, control) = fixture.object(Domain::A, 1);
    assert_eq!(fixture.sim.word(control.queue().next()), pending.task.0 .0);
    assert_eq!(
        fixture.sim.word(pending.wait.queue()),
        control.0 .0
    );
    assert_eq!(fixture.sim.word(pending.wait.owner()), Domain::A.id().word());
    assert_eq!(fixture.sim.dispatches(), vec![-37]);
    signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Task).unwrap();
    assert_eq!(
        resume(&fixture.sim, pending),
        SemaphoreResume::Completed(WaitOutcome::Acquired)
    );
}

#[test]
fn poll_takes_a_count_and_reports_empty() {
    let fixture = Fixture::new(Domain::A);
    fixture.create(Domain::A, 1, 0, 1, 1);
    assert_eq!(
        poll(&fixture.sim, fixture.id(Domain::A, 1)),
        Ok(WaitOutcome::Acquired)
    );
    assert_eq!(count(&fixture, Domain::A, 1), 0);
    assert_eq!(
        poll(&fixture.sim, fixture.id(Domain::A, 1)),
        Ok(WaitOutcome::TimedOut)
    );
    assert_eq!(count(&fixture, Domain::A, 1), 0);
    assert!(fixture.sim.dispatches().is_empty());
}

#[test]
fn a_zero_timeout_polls_and_never_blocks() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    assert_eq!(
        timed_wait(
            &fixture.sim,
            fixture.entry(),
            fixture.id(Domain::A, 1),
            Timeout::Poll
        ),
        Ok(SemaphoreBlocking::Ready(WaitOutcome::TimedOut))
    );
    assert_eq!(state(&fixture, Domain::A, 1), TaskState::READY.bits());
    assert!(fixture.sim.dispatches().is_empty());
}

#[test]
fn a_negative_timeout_blocks_without_a_timer() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    let blocking = timed_wait(
        &fixture.sim,
        fixture.entry(),
        fixture.id(Domain::A, 1),
        Timeout::from_stock(-5),
    )
    .unwrap();
    assert!(matches!(blocking, SemaphoreBlocking::Pending(_)));
    assert_eq!(result(&fixture, Domain::A, 1), 0);
    let timer = Domain::A.words().semaphore_timer(TaskSlot::new(1).unwrap());
    assert!(!fixture.sim.armed(timer));
    assert_eq!(fixture.sim.dispatches(), vec![-39]);
}

#[test]
fn a_positive_timeout_arms_the_task_timer() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    fixture.clock(Domain::A, 100);
    timed_wait(
        &fixture.sim,
        fixture.entry(),
        fixture.id(Domain::A, 1),
        Timeout::from_stock(10),
    )
    .unwrap();
    let timer = Domain::A.words().semaphore_timer(TaskSlot::new(1).unwrap());
    assert!(fixture.sim.armed(timer));
    assert_eq!(fixture.sim.deadline(timer), Some(110));
    assert_eq!(fixture.sim.word(timer.offset(0x04)), 0x44318);
    assert_eq!(
        fixture.sim.word(timer.offset(0x08)),
        fixture.task(Domain::A, 1).0 .0
    );
    assert_eq!(result(&fixture, Domain::A, 1), timer.0 as i32);
}

#[test]
fn a_signal_removes_the_armed_timer_before_resuming() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    fixture.clock(Domain::A, 100);
    let blocking = timed_wait(
        &fixture.sim,
        fixture.entry(),
        fixture.id(Domain::A, 1),
        Timeout::from_stock(10),
    )
    .unwrap();
    let pending = match blocking {
        SemaphoreBlocking::Pending(pending) => pending,
        other => panic!("expected a pending wait, got {other:?}"),
    };
    let timer = Domain::A.words().semaphore_timer(TaskSlot::new(1).unwrap());
    signal(&fixture.sim, fixture.id(Domain::A, 1), WakeMode::Task).unwrap();
    assert!(!fixture.sim.armed(timer));
    assert_eq!(
        resume(&fixture.sim, pending),
        SemaphoreResume::Completed(WaitOutcome::Acquired)
    );
}

#[test]
fn a_timeout_result_resumes_as_timed_out() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    let blocking = timed_wait(
        &fixture.sim,
        fixture.entry(),
        fixture.id(Domain::A, 1),
        Timeout::from_stock(10),
    )
    .unwrap();
    let pending = match blocking {
        SemaphoreBlocking::Pending(pending) => pending,
        other => panic!("expected a pending wait, got {other:?}"),
    };
    fixture.sim.set(pending.wait.result(), (-50i32) as u32);
    fixture.sim.set_byte(pending.task.state(), TaskState::READY.bits());
    assert_eq!(
        resume(&fixture.sim, pending),
        SemaphoreResume::Completed(WaitOutcome::TimedOut)
    );
}

#[test]
fn the_priority_attribute_orders_the_queue() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 1, 0, 1);
    fixture.set_priority(Domain::A, 2, 15);
    fixture.queue_waiter(Domain::A, 2, Domain::A, 1);
    timed_wait(
        &fixture.sim,
        fixture.entry(),
        fixture.id(Domain::A, 1),
        Timeout::from_stock(-1),
    )
    .unwrap();
    let (_, control) = fixture.object(Domain::A, 1);
    assert_eq!(
        fixture.sim.word(control.queue().next()),
        fixture.task(Domain::A, 1).0 .0
    );
    assert_eq!(
        fixture.sim.word(fixture.task(Domain::A, 1).next()),
        fixture.task(Domain::A, 2).0 .0
    );
}

#[test]
fn the_fifo_attribute_appends() {
    let fixture = Fixture::new(Domain::A);
    fixture.running(Domain::A, 1);
    fixture.create(Domain::A, 1, 0, 0, 1);
    fixture.set_priority(Domain::A, 2, 15);
    fixture.queue_waiter(Domain::A, 2, Domain::A, 1);
    timed_wait(
        &fixture.sim,
        fixture.entry(),
        fixture.id(Domain::A, 1),
        Timeout::from_stock(-1),
    )
    .unwrap();
    let (_, control) = fixture.object(Domain::A, 1);
    assert_eq!(
        fixture.sim.word(control.queue().next()),
        fixture.task(Domain::A, 2).0 .0
    );
    assert_eq!(
        fixture.sim.word(fixture.task(Domain::A, 2).next()),
        fixture.task(Domain::A, 1).0 .0
    );
}

#[test]
fn admission_precedence_is_the_stock_order() {
    let fixture = Fixture::new(Domain::A);
    let request = |service: Service, entry: Option<Entry>, id: i32| {
        admit(
            &Request {
                service,
                entry,
                entry_domain: fixture.entry(),
                id: EncodedId(id),
            },
            |_| SemaphoreSlot::COUNT,
        )
    };
    let task = Some(Entry {
        handler_context: 0,
        irq_masked: false,
        dispatch_enabled: 1,
    });
    assert_eq!(
        request(Service::Signal, Some(Entry { handler_context: 1, ..task.unwrap() }), 0x1_0001),
        Err(SemaphoreStatus::Context)
    );
    assert_eq!(
        request(Service::Signal, Some(Entry { irq_masked: true, ..task.unwrap() }), 0x1_0001),
        Err(SemaphoreStatus::Context)
    );
    assert_eq!(
        request(Service::Signal, task, 0x3_0001),
        Err(SemaphoreStatus::InvalidId)
    );
    assert_eq!(
        request(Service::Signal, task, 0x1_0000),
        Err(SemaphoreStatus::InvalidId)
    );
    assert_eq!(
        request(Service::Signal, task, 0x1_00fb),
        Err(SemaphoreStatus::InvalidId)
    );
    assert_eq!(
        request(Service::Wait, Some(Entry { dispatch_enabled: 0, ..task.unwrap() }), 0x1_0001),
        Err(SemaphoreStatus::Context)
    );
    assert_eq!(
        request(Service::Poll, Some(Entry { dispatch_enabled: 0, ..task.unwrap() }), 0x1_0001)
            .map(|id| id.domain),
        Ok(Domain::A)
    );
    assert_eq!(
        request(Service::InterruptSignal, task, 0x1_0001),
        Err(SemaphoreStatus::Context)
    );
    assert_eq!(
        request(Service::InterruptSignal, Some(Entry { handler_context: 1, ..task.unwrap() }), 0x2_0001)
            .map(|id| id.domain),
        Ok(Domain::B)
    );
    assert_eq!(require_created(0), Err(SemaphoreStatus::NotCreated));
    assert_eq!(require_created(1), Ok(()));
}
