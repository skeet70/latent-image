//! Host backend over the fixed-pool bodies; models memory, the wait queue, the timer boundary and
//! the doorbell.
use crate::layout::{
    Domain, DomainId, FixedPoolSlot, FixedPoolWait, GuestAddress, KernelContext, LockRecord, Task,
    TaskSlot, TaskState, WaitBlock, WaitQueue,
};
use crate::machine::{
    Block, DivMod, DispatchTarget, Enqueue, Irq, Locks, Machine, SaveIrq, SavedInterrupts, Selection,
    Wake,
};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

pub struct Sim {
    words: RefCell<BTreeMap<u32, u32>>,
    entry: Domain,
    interrupts: Cell<u32>,
    locks: Cell<u32>,
    armed: RefCell<BTreeMap<u32, u32>>,
    last_dispatch: Cell<Option<i32>>,
    doorbell: [Cell<bool>; 2],
}

impl Sim {
    pub fn new(entry: Domain) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            entry,
            interrupts: Cell::new(0x40),
            locks: Cell::new(0),
            armed: RefCell::new(BTreeMap::new()),
            last_dispatch: Cell::new(None),
            doorbell: [Cell::new(false), Cell::new(false)],
        }
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

    pub fn install(&self, owner: Domain, dispatch_enabled: bool) {
        let record = self.context(owner);
        let base = match owner {
            Domain::A => 0x0096_0000,
            Domain::B => 0x0096_0100,
        };
        self.set(record.id(), if owner == self.entry { 0 } else { owner.id().word() });
        self.set(record.owner(), owner.id().word());
        self.set(record.scheduler_lock().0, base + 0x10);
        self.set(record.scheduler_lock().acquire(), base + 0x14);
        self.set(record.scheduler_lock().release(), base + 0x18);
        self.set(record.object_lock().0, base + 0x20);
        self.set(record.object_lock().acquire(), base + 0x24);
        self.set(record.object_lock().release(), base + 0x28);
        self.set(record.running(), base + 0x30);
        self.set(record.selected(), base + 0x34);
        self.set(record.dispatch_enabled(), base + 0x38);
        self.set(GuestAddress(base + 0x38), dispatch_enabled as u32);
        self.set(record.tick_now(), base + 0x3c);
        self.set(record.fixed_pool_count(), 122);
    }

    pub fn context(&self, domain: Domain) -> KernelContext {
        self.entry.words().context(domain)
    }

    pub fn set_running(&self, domain: Domain, task: Task) {
        let cell = GuestAddress(self.word(self.context(domain).running()));
        self.set(cell, task.0 .0);
    }

    pub fn running(&self, domain: Domain) -> Task {
        Task(GuestAddress(self.word(GuestAddress(self.word(self.context(domain).running())))))
    }

    pub fn install_task(&self, domain: Domain, slot: TaskSlot, priority: u8) {
        let task = domain.words().task(slot);
        let base = if domain == Domain::A { 0 } else { 120 * 0x40 };
        let definition = GuestAddress(0x0096_c000 + base + (slot.get() - 1) * 0x40);
        self.set(task.next(), task.0 .0);
        self.set(task.prev(), task.0 .0);
        self.set(task.definition(), definition.0);
        self.set(definition.offset(0x20), slot.get());
        self.set_byte(definition.offset(0x0c), priority);
        self.set(task.domain(), domain.id().word());
        self.set_byte(task.state(), TaskState::DORMANT.bits());
        self.set_byte(task.priority(), priority);
        self.set_byte(task.flags(), 0);
        self.set(task.saved_sp(), 0);
        self.set(task.resume_pc(), 0);
    }

    pub fn install_pool(&self, domain: Domain, slot: FixedPoolSlot, create: crate::create::PoolCreate) {
        let words = domain.words();
        let config = words.fixed_pool_config(slot);
        let control = words.fixed_pool_control(slot);
        self.set(config.attributes(), create.attributes.0);
        self.set(config.block_size(), create.stride());
        self.set(config.begin(), create.buffer.0);
        self.set(config.end(), create.end().0);
        self.set(config.exists(), 1);
        self.set(control.queue().next(), control.0 .0);
        self.set(control.queue().prev(), control.0 .0);
        self.set(control.queue().config(), config.0 .0);
        self.set(control.next_free(), create.buffer.0);
        self.set(control.recycled(), 0);
    }

    pub fn task(&self, domain: Domain, slot: TaskSlot) -> Task {
        domain.words().task(slot)
    }

    pub fn wait(&self, domain: Domain, slot: TaskSlot) -> FixedPoolWait {
        domain.words().fixed_pool_wait(slot)
    }

    pub fn timer(&self, domain: Domain, slot: TaskSlot) -> GuestAddress {
        domain.words().fixed_pool_timer(slot)
    }

    pub fn armed(&self, timer: GuestAddress) -> Option<u32> {
        self.armed.borrow().get(&timer.0).copied()
    }

    pub fn dispatched(&self) -> Option<i32> {
        self.last_dispatch.get()
    }

    pub fn doorbell(&self, core: usize) -> bool {
        self.doorbell[core].get()
    }

    pub fn interrupts(&self) -> u32 {
        self.interrupts.get()
    }

    pub fn set_interrupts(&self, value: u32) {
        self.interrupts.set(value);
    }

    fn task_priority(&self, task: Task) -> u8 {
        let at = task.priority().0;
        (self.word(GuestAddress(at & !3)) >> (8 * (at & 3))) as u8
    }

    fn enqueue_task(&self, queue: WaitQueue, task: Task, config: GuestAddress) {
        let head = queue.0;
        let priority_order = self.word(config) & 1 != 0;
        let (mut at, mut next) = (GuestAddress(self.word(queue.prev())), head);
        if priority_order {
            at = head;
            next = GuestAddress(self.word(head));
            let candidate = self.task_priority(task);
            while next.0 != head.0 {
                let existing = Task(next);
                if self.task_priority(existing) > candidate {
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
    }

    fn insert_ready(&self, task: Task) -> Selection {
        self.set_byte(task.state(), TaskState::READY.bits());
        Selection::Changed
    }
}

impl Machine for Sim {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.set(at, value);
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        let aligned = GuestAddress(at.0 & !3);
        (self.word(aligned) >> (8 * (at.0 & 3))) as u8
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        self.set_byte(at, value);
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

impl Locks for Sim {
    fn lock(&self, _record: LockRecord) {
        self.locks.set(self.locks.get() + 1);
    }

    fn unlock(&self, _record: LockRecord) {
        assert!(self.locks.get() > 0, "unlock without a lock");
        self.locks.set(self.locks.get() - 1);
    }
}

impl Irq for Sim {
    fn disable_irq(&self) {
        self.interrupts.set(self.interrupts.get() | 0x80);
    }

    fn enable_irq(&self) {
        self.interrupts.set(self.interrupts.get() & !0x80);
    }
}

impl DivMod for Sim {
    fn divmod(&self, numerator: u32, denominator: u32) -> (u32, u32) {
        (numerator / denominator, numerator % denominator)
    }
}

impl SaveIrq for Sim {
    fn save_irq(&self) -> u32 {
        let saved = self.interrupts.get();
        self.interrupts.set(saved | 0x80);
        saved
    }

    fn restore_irq(&self, saved: u32) {
        self.interrupts.set(saved);
    }
}

impl Wake for Sim {
    fn context(&self, domain: Domain) -> KernelContext {
        Sim::context(self, domain)
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

    fn notify(&self, id: u32) -> DispatchTarget {
        if id == 0 || id == self.entry.id().word() {
            DispatchTarget::Local
        } else {
            self.doorbell[(id - 1) as usize].set(true);
            DispatchTarget::Remote
        }
    }

    fn dispatch(&self, code: i32) {
        self.last_dispatch.set(Some(code));
    }

    fn release(&self, _owner: KernelContext, task: Task) -> Selection {
        let wait = GuestAddress(self.word(task.wait()));
        if self.word(wait) != 0 {
            self.armed.borrow_mut().remove(&self.word(wait));
        }
        self.set(wait, 0);
        if self.word(task.terminate_pending()) != 0 {
            self.set_byte(task.state(), TaskState::DORMANT.bits());
            self.set(task.context_discarded(), 1);
            return Selection::Unchanged;
        }
        if self.word(task.priority_change_pending()) != 0 {
            let priority = self.word(task.pending_priority()) as u8;
            self.set_byte(task.priority(), priority);
            self.set(task.priority_change_pending(), 0);
        }
        self.set(task.cancel_pending(), 0);
        if self.load_byte(task.state()) & TaskState::SUSPENDED.bits() != 0 {
            self.set_byte(task.state(), TaskState::SUSPENDED.bits());
            return Selection::Unchanged;
        }
        self.insert_ready(task)
    }

    fn cancel(&self, _owner: KernelContext, task: Task) -> Selection {
        let wait = GuestAddress(self.word(task.wait()));
        if self.word(wait) != 0 {
            self.armed.borrow_mut().remove(&self.word(wait));
        }
        if self.load_byte(task.state()) & TaskState::OBJECT_QUEUED.bits() != 0 {
            let prev = self.word(task.prev());
            let next = self.word(task.next());
            self.set(GuestAddress(prev), next);
            self.set(GuestAddress(next).offset(0x04), prev);
        }
        self.set(wait, (-49i32) as u32);
        self.insert_ready(task)
    }
}

impl Block for Sim {
    fn enqueue(
        &self,
        entry: Domain,
        owner: u32,
        queue: WaitQueue,
        wait: FixedPoolWait,
        how: Enqueue,
    ) {
        let task = self.running(entry);
        self.set_byte(
            task.state(),
            TaskState::WAIT
                .union(TaskState::OBJECT_QUEUED)
                .union(TaskState::OWNER_LOCK_QUEUED)
                .bits(),
        );
        self.set(task.wait(), wait.0 .0);
        match how {
            Enqueue::Plain => self.set(wait.base().result(), 0),
            Enqueue::Timed { timer, timeout } => {
                self.set(wait.base().result(), timer.0);
                let tick_cell = GuestAddress(self.word(self.context(entry).tick_now()));
                let tick = self.word(tick_cell);
                self.armed
                    .borrow_mut()
                    .insert(timer.0, tick.wrapping_add(timeout as u32));
            }
        }
        self.set(WaitBlock(wait.0).queue(), queue.0 .0);
        self.set(WaitBlock(wait.0).owner(), owner);
        let config = GuestAddress(self.word(queue.config()));
        self.enqueue_task(queue, task, config);
    }
}
