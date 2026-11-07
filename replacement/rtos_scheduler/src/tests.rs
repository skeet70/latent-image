use super::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Event {
    Word(GuestAddress, u32),
    Byte(GuestAddress, u8),
    Dmb,
    Dsb,
    Ring(IrqCore, Irq),
}

/// Word memory that panics on an unmapped access, a single-core exclusive claim, and a log of
/// stores, barriers and rings in program order.
struct Model {
    memory: RefCell<BTreeMap<u32, u32>>,
    /// Failed claims left before the other core releases a held word.
    held_for: Cell<u32>,
    claims: Cell<u32>,
    events: RefCell<Vec<Event>>,
}

impl Model {
    fn new() -> Self {
        Self { memory: RefCell::new(BTreeMap::new()), held_for: Cell::new(0), claims: Cell::new(0), events: RefCell::new(Vec::new()) }
    }

    fn map(&self, at: GuestAddress, value: u32) {
        assert_eq!(at.0 & 3, 0);
        self.memory.borrow_mut().insert(at.0, value);
    }

    fn word(&self, at: GuestAddress) -> u32 {
        *self.memory.borrow().get(&at.0).unwrap_or_else(|| panic!("unmapped word {:#x}", at.0))
    }

    fn byte(&self, at: GuestAddress) -> u8 {
        (self.word(GuestAddress(at.0 & !3)) >> ((at.0 & 3) * 8)) as u8
    }

    fn events(&self) -> Vec<Event> {
        self.events.borrow().clone()
    }

    fn clear_events(&self) {
        self.events.borrow_mut().clear();
    }
}

impl Machine for Model {
    fn load_word(&self, at: GuestAddress) -> u32 {
        assert_eq!(at.0 & 3, 0, "unaligned word load {:#x}", at.0);
        self.word(at)
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        self.byte(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        assert_eq!(at.0 & 3, 0, "unaligned word store {:#x}", at.0);
        self.word(at);
        self.memory.borrow_mut().insert(at.0, value);
        self.events.borrow_mut().push(Event::Word(at, value));
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        let word = GuestAddress(at.0 & !3);
        let shift = (at.0 & 3) * 8;
        let merged = (self.word(word) & !(0xff << shift)) | ((value as u32) << shift);
        self.memory.borrow_mut().insert(word.0, merged);
        self.events.borrow_mut().push(Event::Byte(at, value));
    }

    fn try_claim(&self, word: GuestAddress) -> bool {
        self.claims.set(self.claims.get() + 1);
        if self.word(word) != 0 {
            match self.held_for.get() {
                0 => {}
                1 => {
                    self.held_for.set(0);
                    self.memory.borrow_mut().insert(word.0, 0);
                }
                n => self.held_for.set(n - 1),
            }
            return false;
        }
        self.memory.borrow_mut().insert(word.0, 1);
        self.events.borrow_mut().push(Event::Word(word, 1));
        true
    }

    fn dmb(&self) {
        self.events.borrow_mut().push(Event::Dmb);
    }

    fn dsb(&self) {
        self.events.borrow_mut().push(Event::Dsb);
    }

    fn raise_irq(&self, core: IrqCore, irq: Irq) {
        self.events.borrow_mut().push(Event::Ring(core, irq));
    }
}

const DOMAINS: [Domain; 2] = [Domain::A, Domain::B];
const DISPATCH_WORDS: [u32; 4] = [0, 1, 2, 0x8000_0000];
const FIRST_ENTRY: u32 = 0x0004_3728;
const DEFINITIONS: GuestAddress = GuestAddress(0x0070_0000);
const STACK_TOP_BASE: u32 = 0x00a0_0000;

fn p(n: u32) -> Priority {
    Priority::new(n).unwrap()
}

/// One domain's record, heads, bitmap, cells and tasks 1..=8 at their stock addresses, each task
/// with a definition of priority 15.
struct World {
    m: Model,
    words: &'static layout::DomainWords,
    record: KernelContext,
}

impl World {
    fn new(domain: Domain) -> Self {
        let m = Model::new();
        let words = domain.words();
        let record = words.context(domain);
        for offset in (0..KernelContext::SIZE).step_by(4) {
            m.map(record.0.offset(offset), 0);
        }
        m.map(record.ready_heads(), words.ready_heads.0);
        m.map(record.ready_bitmap(), words.ready_bitmap.0);
        m.map(record.running(), words.running.0);
        m.map(record.selected(), words.selected.0);
        m.map(record.dispatch_enabled(), words.dispatch_enabled.0);
        m.map(record.first_entry(), FIRST_ENTRY);
        for n in 0..Priority::COUNT {
            let head = words.ready_head(p(n));
            m.map(head.next(), head.0 .0);
            m.map(head.prev(), head.0 .0);
        }
        for cell in [words.ready_bitmap, words.running, words.selected, words.dispatch_enabled] {
            m.map(cell, 0);
        }
        for slot in 1..=8 {
            let task = Self::task_of(&words, slot);
            for offset in (0..Task::SIZE).step_by(4) {
                m.map(task.0.offset(offset), 0);
            }
            let definition = Self::definition_of(slot);
            for offset in (0..0x20).step_by(4) {
                m.map(definition.0.offset(offset), 0);
            }
            m.map(definition.priority(), 0xffff_ff0f);
            m.map(definition.initial_sp_terms()[0], 0x800 * slot);
            m.map(definition.initial_sp_terms()[1], STACK_TOP_BASE);
            m.map(task.definition(), definition.0 .0);
        }
        Self { m, words, record }
    }

    fn task_of(words: &layout::DomainWords, slot: u32) -> Task {
        words.task(layout::TaskSlot::new(slot).unwrap())
    }

    fn definition_of(slot: u32) -> TaskDefinition {
        TaskDefinition(DEFINITIONS.offset(slot * 0x20))
    }

    fn task(&self, slot: u32) -> Task {
        Self::task_of(&self.words, slot)
    }

    fn slot(&self, word: u32) -> Option<u32> {
        if word == 0 {
            return None;
        }
        Some(self.words.task_slot(Task(GuestAddress(word))).expect("selected cell is a task").get())
    }

    /// Links `lists` (priority, slots in order) with each task's priority byte and state READY,
    /// sets the bitmap, the selected cell and the dispatch-enabled word.
    fn ready(&self, lists: &[(u32, &[u32])], selected: Option<u32>, dispatch: u32) {
        let mut bitmap = 0;
        for &(priority, slots) in lists {
            let head = self.words.ready_head(p(priority));
            let mut prev = head.0 .0;
            for &slot in slots {
                let task = self.task(slot);
                self.m.store_byte(task.priority(), priority as u8);
                self.m.store_byte(task.state(), TaskState::READY.bits());
                self.m.store_word(task.prev(), prev);
                self.m.store_word(GuestAddress(prev), task.0 .0);
                prev = task.0 .0;
            }
            self.m.store_word(GuestAddress(prev), head.0 .0);
            self.m.store_word(head.prev(), prev);
            if !slots.is_empty() {
                bitmap |= 1 << priority;
            }
        }
        self.m.store_word(self.words.ready_bitmap, bitmap);
        self.m.store_word(self.words.selected, selected.map_or(0, |s| self.task(s).0 .0));
        self.m.store_word(self.words.dispatch_enabled, dispatch);
        self.m.clear_events();
    }

    /// Every non-empty list as (priority, slots), walked forward and checked backward.
    fn lists(&self) -> Vec<(u32, Vec<u32>)> {
        let mut lists = Vec::new();
        for n in 0..Priority::COUNT {
            let head = self.words.ready_head(p(n));
            let mut slots = Vec::new();
            let mut prev = head.0 .0;
            let mut at = self.m.word(head.next());
            while at != head.0 .0 {
                assert!(slots.len() < 8, "list {n} does not close");
                let task = Task(GuestAddress(at));
                assert_eq!(self.m.word(task.prev()), prev, "list {n} back link");
                slots.push(self.slot(at).unwrap());
                prev = at;
                at = self.m.word(task.next());
            }
            assert_eq!(self.m.word(head.prev()), prev, "list {n} tail");
            if !slots.is_empty() {
                lists.push((n, slots));
            }
        }
        lists
    }

    fn bitmap(&self) -> u32 {
        self.m.word(self.words.ready_bitmap)
    }

    fn selected(&self) -> Option<u32> {
        self.slot(self.m.word(self.words.selected))
    }

    fn find(&self) -> Task {
        find(&self.m, self.record)
    }

    fn insert(&self, slot: u32) -> Reschedule {
        insert(&self.m, self.record, self.task(slot))
    }

    fn remove(&self, slot: u32) -> Reschedule {
        remove(&self.m, self.record, self.task(slot), || self.find())
    }

    fn requeue(&self, slot: u32, to: u32) -> Reschedule {
        requeue(&self.m, self.record, self.task(slot), p(to), || self.find())
    }

    fn rotate(&self, priority: u32) -> Reschedule {
        rotate(&self.m, self.record, p(priority))
    }

    fn prepare(&self, slot: u32) -> Reschedule {
        prepare(&self.m, self.record, self.task(slot), || self.insert(slot))
    }

    fn state(&self, slot: u32) -> u8 {
        self.m.byte(self.task(slot).state())
    }

    fn priority(&self, slot: u32) -> u8 {
        self.m.byte(self.task(slot).priority())
    }
}

fn owned(lists: &[(u32, &[u32])]) -> Vec<(u32, Vec<u32>)> {
    lists.iter().map(|&(n, slots)| (n, slots.to_vec())).collect()
}

#[test]
fn acquire_spins_until_the_word_is_free_then_orders_with_dmb() {
    for domain in DOMAINS {
        let world = World::new(domain);
        let m = &world.m;
        let lock = world.record.scheduler_lock();
        m.map(lock.word(), world.words.scheduler_lock.0);
        m.map(world.words.scheduler_lock, 1);
        m.held_for.set(3);
        assert_eq!(acquire(m, lock), Acquired::OK);
        assert_eq!(m.claims.get(), 4);
        assert_eq!(m.word(world.words.scheduler_lock), 1);
        assert_eq!(m.events(), [Event::Word(world.words.scheduler_lock, 1), Event::Dmb]);
    }
}

#[test]
fn release_stores_zero_between_dmb_and_dsb() {
    let world = World::new(Domain::A);
    let m = &world.m;
    let lock = world.record.object_lock();
    m.map(lock.word(), world.words.object_lock.0);
    m.map(world.words.object_lock, 1);
    release(m, lock);
    assert_eq!(m.events(), [Event::Dmb, Event::Word(world.words.object_lock, 0), Event::Dsb]);
    assert_eq!(m.claims.get(), 0);
}

#[test]
fn find_returns_the_first_task_of_the_most_urgent_list() {
    for domain in DOMAINS {
        let world = World::new(domain);
        world.ready(&[(0, &[1, 2]), (3, &[3]), (9, &[4, 5]), (15, &[6])], Some(1), 1);
        assert_eq!(world.find(), world.task(1));
        world.ready(&[(3, &[3]), (9, &[4, 5]), (15, &[6])], Some(3), 1);
        assert_eq!(world.find(), world.task(3));
        world.ready(&[(9, &[5, 4]), (15, &[6])], Some(5), 1);
        assert_eq!(world.find(), world.task(5));
        for n in 0..Priority::COUNT {
            world.ready(&[(n, &[7, 8])], Some(7), 1);
            assert_eq!(world.find(), world.task(7), "priority {n}");
        }
        assert_eq!(world.m.events(), []);
    }
}

#[test]
fn insert_appends_marks_ready_and_selects_when_nothing_is_selected() {
    for dispatch in DISPATCH_WORDS {
        let world = World::new(Domain::B);
        world.ready(&[(4, &[1])], None, dispatch);
        world.m.store_byte(world.task(2).priority(), 4);
        world.m.store_byte(world.task(2).state(), 0x06);
        assert_eq!(world.insert(2), Reschedule(dispatch));
        assert_eq!(world.lists(), owned(&[(4, &[1, 2])]));
        assert_eq!(world.state(2), TaskState::READY.bits());
        assert_eq!(world.bitmap(), 1 << 4);
        assert_eq!(world.selected(), Some(2));
    }
}

#[test]
fn insert_preempts_only_a_less_urgent_selection() {
    for (priority, preempts) in [(2, true), (5, false), (9, false)] {
        let world = World::new(Domain::A);
        world.ready(&[(5, &[1]), (9, &[3])], Some(1), 0x8000_0000);
        world.m.store_byte(world.task(2).priority(), priority);
        let result = world.insert(2);
        assert_eq!(world.selected(), Some(if preempts { 2 } else { 1 }), "priority {priority}");
        assert_eq!(result, if preempts { Reschedule(0x8000_0000) } else { Reschedule::NONE });
        assert_eq!(world.bitmap(), (1 << 5) | (1 << 9) | (1 << priority));
    }
}

#[test]
fn insert_writes_in_stock_order() {
    let world = World::new(Domain::A);
    world.ready(&[(7, &[1, 3])], Some(1), 1);
    world.m.store_byte(world.task(2).priority(), 6);
    world.m.clear_events();
    let (t, head, tail) = (world.task(2), world.words.ready_head(p(6)), world.task(3));
    world.insert(2);
    assert_eq!(
        world.m.events(),
        [
            Event::Byte(t.state(), 1),
            Event::Word(t.prev(), head.0 .0),
            Event::Word(t.next(), head.0 .0),
            Event::Word(head.next(), t.0 .0),
            Event::Word(head.prev(), t.0 .0),
            Event::Word(world.words.ready_bitmap, (1 << 7) | (1 << 6)),
            Event::Word(world.words.selected, t.0 .0),
        ]
    );
    world.ready(&[(7, &[1, 3])], Some(1), 1);
    world.m.store_byte(world.task(2).priority(), 7);
    world.m.clear_events();
    world.insert(2);
    assert_eq!(
        world.m.events(),
        [
            Event::Byte(t.state(), 1),
            Event::Word(t.prev(), tail.0 .0),
            Event::Word(t.next(), world.words.ready_head(p(7)).0 .0),
            Event::Word(tail.next(), t.0 .0),
            Event::Word(world.words.ready_head(p(7)).prev(), t.0 .0),
            Event::Word(world.words.ready_bitmap, 1 << 7),
        ]
    );
}

#[test]
fn remove_of_an_unselected_task_unlinks_and_requests_nothing() {
    let world = World::new(Domain::A);
    world.ready(&[(2, &[1, 2, 3]), (8, &[4])], Some(1), 1);
    assert_eq!(world.remove(2), Reschedule::NONE);
    assert_eq!(world.lists(), owned(&[(2, &[1, 3]), (8, &[4])]));
    assert_eq!(world.remove(4), Reschedule::NONE);
    assert_eq!(world.lists(), owned(&[(2, &[1, 3])]));
    assert_eq!(world.bitmap(), 1 << 2);
    assert_eq!(world.selected(), Some(1));
}

#[test]
fn remove_of_the_selected_task_selects_the_next_ready_task() {
    for dispatch in DISPATCH_WORDS {
        let world = World::new(Domain::B);
        world.ready(&[(2, &[1, 2]), (8, &[4])], Some(1), dispatch);
        assert_eq!(world.remove(1), Reschedule(dispatch));
        assert_eq!(world.selected(), Some(2));
        assert_eq!(world.remove(2), Reschedule(dispatch));
        assert_eq!(world.selected(), Some(4));
        assert_eq!(world.bitmap(), 1 << 8);
        assert_eq!(world.remove(4), Reschedule(dispatch));
        assert_eq!(world.selected(), None);
        assert_eq!(world.bitmap(), 0);
        assert_eq!(world.lists(), owned(&[]));
    }
}

#[test]
fn remove_of_a_selected_task_behind_the_head_selects_the_list_head() {
    let world = World::new(Domain::A);
    world.ready(&[(3, &[1, 2, 5])], Some(2), 1);
    assert_eq!(world.remove(2), Reschedule(1));
    assert_eq!(world.selected(), Some(1));
}

#[test]
fn remove_leaves_the_task_state_and_links() {
    let world = World::new(Domain::A);
    world.ready(&[(3, &[1, 2])], Some(1), 1);
    let t = world.task(2);
    let (next, prev) = (world.m.word(t.next()), world.m.word(t.prev()));
    world.remove(2);
    assert_eq!((world.m.word(t.next()), world.m.word(t.prev()), world.state(2)), (next, prev, 1));
    assert!(world.m.events().iter().all(|e| !matches!(e, Event::Byte(..))));
}

#[test]
fn reset_restores_the_definition_priority_and_clears_the_pending_fields() {
    for domain in DOMAINS {
        let world = World::new(domain);
        let t = world.task(5);
        world.m.store_byte(t.priority(), 3);
        world.m.store_byte(t.state(), 0x36);
        world.m.store_byte(t.flags(), 0xff);
        for field in [t.exception_pattern(), t.priority_change_pending(), t.terminate_pending(), t.cancel_pending()] {
            world.m.store_word(field, 0xffff_ffff);
        }
        world.m.store_word(t.wait(), 0x1234);
        world.m.clear_events();
        reset(&world.m, t);
        assert_eq!(
            world.m.events(),
            [
                Event::Byte(t.priority(), 0x0f),
                Event::Byte(t.state(), 0),
                Event::Byte(t.flags(), 0xf9),
                Event::Word(t.exception_pattern(), 0),
                Event::Word(t.priority_change_pending(), 0),
                Event::Word(t.terminate_pending(), 0),
                Event::Word(t.cancel_pending(), 0),
            ]
        );
        assert_eq!(world.m.word(t.wait()), 0x1234);
    }
}

#[test]
fn prepare_sets_the_initial_frame_then_dmb_then_inserts() {
    for domain in DOMAINS {
        let world = World::new(domain);
        world.ready(&[(9, &[1])], Some(1), 2);
        world.m.store_byte(world.task(6).priority(), 4);
        world.m.clear_events();
        let t = world.task(6);
        assert_eq!(world.prepare(6), Reschedule(2));
        let events = world.m.events();
        assert_eq!(
            events[..3],
            [Event::Word(t.saved_sp(), STACK_TOP_BASE + 0x800 * 6), Event::Word(t.resume_pc(), FIRST_ENTRY), Event::Dmb]
        );
        assert_eq!(events.last(), Some(&Event::Word(world.words.selected, t.0 .0)));
        assert_eq!(world.lists(), owned(&[(4, &[6]), (9, &[1])]));
    }
}

#[test]
fn prepare_returns_insert_without_a_reschedule() {
    let world = World::new(Domain::A);
    world.ready(&[(1, &[1])], Some(1), 1);
    world.m.store_byte(world.task(6).priority(), 4);
    assert_eq!(world.prepare(6), Reschedule::NONE);
    assert_eq!(world.selected(), Some(1));
}

#[test]
fn notify_rings_core_id_asr_1_after_dsb_for_remote_ids() {
    let ids = [0, 1, 2, 3, 4, 5, 6, 0x7fff_ffff, 0x8000_0000, 0x8000_0002, 0xffff_fffd, 0xffff_ffff];
    let local = |domain: Domain, id: u32| match domain {
        Domain::A => id <= 1,
        Domain::B => id == 0 || id == 2,
    };
    for domain in DOMAINS {
        for id in ids {
            let m = Model::new();
            let result = notify(&m, domain, RecordId(id));
            if local(domain, id) {
                assert_eq!((result, m.events()), (Notified::Local, vec![]), "{domain:?} {id:#x}");
            } else {
                let core = IrqCore(((id as i32) >> 1) as u32);
                assert_eq!(
                    (result, m.events()),
                    (Notified::Remote, vec![Event::Dsb, Event::Ring(core, Irq(120))]),
                    "{domain:?} {id:#x}"
                );
            }
        }
    }
}

#[test]
fn notify_rings_the_other_domain_by_its_id() {
    let rung = |domain: Domain, id: u32| {
        let m = Model::new();
        notify(&m, domain, RecordId(id));
        m.events()
    };
    assert_eq!(rung(Domain::A, 2), [Event::Dsb, Event::Ring(IrqCore(1), Irq::DOORBELL)]);
    assert_eq!(rung(Domain::B, 1), [Event::Dsb, Event::Ring(IrqCore(0), Irq::DOORBELL)]);
}

#[test]
fn requeue_moves_the_task_to_the_tail_of_its_new_list() {
    let world = World::new(Domain::A);
    world.ready(&[(3, &[1, 2]), (6, &[4, 5])], Some(1), 1);
    assert_eq!(world.requeue(2, 6), Reschedule::NONE);
    assert_eq!(world.lists(), owned(&[(3, &[1]), (6, &[4, 5, 2])]));
    assert_eq!(world.priority(2), 6);
    assert_eq!(world.requeue(4, 6), Reschedule::NONE);
    assert_eq!(world.lists(), owned(&[(3, &[1]), (6, &[5, 2, 4])]));
    assert_eq!(world.requeue(1, 10), Reschedule(1));
    assert_eq!(world.lists(), owned(&[(6, &[5, 2, 4]), (10, &[1])]));
    assert_eq!(world.bitmap(), (1 << 6) | (1 << 10));
    assert_eq!(world.selected(), Some(5));
}

#[test]
fn requeue_keeps_a_selected_task_that_became_more_urgent() {
    let world = World::new(Domain::B);
    world.ready(&[(5, &[1]), (8, &[2])], Some(1), 0x8000_0000);
    assert_eq!(world.requeue(1, 2), Reschedule::NONE);
    assert_eq!(world.selected(), Some(1));
    assert_eq!(world.lists(), owned(&[(2, &[1]), (8, &[2])]));
}

#[test]
fn requeue_of_the_selected_task_reports_whether_dispatch_is_enabled() {
    for (dispatch, expected) in [(0, 0), (1, 1), (2, 1), (0x8000_0000, 1)] {
        let world = World::new(Domain::A);
        world.ready(&[(5, &[1]), (8, &[2])], Some(1), dispatch);
        assert_eq!(world.requeue(1, 9), Reschedule(expected), "dispatch {dispatch:#x}");
        assert_eq!(world.selected(), Some(2));
    }
}

#[test]
fn requeue_of_the_selected_task_at_its_own_priority_yields_to_its_list() {
    let world = World::new(Domain::B);
    world.ready(&[(5, &[1, 2])], Some(1), 2);
    assert_eq!(world.requeue(1, 5), Reschedule(1));
    assert_eq!(world.lists(), owned(&[(5, &[2, 1])]));
    assert_eq!(world.selected(), Some(2));
}

#[test]
fn requeue_of_the_selected_task_that_stays_first_requests_nothing() {
    for to in [5, 7] {
        let world = World::new(Domain::A);
        world.ready(&[(5, &[1]), (8, &[2])], Some(1), 1);
        assert_eq!(world.requeue(1, to), Reschedule::NONE, "to {to}");
        assert_eq!(world.selected(), Some(1));
    }
}

#[test]
fn requeue_of_another_task_preempts_only_when_more_urgent() {
    for (to, preempts) in [(3, true), (5, false), (7, false)] {
        let world = World::new(Domain::B);
        world.ready(&[(5, &[1]), (8, &[2])], Some(1), 0x8000_0000);
        let result = world.requeue(2, to);
        assert_eq!(world.selected(), Some(if preempts { 2 } else { 1 }), "to {to}");
        assert_eq!(result, if preempts { Reschedule(0x8000_0000) } else { Reschedule::NONE });
    }
}

#[test]
fn requeue_writes_in_stock_order() {
    let world = World::new(Domain::A);
    world.ready(&[(3, &[1]), (6, &[2])], Some(2), 1);
    let (t, old, new, tail) = (world.task(1), world.words.ready_head(p(3)), world.words.ready_head(p(6)), world.task(2));
    world.requeue(1, 6);
    assert_eq!(
        world.m.events(),
        [
            Event::Byte(t.priority(), 6),
            Event::Word(old.next(), old.0 .0),
            Event::Word(old.prev(), old.0 .0),
            Event::Word(world.words.ready_bitmap, 1 << 6),
            Event::Word(t.prev(), tail.0 .0),
            Event::Word(t.next(), new.0 .0),
            Event::Word(tail.next(), t.0 .0),
            Event::Word(new.prev(), t.0 .0),
            Event::Word(world.words.ready_bitmap, 1 << 6),
        ]
    );
}

#[test]
fn rotate_moves_the_first_task_to_the_tail() {
    let world = World::new(Domain::A);
    world.ready(&[(4, &[1, 2, 3]), (0, &[5])], Some(5), 1);
    assert_eq!(world.rotate(4), Reschedule::NONE);
    assert_eq!(world.lists(), owned(&[(0, &[5]), (4, &[2, 3, 1])]));
    assert_eq!(world.selected(), Some(5));
}

#[test]
fn rotate_of_the_selected_first_task_selects_the_next() {
    for dispatch in DISPATCH_WORDS {
        let world = World::new(Domain::B);
        world.ready(&[(4, &[1, 2])], Some(1), dispatch);
        assert_eq!(world.rotate(4), Reschedule(dispatch));
        assert_eq!(world.lists(), owned(&[(4, &[2, 1])]));
        assert_eq!(world.selected(), Some(2));
    }
}

#[test]
fn rotate_of_a_short_list_does_nothing() {
    let world = World::new(Domain::A);
    world.ready(&[(4, &[1])], Some(1), 1);
    assert_eq!(world.rotate(4), Reschedule::NONE);
    assert_eq!(world.rotate(7), Reschedule::NONE);
    assert_eq!(world.m.events(), []);
}

#[test]
fn rotate_writes_in_stock_order() {
    let world = World::new(Domain::A);
    world.ready(&[(4, &[1, 2, 3])], Some(1), 1);
    let head = world.words.ready_head(p(4));
    let (first, second, tail) = (world.task(1), world.task(2), world.task(3));
    world.rotate(4);
    assert_eq!(
        world.m.events(),
        [
            Event::Word(head.next(), second.0 .0),
            Event::Word(second.prev(), head.0 .0),
            Event::Word(first.prev(), tail.0 .0),
            Event::Word(first.next(), head.0 .0),
            Event::Word(tail.next(), first.0 .0),
            Event::Word(head.prev(), first.0 .0),
            Event::Word(world.words.selected, second.0 .0),
        ]
    );
}

#[test]
fn sequences_keep_the_bitmap_and_selection_consistent() {
    let world = World::new(Domain::A);
    world.ready(&[], None, 1);
    for (slot, priority) in [(1, 9), (2, 4), (3, 4), (4, 12), (5, 0)] {
        world.m.store_byte(world.task(slot).priority(), priority);
        world.insert(slot);
    }
    world.rotate(4);
    world.requeue(5, 4);
    world.remove(2);
    world.requeue(4, 1);
    let lists = world.lists();
    assert_eq!(lists, owned(&[(1, &[4]), (4, &[3, 5]), (9, &[1])]));
    assert_eq!(world.bitmap(), lists.iter().map(|(n, _)| 1 << n).sum::<u32>());
    assert_eq!(world.selected(), Some(4));
}

#[test]
fn priority_keeps_the_low_four_bits() {
    assert_eq!([0, 15, 16, 0x1f, 32].map(|raw| priority(raw).get()), [0, 15, 0, 15, 0]);
}
