//! Host backend over the owned S1/S2 bodies in `xm1_rtos_scheduler`; models memory, the timer
//! boundary and the doorbell.
use crate::block::{Block, Deadline, IrqScope, SavedMask, Tick, TimerEntryAddress, TimeoutCallback};
use crate::create::{Machine as CreateMachine, SavedInterrupts};
use crate::layout::{Domain, DomainId, GuestAddress, KernelContext, LockRecord, Task, WaitQueue};
use crate::order::QueueOrder;
use crate::locked::{ObjectGuard, ObjectLock};
use crate::wake::{DispatchTarget, Release, SchedulerGuard, Selection, Wake};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use xm1_rtos_scheduler as s1;

pub struct Sim {
    words: RefCell<BTreeMap<u32, u32>>,
    entry: Domain,
    armed: RefCell<BTreeSet<u32>>,
    deadlines: RefCell<BTreeMap<u32, u32>>,
    doorbell: [Cell<bool>; 2],
    held: Cell<bool>,
    interrupts: Cell<u32>,
    last_store_mask: Cell<u32>,
}

fn at(address: GuestAddress) -> s1::GuestAddress {
    s1::GuestAddress(address.0)
}

fn domain(value: Domain) -> s1::Domain {
    match value {
        Domain::A => s1::Domain::A,
        Domain::B => s1::Domain::B,
    }
}

fn back(address: s1::GuestAddress) -> GuestAddress {
    GuestAddress(address.0)
}

fn context(address: s1::GuestAddress) -> s1::KernelContext {
    s1::KernelContext(address)
}

fn task(address: s1::GuestAddress) -> s1::Task {
    s1::Task(address)
}

fn lock(address: GuestAddress) -> s1::LockRecord {
    s1::LockRecord(at(address))
}

impl Sim {
    pub fn new(entry: Domain) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            entry,
            armed: RefCell::new(BTreeSet::new()),
            deadlines: RefCell::new(BTreeMap::new()),
            doorbell: [Cell::new(false), Cell::new(false)],
            held: Cell::new(false),
            interrupts: Cell::new(0x40),
            last_store_mask: Cell::new(0),
        }
    }

    /// Installs one context record's pointer fields and an empty ready set, as the initialiser does.
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

    pub fn delayed(&self) -> u32 {
        self.word(self.entry.words().reschedule)
    }

    pub fn interrupts(&self) -> u32 {
        self.interrupts.get()
    }

    pub fn set_interrupts(&self, value: u32) {
        self.interrupts.set(value);
    }

    pub fn last_store_mask(&self) -> u32 {
        self.last_store_mask.get()
    }

    pub fn doorbell(&self, core: usize) -> bool {
        self.doorbell[core].get()
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

    fn pointed(&self, field: GuestAddress) -> u32 {
        self.word(GuestAddress(self.word(field)))
    }

    fn context(&self, owner: Domain) -> KernelContext {
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
        self.last_store_mask.set(self.interrupts.get());
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set(back(address), value);
    }

    fn store_byte(&self, address: s1::GuestAddress, value: u8) {
        self.last_store_mask.set(self.interrupts.get());
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        let GuestAddress(at) = back(address);
        let aligned = GuestAddress(at & !3);
        let shift = 8 * (at & 3);
        let kept = self.word(aligned) & !(0xff << shift);
        self.set(aligned, kept | (value as u32) << shift);
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

    fn store_byte(&self, address: GuestAddress, value: u8) {
        self.last_store_mask.set(self.interrupts.get());
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set_byte(address, value);
    }

    fn store_word(&self, address: GuestAddress, value: u32) {
        self.last_store_mask.set(self.interrupts.get());
        assert_ne!(self.interrupts.get() & 0x80, 0, "store with IRQ unmasked");
        self.set(address, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0xc0);
        SavedInterrupts(saved)
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        self.interrupts.set(saved.0);
    }

    fn dmb(&self) {}
}

impl ObjectLock for Sim {
    fn lock_object(&self, record: LockRecord) -> ObjectGuard<'_, Self> {
        assert!(!self.held.get(), "object lock acquired twice");
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0x80);
        self.held.set(true);
        s1::acquire(self, lock(record.0));
        ObjectGuard::new(self, record, SavedInterrupts(saved))
    }

    fn release_object(&self, record: LockRecord) {
        assert!(self.held.get(), "object lock released without the lock");
        s1::release(self, lock(record.0));
        self.held.set(false);
    }
}

impl Release for Sim {
    fn context(&self, domain: Domain) -> KernelContext {
        Sim::context(self, domain)
    }

    fn reset_task(&self, waiter: Task) {
        s1::reset(self, task(at(waiter.0)));
    }

    fn prepare(&self, owner: KernelContext, waiter: Task) -> Selection {
        let inserted = s1::prepare(self, context(at(owner.0)), task(at(waiter.0)), || {
            s1::insert(self, context(at(owner.0)), task(at(waiter.0)))
        });
        if inserted == s1::Reschedule::NONE {
            Selection::Unchanged
        } else {
            Selection::Changed
        }
    }

    fn insert(&self, owner: KernelContext, waiter: Task) -> Selection {
        if s1::insert(self, context(at(owner.0)), task(at(waiter.0))) == s1::Reschedule::NONE {
            Selection::Unchanged
        } else {
            Selection::Changed
        }
    }

    fn remove_timer(&self, _owner: KernelContext, entry: GuestAddress) {
        assert!(self.armed.borrow_mut().remove(&entry.0), "timer was not armed");
    }

    fn release(&self, owner: KernelContext, task: Task) -> Selection {
        crate::wake::release(self, owner, task)
    }
}

impl Wake for Sim {
    fn lock_scheduler(&self, owner: Domain) -> SchedulerGuard<'_, Self> {
        assert!(self.held.get(), "scheduler lock taken without the object lock");
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0x80);
        let record = self.context(owner).scheduler_lock();
        s1::acquire(self, lock(record.0));
        SchedulerGuard::new(self, owner, Some(SavedInterrupts(saved)))
    }

    fn release_scheduler(&self, owner: Domain) {
        let record = self.context(owner).scheduler_lock();
        s1::release(self, lock(record.0));
    }

    fn waiter_domain(&self, waiter: Task) -> Domain {
        match DomainId::from_word(self.word(waiter.domain())) {
            Some(DomainId::A) => Domain::A,
            Some(DomainId::B) => Domain::B,
            _ => panic!("waiting task has no domain tag"),
        }
    }

    fn record_id(&self, owner: KernelContext) -> u32 {
        self.word(owner.id())
    }

    fn notify(&self, id: u32) -> DispatchTarget {
        match s1::notify(self, domain(self.entry), s1::RecordId(id)) {
            s1::Notified::Local => DispatchTarget::Local,
            s1::Notified::Remote => DispatchTarget::Remote,
        }
    }

    fn dispatch(&self, _code: i32) {
        let record = self.context(self.entry);
        let cell = GuestAddress(self.word(record.running()));
        let selected = self.pointed(record.selected());
        s1::Machine::store_word(self, at(cell), selected);
    }

    fn request_delayed_dispatch(&self) {
        self.set(self.entry.words().reschedule, 1);
    }
}

impl IrqScope for Sim {
    fn mask_i(&self) -> SavedMask {
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0x80);
        SavedMask(saved)
    }

    fn restore_i(&self, saved: SavedMask) {
        self.interrupts.set(saved.0);
    }
}

impl Block for Sim {
    fn current(&self, entry: Domain) -> Task {
        Task(GuestAddress(self.running(entry)))
    }

    fn remove_ready(&self, owner: KernelContext, waiter: Task) {
        let record = context(at(owner.0));
        s1::remove(self, record, task(at(waiter.0)), || s1::find(self, record));
    }

    fn queue_wait(&self, _owner: KernelContext, task: Task, queue: WaitQueue, order: QueueOrder) {
        crate::wake::queue_insert(self, task, queue, order);
    }

    fn arm_timer(&self, _owner: KernelContext, entry: TimerEntryAddress, deadline: Deadline) {
        self.arm(entry.0);
        self.deadlines.borrow_mut().insert(entry.0 .0, deadline.0);
    }

    fn tick(&self, owner: KernelContext) -> Tick {
        Tick(self.pointed(owner.tick_now()))
    }

    fn timeout_callback(&self) -> TimeoutCallback {
        TimeoutCallback(match self.entry {
            Domain::A => 0x44318,
            Domain::B => 0xd42d8,
        })
    }
}
