use super::*;
use core::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use crate::layout::{GuestAddress, KernelContext};

const NODES: u32 = 0x2000;
const COUNT: u32 = 0x3000;
const BASE: u32 = 0x3004;
const TICK: u32 = 0x3008;
const CONTEXT: u32 = 0x1000;

struct Mock {
    words: RefCell<BTreeMap<u32, u32>>,
    lock_held: Cell<bool>,
    masked: Cell<bool>,
    handler: Cell<u32>,
    events: RefCell<Vec<(u32, u32)>>,
}

impl Mock {
    fn new() -> Self {
        let mock = Self {
            words: RefCell::new(BTreeMap::new()),
            lock_held: Cell::new(false),
            masked: Cell::new(false),
            handler: Cell::new(1),
            events: RefCell::new(Vec::new()),
        };
        let context = KernelContext(GuestAddress(CONTEXT));
        mock.store_word(context.timer_heap(), NODES);
        mock.store_word(context.timer_count(), COUNT);
        mock.store_word(context.timer_base(), BASE);
        mock.store_word(context.tick_now(), TICK);
        mock.set_count(0);
        mock.set_word(BASE, 0);
        mock.set_word(TICK, 0);
        mock
    }

    fn heap(&self, capacity: u32) -> TimerHeap {
        TimerHeap::from_context(self, KernelContext(GuestAddress(CONTEXT)), Capacity::new(capacity).unwrap())
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

    fn release_lock(&self) {
        self.lock_held.set(false);
    }

    fn acquire_lock(&self) {
        self.lock_held.set(true);
    }

    fn irq_clear(&self) {
        self.masked.set(false);
    }

    fn irq_set(&self) {
        self.masked.set(true);
    }
}

fn tick_all(mock: &Mock, heap: TimerHeap) -> Vec<(u32, u32)> {
    tick(mock, heap, |m, callback, argument| {
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
    tick(&mock, heap, |m, _callback, _argument| {
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
    tick(&mock, heap, |m, _callback, _argument| {
        assert!(m.lock_held.get());
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
    assert_eq!(tick(&mock, heap, |_, _, _| {}), Err(TimerError::Context));
    assert_eq!(mock.count(), 1);
    assert!(!mock.lock_held.get());
    mock.handler.set(1);
    mock.masked.set(true);
    assert_eq!(tick(&mock, heap, |_, _, _| {}), Err(TimerError::InterruptMasked));
    assert_eq!(mock.count(), 1);
    assert!(!mock.lock_held.get());
}

#[test]
fn corrupt_count_fails_with_the_lock_released_and_irq_cleared() {
    let mock = Mock::new();
    let heap = mock.heap(8);
    let entry = mock.entry(1);
    insert(&mock, heap, entry, Expiry(10)).unwrap();
    mock.set_word(TICK, 10);
    mock.set_count(9);
    assert_eq!(tick(&mock, heap, |_, _, _| {}), Err(TimerError::CorruptCount));
    assert!(!mock.lock_held.get());
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
    tick(&mock, heap, |_m, callback, _argument| {
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
    assert!(!mock.lock_held.get());
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
    tick(&mock, heap, |_m, callback, _argument| {
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
    assert!(!mock.lock_held.get());
    assert!(!mock.masked.get());
}
