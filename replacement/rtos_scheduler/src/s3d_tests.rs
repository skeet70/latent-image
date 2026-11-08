use super::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

const LOCK_A: u32 = 0x4005_6000;
const LOCK_B: u32 = 0x400e_6000;
const OBJECT_LOCK_A: u32 = 0x4005_6004;
const OBJECT_LOCK_B: u32 = 0x400e_6004;
const QUEUE: u32 = 0x4005_9000;
const WAIT: u32 = 0x4005_9100;
const TIMER: u32 = 0x4005_9200;
const RESUME: u32 = 0x9001_0000;
const STACK: u32 = 0x4006_4000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Frame {
    sp: u32,
    r4_r11: [u32; 8],
    lr: u32,
    cpsr: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Event {
    Switch(SwitchReason),
    Notify(u32),
}

struct Model {
    memory: RefCell<BTreeMap<u32, u8>>,
    irq: Cell<bool>,
    locks: RefCell<BTreeSet<u32>>,
    executing: Domain,
    events: RefCell<Vec<Event>>,
    timers: RefCell<BTreeSet<u32>>,
    handshake: RefCell<Option<Box<dyn Fn(&Model)>>>,
    resume: RefCell<Option<Box<dyn Fn(&Model)>>>,
    caller: Cell<Frame>,
}

impl Model {
    fn new(executing: Domain) -> Self {
        let model = Self {
            memory: RefCell::new(BTreeMap::new()),
            irq: Cell::new(false),
            locks: RefCell::new(BTreeSet::new()),
            executing,
            events: RefCell::new(Vec::new()),
            timers: RefCell::new(BTreeSet::new()),
            handshake: RefCell::new(None),
            resume: RefCell::new(None),
            caller: Cell::new(Frame {
                sp: STACK + 0x800,
                r4_r11: [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47],
                lr: RESUME,
                cpsr: 0x53,
            }),
        };
        for executing in [Domain::A, Domain::B] {
            for target in [Domain::A, Domain::B] {
                model.seed_record(executing, target);
            }
        }
        for target in [Domain::A, Domain::B] {
            model.seed_tasks(target);
        }
        let words = executing.words();
        model.set_word(words.current_context, words.context(executing).0 .0);
        model.set_word(words.handler_context, 0);
        model.set_word(words.dispatch_enabled, 1);
        model.set_word(GuestAddress(model.word(words.tick_now)), 1000);
        model.set_word(words.timer_count, 0);
        model
    }

    fn seed_record(&self, executing: Domain, target: Domain) {
        let words = executing.words();
        let globals = target.words();
        let record = words.context(target);
        let lock_word = if target == Domain::A { LOCK_A } else { LOCK_B };
        let object_word = if target == Domain::A {
            OBJECT_LOCK_A
        } else {
            OBJECT_LOCK_B
        };
        self.set_word(record.id(), target.id().word());
        self.set_word(record.scheduler_lock().word(), lock_word);
        self.set_word(record.object_lock().word(), object_word);
        self.set_word(record.ready_heads(), globals.ready_heads.0);
        self.set_word(record.ready_bitmap(), globals.ready_bitmap.0);
        self.set_word(record.running(), globals.running.0);
        self.set_word(record.selected(), globals.selected.0);
        self.set_word(record.dispatch_enabled(), globals.dispatch_enabled.0);
        self.set_word(record.timer_heap(), globals.timer_heap.0);
        self.set_word(record.timer_count(), globals.timer_count.0);
        self.set_word(record.timer_base(), globals.timer_base.0);
        self.set_word(record.tick_now(), globals.tick_now.0);
        self.set_word(record.tasks(), globals.task_table.0);
        self.set_word(record.task_count(), TASK_COUNT);
        self.set_word(record.first_entry(), RESUME);
    }

    fn seed_tasks(&self, domain: Domain) {
        let words = domain.words();
        for priority in 0..16u32 {
            let head = ReadyHead(GuestAddress(words.ready_heads.0 + priority * ReadyHead::SIZE));
            self.set_word(head.next(), head.0.0);
            self.set_word(head.prev(), head.0.0);
        }
        self.set_word(words.ready_bitmap, 0);
        self.set_word(words.selected, 0);
        self.set_word(words.running, 0);
        self.set_word(words.timer_count, 0);
        for slot in 1..=TASK_COUNT {
            let task = words.task(slot_of(slot));
            let definition = words.task_definition(slot_of(slot));
            self.set_word(task.definition(), definition.0 .0);
            self.set_word(task.state(), 0);
            self.set_word(task.next(), 0);
            self.set_word(task.prev(), 0);
            self.set_word(task.wait(), 0);
            self.set_word(task.domain(), domain.id().word());
            self.set_word(definition.exists(), 1);
            self.set_word(definition.priority(), 15);
            self.set_word(definition.stack_size(), 0x100);
            self.set_word(definition.stack(), STACK);
            self.set_word(definition.slot(), slot);
        }
    }

    fn word(&self, at: GuestAddress) -> u32 {
        let memory = self.memory.borrow();
        let mut value = 0u32;
        for index in 0..4 {
            value |= (*memory.get(&(at.0 + index)).unwrap_or(&0) as u32) << (index * 8);
        }
        value
    }

    fn set_word(&self, at: GuestAddress, value: u32) {
        let mut memory = self.memory.borrow_mut();
        for index in 0..4 {
            memory.insert(at.0 + index, (value >> (index * 8)) as u8);
        }
    }

    fn set_byte(&self, at: GuestAddress, value: u8) {
        self.memory.borrow_mut().insert(at.0, value);
    }

    fn byte(&self, at: GuestAddress) -> u8 {
        *self.memory.borrow().get(&at.0).unwrap_or(&0)
    }

    fn task(&self, domain: Domain, slot: u32) -> Task {
        domain.words().task(slot_of(slot))
    }

    fn definition(&self, domain: Domain, slot: u32) -> TaskDefinition {
        domain.words().task_definition(slot_of(slot))
    }

    fn make_ready(&self, domain: Domain, slot: u32, priority: u8, selected: bool) {
        let task = self.task(domain, slot);
        let words = domain.words();
        self.set_byte(task.state(), TaskState::READY.bits());
        self.set_byte(task.priority(), priority);
        let head = ReadyHead(GuestAddress(words.ready_heads.0 + priority as u32 * ReadyHead::SIZE));
        let previous = self.word(head.prev());
        self.set_word(task.prev(), previous);
        self.set_word(task.next(), head.0.0);
        self.set_word(Task(GuestAddress(previous)).next(), task.0 .0);
        self.set_word(head.prev(), task.0 .0);
        self.set_word(words.ready_bitmap, self.word(words.ready_bitmap) | (1 << priority));
        if selected {
            self.set_word(words.selected, task.0 .0);
        }
    }

    fn make_running(&self, domain: Domain, slot: u32) {
        self.make_ready(domain, slot, 0, true);
        self.set_word(domain.words().running, self.task(domain, slot).0 .0);
    }

    fn set_object_wait(&self, domain: Domain, slot: u32, state: u8) {
        let task = self.task(domain, slot);
        self.set_byte(task.state(), state);
        self.set_word(task.wait(), WAIT);
        self.set_word(GuestAddress(WAIT).offset(0x04), QUEUE);
        self.set_word(GuestAddress(WAIT).offset(0x08), domain.id().word());
        self.set_word(GuestAddress(QUEUE).offset(0x08), QUEUE + 0x20);
        self.set_word(GuestAddress(QUEUE + 0x20), 1);
        self.set_word(GuestAddress(QUEUE), QUEUE);
        self.set_word(GuestAddress(QUEUE + 4), QUEUE);
    }

    fn link_queue(&self, domain: Domain, slot: u32) {
        let task = self.task(domain, slot);
        self.set_word(task.next(), QUEUE);
        self.set_word(task.prev(), QUEUE);
        self.set_word(GuestAddress(QUEUE), task.0 .0);
        self.set_word(GuestAddress(QUEUE + 4), task.0 .0);
    }

    fn arm_test_timer(&self, domain: Domain, slot: u32) {
        let task = self.task(domain, slot);
        self.set_word(task.wait(), WAIT);
        self.set_word(GuestAddress(WAIT), TIMER);
        self.set_word(GuestAddress(TIMER), 0);
        self.set_word(GuestAddress(TIMER + 4), timeout_callback(domain));
        self.set_word(GuestAddress(TIMER + 8), task.0 .0);
        self.timers.borrow_mut().insert(TIMER);
        let words = domain.words();
        let count = self.word(words.timer_count);
        self.set_word(words.timer_count, count + 1);
        self.set_word(GuestAddress(TIMER), count + 1);
        self.set_word(GuestAddress(words.timer_heap.0 + count * 8), 0);
        self.set_word(GuestAddress(words.timer_heap.0 + count * 8 + 4), TIMER);
    }

    fn tick_once(&self, domain: Domain) {
        let words = domain.words();
        let count = self.word(words.timer_count);
        if count == 0 {
            return;
        }
        let heap = words.timer_heap;
        let entry = GuestAddress(self.word(GuestAddress(heap.0 + 4)));
        let callback = self.word(entry.offset(4));
        let argument = self.word(entry.offset(8));
        let last = GuestAddress(heap.0 + (count - 1) * 8);
        let last_entry = self.word(last.offset(4));
        self.set_word(heap, self.word(last));
        self.set_word(GuestAddress(heap.0 + 4), last_entry);
        if count > 1 {
            self.set_word(GuestAddress(last_entry), 1);
        }
        self.set_word(words.timer_count, count - 1);
        let result = if callback == timeout_callback(domain) {
            -50
        } else if callback == delay_callback(domain) {
            0
        } else {
            return;
        };
        let task = Task(GuestAddress(argument));
        let wait = GuestAddress(self.word(task.wait()));
        self.set_word(wait, result as u32);
        if self.word(task.terminate_pending()) != 0 {
            self.reset_task(task);
            self.set_word(task.context_discarded(), 1);
            if self.byte(task.flags()) & 1 != 0 {
                self.set_byte(task.flags(), self.byte(task.flags()) & !1);
                self.restart_task(domain.words().context(domain), task);
            }
            return;
        }
        if self.word(task.priority_change_pending()) != 0 {
            self.set_byte(task.priority(), self.word(task.pending_priority()) as u8);
            self.set_word(task.priority_change_pending(), 0);
        }
        self.set_word(task.cancel_pending(), 0);
        if self.byte(task.state()) & 4 != 0 {
            self.set_byte(task.state(), 4);
        } else {
            self.insert_ready(domain.words().context(domain), task);
        }
    }

    fn finish_wait(&self, domain: Domain, slot: u32, result: i32) {
        let task = self.task(domain, slot);
        let wait = GuestAddress(self.word(task.wait()));
        let receipt = self.word(wait);
        if receipt != 0 {
            let entry = TimerEntry(GuestAddress(receipt));
            let context = domain.words().context(domain);
            if timer_heap_active(self, context, entry) {
                self.timer_remove(context, entry);
            }
        }
        self.set_word(wait, result as u32);
        if self.word(task.terminate_pending()) != 0 {
            self.reset_task(task);
            self.set_word(task.context_discarded(), 1);
            if self.byte(task.flags()) & 1 != 0 {
                self.set_byte(task.flags(), self.byte(task.flags()) & !1);
                self.restart_task(domain.words().context(domain), task);
            }
            return;
        }
        if self.word(task.priority_change_pending()) != 0 {
            self.set_byte(task.priority(), self.word(task.pending_priority()) as u8);
            self.set_word(task.priority_change_pending(), 0);
        }
        self.set_word(task.cancel_pending(), 0);
        if self.byte(task.state()) & 4 != 0 {
            self.set_byte(task.state(), 4);
        } else {
            self.insert_ready(domain.words().context(domain), task);
        }
    }
}

fn slot_of(slot: u32) -> layout::TaskSlot {
    layout::TaskSlot::new(slot).unwrap()
}

impl Machine for Model {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.set_word(at, value);
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        *self.memory.borrow().get(&at.0).unwrap_or(&0)
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        self.set_byte(at, value);
    }

    fn save_and_mask(&self) -> InterruptState {
        let state = InterruptState(if self.irq.get() { 0x80 } else { 0 });
        self.irq.set(true);
        state
    }

    fn restore(&self, state: InterruptState) {
        self.irq.set(state.masked());
    }

    fn masked(&self) -> bool {
        self.irq.get()
    }

    fn lock_acquire(&self, lock: LockRecord) -> bool {
        let word = self.word(lock.word());
        if self.locks.borrow().contains(&word) {
            false
        } else {
            self.locks.borrow_mut().insert(word);
            true
        }
    }

    fn lock_release(&self, lock: LockRecord) {
        let word = self.word(lock.word());
        self.locks.borrow_mut().remove(&word);
    }

    fn remove_ready(&self, context: KernelContext, task: Task) -> bool {
        let priority = self.byte(task.priority());
        let head = ReadyHead(GuestAddress(self.word(context.ready_heads()) + priority as u32 * ReadyHead::SIZE));
        let next = self.word(task.next());
        let previous = self.word(task.prev());
        self.set_word(Task(GuestAddress(previous)).next(), next);
        self.set_word(Task(GuestAddress(next)).prev(), previous);
        let selected_at = GuestAddress(self.word(context.selected()));
        let bitmap_at = GuestAddress(self.word(context.ready_bitmap()));
        let dispatch_at = GuestAddress(self.word(context.dispatch_enabled()));
        let selected = self.word(selected_at);
        if self.word(head.next()) == head.0 .0 {
            let bitmap = self.word(bitmap_at);
            self.set_word(bitmap_at, bitmap & !(1 << priority));
            if selected != task.0 .0 {
                return false;
            }
            let bitmap = self.word(bitmap_at);
            let next_selected = if bitmap == 0 {
                0
            } else {
                self.find_ready(context).0 .0
            };
            self.set_word(selected_at, next_selected);
            return self.word(dispatch_at) != 0;
        }
        if selected == task.0 .0 {
            self.set_word(selected_at, self.word(head.next()));
            return self.word(dispatch_at) != 0;
        }
        false
    }

    fn insert_ready(&self, context: KernelContext, task: Task) -> bool {
        let priority = self.byte(task.priority());
        let head = ReadyHead(GuestAddress(self.word(context.ready_heads()) + priority as u32 * ReadyHead::SIZE));
        self.set_byte(task.state(), TaskState::READY.bits());
        let previous = self.word(head.prev());
        self.set_word(task.prev(), previous);
        self.set_word(task.next(), head.0 .0);
        self.set_word(Task(GuestAddress(previous)).next(), task.0 .0);
        self.set_word(head.prev(), task.0 .0);
        let bitmap_at = GuestAddress(self.word(context.ready_bitmap()));
        let bitmap = self.word(bitmap_at);
        self.set_word(bitmap_at, bitmap | (1 << priority));
        let selected_at = GuestAddress(self.word(context.selected()));
        let selected = self.word(selected_at);
        if selected != 0 && priority >= self.byte(Task(GuestAddress(selected)).priority()) {
            return false;
        }
        self.set_word(selected_at, task.0 .0);
        self.word(GuestAddress(self.word(context.dispatch_enabled()))) != 0
    }

    fn reset_task(&self, task: Task) {
        let definition = TaskDefinition(GuestAddress(self.word(task.definition())));
        self.set_byte(task.priority(), self.word(definition.priority()) as u8);
        self.set_byte(task.state(), 0);
        self.set_byte(task.flags(), self.byte(task.flags()) & !0x06);
        self.set_word(task.exception_pattern(), 0);
        self.set_word(task.priority_change_pending(), 0);
        self.set_word(task.terminate_pending(), 0);
        self.set_word(task.cancel_pending(), 0);
    }

    fn restart_task(&self, context: KernelContext, task: Task) -> bool {
        let definition = TaskDefinition(GuestAddress(self.word(task.definition())));
        let saved = self.word(definition.stack_size()) + self.word(definition.stack());
        self.set_word(task.saved_sp(), saved);
        self.set_word(task.resume_pc(), self.word(context.first_entry()));
        self.insert_ready(context, task)
    }

    fn notify_dispatch(&self, context: KernelContext) -> bool {
        let id = self.word(context.id());
        if id == 0 || id == self.executing.id().word() {
            true
        } else {
            self.events.borrow_mut().push(Event::Notify(id));
            false
        }
    }

    fn switch(&self, reason: SwitchReason) {
        self.events.borrow_mut().push(Event::Switch(reason));
        let task = Task(GuestAddress(self.word(self.executing.words().running)));
        let frame = self.caller.get();
        let saved_sp = frame.sp - 36;
        for (index, value) in frame.r4_r11.iter().enumerate() {
            self.set_word(GuestAddress(saved_sp + index as u32 * 4), *value);
        }
        self.set_word(GuestAddress(saved_sp + 32), frame.lr);
        self.set_word(task.saved_sp(), saved_sp);
        self.set_word(task.resume_pc(), RESUME);
        if let Some(plan) = self.resume.borrow_mut().take() {
            plan(self);
        }
        let saved_sp = self.word(task.saved_sp());
        let mut registers = [0u32; 8];
        for (index, register) in registers.iter_mut().enumerate() {
            *register = self.word(GuestAddress(saved_sp + index as u32 * 4));
        }
        let lr = self.word(GuestAddress(saved_sp + 32));
        self.caller.set(Frame {
            sp: saved_sp + 36,
            r4_r11: registers,
            lr,
            cpsr: frame.cpsr,
        });
    }

    fn timer_insert(&self, context: KernelContext, entry: TimerEntry, expiry: u32) {
        assert!(
            !self.timers.borrow().contains(&entry.0 .0),
            "timer entry inserted twice"
        );
        let count_word = GuestAddress(self.word(context.timer_count()));
        let count = self.word(count_word);
        let node = GuestAddress(self.word(context.timer_heap()) + count * TIMER_NODE_STRIDE);
        self.set_word(node, expiry);
        self.set_word(node.offset(TIMER_NODE_ENTRY), entry.0 .0);
        self.set_word(entry.index(), count + 1);
        self.set_word(count_word, count + 1);
        self.timers.borrow_mut().insert(entry.0 .0);
    }

    fn timer_remove(&self, context: KernelContext, entry: TimerEntry) {
        assert!(
            self.timers.borrow_mut().remove(&entry.0 .0),
            "timer entry removed while inactive"
        );
        let index = self.word(entry.index());
        let count_word = GuestAddress(self.word(context.timer_count()));
        let count = self.word(count_word);
        if index != 0 && index <= count {
            let last = GuestAddress(self.word(context.timer_heap()) + (count - 1) * TIMER_NODE_STRIDE);
            let node = GuestAddress(self.word(context.timer_heap()) + (index - 1) * TIMER_NODE_STRIDE);
            let last_entry = self.word(last.offset(TIMER_NODE_ENTRY));
            if index != count {
                self.set_word(node, self.word(last));
                self.set_word(node.offset(TIMER_NODE_ENTRY), last_entry);
                self.set_word(GuestAddress(last_entry), index);
            }
            self.set_word(count_word, count - 1);
        }
    }

    fn timer_active(&self, context: KernelContext, entry: TimerEntry) -> bool {
        timer_heap_active(self, context, entry)
    }

    fn handshake_window(&self, _task: Task) {
        if let Some(plan) = self.handshake.borrow_mut().take() {
            plan(self);
        }
    }

    fn switch_window(&self, _task: Task) {}
}

impl Model {
    fn find_ready(&self, context: KernelContext) -> Task {
        let bitmap = self.word(GuestAddress(self.word(context.ready_bitmap())));
        let priority = bitmap.trailing_zeros();
        let head = ReadyHead(GuestAddress(self.word(context.ready_heads()) + priority * ReadyHead::SIZE));
        Task(GuestAddress(self.word(head.next())))
    }
}

fn id(domain: Domain, slot: u32) -> EncodedId {
    EncodedId(((domain.id().word() as i32) << 16) | slot as i32)
}

fn status(result: Result<(), ServiceError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => error.status(),
    }
}

#[test]
fn terminate_ready_task_unlinks_and_discards_context() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.make_ready(Domain::A, 2, 3, false);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    let task = model.task(Domain::A, 2);
    assert_eq!(model.byte(task.state()), 0);
    assert_eq!(model.word(task.context_discarded()), 1);
    assert_eq!(model.word(Domain::A.words().ready_bitmap), 1);
    assert!(model.events.borrow().is_empty());
}

#[test]
fn terminate_queued_activation_restarts_the_task() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.make_ready(Domain::A, 2, 3, false);
    let task = model.task(Domain::A, 2);
    model.set_byte(task.flags(), 1);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.byte(task.state()), 1);
    assert_eq!(model.word(task.context_discarded()), 1);
    assert_eq!(model.byte(task.flags()), 0);
    assert_eq!(model.word(task.saved_sp()), 0x100 + STACK);
    assert_eq!(model.word(task.resume_pc()), RESUME);
}

#[test]
fn terminate_refuses_dormant_current_and_absent_tasks() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), -41);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 1))), -28);
    model.set_word(model.definition(Domain::A, 3).exists(), 0);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 3))), -42);
    assert_eq!(status(terminate(&model, Domain::A, EncodedId(3 << 16))), -18);
    assert_eq!(status(terminate(&model, Domain::A, EncodedId(-1))), -18);
}

#[test]
fn terminate_refuses_handler_context_and_masked_interrupts() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_word(Domain::A.words().handler_context, 1);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), -25);
    model.set_word(Domain::A.words().handler_context, 0);
    model.irq.set(true);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), -25);
}

#[test]
fn terminate_sleeping_task_cancels_its_timer() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    model.arm_test_timer(Domain::A, 2);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert!(!model.timers.borrow().contains(&TIMER));
    assert_eq!(model.byte(model.task(Domain::A, 2).state()), 0);
}

#[test]
fn terminate_object_wait_unlinks_the_queue() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x12);
    model.link_queue(Domain::A, 2);
    model.arm_test_timer(Domain::A, 2);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.word(GuestAddress(QUEUE)), QUEUE);
    assert_eq!(model.word(GuestAddress(QUEUE + 4)), QUEUE);
    assert!(!model.timers.borrow().contains(&TIMER));
}

#[test]
fn terminate_cross_object_wait_takes_the_owner_lock() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    model.link_queue(Domain::A, 2);
    model.set_word(GuestAddress(WAIT).offset(0x08), 2);
    model.arm_test_timer(Domain::A, 2);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.byte(model.task(Domain::A, 2).state()), 0);
    assert!(model.locks.borrow().is_empty());
    assert_eq!(model.word(GuestAddress(QUEUE)), QUEUE);
}

#[test]
fn terminate_cross_object_concurrent_release_returns_without_resetting() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    model.link_queue(Domain::A, 2);
    model.set_word(GuestAddress(WAIT).offset(0x08), 2);
    let task = model.task(Domain::A, 2);
    *model.handshake.borrow_mut() = Some(Box::new(move |model: &Model| {
        model.set_word(task.terminate_pending(), 0);
    }));
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.byte(task.state()), 0x32);
    assert_eq!(model.word(task.context_discarded()), 0);
    assert!(!model.irq.get());
    assert!(model.locks.borrow().is_empty());
}

#[test]
fn terminate_cross_object_pending_returns_and_clears_irq() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    let task = model.task(Domain::A, 2);
    model.set_word(task.terminate_pending(), 1);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), -41);
    assert!(!model.irq.get());
    model.set_byte(task.flags(), 1);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.byte(task.flags()), 0);
    assert!(!model.irq.get());
}

#[test]
fn priority_change_requeues_a_ready_task() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.make_ready(Domain::A, 2, 5, false);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(2)
        )),
        0
    );
    let task = model.task(Domain::A, 2);
    assert_eq!(model.byte(task.priority()), 1);
    assert_eq!(model.word(Domain::A.words().ready_bitmap), 0b11);
}

#[test]
fn priority_change_zero_restores_the_creation_priority() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.make_ready(Domain::A, 2, 5, false);
    model.set_word(model.definition(Domain::A, 2).priority(), 7);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(0)
        )),
        0
    );
    assert_eq!(model.byte(model.task(Domain::A, 2).priority()), 7);
}

#[test]
fn priority_change_of_a_waiting_task_writes_only_the_priority() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(3)
        )),
        0
    );
    assert_eq!(model.byte(model.task(Domain::A, 2).priority()), 2);
}

#[test]
fn priority_change_cross_object_reorders_a_priority_queue() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    model.set_word(GuestAddress(QUEUE + 0x20), 1);
    model.link_queue(Domain::A, 2);
    let task = model.task(Domain::A, 2);
    model.set_word(task.priority_change_pending(), 0);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(1)
        )),
        0
    );
    assert_eq!(model.byte(task.priority()), 0);
    assert_eq!(model.word(task.priority_change_pending()), 0);
    assert!(model.locks.borrow().is_empty());
}

#[test]
fn priority_change_concurrent_clear_returns_without_reordering() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    let task = model.task(Domain::A, 2);
    *model.handshake.borrow_mut() = Some(Box::new(move |model: &Model| {
        model.set_word(task.priority_change_pending(), 0);
    }));
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(1)
        )),
        0
    );
    assert_eq!(model.byte(task.priority()), 0);
    assert!(!model.irq.get());
    assert!(model.locks.borrow().is_empty());
}

#[test]
fn priority_change_error_precedence() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(17)
        )),
        -17
    );
    model.set_word(model.definition(Domain::A, 2).exists(), 0);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 2),
            PriorityArgument(1)
        )),
        -42
    );
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 1),
            PriorityArgument(1)
        )),
        0
    );
    model.set_word(model.task(Domain::A, 1).context_discarded(), 1);
    assert_eq!(
        status(change_priority(
            &model,
            Domain::A,
            id(Domain::A, 1),
            PriorityArgument(1)
        )),
        -41
    );
}

#[test]
fn timed_sleep_consumes_a_queued_wakeup() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    let task = model.task(Domain::A, 1);
    model.set_byte(task.flags(), 2);
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(5)), 0);
    assert_eq!(model.byte(task.flags()), 0);
    assert!(model.events.borrow().is_empty());
}

#[test]
fn timed_sleep_zero_polls_and_negative_timeout_is_refused() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(0)), -50);
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(-2)), -17);
    model.set_word(Domain::A.words().dispatch_enabled, 0);
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(1)), -25);
}

#[test]
fn timed_sleep_timeout_resumes_the_original_caller() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    let frame = model.caller.get();
    let task = model.task(Domain::A, 1);
    *model.resume.borrow_mut() = Some(Box::new(|model: &Model| {
        model.finish_wait(Domain::A, 1, -50);
    }));
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(50)), -50);
    assert_eq!(model.caller.get(), frame);
    assert_eq!(model.byte(task.state()), 1);
    assert!(model.timers.borrow().is_empty());
    assert!(model
        .events
        .borrow()
        .contains(&Event::Switch(SwitchReason::Sleep)));
    assert!(!model.irq.get());
}

#[test]
fn timed_sleep_forever_is_released_with_minus_49() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    let frame = model.caller.get();
    *model.resume.borrow_mut() = Some(Box::new(|model: &Model| {
        model.finish_wait(Domain::A, 1, -49);
    }));
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(-1)), -49);
    assert_eq!(model.caller.get(), frame);
}

#[test]
fn timed_sleep_applies_a_pending_priority_on_timeout() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    let task = model.task(Domain::A, 1);
    model.set_word(task.priority_change_pending(), 1);
    model.set_word(task.pending_priority(), 4);
    *model.resume.borrow_mut() = Some(Box::new(|model: &Model| {
        model.finish_wait(Domain::A, 1, -50);
    }));
    assert_eq!(timed_sleep(&model, Domain::A, Ticks(10)), -50);
    assert_eq!(model.byte(task.priority()), 4);
    assert_eq!(model.word(task.priority_change_pending()), 0);
}

#[test]
fn delay_expires_and_restores_the_caller() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    let frame = model.caller.get();
    *model.resume.borrow_mut() = Some(Box::new(|model: &Model| {
        model.finish_wait(Domain::A, 1, 0);
    }));
    assert_eq!(delay(&model, Domain::A, Ticks(30)), 0);
    assert_eq!(model.caller.get(), frame);
    assert!(model
        .events
        .borrow()
        .contains(&Event::Switch(SwitchReason::Delay)));
}

#[test]
fn delay_zero_blocks_and_negative_delay_is_refused() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    *model.resume.borrow_mut() = Some(Box::new(|model: &Model| {
        model.finish_wait(Domain::A, 1, 0);
    }));
    assert_eq!(delay(&model, Domain::A, Ticks(0)), 0);
    assert_eq!(delay(&model, Domain::A, Ticks(-1)), -17);
}

#[test]
fn delay_keeps_a_suspended_task_suspended() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    let task = model.task(Domain::A, 1);
    *model.resume.borrow_mut() = Some(Box::new(|model: &Model| {
        let task = model.task(Domain::A, 1);
        model.set_byte(task.state(), 6);
        model.finish_wait(Domain::A, 1, 0);
    }));
    assert_eq!(delay(&model, Domain::A, Ticks(10)), 0);
    assert_eq!(model.byte(task.state()), 4);
}

#[test]
fn release_wakes_a_sleeping_task_with_minus_49() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    model.arm_test_timer(Domain::A, 2);
    assert_eq!(status(release_wait(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.word(GuestAddress(WAIT)), (-49i32) as u32);
    assert_eq!(model.byte(model.task(Domain::A, 2).state()), 1);
    assert!(!model.timers.borrow().contains(&TIMER));
}

#[test]
fn release_wakes_a_delayed_task_with_minus_49() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 2);
    model.arm_test_timer(Domain::A, 2);
    assert_eq!(status(release_wait(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.word(GuestAddress(WAIT)), (-49i32) as u32);
}

#[test]
fn release_refuses_a_ready_task() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.make_ready(Domain::A, 2, 3, false);
    assert_eq!(
        status(release_wait(&model, Domain::A, id(Domain::A, 2))),
        -41
    );
}

#[test]
fn release_cross_object_wait_takes_the_owner_lock() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    model.link_queue(Domain::A, 2);
    model.set_word(GuestAddress(WAIT).offset(0x08), 2);
    assert_eq!(status(release_wait(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.word(GuestAddress(QUEUE)), QUEUE);
    assert_eq!(model.byte(model.task(Domain::A, 2).state()), 1);
    assert!(model.locks.borrow().is_empty());
}

#[test]
fn release_cross_object_concurrent_clear_returns_minus_41() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 0x32);
    let task = model.task(Domain::A, 2);
    *model.handshake.borrow_mut() = Some(Box::new(move |model: &Model| {
        model.set_word(task.cancel_pending(), 0);
    }));
    assert_eq!(
        status(release_wait(&model, Domain::A, id(Domain::A, 2))),
        -41
    );
    assert!(!model.irq.get());
    assert!(model.locks.borrow().is_empty());
}

#[test]
fn release_of_a_terminated_task_resets_and_restarts_it() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    let task = model.task(Domain::A, 2);
    model.set_word(task.terminate_pending(), 1);
    model.set_byte(task.flags(), 1);
    assert_eq!(status(release_wait(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.byte(task.state()), 1);
    assert_eq!(model.word(task.context_discarded()), 1);
    assert_eq!(model.byte(task.flags()), 0);
}

#[test]
fn handler_release_sets_the_delayed_dispatch_word() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_byte(model.task(Domain::A, 1).priority(), 5);
    model.set_object_wait(Domain::A, 2, 10);
    model.set_word(Domain::A.words().handler_context, 1);
    assert_eq!(
        status(interrupt_release_wait(&model, Domain::A, id(Domain::A, 2))),
        0
    );
    assert_eq!(model.word(Domain::A.words().reschedule), 1);
    assert!(!model
        .events
        .borrow()
        .iter()
        .any(|event| matches!(event, Event::Switch(_))));
}

#[test]
fn handler_release_requires_handler_context() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    assert_eq!(
        status(interrupt_release_wait(&model, Domain::A, id(Domain::A, 2))),
        -25
    );
}

#[test]
fn remote_records_notify_and_do_not_switch() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.make_ready(Domain::B, 2, 3, true);
    model.set_word(Domain::B.words().dispatch_enabled, 1);
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::B, 2))), 0);
    assert_eq!(model.events.borrow().as_slice(), &[Event::Notify(2)]);
    assert!(!model.irq.get());
}

#[test]
fn uncancelled_timer_expires_and_changes_the_task() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    model.arm_test_timer(Domain::A, 2);
    let words = Domain::A.words();
    assert_eq!(model.word(words.timer_count), 1);
    assert_eq!(model.word(GuestAddress(words.timer_heap.0 + 4)), TIMER);
    model.tick_once(Domain::A);
    let task = model.task(Domain::A, 2);
    assert_eq!(model.word(words.timer_count), 0);
    assert_eq!(model.word(GuestAddress(WAIT)), (-50i32) as u32);
    assert_eq!(model.byte(task.state()), 1);
    let head = ReadyHead(GuestAddress(words.ready_heads.0));
    let first = model.word(head.next());
    assert_eq!(model.word(Task(GuestAddress(first)).next()), task.0 .0);
    assert_eq!(model.word(task.next()), head.0 .0);
}

#[test]
fn release_cancels_the_timer_before_the_consumer_runs() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    model.arm_test_timer(Domain::A, 2);
    let words = Domain::A.words();
    assert_eq!(status(release_wait(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.word(words.timer_count), 0);
    model.tick_once(Domain::A);
    let task = model.task(Domain::A, 2);
    assert_eq!(model.word(words.timer_count), 0);
    assert_eq!(model.word(GuestAddress(WAIT)), (-49i32) as u32);
    assert_eq!(model.byte(task.state()), 1);
    let head = ReadyHead(GuestAddress(words.ready_heads.0));
    let first = model.word(head.next());
    assert_eq!(model.word(Task(GuestAddress(first)).next()), task.0 .0);
    assert_eq!(model.word(task.next()), head.0 .0);
}

#[test]
fn terminate_cancels_the_timer_before_the_consumer_runs() {
    let model = Model::new(Domain::A);
    model.make_running(Domain::A, 1);
    model.set_object_wait(Domain::A, 2, 10);
    model.arm_test_timer(Domain::A, 2);
    let words = Domain::A.words();
    assert_eq!(status(terminate(&model, Domain::A, id(Domain::A, 2))), 0);
    assert_eq!(model.word(words.timer_count), 0);
    model.tick_once(Domain::A);
    let task = model.task(Domain::A, 2);
    assert_eq!(model.word(words.timer_count), 0);
    assert_eq!(model.byte(task.state()), 0);
    assert_eq!(model.word(task.context_discarded()), 1);
    assert_eq!(model.word(words.ready_bitmap), 1);
}
