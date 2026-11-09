//! Prints the timer heap's stock words, record offsets, the owned reservation and the scheduler
//! fixture words the tick-to-completion composition cases seed, from `xm1_rtos_layout`.

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
mod layout;

use layout::*;

fn domain(domain: Domain) -> String {
    let w = domain.words();
    format!(
        "{{\"handler_context\":{},\"current_context\":{},\"scheduler_lock\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{}}}",
        w.handler_context.0,
        w.current_context.0,
        w.scheduler_lock.0,
        w.timer_heap.0,
        w.timer_count.0,
        w.timer_base.0,
        w.tick_now.0,
    )
}

fn scheduler_domain(domain: Domain) -> String {
    let w = domain.words();
    format!(
        "{{\"contexts\":{},\"current_context\":{},\"handler_context\":{},\"scheduler_lock\":{},\"object_lock\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"dispatch_enabled\":{},\"selected\":{},\"reschedule\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{},\"task_table\":{},\"task_definitions\":{}}}",
        w.contexts.0,
        w.current_context.0,
        w.handler_context.0,
        w.scheduler_lock.0,
        w.object_lock.0,
        w.ready_heads.0,
        w.ready_bitmap.0,
        w.running.0,
        w.dispatch_enabled.0,
        w.selected.0,
        w.reschedule.0,
        w.timer_heap.0,
        w.timer_count.0,
        w.timer_base.0,
        w.tick_now.0,
        w.task_table.0,
        w.task_definitions.0,
    )
}

/// The recovered stock helper entries the S3d timeout and delay bodies call; `rtos_layout` has no
/// typed view for their addresses.
fn helpers(domain: Domain) -> String {
    let (acquire, release, switch, first_entry, notify, insert_ready, remove_ready, reset_task, restart_task) =
        match domain {
            Domain::A => (0x439d8, 0x43a14, 0x43640, 0x43728, 0x44018, 0x43ca4, 0x43d20, 0x43dd0, 0x43f70),
            Domain::B => (0xd398c, 0xd39c8, 0xd3640, 0xd3728, 0xd3fd4, 0xd3c60, 0xd3cdc, 0xd3d8c, 0xd3f2c),
        };
    format!(
        "{{\"acquire\":{},\"release\":{},\"switch\":{},\"first_entry\":{},\"notify\":{},\"insert_ready\":{},\"remove_ready\":{},\"reset_task\":{},\"restart_task\":{}}}",
        acquire, release, switch, first_entry, notify, insert_ready, remove_ready, reset_task, restart_task,
    )
}

fn span(span: Span) -> String {
    format!("{{\"start\":{},\"end\":{},\"size\":{}}}", span.start(), span.end(), span.len())
}

fn arena(arena: TimerArena) -> String {
    format!(
        "{{\"guest\":{},\"physical\":{},\"capacity\":{},\"node_bytes\":{}}}",
        span(arena.guest),
        span(arena.physical),
        arena.capacity,
        arena.node_bytes(),
    )
}

fn scheduler() -> String {
    let c = KernelContext(GuestAddress(0));
    let t = Task(GuestAddress(0));
    let d = TaskDefinition(GuestAddress(0));
    let l = LockRecord(GuestAddress(0));
    let h = ReadyHead(GuestAddress(0));
    let [sp0, sp1] = d.initial_sp_terms();
    let context = format!(
        "{{\"size\":{},\"id\":{},\"owner\":{},\"scheduler_lock\":{},\"object_lock\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"selected\":{},\"dispatch_enabled\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{},\"tasks\":{},\"task_count\":{},\"first_entry\":{}}}",
        KernelContext::SIZE,
        c.id().0,
        c.owner().0,
        c.scheduler_lock().0 .0,
        c.object_lock().0 .0,
        c.ready_heads().0,
        c.ready_bitmap().0,
        c.running().0,
        c.selected().0,
        c.dispatch_enabled().0,
        c.timer_heap().0,
        c.timer_count().0,
        c.timer_base().0,
        c.tick_now().0,
        c.tasks().0,
        c.task_count().0,
        c.first_entry().0,
    );
    let task = format!(
        "{{\"size\":{},\"next\":{},\"prev\":{},\"definition\":{},\"state\":{},\"priority\":{},\"flags\":{},\"wait\":{},\"saved_sp\":{},\"resume_pc\":{},\"domain\":{},\"terminate_pending\":{},\"priority_change_pending\":{},\"cancel_pending\":{},\"context_discarded\":{},\"pending_priority\":{}}}",
        Task::SIZE,
        t.next().0,
        t.prev().0,
        t.definition().0,
        t.state().0,
        t.priority().0,
        t.flags().0,
        t.wait().0,
        t.saved_sp().0,
        t.resume_pc().0,
        t.domain().0,
        t.terminate_pending().0,
        t.priority_change_pending().0,
        t.cancel_pending().0,
        t.context_discarded().0,
        t.pending_priority().0,
    );
    let definition = format!(
        "{{\"size\":{},\"priority\":{},\"exists\":{},\"sp_terms\":[{},{}]}}",
        TaskDefinition::SIZE,
        d.priority().0,
        d.exists().0,
        sp0.0,
        sp1.0,
    );
    let lock = format!("{{\"word\":{},\"acquire\":{},\"release\":{}}}", l.word().0, l.acquire().0, l.release().0);
    let head = format!("{{\"size\":{},\"next\":{},\"prev\":{}}}", ReadyHead::SIZE, h.next().0, h.prev().0);
    let states = [
        TaskState::DORMANT,
        TaskState::READY,
        TaskState::WAIT,
        TaskState::SUSPENDED,
        TaskState::SLEEPING,
        TaskState::OBJECT_QUEUED,
        TaskState::OWNER_LOCK_QUEUED,
    ]
    .map(|state| state.bits().to_string())
    .join(",");
    format!(
        "{{\"a\":{},\"b\":{},\"context\":{},\"task\":{},\"definition\":{},\"lock\":{},\"ready_head\":{},\"states\":[{}],\"priorities\":{},\"task_slots\":{},\"helpers\":{{\"a\":{},\"b\":{}}}}}",
        scheduler_domain(Domain::A),
        scheduler_domain(Domain::B),
        context,
        task,
        definition,
        lock,
        head,
        states,
        Priority::COUNT,
        TaskSlot::COUNT,
        helpers(Domain::A),
        helpers(Domain::B),
    )
}

fn main() {
    let context = KernelContext(GuestAddress(0));
    let lock = LockRecord(GuestAddress(0));
    println!(
        "{{\"a\":{},\"b\":{},\"context\":{{\"scheduler_lock\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{}}},\"lock\":{{\"word\":{},\"acquire\":{},\"release\":{}}},\"timer_entry_size\":{},\"task_slots\":{},\"scheduler\":{},\"reservation\":{{\"task_pool\":{{\"guest\":{},\"physical\":{},\"allocatable\":{},\"allocatable_physical\":{},\"reserved\":{}}},\"timer_arenas\":{{\"a\":{},\"b\":{}}},\"timer_node_bytes\":{}}}}}",
        domain(Domain::A),
        domain(Domain::B),
        context.scheduler_lock().0 .0,
        context.timer_heap().0,
        context.timer_count().0,
        context.timer_base().0,
        context.tick_now().0,
        lock.word().0,
        lock.acquire().0,
        lock.release().0,
        TIMER_ENTRY_SIZE,
        TaskSlot::COUNT,
        scheduler(),
        span(TASK_POOL.guest),
        span(TASK_POOL.physical()),
        span(TASK_POOL.allocatable),
        span(TASK_POOL.physical_allocatable()),
        span(TASK_POOL.reserved),
        arena(TIMER_ARENAS.a),
        arena(TIMER_ARENAS.b),
        TIMER_NODE_BYTES,
    );
}
