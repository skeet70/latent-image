use crate::slot_table::*;
use crate::{Form, IrqState, Status, TaskId, Timeout};
use std::cell::RefCell;
use std::collections::BTreeMap;

const BLOCK: u32 = 0x960000;
const CORE_STUB: u32 = 0x970000;
const TASK_STUB: u32 = 0x970010;
const HANDLER_STUB: u32 = 0x970020;
const MASK_STUB: u32 = 0x970030;
const RESTORE_STUB: u32 = 0x970040;
const WAIT_STUB: u32 = 0x970050;
const SIGNAL_STUB: u32 = 0x970060;
const HANDLER_SIGNAL_STUB: u32 = 0x970070;
const CALLBACK: u32 = 0x980000;

struct Fixture {
    words: BTreeMap<u32, u32>,
    entries: RefCell<Vec<(u32, [u32; 3])>>,
    reads: RefCell<Vec<u32>>,
    writes: RefCell<Vec<(u32, u32)>>,
    console: RefCell<Vec<(u32, u32, i32)>>,
    critical_enters: RefCell<Vec<u32>>,
    critical_exits: RefCell<Vec<u32>>,
    critical_enter_result: u32,
}

impl Fixture {
    fn new(critical_enter_result: u32) -> Self {
        let mut words = BTreeMap::new();
        words.insert(ROOT_TABLE, BLOCK);
        words.insert(BLOCK + CORE_SLOT, CORE_STUB);
        words.insert(BLOCK + TASK_SLOT, TASK_STUB);
        words.insert(BLOCK + HANDLER_SLOT, HANDLER_STUB);
        words.insert(BLOCK + IRQ_MASK_SLOT, MASK_STUB);
        words.insert(BLOCK + IRQ_RESTORE_SLOT, RESTORE_STUB);
        words.insert(BLOCK + WAIT_SLOT, WAIT_STUB);
        words.insert(BLOCK + SIGNAL_SLOT, SIGNAL_STUB);
        words.insert(BLOCK + HANDLER_SIGNAL_SLOT, HANDLER_SIGNAL_STUB);
        words.insert(SEMAPHORE_CLASSES + 4 * (0x25 - 1), 3);
        words.insert(CORE_STUB, 0);
        words.insert(TASK_STUB, 1);
        words.insert(HANDLER_STUB, 0);
        words.insert(MASK_STUB, 1);
        words.insert(RESTORE_STUB, 0);
        words.insert(WAIT_STUB, 0);
        words.insert(SIGNAL_STUB, 0);
        words.insert(HANDLER_SIGNAL_STUB, 0);
        words.insert(ERROR_CALLBACK, CALLBACK);
        words.insert(CALLBACK, 0);
        Self {
            words,
            entries: RefCell::new(Vec::new()),
            reads: RefCell::new(Vec::new()),
            writes: RefCell::new(Vec::new()),
            console: RefCell::new(Vec::new()),
            critical_enters: RefCell::new(Vec::new()),
            critical_exits: RefCell::new(Vec::new()),
            critical_enter_result,
        }
    }

    fn set(&mut self, address: u32, value: u32) { self.words.insert(address, value); }
    fn entries(&self) -> Vec<(u32, [u32; 3])> { self.entries.borrow().clone() }
    fn writes(&self) -> Vec<(u32, u32)> { self.writes.borrow().clone() }
    fn console(&self) -> Vec<(u32, u32, i32)> { self.console.borrow().clone() }
    fn reads(&self) -> Vec<u32> { self.reads.borrow().clone() }
    fn critical_enters(&self) -> Vec<u32> { self.critical_enters.borrow().clone() }
    fn critical_exits(&self) -> Vec<u32> { self.critical_exits.borrow().clone() }
}

impl Guest for Fixture {
    unsafe fn read32(&self, address: u32) -> u32 {
        self.reads.borrow_mut().push(address);
        *self.words.get(&address).unwrap_or(&0)
    }
    unsafe fn write32(&self, address: u32, value: u32) {
        self.writes.borrow_mut().push((address, value));
    }
    unsafe fn enter(&self, address: u32, arguments: [u32; 3]) -> u32 {
        self.entries.borrow_mut().push((address, arguments));
        *self.words.get(&address).unwrap_or(&0)
    }
    unsafe fn console(&self, kind: u32, semaphore: u32, status: i32) {
        self.console.borrow_mut().push((kind, semaphore, status));
    }
    unsafe fn critical_enter(&self) -> u32 {
        self.critical_enters.borrow_mut().push(CRITICAL_ENTER);
        self.critical_enter_result
    }
    unsafe fn critical_exit(&self, saved: u32) {
        self.critical_enters.borrow_mut().push(CRITICAL_EXIT);
        self.critical_exits.borrow_mut().push(saved);
    }
}

fn id() -> crate::SemaphoreId { crate::SemaphoreId::new(0x25).unwrap() }

#[test]
fn current_task_enters_the_core_then_the_task_slot() {
    let guest = Fixture::new(0);
    assert_eq!(current_task(&guest), Some(TaskId::new(1).unwrap()));
    assert_eq!(guest.entries(), [(CORE_STUB, [0, 0, 0]), (TASK_STUB, [0, 0, 0])]);
}

#[test]
fn in_handler_reads_the_handler_slot() {
    let guest = Fixture::new(0);
    assert!(!in_handler(&guest));
    assert_eq!(guest.entries(), [(CORE_STUB, [0, 0, 0]), (HANDLER_STUB, [0, 0, 0])]);
}

#[test]
fn wait_enters_the_wait_slot_with_the_class_word_and_timeout() {
    let guest = Fixture::new(0);
    assert_eq!(semaphore_wait(&guest, id(), Timeout::Forever), Ok(()));
    assert_eq!(guest.entries(), [(CORE_STUB, [0, 0, 0]), (WAIT_STUB, [0x3_0025, u32::MAX, 0])]);
}

#[test]
fn signal_uses_the_task_slot_for_a_task_form() {
    let guest = Fixture::new(0);
    assert_eq!(semaphore_signal(&guest, id(), Form::Task), Ok(()));
    assert_eq!(guest.entries(), [(CORE_STUB, [0, 0, 0]), (SIGNAL_STUB, [0x3_0025, 0, 0])]);
}

#[test]
fn signal_uses_the_handler_slot_for_a_handler_form() {
    let guest = Fixture::new(0);
    assert_eq!(semaphore_signal(&guest, id(), Form::Handler), Ok(()));
    assert_eq!(guest.entries(), [(CORE_STUB, [0, 0, 0]), (HANDLER_SIGNAL_STUB, [0x3_0025, 0, 0])]);
}

#[test]
fn a_timed_out_wait_reports_nothing() {
    let mut guest = Fixture::new(0);
    guest.set(WAIT_STUB, (-50i32) as u32);
    assert_eq!(semaphore_wait(&guest, id(), Timeout::Poll), Err(crate::Error::TimedOut));
    assert!(guest.console().is_empty());
    assert!(guest.writes().is_empty());
    assert_eq!(guest.entries(), [(CORE_STUB, [0, 0, 0]), (WAIT_STUB, [0x3_0025, 0, 0])]);
}

#[test]
fn a_refused_wait_reports_the_console_record_the_status_and_the_callback() {
    let mut guest = Fixture::new(0);
    guest.set(WAIT_STUB, (-25i32) as u32);
    assert_eq!(semaphore_wait(&guest, id(), Timeout::Forever), Err(crate::Error::Context));
    assert_eq!(guest.console(), [(24, 0x25, -25)]);
    assert_eq!(guest.writes(), [(ERROR_STATUS, (-25i32) as u32)]);
    assert_eq!(
        guest.entries(),
        [(CORE_STUB, [0, 0, 0]), (WAIT_STUB, [0x3_0025, u32::MAX, 0]), (CALLBACK, [(-25i32) as u32, 0, 0])]
    );
}

#[test]
fn a_refused_signal_reports_kind_25() {
    let mut guest = Fixture::new(0);
    guest.set(SIGNAL_STUB, (-43i32) as u32);
    assert_eq!(semaphore_signal(&guest, id(), Form::Task), Err(crate::Error::Overflow));
    assert_eq!(guest.console(), [(25, 0x25, -43)]);
    assert_eq!(guest.writes(), [(ERROR_STATUS, (-43i32) as u32)]);
    assert_eq!(
        guest.entries(),
        [(CORE_STUB, [0, 0, 0]), (SIGNAL_STUB, [0x3_0025, 0, 0]), (CALLBACK, [(-43i32) as u32, 0, 0])]
    );
}

#[test]
fn an_unlisted_status_is_reported_and_kept() {
    let mut guest = Fixture::new(0);
    guest.set(WAIT_STUB, 9);
    assert_eq!(semaphore_wait(&guest, id(), Timeout::Forever), Err(crate::Error::Other(Status(9))));
    assert_eq!(guest.console(), [(24, 0x25, 9)]);
}

#[test]
fn irq_mask_and_restore_enter_the_mask_slots() {
    let guest = Fixture::new(0);
    assert_eq!(irq_mask(&guest), IrqState::new(1));
    irq_restore(&guest, IrqState::new(1));
    assert_eq!(
        guest.entries(),
        [(CORE_STUB, [0, 0, 0]), (MASK_STUB, [0, 0, 0]), (CORE_STUB, [0, 0, 0]), (RESTORE_STUB, [1, 0, 0])]
    );
}

#[test]
fn cores_lock_and_unlock_call_the_stock_pair_in_each_stamp_state() {
    for saved in [0u32, 0x11, 0x21] {
        let guest = Fixture::new(saved);
        assert_eq!(cores_lock(&guest), IrqState::new(saved));
        cores_unlock(&guest, IrqState::new(saved));
        assert_eq!(guest.critical_enters(), [CRITICAL_ENTER, CRITICAL_EXIT]);
        assert_eq!(guest.critical_exits(), [saved]);
        assert!(!guest.reads().contains(&0x40a0f824));
    }
}
