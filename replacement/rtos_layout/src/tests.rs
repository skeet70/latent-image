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
        words_as_array(Domain::A.words()),
        [
            0x4005_45a4, 0x4005_45c0, 0x4005_45c4, 0x4005_45c8, 0x4005_45cc, 0x4005_46d8, 0x4005_46dc, 0x4005_475c,
            0x4005_4760, 0x4005_4764, 0x4005_4768, 0x4005_476c, 0x4005_4770, 0x4005_4774, 0x4005_4778, 0x4005_d718,
            0x4005_ff0c,
        ]
    );
    assert_eq!(
        words_as_array(Domain::B.words()),
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
    let first_and_last = |w: &DomainWords| {
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
fn task_definition_fields_are_the_stock_offsets() {
    let d = TaskDefinition(GuestAddress(0x0070_0000));
    assert_eq!(
        [
            d.attributes(),
            d.argument(),
            d.entry(),
            d.priority(),
            d.stack_size(),
            d.stack(),
            d.exception_attributes(),
            d.exception_handler(),
            d.slot(),
            d.exists(),
        ]
        .map(|a| a.0),
        [
            0x0070_0000, 0x0070_0004, 0x0070_0008, 0x0070_000c, 0x0070_0010, 0x0070_0014, 0x0070_0018, 0x0070_001c,
            0x0070_0020, 0x0070_0024,
        ]
    );
    assert_eq!(d.initial_sp_terms().map(|a| a.0), [0x0070_0010, 0x0070_0014]);
    assert_eq!(TaskDefinition::SIZE, 0x28);
}

#[test]
fn task_tables_are_the_stock_addresses() {
    let a = Domain::A.words();
    assert_eq!(
        [a.task_definitions, a.task_slot_ids, a.default_stack_word.address()].map(|a| a.0),
        [0x4005_beac, 0x4006_22d4, 0x4006_2c50]
    );
    let b = Domain::B.words();
    assert_eq!(
        [b.task_definitions, b.task_slot_ids, b.default_stack_word.address()].map(|a| a.0),
        [0x400e_be6c, 0x400f_2294, 0x400f_2c10]
    );
    let slot = TaskSlot::new(120).unwrap();
    assert_eq!(a.task_definition(slot).0 .0, 0x4005_beac + 119 * 0x28);
    assert_eq!(a.task_creation_id(slot).word().0, 0x4006_22d4 + 119 * 4);
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

#[test]
fn family_tables_are_the_stock_addresses() {
    for (domain, expected) in [
        (
            Domain::A,
            [0x4005_d178, 0x4006_24b4, 0x4005_5fdc, 0x4005_5a3c, 0x4005_d16c, 0x4006_2c34, 0x4005_783c, 0x4005_7fbc],
        ),
        (
            Domain::B,
            [0x400e_d138, 0x400f_2474, 0x400e_5f9c, 0x400e_59fc, 0x400e_d12c, 0x400f_2bf4, 0x400e_77fc, 0x400e_7f7c],
        ),
    ] {
        let w = domain.words();
        let actual = [
            w.flag_configs.0,
            w.flag_controls.0,
            w.flag_waits.0,
            w.flag_timers.0,
            w.mailbox_configs.0,
            w.mailbox_controls.0,
            w.mailbox_waits.0,
            w.mailbox_timers.0,
        ];
        assert_eq!(actual, expected, "{domain:?}");
    }
}

#[test]
fn family_record_fields_are_the_stock_offsets() {
    let base = GuestAddress(0x4005_0000);
    assert_eq!(FlagConfig(base).attributes().0, base.0);
    assert_eq!(FlagConfig(base).initial().0, base.0 + 0x04);
    assert_eq!(FlagConfig(base).exists().0, base.0 + 0x08);
    assert_eq!(FlagControl(base).queue().next().0, base.0);
    assert_eq!(FlagControl(base).pattern().0, base.0 + 0x0c);
    assert_eq!(FlagWait(base).pattern().0, base.0 + 0x0c);
    assert_eq!(FlagWait(base).mode().0, base.0 + 0x10);
    assert_eq!(FlagWait(base).result().0, base.0 + 0x14);
    assert_eq!(MailboxConfig(base).priority_limit().0, base.0 + 0x04);
    assert_eq!(MailboxConfig(base).exists().0, base.0 + 0x08);
    assert_eq!(MailboxControl(base).head().0, base.0 + 0x0c);
    assert_eq!(MailboxControl(base).tail().0, base.0 + 0x10);
    assert_eq!(MailboxWait(base).message().0, base.0 + 0x0c);
    assert_eq!(Message(base).priority().0, base.0 + 0x04);
    assert_eq!(WaitBlock(base).queue().0, base.0 + 0x04);
    assert_eq!(WaitBlock(base).owner().0, base.0 + 0x08);
}

#[test]
fn family_sizes_are_the_stock_strides() {
    assert_eq!(
        [
            FlagConfig::SIZE,
            FlagControl::SIZE,
            FlagWait::SIZE,
            MailboxConfig::SIZE,
            MailboxControl::SIZE,
            MailboxWait::SIZE,
            Message::SIZE,
            WaitQueue::SIZE,
            WaitBlock::SIZE,
        ],
        [0x0c, 0x10, 0x18, 0x0c, 0x14, 0x10, 0x08, 0x0c, 0x0c]
    );
}

#[test]
fn family_slot_accessors_stride_from_the_table() {
    let a = Domain::A.words();
    let flag2 = FlagSlot::new(2, 120).unwrap();
    let task3 = TaskSlot::new(3).unwrap();
    let box1 = MailboxSlot::new(1, 1).unwrap();
    assert_eq!(a.flag_config(flag2).0 .0, 0x4005_d178 + 0x0c);
    assert_eq!(a.flag_control(flag2).0 .0, 0x4006_24b4 + 0x10);
    assert_eq!(a.flag_wait(task3).0 .0, 0x4005_5fdc + 2 * 0x18);
    assert_eq!(a.flag_timer(task3).0, 0x4005_5a3c + 2 * 0x0c);
    assert_eq!(a.mailbox_control(box1).0 .0, 0x4006_2c34);
    assert_eq!(a.mailbox_wait(task3).0 .0, 0x4005_783c + 2 * 0x10);
}

#[test]
fn family_slots_refuse_out_of_range_and_oversized_counts() {
    assert_eq!(FlagSlot::new(0, 120), None);
    assert_eq!(FlagSlot::new(121, 120), None);
    assert_eq!(FlagSlot::new(1, 0), None);
    assert_eq!(FlagSlot::new(200, 200), None);
    assert_eq!(FlagSlot::new(120, 120).map(FlagSlot::get), Some(120));
    assert_eq!(MailboxSlot::new(2, 1), None);
    assert_eq!(MailboxSlot::new(1, 1).map(MailboxSlot::get), Some(1));
}

#[test]
fn fixed_pool_views_are_the_stock_offsets() {
    let base = GuestAddress(0x4006_194c);
    assert_eq!(FixedPoolConfig(base).block_size().0, base.0 + 0x04);
    assert_eq!(FixedPoolConfig(base).begin().0, base.0 + 0x08);
    assert_eq!(FixedPoolConfig(base).end().0, base.0 + 0x0c);
    assert_eq!(FixedPoolConfig(base).exists().0, base.0 + 0x10);
    assert_eq!(FixedPoolControl(base).next_free().0, base.0 + 0x0c);
    assert_eq!(FixedPoolControl(base).recycled().0, base.0 + 0x10);
    assert_eq!(FixedPoolWait(base).block().0, base.0 + 0x0c);
    assert_eq!(
        [FixedPoolConfig::SIZE, FixedPoolControl::SIZE, FixedPoolWait::SIZE],
        [0x14, 0x14, 0x10]
    );
}

#[test]
fn fixed_pool_tables_are_the_stock_addresses() {
    let a = Domain::A.words();
    let b = Domain::B.words();
    assert_eq!(
        [
            a.fixed_pool_configs.0,
            a.fixed_pool_controls.0,
            a.fixed_pool_waits.0,
            a.fixed_pool_timers.0,
            a.default_pool_storage.0,
        ],
        [0x4006_194c, 0x4005_f584, 0x4005_855c, 0x4005_8cdc, 0x4006_2c4c]
    );
    assert_eq!(
        [
            b.fixed_pool_configs.0,
            b.fixed_pool_controls.0,
            b.fixed_pool_waits.0,
            b.fixed_pool_timers.0,
            b.default_pool_storage.0,
        ],
        [0x400f_190c, 0x400e_f544, 0x400e_851c, 0x400e_8c9c, 0x400f_2c0c]
    );
}

#[test]
fn fixed_pool_slot_accessors_stride_from_the_table() {
    let a = Domain::A.words();
    let pool2 = FixedPoolSlot::new(2, 122).unwrap();
    let task3 = TaskSlot::new(3).unwrap();
    assert_eq!(a.fixed_pool_config(pool2).0 .0, 0x4006_194c + 0x14);
    assert_eq!(a.fixed_pool_control(pool2).0 .0, 0x4005_f584 + 0x14);
    assert_eq!(a.fixed_pool_wait(task3).0 .0, 0x4005_855c + 2 * 0x10);
    assert_eq!(a.fixed_pool_timer(task3).0, 0x4005_8cdc + 2 * 0x0c);
    assert_eq!(FixedPoolSlot::new(122, 122).map(FixedPoolSlot::get), Some(122));
    assert_eq!(FixedPoolSlot::new(123, 122), None);
    assert_eq!(FixedPoolSlot::new(1, 0), None);
}

#[test]
fn task_pool_reserves_the_timer_arena_tail() {
    assert_eq!(TASK_POOL.guest.start(), 0x40a1_0200);
    assert_eq!(TASK_POOL.guest.end(), 0x40a5_b200);
    assert_eq!(TASK_POOL.allocatable.end(), 0x40a5_a200);
    assert_eq!(TASK_POOL.allocatable_bytes(), 0x4a000);
    assert_eq!(TASK_POOL.reserved.len(), 0x1000);
    assert_eq!(TASK_POOL.physical().start(), 0xa1_0200);
    assert_eq!(TASK_POOL.physical().end(), 0xa5_b200);
}

#[test]
fn timer_arenas_are_aligned_disjoint_and_fit_their_capacity() {
    let a = Domain::A.timer_arena();
    let b = Domain::B.timer_arena();
    assert_eq!((a.guest.start(), a.guest.end()), (0x40a5_a200, 0x40a5_aa00));
    assert_eq!((b.guest.start(), b.guest.end()), (0x40a5_aa00, 0x40a5_b200));
    assert_eq!((a.physical.start(), a.physical.end()), (0xa5_a200, 0xa5_aa00));
    assert!(a.guest.aligned(32) && b.guest.aligned(32));
    assert!(!a.guest.overlaps(b.guest));
    assert!(TASK_POOL.reserved.contains(a.guest) && TASK_POOL.reserved.contains(b.guest));
    assert_eq!(a.node_bytes(), 0x5c0);
    assert!(a.fits() && b.fits());
    assert_eq!(GuestAddress(0x40a5_a200).physical(), 0xa5_a200);
    assert_eq!(GuestAddress(0x0005_a200).physical(), 0x0005_a200);
}
