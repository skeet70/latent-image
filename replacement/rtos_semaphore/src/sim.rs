//! Host backend over the owned S1 helpers in `xm1_rtos_scheduler`; models memory, the timer
//! boundary, the doorbell and the dispatch decision.
use crate::block::{Block, Enqueue, TimerEntryAddress};
use crate::create::{Machine as CreateMachine, SavedInterrupts};
use crate::layout::{
    Domain, DomainId, GuestAddress, KernelContext, LockRecord, Task, TaskState, WaitBlock,
    WaitQueue,
};
use crate::locks::{IrqScope, Locks};
use crate::order::QueueOrder;
use crate::wake::{Primitives, Release, Selection, WaitCompletion, Wake};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use xm1_rtos_scheduler as s1;

pub struct Sim {
    words: RefCell<BTreeMap<u32, u32>>,
    entry: Domain,
    armed: RefCell<BTreeSet<u32>>,
    deadlines: RefCell<BTreeMap<u32, u32>>,
    doorbell: [Cell<bool>; 2],
    interrupts: Cell<u32>,
    dispatches: RefCell<Vec<i32>>,
    dispatch_masks: RefCell<Vec<u32>>,
}

fn at(address: GuestAddress) -> s1::GuestAddress {
    s1::GuestAddress(address.0)
}

fn back(address: s1::GuestAddress) -> GuestAddress {
    GuestAddress(address.0)
}

fn record(address: s1::GuestAddress) -> s1::KernelContext {
    s1::KernelContext(address)
}

fn node(address: s1::GuestAddress) -> s1::Task {
    s1::Task(address)
}

fn lock_record(address: GuestAddress) -> s1::LockRecord {
    s1::LockRecord(at(address))
}

fn domain(value: Domain) -> s1::Domain {
    match value {
        Domain::A => s1::Domain::A,
        Domain::B => s1::Domain::B,
    }
}

impl Sim {
    pub fn new(entry: Domain) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            entry,
            armed: RefCell::new(BTreeSet::new()),
            deadlines: RefCell::new(BTreeMap::new()),
            doorbell: [Cell::new(false), Cell::new(false)],
            interrupts: Cell::new(0x40),
            dispatches: RefCell::new(Vec::new()),
            dispatch_masks: RefCell::new(Vec::new()),
        }
    }

    pub fn install(&self, owner: Domain, dispatch_enabled: bool) {
        let record = self.context(owner);
        let base = match owner {
            Domain::A => 0x0096_0000,
            Domain::B => 0x0096_0100,
        };
        let heads = GuestAddress(base);
        for priority in 0..16u32 {
            let head = heads.offset(priority * 8);
            self.set(head, head.0);
            self.set(head.offset(0x04), head.0);
        }
        self.set(record.ready_heads(), heads.0);
        self.set(record.ready_bitmap(), base + 0x80);
        self.set(record.selected(), base + 0x84);
        self.set(record.running(), base + 0x88);
        self.set(record.id(), if owner == self.entry { 0 } else { owner.id().word() });
        self.set(record.owner(), owner.id().word());
        self.set(record.dispatch_enabled(), base + 0x8c);
        self.set(GuestAddress(base + 0x8c), dispatch_enabled as u32);
        self.set(record.first_entry(), base + 0x90);
        self.set(record.scheduler_lock().0, base + 0x94);
        self.set(GuestAddress(base + 0x94), 0);
        self.set(record.object_lock().0, base + 0x98);
        self.set(GuestAddress(base + 0x98), 0);
    }

    pub fn entry(&self) -> Domain {
        self.entry
    }

    pub fn word(&self, address: GuestAddress) -> u32 {
        *self.words.borrow().get(&address.0).unwrap_or(&0)
    }

    pub fn set(&self, address: GuestAddress, value: u32) {
        self.words.borrow_mut().insert(address.0, value);
    }

    pub fn set_byte(&self, address: GuestAddress, value: u8) {
        let aligned = GuestAddress(address.0 & !3);
        let shift = 8 * (address.0 & 3);
        let kept = self.word(aligned) & !(0xff << shift);
        self.set(aligned, kept | (value as u32) << shift);
    }

    pub fn arm(&self, entry: GuestAddress) {
        self.armed.borrow_mut().insert(entry.0);
    }

    pub fn armed(&self, entry: GuestAddress) -> bool {
        self.armed.borrow().contains(&entry.0)
    }

    pub fn deadline(&self, entry: GuestAddress) -> Option<u32> {
        self.deadlines.borrow().get(&entry.0).copied()
    }

    pub fn set_running(&self, owner: Domain, task: Task) {
        let cell = GuestAddress(self.word(self.context(owner).running()));
        self.set(cell, task.0 .0);
    }

    pub fn running(&self, owner: Domain) -> u32 {
        self.pointed(self.context(owner).running())
    }

    pub fn selected(&self, owner: Domain) -> u32 {
        self.pointed(self.context(owner).selected())
    }

    pub fn ready(&self, owner: Domain, priority: u32) -> u32 {
        let heads = GuestAddress(self.word(self.context(owner).ready_heads()));
        self.word(heads.offset(priority * 8))
    }

    pub fn ready_bit(&self, owner: Domain, priority: u32) -> bool {
        self.pointed(self.context(owner).ready_bitmap()) & (1 << priority) != 0
    }

    pub fn delayed(&self) -> u32 {
        self.word(self.entry.words().reschedule)
    }

    pub fn interrupts(&self) -> u32 {
        self.interrupts.get()
    }

    pub fn set_interrupts(&self, value: u32) {
        self.interrupts.set(value);
    }

    pub fn doorbell(&self, core: usize) -> bool {
        self.doorbell[core].get()
    }

    pub fn dispatches(&self) -> Vec<i32> {
        self.dispatches.borrow().clone()
    }

    pub fn dispatch_masks(&self) -> Vec<u32> {
        self.dispatch_masks.borrow().clone()
    }

    fn pointed(&self, field: GuestAddress) -> u32 {
        self.word(GuestAddress(self.word(field)))
    }

    pub fn context(&self, owner: Domain) -> KernelContext {
        match self.entry {
            Domain::A => match owner {
                Domain::A => KernelContext(GuestAddress(0x4005_45cc)),
                Domain::B => KernelContext(GuestAddress(0x4005_4650)),
            },
            Domain::B => match owner {
                Domain::A => KernelContext(GuestAddress(0x400e_458c)),
                Domain::B => KernelContext(GuestAddress(0x400e_4610)),
            },
        }
    }

    fn timeout_callback(&self) -> u32 {
        match self.entry {
            Domain::A => 0x44318,
            Domain::B => 0xd42d8,
        }
    }
}

impl s1::Machine for Sim {
    fn load_word(&self, address: s1::GuestAddress) -> u32 {
        self.word(back(address))
    }

    fn load_byte(&self, address: s1::GuestAddress) -> u8 {
        let GuestAddress(at) = back(address);
        (self.word(GuestAddress(at & !3)) >> (8 * (at & 3))) as u8
    }

    fn store_word(&self, address: s1::GuestAddress, value: u32) {
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set(back(address), value);
    }

    fn store_byte(&self, address: s1::GuestAddress, value: u8) {
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set_byte(back(address), value);
    }

    fn try_claim(&self, word: s1::GuestAddress) -> bool {
        if self.word(back(word)) != 0 {
            return false;
        }
        self.set(back(word), 1);
        true
    }

    fn dmb(&self) {}

    fn dsb(&self) {}

    fn raise_irq(&self, core: s1::IrqCore, _irq: s1::Irq) {
        self.doorbell[core.0 as usize].set(true);
    }
}

impl CreateMachine for Sim {
    fn load_word(&self, address: GuestAddress) -> u32 {
        self.word(address)
    }

    fn load_byte(&self, address: GuestAddress) -> u8 {
        let aligned = GuestAddress(address.0 & !3);
        (self.word(aligned) >> (8 * (address.0 & 3))) as u8
    }

    fn store_word(&self, address: GuestAddress, value: u32) {
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set(address, value);
    }

    fn store_byte(&self, address: GuestAddress, value: u8) {
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set_byte(address, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0xc0);
        SavedInterrupts(saved)
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        self.interrupts.set(saved.0);
    }

    fn restore_f(&self, saved: SavedInterrupts) {
        let current = self.interrupts.get();
        self.interrupts.set((current & !0x40) | (saved.0 & 0x40));
    }


    fn dmb(&self) {}
}

impl IrqScope for Sim {
    fn disable_irq(&self) {
        self.interrupts.set(self.interrupts.get() | 0x80);
    }

    fn enable_irq(&self) {
        self.interrupts.set(self.interrupts.get() & !0x80);
    }
}

impl Locks for Sim {
    fn lock(&self, record: LockRecord) {
        let word = GuestAddress(self.word(record.word()));
        assert_eq!(self.word(word), 0, "lock acquired while held");
        s1::acquire(self, lock_record(record.0));
    }

    fn unlock(&self, record: LockRecord) {
        let word = GuestAddress(self.word(record.word()));
        assert!(
            self.word(word) == 1,
            "lock released while free at {:08x} (record {:08x}): {}",
            word.0,
            record.0 .0,
            self.word(word)
        );
        s1::release(self, lock_record(record.0));
    }
}

impl Primitives for Sim {
    fn reset_task(&self, task: Task) {
        s1::reset(self, node(at(task.0)));
    }

    fn prepare(&self, owner: KernelContext, task: Task) -> Selection {
        let inserted = s1::prepare(self, record(at(owner.0)), node(at(task.0)), || {
            s1::insert(self, record(at(owner.0)), node(at(task.0)))
        });
        if inserted == s1::Reschedule::NONE {
            Selection::Unchanged
        } else {
            Selection::Changed
        }
    }

    fn insert(&self, owner: KernelContext, task: Task) -> Selection {
        if s1::insert(self, record(at(owner.0)), node(at(task.0))) == s1::Reschedule::NONE {
            Selection::Unchanged
        } else {
            Selection::Changed
        }
    }

    fn remove_timer(&self, _owner: KernelContext, entry: GuestAddress) {
        assert!(self.armed.borrow_mut().remove(&entry.0), "timer was not armed");
        self.deadlines.borrow_mut().remove(&entry.0);
    }
}

impl Release for Sim {
    fn context(&self, owner: Domain) -> KernelContext {
        Sim::context(self, owner)
    }

    fn release(&self, owner: KernelContext, task: Task) -> Selection {
        crate::wake::release_with(self, owner, task, WaitCompletion::Satisfied)
    }

    fn cancel(&self, owner: KernelContext, task: Task) -> Selection {
        crate::wake::release_with(self, owner, task, WaitCompletion::Cancelled)
    }
}

impl Wake for Sim {
    fn in_handler(&self) -> bool {
        self.word(self.entry.words().handler_context) != 0
    }

    fn waiter_domain(&self, task: Task) -> Domain {
        match DomainId::from_word(self.word(task.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => panic!("waiting task has no domain tag"),
        }
    }

    fn record_id(&self, owner: KernelContext) -> u32 {
        self.word(owner.id())
    }

    fn notify(&self, id: u32) -> crate::wake::DispatchTarget {
        match s1::notify(self, domain(self.entry), s1::RecordId(id)) {
            s1::Notified::Local => crate::wake::DispatchTarget::Local,
            s1::Notified::Remote => crate::wake::DispatchTarget::Remote,
        }
    }

    fn dispatch(&self, code: i32) {
        self.dispatches.borrow_mut().push(code);
        self.dispatch_masks.borrow_mut().push(self.interrupts.get());
        let record = self.context(self.entry);
        let cell = GuestAddress(self.word(record.running()));
        let selected = self.pointed(record.selected());
        self.set(cell, selected);
    }

    fn request_delayed_dispatch(&self) {
        self.set(self.entry.words().reschedule, 1);
    }
}

impl Block for Sim {
    fn enqueue(
        &self,
        entry: Domain,
        owner: KernelContext,
        queue: WaitQueue,
        wait: WaitBlock,
        how: Enqueue,
    ) {
        let scheduler = self.context(entry);
        let task = Task(GuestAddress(self.pointed(scheduler.running())));
        self.set_byte(
            task.state(),
            TaskState::WAIT
                .union(TaskState::OBJECT_QUEUED)
                .union(TaskState::OWNER_LOCK_QUEUED)
                .bits(),
        );
        s1::remove(self, record(at(scheduler.0)), node(at(task.0)), || {
            s1::find(self, record(at(scheduler.0)))
        });
        self.set(task.wait(), wait.0 .0);
        match how {
            Enqueue::Plain => self.set(wait.result(), 0),
            Enqueue::Timed {
                timer: TimerEntryAddress(entry_address),
                timeout,
            } => {
                if timeout > 0 {
                    self.set(wait.result(), entry_address.0);
                    self.set(entry_address.offset(0x04), self.timeout_callback());
                    self.set(entry_address.offset(0x08), task.0 .0);
                    let now = self.pointed(scheduler.tick_now());
                    self.arm(entry_address);
                    self.deadlines
                        .borrow_mut()
                        .insert(entry_address.0, now.wrapping_add(timeout as u32));
                } else {
                    self.set(wait.result(), 0);
                }
            }
        }
        let order = QueueOrder::from_attribute(self.word(GuestAddress(self.word(queue.config()))));
        let head = queue.0;
        let (mut at, mut next) = (GuestAddress(self.word(queue.prev())), head);
        if order == QueueOrder::Priority {
            at = head;
            next = GuestAddress(self.word(head));
            let candidate = self.priority_byte(task);
            while next.0 != head.0 {
                let existing = Task(next);
                if self.priority_byte(existing) > candidate {
                    break;
                }
                at = next;
                next = GuestAddress(self.word(existing.next()));
            }
        }
        self.set(task.next(), next.0);
        self.set(task.prev(), at.0);
        self.set(GuestAddress(at.0), task.0 .0);
        self.set(GuestAddress(next.0).offset(0x04), task.0 .0);
        self.set(wait.owner(), self.word(owner.owner()));
        self.set(wait.queue(), queue.0 .0);
    }
}

impl Sim {
    fn priority_byte(&self, task: Task) -> u8 {
        let GuestAddress(at) = task.priority();
        (self.word(GuestAddress(at & !3)) >> (8 * (at & 3))) as u8
    }
}
