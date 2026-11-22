use super::layout::{Domain, GuestAddress};
use super::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Store(u32, u32),
    StoreByte(u32, u8),
    Call(u32, [u32; 3]),
    CallRecord(u32, u32, [u32; 5]),
    Phase(Phase),
    Publish,
    Barrier,
    Pause,
}

struct Recorder {
    memory: RefCell<BTreeMap<u32, u8>>,
    events: RefCell<Vec<Event>>,
    core_index: u32,
    /// Core 1's release word turns nonzero after this many pauses.
    release_after: Option<u32>,
    pauses: Cell<u32>,
}

const PHASE_RESULT: u32 = 0x5150_0000;

impl Recorder {
    fn new(core: Core) -> Self {
        Recorder {
            memory: RefCell::new(BTreeMap::new()),
            events: RefCell::new(Vec::new()),
            core_index: if core == Core::Primary { 0 } else { 1 },
            release_after: None,
            pauses: Cell::new(0),
        }
    }

    fn poke(&self, at: u32, value: u32) {
        let mut memory = self.memory.borrow_mut();
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
            memory.insert(at + index as u32, byte);
        }
    }

    fn peek(&self, at: u32) -> u32 {
        let memory = self.memory.borrow();
        u32::from_le_bytes(core::array::from_fn(|index| *memory.get(&(at + index as u32)).unwrap_or(&0)))
    }

    fn events(&self) -> Vec<Event> {
        self.events.borrow().clone()
    }

    fn stores(&self) -> Vec<(u32, u32)> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Store(at, value) => Some((at, value)),
                _ => None,
            })
            .collect()
    }

    fn calls_to(&self, routine: CodeAddress) -> Vec<[u32; 3]> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Call(at, arguments) if at == routine.0 => Some(arguments),
                _ => None,
            })
            .collect()
    }
}

impl Machine for Recorder {
    fn load(&self, at: GuestAddress) -> u32 {
        self.peek(at.0)
    }

    fn store(&self, at: GuestAddress, value: u32) {
        self.poke(at.0, value);
        self.events.borrow_mut().push(Event::Store(at.0, value));
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        *self.memory.borrow().get(&at.0).unwrap_or(&0)
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        self.memory.borrow_mut().insert(at.0, value);
        self.events.borrow_mut().push(Event::StoreByte(at.0, value));
    }

    fn call(&self, routine: CodeAddress, arguments: [u32; 3]) -> u32 {
        self.events.borrow_mut().push(Event::Call(routine.0, arguments));
        if routine.0 & 0xffff == 0x27b8 {
            self.core_index
        } else {
            0
        }
    }

    fn call_with_record(&self, routine: CodeAddress, first: u32, record: &[u32; 5]) -> u32 {
        self.events.borrow_mut().push(Event::CallRecord(routine.0, first, *record));
        0
    }

    fn phase(&self, _routine: PhaseRoutine, phase: Phase) -> u32 {
        self.events.borrow_mut().push(Event::Phase(phase));
        PHASE_RESULT + phase as u32
    }

    fn publish_interfaces(&self) {
        self.events.borrow_mut().push(Event::Publish);
    }

    fn barrier(&self) {
        self.events.borrow_mut().push(Event::Barrier);
    }

    fn pause(&self) {
        self.events.borrow_mut().push(Event::Pause);
        self.pauses.set(self.pauses.get() + 1);
        if Some(self.pauses.get()) == self.release_after {
            self.poke(shared::CORE1_RELEASE.0, 1);
        }
    }
}

const A: Startup = Startup::of(Domain::A);
const B: Startup = Startup::of(Domain::B);

/// Calls, phases and the release word from `kernel_start`'s own body.
fn start_outline(recorder: &Recorder, startup: &Startup) -> Vec<String> {
    let r = &startup.routines;
    let named = [
        (r.zero_words, "zero"),
        (r.syslog_initialize, "syslog"),
        (r.enter_handler_context, "handler-context"),
        (r.initialize_irqs, "irqs"),
        (r.enable_vfp, "vfp"),
        (r.register_core_notify, "notify"),
        (r.reset_core_sync, "reset-sync"),
        (r.wait_core_sync, "wait-sync"),
        (r.initialize_timers, "timers"),
        (r.patch_vectors, "patch-vectors"),
        (r.startup_hook, "startup-hook"),
        (r.core_rendezvous, "rendezvous"),
        (r.define_irq, "define-irq"),
        (r.enable_irq_slot, "enable-irq"),
        (r.set_exception_hook, "exception-hook"),
        (r.make_ready, "make-ready"),
        (r.initialize_cyclics, "cyclics"),
        (r.dispatch, "dispatch"),
        (r.syslog_record, "record"),
    ];
    let name = |at: u32| named.iter().find(|(routine, _)| routine.0 == at).map(|(_, name)| *name);
    recorder
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Call(at, arguments) => name(at).map(|name| format!("{name}{arguments:x?}")),
            Event::CallRecord(at, first, _) => name(at).map(|name| format!("{name}({first})")),
            Event::Phase(phase) => Some(format!("phase {}", phase as u32)),
            Event::Publish => Some("publish".to_owned()),
            Event::Barrier => Some("dsb".to_owned()),
            Event::Store(at, value) if at == shared::CORE1_RELEASE.0 || at == startup.started.0 => Some(format!("store {at:x}={value}")),
            _ => None,
        })
        .collect()
}

#[test]
fn primary_start_releases_core1_after_setup_and_dispatches_last() {
    let recorder = Recorder::new(Core::Primary);
    recorder.poke(A.task_count.0 .0, 0);
    kernel_start(&recorder, &A, PhaseRoutine(0x9000));
    let outline = start_outline(&recorder, &A);
    let expected = [
        "zero[400a0800, 20100, 0]",
        "publish",
        "syslog[0, 0, 0]",
        "handler-context[0, 0, 0]",
        "irqs[0, 0, 0]",
        "vfp[0, 0, 0]",
        "notify[0, 0, 0]",
        "phase 0",
        "store 400a00c4=1",
        "dsb",
        "reset-sync[0, 0, 0]",
        "timers[0, 0, 0]",
        "patch-vectors[0, 0, 0]",
        "startup-hook[0, 0, 0]",
        "rendezvous[0, 0, 0]",
        "define-irq(32)",
        "enable-irq[20, 0, 0]",
        "phase 1",
        "phase 3",
        "phase 2",
        "exception-hook[51500002, 0, 0]",
        "cyclics[0, 0, 0]",
        "syslog[1, 0, 0]",
        "store 4005927c=1",
        "dispatch[fffffff6, 0, 0]",
        "record[1, 0, 0]",
    ];
    assert_eq!(outline, expected);
}

#[test]
fn peer_start_waits_for_release_and_never_clears_the_syslog_area() {
    let mut recorder = Recorder::new(Core::Peer);
    recorder.release_after = Some(250);
    recorder.poke(B.task_count.0 .0, 0);
    kernel_start(&recorder, &B, PhaseRoutine(0x9000));
    let outline = start_outline(&recorder, &B);
    assert_eq!(outline[0], "publish");
    assert!(!outline.iter().any(|step| step.starts_with("zero") || step.starts_with("store 400a00c4")));
    assert_eq!(recorder.pauses.get() % RELEASE_POLL_PAUSES, 0);
    assert!(recorder.pauses.get() >= 250);
    let phases: Vec<_> = outline.iter().filter(|step| step.starts_with("phase")).cloned().collect();
    assert_eq!(phases, ["phase 4", "phase 2"]);
    let wait = outline.iter().position(|step| step.starts_with("wait-sync")).unwrap();
    let timers = outline.iter().position(|step| step.starts_with("timers")).unwrap();
    assert!(outline[..wait].iter().any(|step| step.starts_with("notify")) && wait < timers);
    assert!(!outline.iter().any(|step| step.starts_with("reset-sync")));
    assert_eq!(outline.last().unwrap(), "record[1, 0, 0]");
}

#[test]
fn core_configuration_names_the_own_config_locks_and_context() {
    for (startup, config, id, scheduler, object, context) in [
        (A, 0x400c_0818, 1, 0x4005_45c0, 0x4005_45c4, 0x4005_45cc),
        (B, 0x400c_0888, 2, 0x400e_4580, 0x400e_4584, 0x400e_4610),
    ] {
        let recorder = Recorder::new(Core::Primary);
        recorder.poke(scheduler, 7);
        recorder.poke(object, 7);
        configure_core(&recorder, &startup);
        assert_eq!(
            recorder.stores(),
            [
                (startup.config_pointer.0, config),
                (config, id),
                (scheduler, 0),
                (config + 4, scheduler),
                (object, 0),
                (config + 8, object),
                (startup.words.current_context.0, context),
            ]
        );
    }
}

#[test]
fn owner_contexts_copy_each_core_config_and_zero_only_the_own_id() {
    for (startup, own) in [(A, 1u32), (B, 2u32)] {
        let recorder = Recorder::new(Core::Primary);
        for owner in 1..=2u32 {
            for offset in (0..0x70).step_by(4) {
                recorder.poke(0x400c_0818 + (owner - 1) * 0x70 + offset, owner << 16 | offset);
            }
        }
        initialize_owner_contexts(&recorder, &startup);
        for owner in 1..=2u32 {
            let context = startup.words.contexts.0 + (owner - 1) * 0x84;
            let id = if owner == own { 0 } else { owner << 16 };
            assert_eq!(recorder.peek(context), id);
            assert_eq!(recorder.peek(context + 4), owner);
            assert_eq!(recorder.peek(context + 8), owner << 16 | 4);
            assert_eq!(recorder.peek(context + 0x14), owner << 16 | 8);
            for lock in [0xc, 0x18] {
                assert_eq!(recorder.peek(context + lock), startup.lock_acquire.0);
                assert_eq!(recorder.peek(context + lock + 4), startup.lock_release.0);
            }
            for offset in (0x20..=0x80).step_by(4) {
                assert_eq!(recorder.peek(context + offset), owner << 16 | (offset - 0x14), "owner {owner} +{offset:#x}");
            }
        }
        assert_eq!(recorder.stores().len(), 2 * 33);
    }
}

#[test]
fn primary_tick_timer_starts_at_the_counter_phase() {
    for (count, reload, tick) in [
        (0u32, 30_000u32, 0u32),
        (1, 29_984, 0),
        (1874, 16, 0),
        (1875, 30_000, 1),
        (123_456_789, 30_000 - 1_164 * 16, 65_843),
        (u32::MAX, 30_000 - 420 * 16, 2_290_649),
    ] {
        let recorder = Recorder::new(Core::Primary);
        recorder.poke(shared::TIMER_COUNT.0, count);
        start_tick_timer(&recorder, &A);
        let timer: Vec<_> = recorder.stores().into_iter().filter(|(at, _)| *at >= 0xfff9_1000).collect();
        assert_eq!(
            timer,
            [(0xfff9_1008, 0), (0xfff9_1004, reload), (0xfff9_1008, 0x23), (0xfff9_1004, 30_000)],
            "count {count}"
        );
        assert_eq!(recorder.peek(shared::PRIMARY_TICK_COUNT.0), tick, "count {count}");
        let events = recorder.events();
        let stopped = events.iter().position(|event| *event == Event::Store(0xfff9_1004, reload)).unwrap();
        let running = events.iter().position(|event| *event == Event::Store(0xfff9_1008, 0x23)).unwrap();
        assert_eq!(events[stopped + 1..running].iter().filter(|event| **event == Event::Pause).count(), 100);
    }
}

#[test]
fn every_core_defines_and_enables_the_tick_line() {
    for (startup, core, handler) in [(A, Core::Primary, 0x40b24), (A, Core::Peer, 0x40b24), (B, Core::Peer, 0xd0b24)] {
        let recorder = Recorder::new(core);
        recorder.poke(shared::PRIMARY_TICK_COUNT.0, 77);
        start_tick_timer(&recorder, &startup);
        let events = recorder.events();
        assert_eq!(events[0], Event::Store(shared::PRIMARY_TICK_COUNT.0, 0));
        assert_eq!(events[1], Event::CallRecord(startup.routines.define_irq.0, 32, [0, handler, 1, 1, 1]));
        assert_eq!(*events.last().unwrap(), Event::Call(startup.routines.enable_irq_slot.0, [32, 0, 0]));
        if core == Core::Peer {
            assert_eq!(recorder.stores(), [(shared::PRIMARY_TICK_COUNT.0, 0)]);
        }
    }
}

#[test]
fn task_initialization_follows_the_loader_count_and_keeps_other_flag_bits() {
    for (startup, domain_word) in [(A, 1u32), (B, 2u32)] {
        let recorder = Recorder::new(Core::Primary);
        let config = 0x400c_0818;
        recorder.poke(startup.config_pointer.0, config);
        recorder.poke(startup.task_count.0 .0, 3);
        let tasks = startup.words.task_table.0;
        for index in 0..4 {
            recorder.memory.borrow_mut().insert(tasks + index * 0x38 + 0x0e, 0xa7);
        }
        initialize_tasks(&recorder, &startup);
        let w = startup.words;
        assert_eq!((recorder.peek(w.selected.0), recorder.peek(w.running.0), recorder.peek(w.reschedule.0)), (0, 0, 0));
        assert_eq!(recorder.peek(w.dispatch_enabled.0), 1);
        for priority in 0..16 {
            let head = w.ready_heads.0 + priority * 8;
            assert_eq!((recorder.peek(head), recorder.peek(head + 4)), (head, head));
        }
        for index in 0..3 {
            let task = tasks + index * 0x38;
            assert_eq!(recorder.peek(task + 8), w.task_definitions.0 + index * 0x28);
            assert_eq!(recorder.load_byte(GuestAddress(task + 0x0e)), 0xa6);
            assert_eq!(recorder.peek(task + 0x20), domain_word);
        }
        assert_eq!(recorder.load_byte(GuestAddress(tasks + 3 * 0x38 + 0x0e)), 0xa7);
        assert_eq!(recorder.calls_to(startup.routines.reset_task), [[tasks, 0, 0], [tasks + 0x38, 0, 0], [tasks + 0x70, 0, 0]]);
        assert_eq!(
            [0x0c, 0x10, 0x14, 0x18, 0x1c, 0x30, 0x34, 0x68].map(|offset| recorder.peek(config + offset)),
            [w.ready_heads.0, w.ready_bitmap.0, w.running.0, w.selected.0, w.dispatch_enabled.0, tasks, 3, startup.first_entry.0]
        );
    }
}

#[test]
fn autostart_readies_tasks_whose_definition_has_bit_one_in_slot_order() {
    let recorder = Recorder::new(Core::Primary);
    let w = A.words;
    recorder.poke(A.task_count.0 .0, 4);
    recorder.poke(w.current_context.0, 0x4005_45cc);
    for (index, slot) in [7u32, 2, 120, 5].into_iter().enumerate() {
        recorder.poke(w.task_slot_ids.0 + index as u32 * 4, slot);
        let task = w.task_table.0 + (slot - 1) * 0x38;
        let definition = 0x4100_0000 + slot * 0x28;
        recorder.poke(task + 8, definition);
        recorder.poke(definition, if slot == 5 { 0x1 } else { 0x3 });
    }
    autostart_tasks(&recorder, &A);
    let ready: Vec<_> = recorder.calls_to(A.routines.make_ready).into_iter().map(|arguments| (arguments[0], arguments[1])).collect();
    let task = |slot: u32| w.task_table.0 + (slot - 1) * 0x38;
    assert_eq!(ready, [(0x4005_45cc, task(7)), (0x4005_45cc, task(2)), (0x4005_45cc, task(120))]);
}

#[test]
fn object_tables_link_each_control_to_its_config_and_publish_the_table() {
    let recorder = Recorder::new(Core::Primary);
    let config = 0x400c_0888;
    recorder.poke(B.config_pointer.0, config);
    for kind in [ObjectKind::Semaphore, ObjectKind::EventFlag, ObjectKind::DataQueue, ObjectKind::Mailbox, ObjectKind::FixedPool] {
        let table = B.table(kind);
        recorder.poke(table.count.0 .0, 2);
        for index in 0..2 {
            for offset in (0..table.config_size).step_by(4) {
                recorder.poke(table.configs.0 + index * table.config_size + offset, 0x100 * index + offset);
            }
        }
        initialize_object_table(&recorder, &B, kind);
        for index in 0..2 {
            let control = table.controls.0 + index * table.control_size;
            let source = table.configs.0 + index * table.config_size;
            assert_eq!([0, 4, 8].map(|offset| recorder.peek(control + offset)), [control, control, source]);
            let rest: Vec<u32> = (0x0c..table.control_size).step_by(4).map(|offset| recorder.peek(control + offset)).collect();
            let expected: Vec<u32> = match kind {
                ObjectKind::Semaphore => vec![0x100 * index + 4],
                ObjectKind::EventFlag => vec![0x100 * index + 4],
                ObjectKind::DataQueue => vec![control + 0xc, control + 0xc, 0, 0, 0],
                ObjectKind::Mailbox => vec![0, 0],
                ObjectKind::FixedPool => vec![0x100 * index + 8, 0],
            };
            assert_eq!(rest, expected, "{kind:?} {index}");
        }
        assert_eq!((recorder.peek(config + table.config_offset), recorder.peek(config + table.config_offset + 4)), (table.controls.0, 2));
    }
    assert_eq!(
        [ObjectKind::Semaphore, ObjectKind::EventFlag, ObjectKind::DataQueue, ObjectKind::Mailbox, ObjectKind::FixedPool].map(|kind| B.table(kind).count.0 .0),
        [0xdda7c, 0xdda88, 0xdda84, 0xdda78, 0xdda74]
    );
}

#[test]
fn object_initialization_resets_every_creation_record_then_runs_the_initializers() {
    let recorder = Recorder::new(Core::Primary);
    recorder.poke(A.config_pointer.0, 0x400c_0818);
    recorder.poke(A.task_count.0 .0, 1);
    initialize_objects(&recorder, &A);
    let w = A.words;
    let definition = |slot: u32, offset: u32| recorder.peek(w.task_definitions.0 + (slot - 1) * 0x28 + offset);
    assert_eq!(
        [0, 4, 8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24].map(|offset| definition(120, offset)),
        [0, 119, 0, 15, 4, 0x4005_9280, 0, 0, 120, 0]
    );
    assert_eq!(recorder.peek(w.task_slot_ids.0 + 119 * 4), 120);
    assert_eq!([0, 4, 8, 0xc].map(|offset| recorder.peek(w.semaphore_configs.0 + 249 * 16 + offset)), [0, 1, 1, 0]);
    assert_eq!([0, 4, 8, 0xc].map(|offset| recorder.peek(w.data_queue_configs.0 + 119 * 16 + offset)), [0, 1, 0x4005_9284, 0]);
    assert_eq!([0, 4, 8].map(|offset| recorder.peek(w.mailbox_configs.0 + offset)), [2, 32, 0]);
    assert_eq!(
        [0, 4, 8, 0xc, 0x10].map(|offset| recorder.peek(w.fixed_pool_configs.0 + 121 * 20 + offset)),
        [0, 4, 0x4005_9288, 0x4005_928c, 0]
    );
    let stores = recorder.stores();
    let written = |at: u32| stores.iter().any(|(address, _)| *address == at);
    assert!(!written(w.cyclic_configs.0) && written(w.cyclic_configs.0 + 4) && written(w.cyclic_configs.0 + 63 * 24 + 0x14));
    assert!(written(0x4005_dcd8 + 63 * 12 + 8) && written(0x4005_f578 + 8));
    assert!(!written(0x4005_f578 + 12) && !written(w.flag_configs.0 + 120 * 12));
    let calls: Vec<u32> = recorder
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Call(at, _) => Some(at),
            _ => None,
        })
        .collect();
    let r = A.routines;
    assert_eq!(calls, [r.reset_task.0, r.configure_cyclics.0, r.object_hook.0, r.patch_vectors.0]);
    assert_eq!(recorder.peek(0x400c_0818 + 0x5c), recorder.peek(0x4dab0));
}

#[test]
fn loader_b_publication_fills_the_root_table() {
    let recorder = Recorder::new(Core::Peer);
    interfaces_b::publish(&recorder);
    let root = interfaces_b::ROOT.0;
    assert_eq!(recorder.peek(root), 0xdd054);
    assert_eq!(recorder.peek(root + 0x104), 0xd4d4c);
    assert_eq!(recorder.peek(root + 0x160), 0xdd8c0);
    assert_eq!(recorder.peek(root + 0x174), 0x400c_0800);
    assert_eq!([0x1618, 0x161c, 0x1620, 0x1624, 0x1628].map(|offset| recorder.peek(root + offset)), [0xd12bc, 0xd12cc, 0xd12e8, 0xd1304, 0x400f_3420]);
    let last = root + 0x178 + 119 * 44;
    assert_eq!(recorder.peek(last), 120);
    assert_eq!(recorder.peek(last + 4), 0x400e_be6c + 119 * 40);
    assert_eq!(recorder.peek(last + 32), 0x400e_be6c + 119 * 40 + 28);
    assert_eq!((recorder.peek(last + 36), recorder.peek(last + 40)), (0x400e_fecc + 119 * 56 + 24, 0x400e_fecc + 119 * 56 + 28));
    assert_eq!(recorder.stores().len(), 93 + 1 + 5 + 120 * 11);
}
