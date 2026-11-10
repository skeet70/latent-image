use super::*;
use core::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use crate::layout::{GuestAddress, KernelContext, LockRecord};

const NODES: u32 = 0x2000;
const COUNT: u32 = 0x3000;
const BASE: u32 = 0x3004;
const TICK: u32 = 0x3008;
const CONTEXT: u32 = 0x1000;
const SECOND_CONTEXT: u32 = 0x1100;
const SECOND_NODES: u32 = 0x7000;
const LOCK_CELL: u32 = 0x1200;
const SECOND_LOCK_CELL: u32 = 0x1204;


#[test]
fn cold_storage_publication_initializes_both_domains_without_clearing_entries() {
    for domain in [layout::Domain::A, layout::Domain::B] {
        let mock = Mock::new();
        let configuration = GuestAddress(0x9000);
        let words = domain.words();
        let arena = domain.timer_arena();
        mock.set_word(words.timer_count.0, 184);
        mock.set_word(words.timer_base.0, 123);
        mock.set_word(words.tick_now.0, 456);
        mock.set_word(words.time_offset.0, 789);
        mock.set_word(arena.guest.start(), 0xfeed);
        initialize(&mock, domain, configuration);
        assert_eq!(mock.word(words.timer_count.0), 0);
        assert_eq!(mock.word(words.timer_base.0), 0);
        assert_eq!(mock.word(words.tick_now.0), 10);
        assert_eq!(mock.word(words.time_offset.0), 0);
        assert_eq!(mock.word(configuration.offset(0x20).0), arena.guest.start());
        assert_eq!(mock.word(configuration.offset(0x24).0), words.timer_count.0);
        assert_eq!(mock.word(configuration.offset(0x28).0), words.timer_base.0);
        assert_eq!(mock.word(configuration.offset(0x2c).0), words.tick_now.0);
        assert_eq!(mock.word(arena.guest.start()), 0xfeed);
    }
}
struct Mock {
    words: RefCell<BTreeMap<u32, u32>>,
    held: RefCell<Vec<u32>>,
    masked: Cell<bool>,
    handler: Cell<u32>,
    context: Cell<u32>,
    events: RefCell<Vec<(u32, u32)>>,
}

impl Mock {
    fn new() -> Self {
        let mock = Self {
            words: RefCell::new(BTreeMap::new()),
            held: RefCell::new(Vec::new()),
            masked: Cell::new(false),
            handler: Cell::new(1),
            context: Cell::new(CONTEXT),
            events: RefCell::new(Vec::new()),
        };
        let context = KernelContext(GuestAddress(CONTEXT));
        mock.store_word(context.timer_heap(), NODES);
        mock.store_word(context.timer_count(), COUNT);
        mock.store_word(context.timer_base(), BASE);
        mock.store_word(context.tick_now(), TICK);
        mock.store_word(context.scheduler_lock().word(), LOCK_CELL);
        mock.set_count(0);
        mock.set_word(BASE, 0);
        mock.set_word(TICK, 0);
        mock
    }

    fn heap(&self, capacity: u32) -> TimerHeap {
        TimerHeap::from_context(
            self,
            KernelContext(GuestAddress(self.context.get())),
            Capacity::new(capacity).unwrap(),
        )
    }

    fn second_context(&self) {
        let context = KernelContext(GuestAddress(SECOND_CONTEXT));
        self.set_word(context.timer_heap().0, SECOND_NODES);
        self.set_word(context.timer_count().0, COUNT);
        self.set_word(context.timer_base().0, BASE);
        self.set_word(context.tick_now().0, TICK);
        self.set_word(context.scheduler_lock().word().0, LOCK_CELL);
    }

    fn second_lock_cell(&self) {
        let context = KernelContext(GuestAddress(SECOND_CONTEXT));
        self.set_word(context.scheduler_lock().word().0, SECOND_LOCK_CELL);
    }

    fn lock_cell(&self, lock: LockRecord) -> u32 {
        self.word(lock.word().0)
    }

    fn current_lock(&self) -> LockRecord {
        KernelContext(GuestAddress(self.context.get())).scheduler_lock()
    }

    fn held(&self) -> Vec<u32> {
        self.held.borrow().clone()
    }

    fn set_word(&self, at: u32, value: u32) {
        self.words.borrow_mut().insert(at, value);
    }

    fn word(&self, at: u32) -> u32 {
        *self.words.borrow().get(&at).unwrap_or(&0)
    }

    fn set_count(&self, value: u32) {
        self.set_word(COUNT, value);
    }

    fn count(&self) -> u32 {
        self.word(COUNT)
    }

    fn entry(&self, slot: u32) -> TimerEntry {
        let address = 0x5000 + slot * 0x10;
        self.set_word(address, 0);
        self.set_word(address + 4, 0xc000 + slot);
        self.set_word(address + 8, 0xa000 + slot);
        TimerEntry::at(GuestAddress(address))
    }

    fn timed_entry(&self, slot: u32, callback: u32) -> TimerEntry {
        let entry = self.entry(slot);
        self.set_word(entry.address().0 + 4, callback);
        entry
    }

    fn index(&self, entry: TimerEntry) -> u32 {
        self.word(entry.address().0)
    }
}

impl Machine for Mock {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at.0)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.set_word(at.0, value);
    }
}

impl Scheduler for Mock {
    fn handler_context(&self) -> u32 {
        self.handler.get()
    }

    fn irq_masked(&self) -> bool {
        self.masked.get()
    }

    fn release_lock(&self, lock: LockRecord) {
        let cell = self.lock_cell(lock);
        let position = self.held.borrow().iter().position(|held| *held == cell);
        assert!(position.is_some(), "released a lock this machine does not hold");
        self.held.borrow_mut().remove(position.unwrap());
    }

    fn acquire_lock(&self, lock: LockRecord) {
        let cell = self.lock_cell(lock);
        assert!(
            !self.held.borrow().contains(&cell),
            "acquired a lock this machine already holds"
        );
        self.held.borrow_mut().push(cell);
    }

    fn irq_clear(&self) {
        self.masked.set(false);
    }

    fn irq_set(&self) {
        self.masked.set(true);
    }
}

fn tick_all(mock: &Mock, heap: TimerHeap) -> Vec<(u32, u32)> {
    tick(mock, |_| heap, |m| m.current_lock(), |m, callback, argument| {
        m.events.borrow_mut().push((callback.0, argument.0));
    })
    .unwrap();
    mock.events.borrow().clone()
}

#[test]
fn ordered_callbacks_with_distinct_arguments() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let e30 = mock.entry(1);
    let e10 = mock.entry(2);
    let e20 = mock.entry(3);
    insert(&mock, heap, e30, Expiry(30)).unwrap();
    insert(&mock, heap, e10, Expiry(10)).unwrap();
    insert(&mock, heap, e20, Expiry(20)).unwrap();
    mock.set_word(TICK, 30);
    let events = tick_all(&mock, heap);
    assert_eq!(events, vec![(0xc000 + 2, 0xa000 + 2), (0xc000 + 3, 0xa000 + 3), (0xc000 + 1, 0xa000 + 1)]);
    assert_eq!(mock.count(), 0);
}

#[test]
fn middle_remove_and_reinsert_keep_order_and_membership() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let e10 = mock.entry(1);
    let e20 = mock.entry(2);
    let e30 = mock.entry(3);
    insert(&mock, heap, e10, Expiry(10)).unwrap();
    insert(&mock, heap, e20, Expiry(20)).unwrap();
    insert(&mock, heap, e30, Expiry(30)).unwrap();
    assert_eq!(remove(&mock, heap, e20), Ok(Removed::Detached));
    assert_eq!(mock.index(e20), 0);
    insert(&mock, heap, e20, Expiry(20)).unwrap();
    assert_ne!(mock.index(e20), 0);
    mock.set_word(TICK, 30);
    let events = tick_all(&mock, heap);
    assert_eq!(events, vec![(0xc000 + 1, 0xa000 + 1), (0xc000 + 2, 0xa000 + 2), (0xc000 + 3, 0xa000 + 3)]);
}

#[test]
fn last_entry_and_empty_detached_cancel_change_nothing() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let only = mock.entry(1);
    insert(&mock, heap, only, Expiry(10)).unwrap();
    assert_eq!(remove(&mock, heap, only), Ok(Removed::Detached));
    assert_eq!(mock.count(), 0);
    assert_eq!(mock.index(only), 0);
    let before = mock.word(COUNT);
    assert_eq!(remove(&mock, heap, only), Ok(Removed::Noop));
    assert_eq!(mock.word(COUNT), before);
}

#[test]
fn pop_then_competing_cancel_is_a_noop_and_the_other_entry_survives() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let first = mock.entry(1);
    let other = mock.entry(2);
    insert(&mock, heap, first, Expiry(10)).unwrap();
    insert(&mock, heap, other, Expiry(20)).unwrap();
    mock.set_word(TICK, 10);
    let popped = first;
    tick(&mock, |_| heap, |m| m.current_lock(), |m, _callback, _argument| {
        assert_eq!(remove(m, heap, popped), Ok(Removed::Noop));
    })
    .unwrap();
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.index(other), 1);
}

#[test]
fn callback_rearm_keeps_fresh_membership() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let entry = mock.entry(1);
    insert(&mock, heap, entry, Expiry(10)).unwrap();
    mock.set_word(TICK, 10);
    tick(&mock, |_| heap, |m| m.current_lock(), |m, _callback, _argument| {
        assert!(!m.held().is_empty());
        insert(m, heap, entry, Expiry(40)).unwrap();
    })
    .unwrap();
    assert_eq!(mock.count(), 1);
    assert_ne!(mock.index(entry), 0);
    mock.set_word(TICK, 40);
    let events = tick_all(&mock, heap);
    assert_eq!(events, vec![(0xc000 + 1, 0xa000 + 1)]);
}

#[test]
fn wrap_relative_order_pops_in_order() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let low = mock.entry(1);
    let high = mock.entry(2);
    mock.set_word(BASE, 0xfffffff0);
    mock.set_word(TICK, 0x00000020);
    insert(&mock, heap, high, Expiry(0x00000000)).unwrap();
    insert(&mock, heap, low, Expiry(0xfffffff5)).unwrap();
    let events = tick_all(&mock, heap);
    assert_eq!(events, vec![(0xc000 + 1, 0xa000 + 1), (0xc000 + 2, 0xa000 + 2)]);
}

#[test]
fn invalid_context_or_irq_changes_nothing() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let entry = mock.entry(1);
    insert(&mock, heap, entry, Expiry(10)).unwrap();
    mock.set_word(TICK, 10);
    mock.handler.set(0);
    assert_eq!(tick(&mock, |_| heap, |m| m.current_lock(), |_, _, _| {}), Err(TimerError::Context));
    assert_eq!(mock.count(), 1);
    assert!(mock.held().is_empty());
    mock.handler.set(1);
    mock.masked.set(true);
    assert_eq!(tick(&mock, |_| heap, |m| m.current_lock(), |_, _, _| {}), Err(TimerError::InterruptMasked));
    assert_eq!(mock.count(), 1);
    assert!(mock.held().is_empty());
}

#[test]
fn corrupt_count_fails_with_the_lock_released_and_irq_cleared() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let entry = mock.entry(1);
    insert(&mock, heap, entry, Expiry(10)).unwrap();
    mock.set_word(TICK, 10);
    mock.set_count(9);
    assert_eq!(tick(&mock, |_| heap, |m| m.current_lock(), |_, _, _| {}), Err(TimerError::CorruptCount));
    assert!(mock.held().is_empty());
    assert!(!mock.masked.get());
    assert_eq!(mock.index(entry), 1);
}

#[test]
fn upward_repair_delivers_remaining_timers_in_expiry_order() {
    let mock = Mock::new();
    let heap = mock.heap(32);
    let expiries = [0u32, 100, 1, 101, 102, 2, 3, 201, 202, 203, 204, 4, 5, 6, 7];
    let mut entries = Vec::new();
    for (position, expiry) in expiries.iter().enumerate() {
        let entry = mock.timed_entry(position as u32 + 1, *expiry);
        insert(&mock, heap, entry, Expiry(*expiry)).unwrap();
        entries.push(entry);
    }
    let canceled = entries[7];
    assert_eq!(remove(&mock, heap, canceled), Ok(Removed::Detached));
    mock.set_word(TICK, 1000);
    let callbacks = RefCell::new(Vec::new());
    tick(&mock, |_| heap, |m| m.current_lock(), |_m, callback, _argument| {
        callbacks.borrow_mut().push(callback.0);
    })
    .unwrap();
    assert_eq!(
        callbacks.into_inner(),
        vec![0, 1, 2, 3, 4, 5, 6, 7, 100, 101, 102, 202, 203, 204]
    );
    assert_eq!(mock.count(), 0);
    for entry in entries.iter() {
        assert_eq!(mock.index(*entry), 0);
    }
    assert!(mock.held().is_empty());
    assert!(!mock.masked.get());
}

#[test]
fn downward_repair_delivers_remaining_timers_in_expiry_order() {
    let mock = Mock::new();
    let heap = mock.heap(32);
    let expiries = [0u32, 1, 100, 2, 3, 101, 102, 4, 5, 6, 7, 103, 104, 105, 106];
    let mut entries = Vec::new();
    for (position, expiry) in expiries.iter().enumerate() {
        let entry = mock.timed_entry(position as u32 + 1, *expiry);
        insert(&mock, heap, entry, Expiry(*expiry)).unwrap();
        entries.push(entry);
    }
    let canceled = entries[1];
    assert_eq!(remove(&mock, heap, canceled), Ok(Removed::Detached));
    mock.set_word(TICK, 1000);
    let callbacks = RefCell::new(Vec::new());
    tick(&mock, |_| heap, |m| m.current_lock(), |_m, callback, _argument| {
        callbacks.borrow_mut().push(callback.0);
    })
    .unwrap();
    assert_eq!(
        callbacks.into_inner(),
        vec![0, 2, 3, 4, 5, 6, 7, 100, 101, 102, 103, 104, 105, 106]
    );
    assert_eq!(mock.count(), 0);
    for entry in entries.iter() {
        assert_eq!(mock.index(*entry), 0);
    }
    assert!(mock.held().is_empty());
    assert!(!mock.masked.get());
}

#[test]
fn capacity_validation_accepts_the_arithmetic_bound_and_refuses_its_neighbours() {
    assert_eq!(Capacity::new(0), None);
    assert_eq!(Capacity::new(MAX_CAPACITY + 1), None);
    assert_eq!(Capacity::new(MAX_CAPACITY).map(Capacity::get), Some(MAX_CAPACITY));
    assert_eq!(Capacity::from_static(MAX_CAPACITY).get(), MAX_CAPACITY);
}

#[test]
#[should_panic(expected = "timer capacity")]
fn static_capacity_refuses_zero() {
    let _ = Capacity::from_static(0);
}

#[test]
#[should_panic(expected = "timer capacity")]
fn static_capacity_refuses_above_the_arithmetic_bound() {
    let _ = Capacity::from_static(MAX_CAPACITY + 1);
}

#[test]
fn full_capacity_refuses_the_next_insert_without_a_write() {
    let mock = Mock::new();
    let heap = mock.heap(1);
    let first = mock.entry(1);
    let second = mock.entry(2);
    insert(&mock, heap, first, Expiry(10)).unwrap();
    let count = mock.count();
    let node = mock.word(NODES);
    assert_eq!(insert(&mock, heap, second, Expiry(20)), Err(TimerError::CorruptCount));
    assert_eq!(mock.count(), count);
    assert_eq!(mock.word(NODES), node);
    assert_eq!(mock.index(second), 0);
}

#[test]
fn a_distinct_context_lock_is_released_and_a_later_service_progresses() {
    let mock = Mock::new();
    let capacity = 8;
    let heap = mock.heap(capacity);
    let first = mock.entry(1);
    let second = mock.entry(2);
    insert(&mock, heap, first, Expiry(10)).unwrap();
    mock.second_context();
    mock.second_lock_cell();
    mock.set_word(SECOND_NODES, 10);
    mock.set_word(SECOND_NODES + 4, second.address().0);
    mock.set_word(second.address().0, 1);
    mock.set_word(TICK, 10);
    tick(&mock, |m| m.heap(capacity), |m| m.current_lock(), |m, _callback, _argument| {
        if m.context.get() == CONTEXT {
            m.context.set(SECOND_CONTEXT);
        }
    })
    .unwrap();
    assert!(mock.held().is_empty());
    let first_context = KernelContext(GuestAddress(CONTEXT));
    mock.acquire_lock(first_context.scheduler_lock());
    assert_eq!(mock.held(), vec![LOCK_CELL]);
    mock.release_lock(first_context.scheduler_lock());
    assert!(mock.held().is_empty());
}

#[test]
fn a_callback_context_change_moves_the_tick_to_the_new_heap() {
    let mock = Mock::new();
    let capacity = 8;
    let heap = mock.heap(capacity);
    let first = mock.entry(1);
    let second = mock.entry(2);
    let third = mock.entry(3);
    let fourth = mock.entry(4);
    insert(&mock, heap, first, Expiry(10)).unwrap();
    insert(&mock, heap, third, Expiry(20)).unwrap();
    mock.second_context();
    mock.set_word(SECOND_NODES, 10);
    mock.set_word(SECOND_NODES + 4, second.address().0);
    mock.set_word(SECOND_NODES + 8, 20);
    mock.set_word(SECOND_NODES + 12, fourth.address().0);
    mock.set_word(second.address().0, 1);
    mock.set_word(fourth.address().0, 2);
    mock.set_word(TICK, 10);
    let seen = RefCell::new(Vec::new());
    tick(&mock, |m| m.heap(capacity), |m| m.current_lock(), |m, callback, argument| {
        seen.borrow_mut().push((callback.0, argument.0));
        if seen.borrow().len() == 1 {
            m.context.set(SECOND_CONTEXT);
        }
    })
    .unwrap();
    assert_eq!(
        seen.into_inner(),
        vec![(0xc000 + 1, 0xa000 + 1), (0xc000 + 2, 0xa000 + 2)]
    );
    assert_eq!(mock.count(), 0);
    assert_eq!(mock.index(first), 0);
    assert_eq!(mock.index(second), 0);
    assert_eq!(mock.index(third), 1);
    assert_eq!(mock.word(SECOND_NODES), 10);
    assert_eq!(mock.word(BASE), 10);
    assert_eq!(mock.word(TICK), 20);
}
