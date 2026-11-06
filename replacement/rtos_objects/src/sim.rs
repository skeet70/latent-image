//! In-memory scheduler backend for host tests and the external consumer.
use crate::create::Machine;
use crate::id::EntryDomain;
use crate::layout::{
    Domain, DomainId, GuestAddress, KernelContext, LockRecord, Priority, ReadyHead, Task, TaskDefinition,
    TaskFlags, TaskState,
};
use crate::locked::ObjectLock;
use crate::wake::{DispatchTarget, Selection, Wake};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

pub struct Sim {
    words: RefCell<BTreeMap<u32, u32>>,
    entry: Domain,
    armed: RefCell<BTreeSet<u32>>,
    doorbell: [Cell<bool>; 2],
    held: Cell<bool>,
    interrupts: Cell<u32>,
}

impl Sim {
    pub fn new(entry: Domain) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            entry,
            armed: RefCell::new(BTreeSet::new()),
            doorbell: [Cell::new(false), Cell::new(false)],
            held: Cell::new(false),
            interrupts: Cell::new(0x40),
        }
    }

    pub fn entry(&self) -> Domain {
        self.entry
    }

    pub fn word(&self, at: GuestAddress) -> u32 {
        *self.words.borrow().get(&at.0).unwrap_or(&0)
    }

    pub fn set(&self, at: GuestAddress, value: u32) {
        self.words.borrow_mut().insert(at.0, value);
    }

    pub fn arm(&self, entry: GuestAddress) {
        self.armed.borrow_mut().insert(entry.0);
    }

    pub fn armed(&self, entry: GuestAddress) -> bool {
        self.armed.borrow().contains(&entry.0)
    }

    pub fn doorbell(&self, core: usize) -> bool {
        self.doorbell[core].get()
    }

    pub fn running(&self, domain: Domain) -> u32 {
        self.word(self.context(domain).running())
    }

    pub fn selected(&self, domain: Domain) -> u32 {
        self.word(self.context(domain).selected())
    }

    pub fn ready(&self, domain: Domain, priority: u32) -> u32 {
        let head = self.context(domain).ready_heads().offset(priority * ReadyHead::SIZE);
        self.word(head)
    }

    pub fn ready_bit(&self, domain: Domain, priority: u32) -> bool {
        self.word(self.context(domain).ready_bitmap()) & (1 << priority) != 0
    }

    fn context(&self, domain: Domain) -> KernelContext {
        match self.entry {
            Domain::A => match domain {
                Domain::A => KernelContext(GuestAddress(0x4005_45cc)),
                Domain::B => KernelContext(GuestAddress(0x4005_4650)),
            },
            Domain::B => match domain {
                Domain::A => KernelContext(GuestAddress(0x400e_458c)),
                Domain::B => KernelContext(GuestAddress(0x400e_4610)),
            },
        }
    }

    fn record_id(&self, owner: Domain) -> u32 {
        if owner == self.entry {
            0
        } else {
            match owner {
                Domain::A => 1,
                Domain::B => 2,
            }
        }
    }

    fn dispatch_enabled(&self, domain: Domain) -> u32 {
        let pointer = self.word(self.context(domain).dispatch_enabled());
        self.word(GuestAddress(pointer))
    }
}

impl Machine for Sim {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        let state = self.interrupts.get();
        assert_ne!(state & 0x80, 0, "store with IRQ unmasked");
        self.set(at, value);
    }

    fn mask_interrupts(&self) -> crate::create::SavedInterrupts {
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0xc0);
        crate::create::SavedInterrupts(saved)
    }

    fn restore_interrupts(&self, saved: crate::create::SavedInterrupts) {
        self.interrupts.set(saved.0);
    }

    fn dmb(&self) {}
}

impl ObjectLock for Sim {
    fn with_object_lock<R>(&self, _lock: LockRecord, body: impl FnOnce() -> R) -> R {
        assert!(!self.held.get(), "object lock acquired twice");
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0x80);
        self.held.set(true);
        let result = body();
        self.held.set(false);
        self.interrupts.set(saved);
        result
    }
}

impl Wake for Sim {
    fn with_scheduler_lock<R>(&self, _domain: Domain, body: impl FnOnce() -> R) -> R {
        assert!(self.held.get(), "scheduler lock taken without the object lock");
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0x80);
        let result = body();
        self.interrupts.set(saved);
        result
    }

    fn waiter_domain(&self, task: Task) -> Domain {
        match DomainId::from_word(self.word(task.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => panic!("waiting task has no domain tag"),
        }
    }

    fn remove_timer(&self, _owner: Domain, entry: GuestAddress) {
        assert!(self.armed.borrow_mut().remove(&entry.0), "timer was not armed");
    }

    fn reset_task(&self, task: Task) {
        let definition = TaskDefinition(GuestAddress(self.word(task.definition())));
        let priority = self.word(definition.priority()) & 0x0f;
        self.set(task.priority(), priority);
        self.set(task.state(), TaskState::DORMANT.bits() as u32);
        let flags = self.word(task.flags())
            & !(TaskFlags::QUEUED_WAKEUP.bits() as u32 | TaskFlags::EXCEPTION_ENABLED.bits() as u32);
        self.set(task.flags(), flags);
        self.set(task.exception_pattern(), 0);
        self.set(task.terminate_pending(), 0);
        self.set(task.priority_change_pending(), 0);
        self.set(task.cancel_pending(), 0);
    }

    fn prepare(&self, owner: Domain, task: Task) -> Selection {
        let definition = TaskDefinition(GuestAddress(self.word(task.definition())));
        let [low, high] = definition.initial_sp_terms();
        let sp = self.word(low).wrapping_add(self.word(high));
        self.set(task.saved_sp(), sp);
        self.set(task.resume_pc(), self.word(self.context(owner).first_entry()));
        self.insert(owner, task)
    }

    fn insert(&self, owner: Domain, task: Task) -> Selection {
        let context = self.context(owner);
        let priority = self.word(task.priority()) & 0x0f;
        self.set(task.state(), TaskState::READY.bits() as u32);
        let head = context.ready_heads().offset(priority * ReadyHead::SIZE);
        let prev = self.word(head.offset(0x04));
        self.set(task.prev(), prev);
        self.set(task.next(), head.0);
        self.set(GuestAddress(prev), task.0 .0);
        self.set(head.offset(0x04), task.0 .0);
        self.set(context.ready_bitmap(), self.word(context.ready_bitmap()) | (1 << priority));
        let selected = self.word(context.selected());
        if selected != 0 && priority >= (self.word(Task(GuestAddress(selected)).priority()) & 0x0f) {
            return Selection::Unchanged;
        }
        self.set(context.selected(), task.0 .0);
        if self.dispatch_enabled(owner) != 0 {
            Selection::Changed
        } else {
            Selection::Unchanged
        }
    }

    fn notify(&self, owner: Domain) -> DispatchTarget {
        let id = self.record_id(owner);
        if id == 0 || id == match self.entry { Domain::A => 1, Domain::B => 2 } {
            return DispatchTarget::Local;
        }
        self.doorbell[(id >> 1) as usize].set(true);
        DispatchTarget::Remote
    }

    fn dispatch(&self) {
        let owner = self.entry;
        let selected = self.word(self.context(owner).selected());
        self.set(self.context(owner).running(), selected);
    }
}
