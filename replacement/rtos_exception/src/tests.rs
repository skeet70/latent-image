use super::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Event {
    Try(u32, u32),
    Release(u32),
    Store(u32, u32),
}

struct TestMachine {
    words: RefCell<BTreeMap<u32, u32>>,
    tpidrurw: Cell<u32>,
    log: RefCell<Vec<(u32, u32)>>,
    calls: RefCell<Vec<(u32, u32, u32)>>,
    events: RefCell<Vec<Event>>,
    primary: bool,
    lost_tries: Cell<u32>,
}

impl TestMachine {
    fn new(primary: bool) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            tpidrurw: Cell::new(0),
            log: RefCell::new(Vec::new()),
            calls: RefCell::new(Vec::new()),
            events: RefCell::new(Vec::new()),
            primary,
            lost_tries: Cell::new(0),
        }
    }

    fn put(&self, address: u32, value: u32) {
        self.words.borrow_mut().insert(address, value);
    }

    fn get(&self, address: u32) -> u32 {
        self.words.borrow().get(&address).copied().unwrap_or(0)
    }

    fn events(&self) -> Vec<Event> {
        self.events.borrow().clone()
    }
}

impl Machine for TestMachine {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.get(at.0)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.put(at.0, value);
        self.events.borrow_mut().push(Event::Store(at.0, value));
    }

    fn try_acquire(&self, word: GuestAddress, stamp: IrqTag) -> bool {
        self.events.borrow_mut().push(Event::Try(word.0, stamp.word()));
        if self.lost_tries.get() != 0 {
            self.lost_tries.set(self.lost_tries.get() - 1);
            return false;
        }
        if self.get(word.0) != 0 {
            return false;
        }
        self.put(word.0, stamp.word());
        true
    }

    fn release(&self, word: GuestAddress) {
        self.events.borrow_mut().push(Event::Release(word.0));
        if self.get(word.0) != 0 {
            self.put(word.0, 0);
        }
    }

    fn primary_identity(&self) -> bool {
        self.primary
    }

    fn set_tpidrurw_bit31(&self) {
        self.tpidrurw.set(self.tpidrurw.get() | 0x8000_0000);
    }

    fn clear_tpidrurw_bit31(&self) {
        self.tpidrurw.set(self.tpidrurw.get() & !0x8000_0000);
    }

    fn log(&self, code: u32, value: u32) {
        self.log.borrow_mut().push((code, value));
    }

    fn call(&self, address: u32, first: u32, second: u32) {
        self.calls.borrow_mut().push((address, first, second));
    }
}

const LINE: u32 = 37;
const PRIORITY: u32 = 3;
const VECTOR: u32 = 0x1234_5678;

fn domain() -> ExceptionDomain {
    ExceptionDomain::of(Domain::A)
}

fn controller(machine: &TestMachine, d: &ExceptionDomain) {
    let base = Bank::PRIMARY.address().0;
    machine.put(base + 0x200, VECTOR);
    machine.put(base + 0x230 + 4 * (LINE / 32), 1 << (LINE % 32));
    machine.put(base + 0x1c0, 0xabcd_ef01);
    machine.put(d.handler_table.0 + 24 * LINE + 8, PRIORITY);
    machine.put(d.handler_table.0 + 24 * 38 + 8, PRIORITY + 1);
}

#[test]
fn outer_entry_acquires_and_acknowledges() {
    let machine = TestMachine::new(true);
    let d = domain();
    controller(&machine, &d);
    assert_eq!(enter(&machine, &d), EntryClass::Outer);
    lock_acquire(&machine, &d);
    let vector = acknowledge(&machine, &d);
    assert_eq!(vector, VectorWord(VECTOR));
    assert_eq!(machine.get(d.words.handler_context.0), 1);
    assert_eq!(machine.get(d.lock_word.0), IrqTag::Primary.word());
    assert_eq!(machine.tpidrurw.get(), 0x8000_0000);
    let slot = d.active_irqs.0 + 8 * PRIORITY;
    assert_eq!(machine.get(slot), 0xabcd_ef01);
    assert_eq!(machine.get(slot + 4), LINE);
}

#[test]
fn nested_entry_uses_the_own_tag_without_storing_it() {
    let machine = TestMachine::new(true);
    let d = domain();
    controller(&machine, &d);
    enter(&machine, &d);
    lock_acquire(&machine, &d);
    assert_eq!(enter(&machine, &d), EntryClass::Nested);
    lock_acquire(&machine, &d);
    assert_eq!(machine.get(d.lock_word.0), IrqTag::Primary.word());
    assert_eq!(machine.events().iter().filter(|event| matches!(event, Event::Try(..))).count(), 1);
}

#[test]
fn nested_exit_keeps_the_lock_and_the_dispatch_flag() {
    let machine = TestMachine::new(true);
    let d = domain();
    enter(&machine, &d);
    lock_acquire(&machine, &d);
    machine.put(d.words.reschedule.0, 1);
    enter(&machine, &d);
    assert_eq!(exit(&machine, &d), ExitClass::Nested);
    assert_eq!(machine.get(d.words.handler_context.0), 1);
    assert_eq!(machine.get(d.lock_word.0), IrqTag::Primary.word());
    assert_eq!(machine.get(d.words.reschedule.0), 1);
}

#[test]
fn outer_exit_releases_before_the_decrement_and_dispatch() {
    let machine = TestMachine::new(true);
    let d = domain();
    enter(&machine, &d);
    lock_acquire(&machine, &d);
    machine.put(d.words.reschedule.0, 1);
    assert_eq!(exit(&machine, &d), ExitClass::Dispatch);
    assert_eq!(machine.get(d.words.handler_context.0), 0);
    assert_eq!(machine.get(d.lock_word.0), 0);
    assert_eq!(machine.get(d.words.reschedule.0), 0);
    let events = machine.events();
    let release = events.iter().position(|event| *event == Event::Release(d.lock_word.0)).unwrap();
    let decrement = events.iter().position(|event| *event == Event::Store(d.words.handler_context.0, 0)).unwrap();
    let flag = events.iter().position(|event| *event == Event::Store(d.words.reschedule.0, 0)).unwrap();
    assert!(release < decrement && decrement < flag);
}

#[test]
fn outer_exit_without_dispatch_returns_the_frame() {
    let machine = TestMachine::new(true);
    let d = domain();
    enter(&machine, &d);
    lock_acquire(&machine, &d);
    assert_eq!(exit(&machine, &d), ExitClass::OuterReturn);
    assert_eq!(machine.get(d.lock_word.0), 0);
    assert_eq!(machine.get(d.words.handler_context.0), 0);
}

#[test]
fn premature_unmask_at_counter_one_never_releases() {
    let machine = TestMachine::new(true);
    let d = domain();
    machine.put(d.words.handler_context.0, 1);
    assert_eq!(enter(&machine, &d), EntryClass::Nested);
    lock_acquire(&machine, &d);
    assert_eq!(exit(&machine, &d), ExitClass::Nested);
    assert_eq!(machine.get(d.words.handler_context.0), 1);
    assert_eq!(machine.get(d.lock_word.0), IrqTag::Primary.word());
}

#[test]
fn acknowledge_clears_no_pending_bit() {
    let machine = TestMachine::new(true);
    let d = domain();
    controller(&machine, &d);
    acknowledge(&machine, &d);
    let base = Bank::PRIMARY.address().0;
    assert!(machine.events().iter().all(|event| !matches!(event, Event::Store(address, _) if *address == base + 0x120)));
}

#[test]
fn unhandled_acknowledge_counts_and_logs_code_six() {
    for vector in [0, 0x42840] {
        let machine = TestMachine::new(true);
        let d = domain();
        machine.put(Bank::PRIMARY.address().0 + 0x200, vector);
        machine.put(d.unhandled.0, 7);
        assert_eq!(acknowledge(&machine, &d), VectorWord::NONE);
        assert_eq!(machine.get(d.unhandled.0), 8);
        assert_eq!(*machine.log.borrow(), vec![(6, 8)]);
        assert_eq!(machine.tpidrurw.get(), 0);
        assert_eq!(machine.get(d.active_irqs.0), 0);
    }
}

#[test]
fn secondary_unhandled_marker_is_domain_specific() {
    let machine = TestMachine::new(false);
    let d = ExceptionDomain::of(Domain::B);
    machine.put(Bank::SECONDARY.address().0 + 0x200, 0xd2840);
    assert_eq!(acknowledge(&machine, &d), VectorWord::NONE);
    assert_eq!(machine.get(d.unhandled.0), 1);
    assert_eq!(*machine.log.borrow(), vec![(6, 1)]);
}

#[test]
fn end_of_interrupt_rederives_the_line() {
    let machine = TestMachine::new(true);
    let d = domain();
    controller(&machine, &d);
    acknowledge(&machine, &d);
    let base = Bank::PRIMARY.address().0;
    machine.put(base + 0x230 + 4, 1 << 6);
    end_of_interrupt(&machine, &d);
    assert_eq!(machine.get(base + 0x124), 1 << 6);
    assert_eq!(machine.get(base + 0x120), 0);
    assert_eq!(machine.get(d.active_irqs.0 + 8 * (PRIORITY + 1)), 0);
    assert_eq!(machine.get(d.active_irqs.0 + 8 * (PRIORITY + 1) + 4), u32::MAX);
    assert_eq!(machine.get(d.active_irqs.0 + 8 * PRIORITY), 0xabcd_ef01);
    assert_eq!(machine.get(base + 0x200), 0);
}

#[test]
fn end_of_interrupt_clears_tpidrurw_only_at_depth_one() {
    let machine = TestMachine::new(true);
    let d = domain();
    controller(&machine, &d);
    machine.put(d.words.handler_context.0, 2);
    acknowledge(&machine, &d);
    end_of_interrupt(&machine, &d);
    assert_eq!(machine.tpidrurw.get(), 0x8000_0000);
    machine.put(d.words.handler_context.0, 1);
    end_of_interrupt(&machine, &d);
    assert_eq!(machine.tpidrurw.get(), 0);
}

#[test]
fn lost_try_retries_before_the_lock_is_held() {
    let machine = TestMachine::new(true);
    let d = domain();
    machine.lost_tries.set(2);
    lock_acquire(&machine, &d);
    assert_eq!(machine.get(d.lock_word.0), IrqTag::Primary.word());
    assert_eq!(machine.events().iter().filter(|event| matches!(event, Event::Try(..))).count(), 3);
}

#[test]
fn secondary_identity_uses_tag_eighteen() {
    let machine = TestMachine::new(false);
    let d = domain();
    lock_acquire(&machine, &d);
    assert_eq!(machine.get(d.lock_word.0), IrqTag::Secondary.word());
}

#[test]
fn record_fault_publishes_the_record_log_and_hook() {
    let machine = TestMachine::new(true);
    let d = domain();
    let frame = GuestAddress(0x4001_0000);
    for index in 0..FaultFrame::WORDS {
        machine.put(frame.0 + 4 * index, 0x100 + index);
    }
    machine.put(d.exception_hook.0, 0x4002_0000);
    machine.put(d.resume_gate.0, 0x55);
    record_fault(&machine, &d, FaultKind::DataAbort, FaultFrame(frame));
    assert_eq!(machine.get(d.fault_record.0), FaultKind::DataAbort as u32);
    for index in 0..FaultFrame::WORDS {
        assert_eq!(machine.get(d.fault_record.0 + 4 + 4 * index), 0x100 + index);
    }
    assert_eq!(*machine.log.borrow(), vec![(4, 0x10f)]);
    assert_eq!(*machine.calls.borrow(), vec![(0x4002_0000, 4, 0x10f)]);
    assert_eq!(machine.get(d.resume_gate.0), 0x55);
}

fn restore_fc(saved: u32, current: u32) -> u32 {
    let merged = restore_status(saved, current);
    (current & !0x41) | (merged & 0x41)
}

#[test]
fn restore_status_preserves_saved_a_and_uses_current_f() {
    let saved = 0x2000_0153;
    assert_eq!(restore_status(saved, 0), saved & !0x40);
    assert_eq!(restore_status(saved, 0x40), saved);
    assert_eq!(restore_status(saved, 0) & 0x100, 0x100);
}

#[test]
fn fc_only_restore_loses_saved_a() {
    let saved = 0x2000_0153;
    assert_eq!(restore_status(saved, 0) & 0x100, 0x100);
    assert_eq!(restore_fc(saved, 0) & 0x100, 0);
}

#[test]
fn absent_hooks_are_not_called() {
    let machine = TestMachine::new(true);
    let d = domain();
    let frame = GuestAddress(0x4001_0000);
    machine.put(frame.0 + 0x3c, 0x4000_1000);
    record_fault(&machine, &d, FaultKind::Svc, FaultFrame(frame));
    fiq_hook(&machine, &d, FaultFrame(frame));
    assert!(machine.calls.borrow().is_empty());
}

#[test]
fn fiq_hook_receives_the_saved_pc_only() {
    let machine = TestMachine::new(true);
    let d = domain();
    let frame = GuestAddress(0x4001_0000);
    machine.put(frame.0 + 0x3c, 0x4000_1000);
    machine.put(d.fiq_hook.0, 0x4002_0000);
    fiq_hook(&machine, &d, FaultFrame(frame));
    assert_eq!(*machine.calls.borrow(), vec![(0x4002_0000, 0x4000_1000, 0)]);
}
