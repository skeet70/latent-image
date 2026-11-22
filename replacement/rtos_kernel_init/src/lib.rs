//! X-M1 1.50 kernel startup in both loaders: interface publication, `kernel_start`, the
//! per-core configuration record, owner contexts, the primary tick timer, task
//! initialization and autostart, `kernel_initialize_objects` and the five object-table
//! initializers. Loader A's publication is `primary_initializer`'s.
//! Every other routine startup calls stays stock and is reached at its loader address.

#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub mod interfaces_b;

#[cfg(test)]
mod tests;

use layout::{Domain, DomainWords, GuestAddress, KernelContext, Task, TaskDefinition};

/// A routine in the loader image of the executing domain.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CodeAddress(pub u32);

/// `kernel_core_index`: zero on the core whose CP14 identity is `0x4003`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Core {
    Primary,
    Peer,
}

impl Core {
    #[inline(always)]
    pub const fn from_index(index: u32) -> Self {
        if index == 0 {
            Core::Primary
        } else {
            Core::Peer
        }
    }
}

/// The argument `kernel_start` passes to its caller's phase routine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Phase {
    Setup = 0,
    Root = 1,
    Diagnostic = 2,
    Idle = 3,
    PeerTasks = 4,
}

/// The `kernel_start` argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct PhaseRoutine(pub u32);

/// A word of the loader image that holds a table length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CountWord(pub GuestAddress);

/// One core's 0x70-byte record in the shared table at `0x400c0818`. `owner_contexts`
/// copies `+0x4..+0x6c` into a [`KernelContext`] at `+0x8..+0x80`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct KernelConfig(pub GuestAddress);

impl KernelConfig {
    pub const SIZE: u32 = 0x70;
    const CONTEXT_SHIFT: u32 = 0x14;

    #[inline(always)]
    pub const fn id(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn scheduler_lock_word(self) -> GuestAddress {
        self.0.offset(0x04)
    }

    #[inline(always)]
    pub const fn object_lock_word(self) -> GuestAddress {
        self.0.offset(0x08)
    }

    /// The word `owner_contexts` copies to `context + offset`, for `offset >= 0x20`.
    #[inline(always)]
    const fn source_of(self, context_offset: u32) -> GuestAddress {
        self.0.offset(context_offset - Self::CONTEXT_SHIFT)
    }

    #[inline(always)]
    pub const fn ready_heads(self) -> GuestAddress {
        self.0.offset(0x0c)
    }

    #[inline(always)]
    pub const fn ready_bitmap(self) -> GuestAddress {
        self.0.offset(0x10)
    }

    #[inline(always)]
    pub const fn running(self) -> GuestAddress {
        self.0.offset(0x14)
    }

    #[inline(always)]
    pub const fn selected(self) -> GuestAddress {
        self.0.offset(0x18)
    }

    #[inline(always)]
    pub const fn dispatch_enabled(self) -> GuestAddress {
        self.0.offset(0x1c)
    }

    #[inline(always)]
    pub const fn tasks(self) -> GuestAddress {
        self.0.offset(0x30)
    }

    #[inline(always)]
    pub const fn task_count(self) -> GuestAddress {
        self.0.offset(0x34)
    }

    #[inline(always)]
    pub const fn first_entry(self) -> GuestAddress {
        self.0.offset(0x68)
    }
}

/// The control-block table and length an object initializer publishes in the
/// executing core's [`KernelConfig`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjectTable {
    pub controls: GuestAddress,
    pub control_size: u32,
    pub configs: GuestAddress,
    pub config_size: u32,
    pub count: CountWord,
    pub config_offset: u32,
}

/// The five object kinds `kernel_initialize_objects` initializes, in its call order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectKind {
    Semaphore,
    EventFlag,
    DataQueue,
    Mailbox,
    FixedPool,
}

/// Retained stock routines of one loader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Routines {
    pub core_index: CodeAddress,
    pub zero_words: CodeAddress,
    pub syslog_initialize: CodeAddress,
    pub syslog_record: CodeAddress,
    pub enter_handler_context: CodeAddress,
    pub initialize_irqs: CodeAddress,
    pub enable_vfp: CodeAddress,
    pub register_core_notify: CodeAddress,
    pub reset_core_sync: CodeAddress,
    pub wait_core_sync: CodeAddress,
    pub core_rendezvous: CodeAddress,
    pub initialize_timers: CodeAddress,
    pub startup_hook: CodeAddress,
    pub set_exception_hook: CodeAddress,
    pub initialize_cyclics: CodeAddress,
    pub configure_cyclics: CodeAddress,
    pub object_hook: CodeAddress,
    pub patch_vectors: CodeAddress,
    pub dispatch: CodeAddress,
    pub define_irq: CodeAddress,
    pub enable_irq_slot: CodeAddress,
    pub reset_task: CodeAddress,
    pub make_ready: CodeAddress,
}

/// Addresses `kernel_start` and its initializers use in one loader domain.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Startup {
    pub domain: Domain,
    pub words: DomainWords,
    pub routines: Routines,
    /// Stored, never called, by the startup code.
    pub tick_handler: CodeAddress,
    pub first_entry: CodeAddress,
    pub lock_acquire: CodeAddress,
    pub lock_release: CodeAddress,
    /// Holds the executing core's [`KernelConfig`].
    pub config_pointer: GuestAddress,
    pub task_count: CountWord,
    pub started: GuestAddress,
    pub initial_stack_word: GuestAddress,
    pub initial_queue_word: GuestAddress,
    pub initial_pool_storage: GuestAddress,
    pub reserved_configs: GuestAddress,
    pub vector_patch: GuestAddress,
}

/// Cross-core words, the syslog area and the primary tick timer; both loaders use the same.
pub mod shared {
    use super::GuestAddress;

    /// Core 1 polls it before its own startup.
    pub const CORE1_RELEASE: GuestAddress = GuestAddress(0x400a_00c4);
    pub const SYSLOG_AREA: GuestAddress = GuestAddress(0x400a_0800);
    pub const SYSLOG_AREA_BYTES: u32 = 0x2_0100;
    pub const PRIMARY_TICK_COUNT: GuestAddress = GuestAddress(0x400c_0800);
    pub const CONFIGS: GuestAddress = GuestAddress(0x400c_0818);
    pub const TIMER_COUNT: GuestAddress = GuestAddress(0xfff9_1000);
    pub const TIMER_RELOAD: GuestAddress = GuestAddress(0xfff9_1004);
    pub const TIMER_CONTROL: GuestAddress = GuestAddress(0xfff9_1008);
}

pub const OWNERS: u32 = 2;
pub const READY_PRIORITIES: u32 = 16;
pub const TICK_IRQ: u32 = 32;
pub const TICK_RELOAD: u32 = 30_000;
pub const TIMER_COUNTS_PER_TICK: u32 = 1875;
/// Enable, start, periodic, clock 2, interrupt clear.
pub const TIMER_RUNNING: u32 = 0x23;
pub const TIMER_STOPPED: u32 = 0;
pub const TIMER_SETTLE_PAUSES: u32 = 100;
pub const RELEASE_POLL_PAUSES: u32 = 100;
pub const DISPATCH_CODE: u32 = -10i32 as u32;
pub const TICK_IRQ_PRIORITY: u32 = 1;
const TASK_AUTOSTART: u32 = 2;
const TASK_FLAG_QUEUED: u8 = 1;

impl Startup {
    pub const fn of(domain: Domain) -> Self {
        let (code, ram) = match domain {
            Domain::A => (0u32, 0u32),
            Domain::B => (0x9_0000, 0x8_ffc0),
        };
        const fn shift(address: u32, by: u32) -> u32 {
            address + by
        }
        // Routines whose B entry is not A + 0x90000.
        let (irq_init, timers, objects_hook, cyclic_conf, cyclic_init, define, reset, ready, acquire, release) = match domain {
            Domain::A => (0x43a3c, 0x44a50, 0x4ce78, 0x4be3c, 0x4bf3c, 0x4d8fc, 0x43dd0, 0x43f70, 0x439d8, 0x43a14),
            Domain::B => (0xd39f0, 0xd4a10, 0xdce38, 0xdbdfc, 0xdbefc, 0xdd8c0, 0xd3d8c, 0xd3f2c, 0xd398c, 0xd39c8),
        };
        let (object_hook, patch_vectors, counts) = match domain {
            Domain::A => (0x4ccbc, 0x4ccd0, 0x4dab0),
            Domain::B => (0xdcc7c, 0xdcc90, 0xdda74),
        };
        Startup {
            domain,
            words: domain.words(),
            routines: Routines {
                core_index: CodeAddress(shift(0x427b8, code)),
                zero_words: CodeAddress(shift(0x434f0, code)),
                syslog_initialize: CodeAddress(shift(0x41354, code)),
                syslog_record: CodeAddress(shift(0x415a0, code)),
                enter_handler_context: CodeAddress(shift(0x43834, code)),
                initialize_irqs: CodeAddress(irq_init),
                enable_vfp: CodeAddress(shift(0x424f8, code)),
                register_core_notify: CodeAddress(shift(0x40d98, code)),
                reset_core_sync: CodeAddress(shift(0x4386c, code)),
                wait_core_sync: CodeAddress(shift(0x438a8, code)),
                core_rendezvous: CodeAddress(shift(0x43934, code)),
                initialize_timers: CodeAddress(timers),
                startup_hook: CodeAddress(objects_hook),
                set_exception_hook: CodeAddress(shift(0x40c6c, code)),
                initialize_cyclics: CodeAddress(cyclic_init),
                configure_cyclics: CodeAddress(cyclic_conf),
                object_hook: CodeAddress(object_hook),
                patch_vectors: CodeAddress(patch_vectors),
                dispatch: CodeAddress(shift(0x43688, code)),
                define_irq: CodeAddress(define),
                enable_irq_slot: CodeAddress(shift(0x41108, code)),
                reset_task: CodeAddress(reset),
                make_ready: CodeAddress(ready),
            },
            tick_handler: CodeAddress(shift(0x40b24, code)),
            first_entry: CodeAddress(shift(0x43728, code)),
            lock_acquire: CodeAddress(acquire),
            lock_release: CodeAddress(release),
            config_pointer: GuestAddress(shift(0x4005_46d4, ram)),
            task_count: CountWord(GuestAddress(counts + 0x0c)),
            started: GuestAddress(shift(0x4005_927c, ram)),
            initial_stack_word: GuestAddress(shift(0x4005_9280, ram)),
            initial_queue_word: GuestAddress(shift(0x4005_9284, ram)),
            initial_pool_storage: GuestAddress(shift(0x4005_9288, ram)),
            reserved_configs: GuestAddress(shift(0x4005_dcd8, ram)),
            vector_patch: GuestAddress(shift(0x4005_f578, ram)),
        }
    }

    /// The owner number of the executing loader's own context and config.
    #[inline(always)]
    pub const fn own_owner(&self) -> u32 {
        self.domain.id().word()
    }

    #[inline(always)]
    pub const fn config(&self, owner: u32) -> KernelConfig {
        KernelConfig(shared::CONFIGS.offset((owner - 1) * KernelConfig::SIZE))
    }

    #[inline(always)]
    pub const fn context(&self, owner: u32) -> KernelContext {
        KernelContext(self.words.contexts.offset((owner - 1) * KernelContext::SIZE))
    }

    #[inline(always)]
    const fn count(&self, offset: u32) -> CountWord {
        CountWord(self.task_count.0.offset(offset.wrapping_sub(0x0c)))
    }

    pub const fn table(&self, kind: ObjectKind) -> ObjectTable {
        let w = &self.words;
        match kind {
            ObjectKind::Semaphore => ObjectTable { controls: w.semaphore_controls, control_size: 0x10, configs: w.semaphore_configs, config_size: 0x10, count: self.count(0x08), config_offset: 0x38 },
            ObjectKind::EventFlag => ObjectTable { controls: w.flag_controls, control_size: 0x10, configs: w.flag_configs, config_size: 0x0c, count: self.count(0x14), config_offset: 0x40 },
            ObjectKind::DataQueue => ObjectTable { controls: w.data_queue_controls, control_size: 0x20, configs: w.data_queue_configs, config_size: 0x10, count: self.count(0x10), config_offset: 0x48 },
            ObjectKind::Mailbox => ObjectTable { controls: w.mailbox_controls, control_size: 0x14, configs: w.mailbox_configs, config_size: 0x0c, count: self.count(0x04), config_offset: 0x50 },
            ObjectKind::FixedPool => ObjectTable { controls: w.fixed_pool_controls, control_size: 0x14, configs: w.fixed_pool_configs, config_size: 0x14, count: self.count(0x00), config_offset: 0x58 },
        }
    }
}

/// The machine the startup code runs on: guest memory, the retained stock routines and
/// the instructions that are not memory accesses.
pub trait Machine {
    fn load(&self, at: GuestAddress) -> u32;
    fn store(&self, at: GuestAddress, value: u32);
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_byte(&self, at: GuestAddress, value: u8);
    /// A retained routine with up to three word arguments; unused ones are ignored.
    fn call(&self, routine: CodeAddress, arguments: [u32; 3]) -> u32;
    /// A retained routine taking a word and a pointer to `record`.
    fn call_with_record(&self, routine: CodeAddress, first: u32, record: &[u32; 5]) -> u32;
    fn phase(&self, routine: PhaseRoutine, phase: Phase) -> u32;
    /// The loader domain's interface publication.
    fn publish_interfaces(&self);
    /// `dsb sy`.
    fn barrier(&self);
    /// One `nop`.
    fn pause(&self);
}

#[inline(always)]
fn call0<M: Machine>(machine: &M, routine: CodeAddress) -> u32 {
    machine.call(routine, [0; 3])
}

#[inline(always)]
fn call1<M: Machine>(machine: &M, routine: CodeAddress, first: u32) -> u32 {
    machine.call(routine, [first, 0, 0])
}

/// Stock `kernel_start` (A `0x409ac`, B `0xd09ac`). Returns only if the dispatcher does.
#[inline(never)]
pub fn kernel_start<M: Machine>(machine: &M, startup: &Startup, phase: PhaseRoutine) {
    let r = &startup.routines;
    let core = Core::from_index(call0(machine, r.core_index));
    match core {
        Core::Primary => {
            machine.call(r.zero_words, [shared::SYSLOG_AREA.0, shared::SYSLOG_AREA_BYTES, 0]);
        }
        Core::Peer => {
            while machine.load(shared::CORE1_RELEASE) == 0 {
                for _ in 0..RELEASE_POLL_PAUSES {
                    machine.pause();
                }
            }
        }
    }
    machine.publish_interfaces();
    call1(machine, r.syslog_initialize, 0);
    call0(machine, r.enter_handler_context);
    call0(machine, r.initialize_irqs);
    call0(machine, r.enable_vfp);
    call0(machine, r.register_core_notify);
    match core {
        Core::Primary => {
            machine.phase(phase, Phase::Setup);
            machine.store(shared::CORE1_RELEASE, 1);
            machine.barrier();
            call0(machine, r.reset_core_sync);
        }
        Core::Peer => {
            call0(machine, r.wait_core_sync);
        }
    }
    configure_core(machine, startup);
    call0(machine, r.initialize_timers);
    initialize_objects(machine, startup);
    call0(machine, r.startup_hook);
    call0(machine, r.core_rendezvous);
    initialize_owner_contexts(machine, startup);
    start_tick_timer(machine, startup);
    match core {
        Core::Primary => {
            machine.phase(phase, Phase::Root);
            machine.phase(phase, Phase::Idle);
        }
        Core::Peer => {
            machine.phase(phase, Phase::PeerTasks);
        }
    }
    let hook = machine.phase(phase, Phase::Diagnostic);
    call1(machine, r.set_exception_hook, hook);
    autostart_tasks(machine, startup);
    call0(machine, r.initialize_cyclics);
    call1(machine, r.syslog_initialize, 1);
    machine.store(startup.started, 1);
    call1(machine, r.dispatch, DISPATCH_CODE);
    machine.call(r.syslog_record, [1, 0, 0]);
}

/// Stock A `0x43a58`, B `0xd3a0c`: the executing core's config record and current context.
#[inline(never)]
pub fn configure_core<M: Machine>(machine: &M, startup: &Startup) {
    let owner = startup.own_owner();
    let config = startup.config(owner);
    let words = &startup.words;
    machine.store(startup.config_pointer, config.0 .0);
    machine.store(config.id(), owner);
    machine.store(words.scheduler_lock, 0);
    machine.store(config.scheduler_lock_word(), words.scheduler_lock.0);
    machine.store(words.object_lock, 0);
    machine.store(config.object_lock_word(), words.object_lock.0);
    machine.store(words.current_context, startup.context(owner).0 .0);
}

/// `kernel_initialize_owner_contexts` (A `0x43ac0`, B `0xd3a7c`): both contexts from both
/// cores' config records.
#[inline(never)]
pub fn initialize_owner_contexts<M: Machine>(machine: &M, startup: &Startup) {
    let acquire = startup.lock_acquire.0;
    let release = startup.lock_release.0;
    for owner in 1..=OWNERS {
        let config = startup.config(owner);
        let context = startup.context(owner);
        let id = if owner == startup.own_owner() { 0 } else { machine.load(config.id()) };
        machine.store(context.id(), id);
        machine.store(context.owner(), owner);
        machine.store(context.scheduler_lock().acquire(), acquire);
        machine.store(context.scheduler_lock().release(), release);
        machine.store(context.object_lock().acquire(), acquire);
        machine.store(context.object_lock().release(), release);
        machine.store(context.object_lock().word(), machine.load(config.object_lock_word()));
        machine.store(context.scheduler_lock().word(), machine.load(config.scheduler_lock_word()));
        let mut offset = 0x20;
        while offset <= 0x80 {
            machine.store(context.0.offset(offset), machine.load(config.source_of(offset)));
            offset += 4;
        }
    }
}

/// The reload that starts the first tick at the timer's current phase.
#[inline(always)]
pub const fn first_reload(count: u32) -> u32 {
    TICK_RELOAD - (count % TIMER_COUNTS_PER_TICK).wrapping_mul(TICK_RELOAD) / TIMER_COUNTS_PER_TICK
}

/// `kernel_start_tick_timer` (A `0x40b5c`, B `0xd0b5c`).
#[inline(never)]
pub fn start_tick_timer<M: Machine>(machine: &M, startup: &Startup) {
    let r = &startup.routines;
    machine.store(shared::PRIMARY_TICK_COUNT, 0);
    let request = [0, startup.tick_handler.0, TICK_IRQ_PRIORITY, 1, 1];
    machine.call_with_record(r.define_irq, TICK_IRQ, &request);
    if Core::from_index(call0(machine, r.core_index)) == Core::Primary {
        let reload = first_reload(machine.load(shared::TIMER_COUNT));
        machine.store(shared::PRIMARY_TICK_COUNT, machine.load(shared::TIMER_COUNT) / TIMER_COUNTS_PER_TICK);
        machine.store(shared::TIMER_CONTROL, TIMER_STOPPED);
        machine.store(shared::TIMER_RELOAD, reload);
        for _ in 0..TIMER_SETTLE_PAUSES {
            machine.pause();
        }
        machine.store(shared::TIMER_CONTROL, TIMER_RUNNING);
        machine.store(shared::TIMER_RELOAD, TICK_RELOAD);
    }
    call1(machine, r.enable_irq_slot, TICK_IRQ);
}

#[inline(always)]
fn task_at(startup: &Startup, index: u32) -> Task {
    Task(startup.words.task_table.offset(index.wrapping_mul(Task::SIZE)))
}

/// `kernel_initialize_tasks` (A `0x43e18`, B `0xd3dd4`).
#[inline(never)]
pub fn initialize_tasks<M: Machine>(machine: &M, startup: &Startup) {
    let w = &startup.words;
    machine.store(w.selected, 0);
    machine.store(w.running, 0);
    machine.store(w.reschedule, 0);
    machine.store(w.dispatch_enabled, 1);
    for priority in 0..READY_PRIORITIES {
        let head = w.ready_heads.offset(priority * 8);
        machine.store(head, head.0);
        machine.store(head.offset(4), head.0);
    }
    machine.store(w.ready_bitmap, 0);
    let mut index = 0;
    while machine.load(startup.task_count.0) > index {
        let task = task_at(startup, index);
        let definition = TaskDefinition(w.task_definitions.offset(index * TaskDefinition::SIZE));
        machine.store(task.definition(), definition.0 .0);
        machine.store_byte(task.flags(), machine.load_byte(task.flags()) & !TASK_FLAG_QUEUED);
        machine.store(task.domain(), startup.domain.id().word());
        machine.store(task.priority_change_pending(), 0);
        machine.store(task.terminate_pending(), 0);
        machine.store(task.cancel_pending(), 0);
        machine.store(task.context_discarded(), 0);
        call1(machine, startup.routines.reset_task, task.0 .0);
        index += 1;
    }
    let count = machine.load(startup.task_count.0);
    let config = KernelConfig(GuestAddress(machine.load(startup.config_pointer)));
    machine.store(config.ready_heads(), w.ready_heads.0);
    machine.store(config.ready_bitmap(), w.ready_bitmap.0);
    machine.store(config.running(), w.running.0);
    machine.store(config.selected(), w.selected.0);
    machine.store(config.dispatch_enabled(), w.dispatch_enabled.0);
    machine.store(config.tasks(), w.task_table.0);
    machine.store(config.task_count(), count);
    machine.store(config.first_entry(), startup.first_entry.0);
}

/// `kernel_autostart_tasks` (A `0x43f9c`, B `0xd3f58`). Slot ids are not range-checked.
#[inline(never)]
pub fn autostart_tasks<M: Machine>(machine: &M, startup: &Startup) {
    let mut index = 0;
    while machine.load(startup.task_count.0) > index {
        let slot = machine.load(startup.words.task_slot_ids.offset(index * 4));
        let task = task_at(startup, slot.wrapping_sub(1));
        let definition = machine.load(task.definition());
        if machine.load(GuestAddress(definition)) & TASK_AUTOSTART != 0 {
            let context = machine.load(startup.words.current_context);
            machine.call(startup.routines.make_ready, [context, task.0 .0, 0]);
        }
        index += 1;
    }
}

/// One object-table initializer: semaphore A `0x47580`, flag `0x48044`, data queue
/// `0x48e38`, mailbox `0x4a9bc`, fixed pool `0x4b38c`; B entries in the contract.
#[inline(always)]
pub fn initialize_object_table<M: Machine>(machine: &M, startup: &Startup, kind: ObjectKind) {
    let table = startup.table(kind);
    let count = machine.load(table.count.0);
    for index in 0..count {
        let control = table.controls.offset(index * table.control_size);
        let config = table.configs.offset(index * table.config_size);
        machine.store(control, control.0);
        machine.store(control.offset(4), control.0);
        machine.store(control.offset(8), config.0);
        match kind {
            ObjectKind::Semaphore | ObjectKind::EventFlag => {
                machine.store(control.offset(0x0c), machine.load(config.offset(4)));
            }
            ObjectKind::DataQueue => {
                let receivers = control.offset(0x0c);
                machine.store(receivers, receivers.0);
                machine.store(receivers.offset(4), receivers.0);
                machine.store(control.offset(0x14), 0);
                machine.store(control.offset(0x18), 0);
                machine.store(control.offset(0x1c), 0);
            }
            ObjectKind::Mailbox => {
                machine.store(control.offset(0x0c), 0);
            }
            ObjectKind::FixedPool => {
                machine.store(control.offset(0x0c), machine.load(config.offset(8)));
                machine.store(control.offset(0x10), 0);
            }
        }
    }
    let config = GuestAddress(machine.load(startup.config_pointer));
    machine.store(config.offset(table.config_offset), table.controls.0);
    machine.store(config.offset(table.config_offset + 4), count);
}

#[inline(never)]
pub fn initialize_semaphores<M: Machine>(machine: &M, startup: &Startup) {
    initialize_object_table(machine, startup, ObjectKind::Semaphore);
}

#[inline(never)]
pub fn initialize_event_flags<M: Machine>(machine: &M, startup: &Startup) {
    initialize_object_table(machine, startup, ObjectKind::EventFlag);
}

#[inline(never)]
pub fn initialize_data_queues<M: Machine>(machine: &M, startup: &Startup) {
    initialize_object_table(machine, startup, ObjectKind::DataQueue);
}

#[inline(never)]
pub fn initialize_mailboxes<M: Machine>(machine: &M, startup: &Startup) {
    initialize_object_table(machine, startup, ObjectKind::Mailbox);
}

#[inline(never)]
pub fn initialize_fixed_pools<M: Machine>(machine: &M, startup: &Startup) {
    initialize_object_table(machine, startup, ObjectKind::FixedPool);
}

#[inline(always)]
fn each_record(records: GuestAddress, count: u32, size: u32, mut write: impl FnMut(GuestAddress)) {
    for index in 0..count {
        write(records.offset(index * size));
    }
}

/// Ascending word stores of zero.
#[inline(always)]
fn zero_words<M: Machine>(machine: &M, start: GuestAddress, words: u32) {
    for index in 0..words {
        machine.store(start.offset(index * 4), 0);
    }
}

/// `kernel_initialize_objects` (A `0x4cea0`, B `0xdce60`): the creation records, then the
/// task and object initializers and the retained cyclic, hook and vector-patch routines.
#[inline(never)]
pub fn initialize_objects<M: Machine>(machine: &M, startup: &Startup) {
    let w = &startup.words;
    for index in 0..layout::TaskSlot::COUNT {
        let definition = TaskDefinition(w.task_definitions.offset(index * TaskDefinition::SIZE));
        machine.store(definition.attributes(), 0);
        machine.store(definition.argument(), index);
        machine.store(definition.entry(), 0);
        machine.store(definition.priority(), 15);
        machine.store(definition.stack_size(), 4);
        machine.store(definition.stack(), startup.initial_stack_word.0);
        machine.store(definition.exception_attributes(), 0);
        machine.store(definition.exception_handler(), 0);
        machine.store(definition.slot(), index + 1);
        machine.store(definition.exists(), 0);
        machine.store(w.task_slot_ids.offset(index * 4), index + 1);
    }
    each_record(w.semaphore_configs, layout::SemaphoreSlot::COUNT, 0x10, |config| {
        machine.store(config, 0);
        machine.store(config.offset(4), 1);
        machine.store(config.offset(8), 1);
        machine.store(config.offset(0xc), 0);
    });
    each_record(w.flag_configs, layout::FlagSlot::COUNT, 0x0c, |config| {
        machine.store(config, 0);
        machine.store(config.offset(4), 0);
        machine.store(config.offset(8), 0);
    });
    each_record(w.data_queue_configs, layout::DataQueueSlot::COUNT, 0x10, |config| {
        machine.store(config, 0);
        machine.store(config.offset(4), 1);
        machine.store(config.offset(8), startup.initial_queue_word.0);
        machine.store(config.offset(0xc), 0);
    });
    each_record(w.mailbox_configs, layout::MailboxSlot::COUNT, 0x0c, |config| {
        machine.store(config, 2);
        machine.store(config.offset(4), 32);
        machine.store(config.offset(8), 0);
    });
    let pool = startup.initial_pool_storage;
    each_record(w.fixed_pool_configs, layout::FixedPoolSlot::COUNT, 0x14, |config| {
        machine.store(config, 0);
        machine.store(config.offset(4), 4);
        machine.store(config.offset(8), pool.0);
        machine.store(config.offset(0xc), pool.offset(4).0);
        machine.store(config.offset(0x10), 0);
    });
    each_record(w.cyclic_configs, layout::CyclicSlot::COUNT, 0x18, |config| zero_words(machine, config.offset(4), 5));
    each_record(startup.reserved_configs, 64, 0x0c, |record| zero_words(machine, record, 3));
    zero_words(machine, startup.vector_patch, 3);
    initialize_tasks(machine, startup);
    initialize_semaphores(machine, startup);
    initialize_event_flags(machine, startup);
    initialize_data_queues(machine, startup);
    initialize_mailboxes(machine, startup);
    initialize_fixed_pools(machine, startup);
    let r = &startup.routines;
    call0(machine, r.configure_cyclics);
    call0(machine, r.object_hook);
    call0(machine, r.patch_vectors);
}
