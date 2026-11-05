//! Host tests of `xm1_rtos_layout`: every address against the stock literals of both loaders.

#[path = "lib.rs"]
mod layout;

use layout::*;

const DOMAINS: [Domain; 2] = [Domain::A, Domain::B];

fn words_as_array(w: &DomainWords) -> [u32; 17] {
    [
        w.handler_context,
        w.scheduler_lock,
        w.object_lock,
        w.current_context,
        w.contexts,
        w.reschedule,
        w.ready_heads,
        w.ready_bitmap,
        w.running,
        w.dispatch_enabled,
        w.selected,
        w.timer_base,
        w.tick_now,
        w.time_offset,
        w.timer_count,
        w.timer_heap,
        w.task_table,
    ]
    .map(|a| a.0)
}

#[test]
fn domain_words_are_the_stock_addresses() {
    assert_eq!(
        words_as_array(&Domain::A.words()),
        [
            0x4005_45a4, 0x4005_45c0, 0x4005_45c4, 0x4005_45c8, 0x4005_45cc, 0x4005_46d8, 0x4005_46dc, 0x4005_475c,
            0x4005_4760, 0x4005_4764, 0x4005_4768, 0x4005_476c, 0x4005_4770, 0x4005_4774, 0x4005_4778, 0x4005_d718,
            0x4005_ff0c,
        ]
    );
    assert_eq!(
        words_as_array(&Domain::B.words()),
        [
            0x400e_4564, 0x400e_4580, 0x400e_4584, 0x400e_4588, 0x400e_458c, 0x400e_4698, 0x400e_469c, 0x400e_471c,
            0x400e_4720, 0x400e_4724, 0x400e_4728, 0x400e_472c, 0x400e_4730, 0x400e_4734, 0x400e_4738, 0x400e_d6d8,
            0x400e_fecc,
        ]
    );
}

#[test]
fn context_records_are_the_stock_addresses() {
    let a = Domain::A.words();
    let b = Domain::B.words();
    assert_eq!(a.context(Domain::A).0, GuestAddress(0x4005_45cc));
    assert_eq!(a.context(Domain::B).0, GuestAddress(0x4005_4650));
    assert_eq!(b.context(Domain::A).0, GuestAddress(0x400e_458c));
    assert_eq!(b.context(Domain::B).0, GuestAddress(0x400e_4610));
}

#[test]
fn context_fields_are_the_stock_offsets() {
    let c = Domain::B.words().context(Domain::B);
    let fields = [
        c.id(),
        c.owner(),
        c.scheduler_lock().word(),
        c.scheduler_lock().acquire(),
        c.scheduler_lock().release(),
        c.object_lock().word(),
        c.object_lock().acquire(),
        c.object_lock().release(),
        c.ready_heads(),
        c.ready_bitmap(),
        c.running(),
        c.selected(),
        c.dispatch_enabled(),
        c.timer_heap(),
        c.timer_count(),
        c.timer_base(),
        c.tick_now(),
        c.tasks(),
        c.task_count(),
        c.semaphores(),
        c.semaphore_count(),
        c.event_flags(),
        c.event_flag_count(),
        c.data_queues(),
        c.data_queue_count(),
        c.mailboxes(),
        c.mailbox_count(),
        c.fixed_pools(),
        c.fixed_pool_count(),
        c.cyclics(),
        c.cyclic_count(),
        c.first_entry(),
        c.cyclic_callback(),
    ];
    let expected: Vec<u32> = (0..33).map(|i| 0x400e_4610 + 4 * i).collect();
    assert_eq!(fields.map(|a| a.0).to_vec(), expected);
    assert_eq!(c.cyclic_callback().0 + 4, 0x400e_4610 + KernelContext::SIZE);
    assert_eq!(KernelContext::SIZE, 0x84);
    assert_eq!(LockRecord::SIZE, 0x0c);
}

#[test]
fn task_records_are_the_stock_addresses() {
    let first_and_last = |w: DomainWords| {
        [TaskSlot::new(1).unwrap(), TaskSlot::new(60).unwrap(), TaskSlot::new(120).unwrap()].map(|s| w.task(s).0 .0)
    };
    assert_eq!(first_and_last(Domain::A.words()), [0x4005_ff0c, 0x4006_0bf4, 0x4006_1914]);
    assert_eq!(first_and_last(Domain::B.words()), [0x400e_fecc, 0x400f_0bb4, 0x400f_18d4]);
}

#[test]
fn task_fields_are_the_stock_offsets() {
    let t = Task(GuestAddress(0x4005_ff0c));
    let words = [
        t.next(),
        t.prev(),
        t.definition(),
        t.state(),
        t.exception_pattern(),
        t.wait(),
        t.saved_sp(),
        t.resume_pc(),
        t.domain(),
        t.terminate_pending(),
        t.priority_change_pending(),
        t.cancel_pending(),
        t.context_discarded(),
        t.pending_priority(),
    ];
    assert_eq!(
        words.map(|a| a.0),
        [
            0x4005_ff0c, 0x4005_ff10, 0x4005_ff14, 0x4005_ff18, 0x4005_ff1c, 0x4005_ff20, 0x4005_ff24, 0x4005_ff28,
            0x4005_ff2c, 0x4005_ff30, 0x4005_ff34, 0x4005_ff38, 0x4005_ff3c, 0x4005_ff40,
        ]
    );
    assert_eq!([t.state(), t.priority(), t.flags()].map(|a| a.0), [0x4005_ff18, 0x4005_ff19, 0x4005_ff1a]);
    assert_eq!(Task::SIZE, 0x38);
}

#[test]
fn ready_heads_fill_the_space_before_the_bitmap() {
    for w in DOMAINS.map(Domain::words) {
        let last = w.ready_head(Priority::new(15).unwrap());
        assert_eq!(last.prev().offset(4), w.ready_bitmap);
    }
    let head = Domain::A.words().ready_head(Priority::new(3).unwrap());
    assert_eq!([head.next(), head.prev()].map(|a| a.0), [0x4005_46f4, 0x4005_46f8]);
    let head = Domain::B.words().ready_head(Priority::new(0).unwrap());
    assert_eq!([head.next(), head.prev()].map(|a| a.0), [0x400e_469c, 0x400e_46a0]);
}

#[test]
fn task_slot_round_trips_only_through_record_starts() {
    for w in DOMAINS.map(Domain::words) {
        for slot in 1..=120 {
            let slot = TaskSlot::new(slot).unwrap();
            assert_eq!(w.task_slot(w.task(slot)), Some(slot));
        }
        for bytes in [4, 0x37, 0x39, 120 * 0x38] {
            assert_eq!(w.task_slot(Task(w.task_table.offset(bytes))), None, "{bytes:#x}");
        }
        assert_eq!(w.task_slot(Task(GuestAddress(w.task_table.0 - 0x38))), None);
    }
}

#[test]
fn task_slots_are_1_to_120() {
    assert_eq!(TaskSlot::new(0), None);
    assert_eq!(TaskSlot::new(1).map(TaskSlot::get), Some(1));
    assert_eq!(TaskSlot::new(120).map(TaskSlot::get), Some(120));
    assert_eq!(TaskSlot::new(121), None);
    assert_eq!(TaskSlot::new(0x1_0001), None);
}

#[test]
fn priorities_are_0_to_15() {
    assert_eq!(Priority::new(0).map(Priority::bit), Some(1));
    assert_eq!(Priority::new(15).map(Priority::bit), Some(0x8000));
    assert_eq!(Priority::new(16), None);
    assert_eq!(Priority::new(0x1_0000), None);
}

#[test]
fn domain_ids_are_0_1_2() {
    assert_eq!([0, 1, 2, 3, 0xffff_ffff].map(DomainId::from_word), [
        Some(DomainId::Own),
        Some(DomainId::A),
        Some(DomainId::B),
        None,
        None
    ]);
    assert_eq!(DOMAINS.map(|d| d.id().word()), [1, 2]);
}

#[test]
fn task_state_accepts_every_defined_combination() {
    for byte in 0..=0x3f {
        assert_eq!(TaskState::from_byte(byte).map(TaskState::bits), Some(byte), "{byte:#x}");
    }
    for byte in 0x40..=0xff {
        assert_eq!(TaskState::from_byte(byte), None, "{byte:#x}");
    }
    let suspended_sleeper = TaskState::from_byte(0x0e).unwrap();
    assert!(suspended_sleeper.contains(TaskState::WAIT.union(TaskState::SLEEPING)));
    assert!(suspended_sleeper.contains(TaskState::SUSPENDED));
    assert!(!suspended_sleeper.contains(TaskState::READY));
    assert_eq!(suspended_sleeper.difference(TaskState::SUSPENDED).bits(), 0x0a);
    let written = [
        TaskState::READY,
        TaskState::WAIT,
        TaskState::SUSPENDED,
        TaskState::WAIT.union(TaskState::SLEEPING),
        TaskState::WAIT.union(TaskState::OBJECT_QUEUED),
        TaskState::WAIT.union(TaskState::OBJECT_QUEUED).union(TaskState::OWNER_LOCK_QUEUED),
    ];
    assert_eq!(written.map(TaskState::bits), [0x01, 0x02, 0x04, 0x0a, 0x12, 0x32]);
    assert_eq!(TaskState::DORMANT.bits(), 0);
}

#[test]
fn task_flags_bits() {
    assert_eq!(
        [TaskFlags::QUEUED_ACTIVATION, TaskFlags::QUEUED_WAKEUP, TaskFlags::EXCEPTION_ENABLED].map(|f| f.0),
        [0x01, 0x02, 0x04]
    );
    assert!(TaskFlags(0xfc).contains(TaskFlags::EXCEPTION_ENABLED));
    assert!(!TaskFlags(0xfb).contains(TaskFlags::EXCEPTION_ENABLED));
}
