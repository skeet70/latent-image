use crate::admission::{admit, Counts, Entry, Request, Service};
use crate::create::{create_pool, PoolAttributes, PoolCreate};
use crate::get::{get, poll, poll_unchecked, resume, OutWord, PoolBlocking, PoolResume};
use crate::id::{EncodedId, EntryDomain, PoolId};
use crate::layout::{Domain, FixedPoolSlot, GuestAddress, TaskSlot, TaskState};
use crate::machine::{Block, Machine};
use crate::release::release;
use crate::sim::Sim;
use crate::status::{PoolStatus, WaitOutcome};
use crate::take::{take, BlockAddress};
use crate::teardown::delete_pool;
use crate::timeout::Timeout;

const BUFFER: u32 = 0x4000_0000;

fn sim(entry: Domain) -> Sim {
    let sim = Sim::new(entry);
    sim.install(Domain::A, true);
    sim.install(Domain::B, true);
    for slot in 1..=3 {
        let priority = match slot {
            1 => 10,
            2 => 5,
            3 => 3,
            _ => 15,
        };
        sim.install_task(Domain::A, task_slot(slot), priority);
        sim.install_task(Domain::B, task_slot(slot), priority);
    }
    sim.set_running(entry, sim.task(entry, task_slot(1)));
    sim.set_running(peer(entry), sim.task(peer(entry), task_slot(1)));
    sim
}

fn peer(domain: Domain) -> Domain {
    match domain {
        Domain::A => Domain::B,
        Domain::B => Domain::A,
    }
}

fn task_slot(slot: u32) -> TaskSlot {
    TaskSlot::new(slot).unwrap()
}

fn pool_slot(slot: u32) -> FixedPoolSlot {
    FixedPoolSlot::new(slot, 122).unwrap()
}

fn pool(domain: Domain, slot: u32) -> PoolId {
    PoolId {
        domain,
        slot: pool_slot(slot),
    }
}

fn create(count: u32, size: u32, buffer: u32) -> PoolCreate {
    PoolCreate {
        attributes: PoolAttributes(0),
        block_count: count,
        block_size: size,
        buffer: GuestAddress(buffer),
    }
}

fn install(sim: &Sim, domain: Domain, slot: u32, count: u32, size: u32, attributes: u32) {
    let mut request = create(count, size, BUFFER);
    request.attributes = PoolAttributes(attributes);
    sim.install_pool(domain, pool_slot(slot), request);
}

fn running(sim: &Sim, domain: Domain, slot: u32) {
    let task = sim.task(domain, task_slot(slot));
    sim.set_byte(task.state(), TaskState::READY.bits());
    sim.set_running(domain, task);
}

fn out(value: u32) -> OutWord {
    OutWord(GuestAddress(0x4000_1000 + value))
}

fn entry_state(_sim: &Sim, handler: u32, masked: bool, dispatch: u32) -> Entry {
    Entry {
        handler_context: handler,
        irq_masked: masked,
        dispatch_enabled: dispatch,
    }
}

fn counts() -> Counts {
    Counts { a: 122, b: 122 }
}

fn check(sim: &Sim, request: &Request) -> Result<crate::admission::Admitted, PoolStatus> {
    admit(request, |pool| {
        sim.word(pool.domain.words().fixed_pool_config(pool.slot).exists()) != 0
    })
}

#[test]
fn create_rounds_the_stride_and_uses_it_for_the_end() {
    let sim = sim(Domain::A);
    let request = create(3, 5, BUFFER);
    assert_eq!(request.stride(), 8);
    create_pool(&sim, pool(Domain::A, 1), request).unwrap();
    let words = Domain::A.words();
    let config = words.fixed_pool_config(pool_slot(1));
    let control = words.fixed_pool_control(pool_slot(1));
    assert_eq!(sim.word(config.attributes()), 0);
    assert_eq!(sim.word(config.block_size()), 8);
    assert_eq!(sim.word(config.begin()), BUFFER);
    assert_eq!(sim.word(config.end()), BUFFER + 3 * 8);
    assert_eq!(sim.word(config.exists()), 1);
    assert_eq!(sim.word(control.next_free()), BUFFER);
    assert_eq!(sim.word(control.recycled()), 0);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    assert_eq!(sim.word(control.queue().prev()), control.0 .0);
}

#[test]
fn create_refuses_an_existing_pool_without_writing() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 2, 8, 0);
    let config = Domain::A.words().fixed_pool_config(pool_slot(1));
    sim.set(config.attributes(), 7);
    assert_eq!(
        create_pool(&sim, pool(Domain::A, 1), create(3, 16, BUFFER + 0x100)),
        Err(PoolStatus::AlreadyExists)
    );
    assert_eq!(sim.word(config.attributes()), 7);
    assert_eq!(sim.word(config.block_size()), 8);
    assert_eq!(sim.word(config.begin()), BUFFER);
}

#[test]
fn create_refuses_an_out_of_range_id_before_any_write() {
    let sim = sim(Domain::A);
    let config = Domain::A.words().fixed_pool_config(pool_slot(1));
    let request = Request {
        service: Service::Create,
        entry: None,
        entry_domain: EntryDomain::new(Domain::A),
        id: EncodedId::of(3, 123),
        counts: counts(),
        timeout: 0,
    };
    assert_eq!(check(&sim, &request).unwrap_err(), PoolStatus::InvalidIndex);
    assert_eq!(sim.word(config.attributes()), 0);
    assert_eq!(sim.word(config.exists()), 0);
}

#[test]
fn the_allocator_bumps_then_refuses() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 2, 8, 0);
    let out0 = out(0);
    let out1 = out(1);
    let out2 = out(2);
    assert_eq!(poll(&sim, pool(Domain::A, 1), out0).unwrap(), WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER))));
    assert_eq!(poll(&sim, pool(Domain::A, 1), out1).unwrap(), WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER + 8))));
    sim.set(out2.0, 0xdead_beef);
    assert_eq!(poll(&sim, pool(Domain::A, 1), out2).unwrap(), WaitOutcome::TimedOut);
    assert_eq!(sim.word(out2.0), 0xdead_beef);
    assert_eq!(sim.word(out0.0), BUFFER);
    assert_eq!(sim.word(out1.0), BUFFER + 8);
}

#[test]
fn a_released_block_is_reused_last_in_first_out() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 2, 8, 0);
    poll(&sim, pool(Domain::A, 1), out(0)).unwrap();
    poll(&sim, pool(Domain::A, 1), out(1)).unwrap();
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER + 8))).unwrap();
    assert_eq!(poll(&sim, pool(Domain::A, 1), out(2)).unwrap(), WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER + 8))));
    assert_eq!(poll(&sim, pool(Domain::A, 1), out(3)).unwrap(), WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER))));
}

#[test]
fn release_hands_the_block_to_the_head_waiter() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    poll(&sim, pool(Domain::A, 1), out(9)).unwrap();
    running(&sim, Domain::A, 2);
    let pending = match get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Forever,
    )
    .unwrap()
    {
        PoolBlocking::Pending(pending) => pending,
        PoolBlocking::Ready(_) => panic!("the empty pool did not block"),
    };
    let wait = Domain::A.words().fixed_pool_wait(task_slot(2));
    assert_eq!(sim.load_byte(sim.task(Domain::A, task_slot(2)).state()) & TaskState::WAIT.bits(), TaskState::WAIT.bits());
    assert_eq!(sim.word(wait.base().queue()), Domain::A.words().fixed_pool_control(pool_slot(1)).0 .0);
    assert_eq!(sim.word(wait.base().owner()), Domain::A.id().word());
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    assert_eq!(sim.word(wait.block()), BUFFER);
    assert_eq!(sim.word(wait.base().result()), 0);
    assert_eq!(sim.word(out(0).0), 0);
    match resume(&sim, pending) {
        PoolResume::Completed(WaitOutcome::Satisfied(block)) => assert_eq!(block, BlockAddress(GuestAddress(BUFFER))),
        other => panic!("unexpected resume: {other:?}"),
    }
    assert_eq!(sim.word(out(0).0), BUFFER);
    assert_eq!(sim.interrupts() & 0x80, 0);
}

#[test]
fn a_timed_out_resume_leaves_the_output_untouched() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 0, 8, 0);
    running(&sim, Domain::A, 2);
    let pending = match get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Units(30),
    )
    .unwrap()
    {
        PoolBlocking::Pending(pending) => pending,
        PoolBlocking::Ready(_) => panic!("the empty pool did not block"),
    };
    let wait = Domain::A.words().fixed_pool_wait(task_slot(2));
    assert_eq!(sim.word(wait.base().result()), Domain::A.words().fixed_pool_timer(task_slot(2)).0);
    assert_eq!(sim.armed(Domain::A.words().fixed_pool_timer(task_slot(2))), Some(30));
    sim.set(wait.base().result(), (-50i32) as u32);
    sim.set_byte(sim.task(Domain::A, task_slot(2)).state(), TaskState::READY.bits());
    sim.set(out(0).0, 0xdead_beef);
    assert_eq!(resume(&sim, pending), PoolResume::Completed(WaitOutcome::TimedOut));
    assert_eq!(sim.word(out(0).0), 0xdead_beef);
    assert_eq!(sim.interrupts() & 0x80, 0);
}

#[test]
fn a_released_resume_leaves_the_output_untouched() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 0, 8, 0);
    running(&sim, Domain::A, 2);
    let pending = match get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Forever,
    )
    .unwrap()
    {
        PoolBlocking::Pending(pending) => pending,
        PoolBlocking::Ready(_) => panic!("the empty pool did not block"),
    };
    let wait = Domain::A.words().fixed_pool_wait(task_slot(2));
    sim.set(wait.base().result(), (-49i32) as u32);
    sim.set_byte(sim.task(Domain::A, task_slot(2)).state(), TaskState::READY.bits());
    sim.set(out(0).0, 0xdead_beef);
    assert_eq!(resume(&sim, pending), PoolResume::Completed(WaitOutcome::Released));
    assert_eq!(sim.word(out(0).0), 0xdead_beef);
}

#[test]
fn release_refuses_a_second_release_and_a_never_allocated_block() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 2, 8, 0);
    let control = Domain::A.words().fixed_pool_control(pool_slot(1));
    poll(&sim, pool(Domain::A, 1), out(0)).unwrap();
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    assert_eq!(
        release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))),
        Err(PoolStatus::Param)
    );
    assert_eq!(sim.word(control.recycled()), BUFFER);
    assert_eq!(sim.word(GuestAddress(BUFFER)), 0);
    assert_eq!(
        release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER + 8))),
        Err(PoolStatus::Param)
    );
    assert_eq!(sim.word(control.recycled()), BUFFER);
}

#[test]
fn release_refuses_out_of_range_and_misaligned_blocks() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 2, 8, 0);
    for address in [BUFFER - 4, BUFFER + 16, BUFFER + 4] {
        assert_eq!(
            release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(address))),
            Err(PoolStatus::Param)
        );
    }
}

#[test]
fn delete_releases_every_waiter_and_resets_the_record() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 0, 8, 0);
    running(&sim, Domain::A, 2);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Forever,
    )
    .unwrap();
    running(&sim, Domain::A, 3);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(1),
        Timeout::Forever,
    )
    .unwrap();
    delete_pool(&sim, pool(Domain::A, 1)).unwrap();
    let words = Domain::A.words();
    let config = words.fixed_pool_config(pool_slot(1));
    let control = words.fixed_pool_control(pool_slot(1));
    assert_eq!(sim.word(config.exists()), 0);
    assert_eq!(sim.word(config.attributes()), 0);
    assert_eq!(sim.word(config.block_size()), 4);
    assert_eq!(sim.word(config.begin()), words.default_pool_storage.0);
    assert_eq!(sim.word(config.end()), words.default_pool_storage.0 + 4);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    assert_eq!(sim.word(control.queue().prev()), control.0 .0);
    assert_eq!(sim.word(control.next_free()), words.default_pool_storage.0);
    assert_eq!(sim.word(control.recycled()), 0);
    for slot in [2, 3] {
        let task = sim.task(Domain::A, task_slot(slot));
        let wait = words.fixed_pool_wait(task_slot(slot));
        assert_eq!(sim.load_byte(task.state()), TaskState::READY.bits());
        assert_eq!(sim.word(wait.base().result()), (-49i32) as u32);
    }
    assert_eq!(sim.dispatched(), Some(0));
}

#[test]
fn delete_and_recreate_use_the_new_region() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    delete_pool(&sim, pool(Domain::A, 1)).unwrap();
    let request = create(2, 8, BUFFER + 0x100);
    create_pool(&sim, pool(Domain::A, 1), request).unwrap();
    assert_eq!(
        poll(&sim, pool(Domain::A, 1), out(0)).unwrap(),
        WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER + 0x100)))
    );
    assert_eq!(
        poll(&sim, pool(Domain::A, 1), out(1)).unwrap(),
        WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER + 0x108)))
    );
}

#[test]
fn the_unchecked_extension_refuses_a_deleted_pool() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    delete_pool(&sim, pool(Domain::A, 1)).unwrap();
    sim.set(out(0).0, 0xdead_beef);
    assert_eq!(
        poll_unchecked(&sim, pool(Domain::A, 1), out(0)),
        Err(PoolStatus::NotCreated)
    );
    assert_eq!(sim.word(out(0).0), 0xdead_beef);
}

#[test]
fn the_unchecked_extension_ignores_context_and_mask_but_not_exists() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    sim.set_interrupts(0xc0);
    assert_eq!(
        poll_unchecked(&sim, pool(Domain::A, 1), out(0)).unwrap(),
        WaitOutcome::Satisfied(BlockAddress(GuestAddress(BUFFER)))
    );
    assert_eq!(sim.word(out(0).0), BUFFER);
    assert_eq!(sim.interrupts(), 0xc0);
}

#[test]
fn admission_keeps_the_stock_order() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    let base = Request {
        service: Service::Get,
        entry: Some(entry_state(&sim, 0, false, 1)),
        entry_domain: EntryDomain::new(Domain::A),
        id: EncodedId::of(0, 1),
        counts: counts(),
        timeout: 0,
    };
    assert_eq!(check(&sim, &Request { entry: Some(entry_state(&sim, 1, false, 1)), ..base }).unwrap_err(), PoolStatus::Context);
    assert_eq!(check(&sim, &Request { entry: Some(entry_state(&sim, 0, true, 1)), ..base }).unwrap_err(), PoolStatus::Context);
    assert_eq!(check(&sim, &Request { entry: Some(entry_state(&sim, 0, false, 0)), ..base }).unwrap_err(), PoolStatus::Context);
    assert_eq!(check(&sim, &Request { id: EncodedId::of(3, 1), ..base }).unwrap_err(), PoolStatus::InvalidClass);
    assert_eq!(check(&sim, &Request { id: EncodedId::of(0, 0), ..base }).unwrap_err(), PoolStatus::InvalidIndex);
    assert_eq!(check(&sim, &Request { id: EncodedId::of(0, 123), ..base }).unwrap_err(), PoolStatus::InvalidIndex);
    let timed = Request {
        service: Service::TimedGet,
        timeout: -2,
        ..base
    };
    assert_eq!(check(&sim, &Request { id: EncodedId::of(0, 123), ..timed }).unwrap_err(), PoolStatus::InvalidIndex);
    assert_eq!(check(&sim, &timed).unwrap_err(), PoolStatus::Param);
    let polled = Request {
        service: Service::Poll,
        entry: Some(entry_state(&sim, 0, false, 0)),
        ..base
    };
    assert!(check(&sim, &polled).is_ok());
    let released = Request {
        service: Service::Release,
        entry: Some(entry_state(&sim, 0, false, 0)),
        ..base
    };
    assert!(check(&sim, &released).is_ok());
    let unchecked = Request {
        service: Service::PollUnchecked,
        entry: None,
        ..base
    };
    assert!(check(&sim, &unchecked).is_ok());
    let created = Request {
        service: Service::Create,
        entry: None,
        id: EncodedId::of(3, 1),
        ..base
    };
    assert_eq!(check(&sim, &created).unwrap().pool, pool(Domain::A, 1));
}

#[test]
fn waiters_are_ordered_by_the_creation_attribute() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 1);
    poll(&sim, pool(Domain::A, 1), out(9)).unwrap();
    running(&sim, Domain::A, 2);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Forever,
    )
    .unwrap();
    running(&sim, Domain::A, 3);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(1),
        Timeout::Forever,
    )
    .unwrap();
    let control = Domain::A.words().fixed_pool_control(pool_slot(1));
    assert_eq!(sim.word(control.queue().next()), sim.task(Domain::A, task_slot(3)).0 .0);
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    assert_eq!(sim.word(control.queue().next()), sim.task(Domain::A, task_slot(2)).0 .0);
    assert_eq!(sim.word(Domain::A.words().fixed_pool_wait(task_slot(3)).block()), BUFFER);
}

#[test]
fn fifo_waiters_keep_arrival_order() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    poll(&sim, pool(Domain::A, 1), out(9)).unwrap();
    running(&sim, Domain::A, 2);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Forever,
    )
    .unwrap();
    running(&sim, Domain::A, 3);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(1),
        Timeout::Forever,
    )
    .unwrap();
    let control = Domain::A.words().fixed_pool_control(pool_slot(1));
    assert_eq!(sim.word(control.queue().next()), sim.task(Domain::A, task_slot(2)).0 .0);
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    assert_eq!(sim.word(control.queue().next()), sim.task(Domain::A, task_slot(3)).0 .0);
    assert_eq!(sim.word(Domain::A.words().fixed_pool_wait(task_slot(2)).block()), BUFFER);
}

#[test]
fn a_cross_domain_wake_rings_the_waiter_core() {
    let sim = sim(Domain::B);
    install(&sim, Domain::A, 1, 1, 8, 0);
    poll(&sim, pool(Domain::A, 1), out(9)).unwrap();
    running(&sim, Domain::A, 2);
    sim.enqueue(
        Domain::A,
        Domain::A.id().word(),
        Domain::A.words().fixed_pool_control(pool_slot(1)).queue(),
        Domain::A.words().fixed_pool_wait(task_slot(2)),
        crate::machine::Enqueue::Plain,
    );
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    assert!(sim.doorbell(0));
    assert_eq!(sim.dispatched(), None);
    assert_eq!(sim.word(Domain::A.words().fixed_pool_wait(task_slot(2)).block()), BUFFER);
}

#[test]
fn a_release_with_an_armed_timer_cancels_it() {
    let sim = sim(Domain::A);
    install(&sim, Domain::A, 1, 1, 8, 0);
    poll(&sim, pool(Domain::A, 1), out(9)).unwrap();
    running(&sim, Domain::A, 2);
    get(
        &sim,
        EntryDomain::new(Domain::A),
        pool(Domain::A, 1),
        out(0),
        Timeout::Units(30),
    )
    .unwrap();
    let timer = Domain::A.words().fixed_pool_timer(task_slot(2));
    assert_eq!(sim.armed(timer), Some(30));
    release(&sim, pool(Domain::A, 1), BlockAddress(GuestAddress(BUFFER))).unwrap();
    assert_eq!(sim.armed(timer), None);
    assert_eq!(sim.word(Domain::A.words().fixed_pool_wait(task_slot(2)).base().result()), 0);
}

#[test]
fn an_uncreated_pool_has_no_block_and_admission_refuses_it() {
    let sim = sim(Domain::A);
    assert_eq!(take(&sim, pool(Domain::A, 1)), None);
    let request = Request {
        service: Service::Get,
        entry: Some(entry_state(&sim, 0, false, 1)),
        entry_domain: EntryDomain::new(Domain::A),
        id: EncodedId::of(0, 1),
        counts: counts(),
        timeout: 0,
    };
    assert_eq!(check(&sim, &request).unwrap_err(), PoolStatus::NotCreated);
}
