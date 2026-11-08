//! Prints the data-queue addresses, record offsets and timer spans the native oracle builds its
//! cases from, as JSON.

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
mod layout;
#[allow(unused_attributes)]
#[path = "../../rtos_reservation/src/lib.rs"]
mod reservation;

use layout::*;
use reservation::{Span, TimerArena, TIMER_ARENAS, TIMER_NODE_BYTES};

fn domain(domain: Domain) -> String {
    let w = domain.words();
    format!(
        "{{\"id\":{},\"scheduler_lock\":{},\"object_lock\":{},\"contexts\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"dispatch_enabled\":{},\"selected\":{},\"task_table\":{},\"context_a\":{},\"context_b\":{},\"handler_context\":{},\"current_context\":{},\"reschedule\":{},\"timer_base\":{},\"timer_count\":{},\"timer_heap\":{},\"tick_now\":{},\"data_queue_configs\":{},\"data_queue_controls\":{},\"data_queue_waits\":{},\"data_queue_timers\":{},\"data_queue_placeholder\":{}}}",
        domain.id().word(),
        w.scheduler_lock.0,
        w.object_lock.0,
        w.contexts.0,
        w.ready_heads.0,
        w.ready_bitmap.0,
        w.running.0,
        w.dispatch_enabled.0,
        w.selected.0,
        w.task_table.0,
        w.context(Domain::A).0 .0,
        w.context(Domain::B).0 .0,
        w.handler_context.0,
        w.current_context.0,
        w.reschedule.0,
        w.timer_base.0,
        w.timer_count.0,
        w.timer_heap.0,
        w.tick_now.0,
        w.data_queue_configs.0,
        w.data_queue_controls.0,
        w.data_queue_waits.0,
        w.data_queue_timers.0,
        w.data_queue_placeholder.0,
    )
}

fn span(span: Span) -> String {
    format!("{{\"start\":{},\"end\":{},\"size\":{}}}", span.start(), span.end(), span.len())
}

fn arena(arena: TimerArena) -> String {
    format!(
        "{{\"guest\":{},\"capacity\":{},\"node_bytes\":{}}}",
        span(arena.guest),
        arena.capacity,
        arena.node_bytes(),
    )
}

fn main() {
    let c = KernelContext(GuestAddress(0));
    let t = Task(GuestAddress(0));
    let d = TaskDefinition(GuestAddress(0));
    let l = LockRecord(GuestAddress(0));
    let h = ReadyHead(GuestAddress(0));
    let [sp0, sp1] = d.initial_sp_terms();
    let context = format!(
        "{{\"size\":{},\"id\":{},\"owner\":{},\"scheduler_lock\":{},\"object_lock\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"selected\":{},\"dispatch_enabled\":{},\"tick_now\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"first_entry\":{},\"tasks\":{},\"task_count\":{},\"data_queues\":{},\"data_queue_count\":{}}}",
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
        c.tick_now().0,
        c.timer_heap().0,
        c.timer_count().0,
        c.timer_base().0,
        c.first_entry().0,
        c.tasks().0,
        c.task_count().0,
        c.data_queues().0,
        c.data_queue_count().0,
    );
    let lock = format!(
        "{{\"word\":{},\"acquire\":{},\"release\":{}}}",
        l.word().0,
        l.acquire().0,
        l.release().0,
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
        "{{\"priority\":{},\"sp_terms\":[{},{}]}}",
        d.priority().0,
        sp0.0,
        sp1.0,
    );
    let head = format!(
        "{{\"size\":{},\"next\":{},\"prev\":{}}}",
        ReadyHead::SIZE,
        h.next().0,
        h.prev().0,
    );
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
    let dq = DataQueueConfig(GuestAddress(0));
    let dc = DataQueueControl(GuestAddress(0));
    let dw = DataQueueWait(GuestAddress(0));
    let data_queue_config = format!(
        "{{\"size\":{},\"attributes\":{},\"capacity\":{},\"ring_base\":{},\"exists\":{},\"sender_priority_order\":{}}}",
        DataQueueConfig::SIZE,
        dq.attributes().0,
        dq.capacity().0,
        dq.ring_base().0,
        dq.exists().0,
        DataQueueConfig::SENDER_PRIORITY_ORDER,
    );
    let data_queue_control = format!(
        "{{\"size\":{},\"sender_queue\":{},\"creation\":{},\"receivers\":{},\"count\":{},\"read\":{},\"write\":{}}}",
        DataQueueControl::SIZE,
        dc.sender_queue().0 .0,
        dc.creation().0,
        dc.receivers().0 .0,
        dc.count().0,
        dc.read().0,
        dc.write().0,
    );
    let data_queue_wait = format!(
        "{{\"size\":{},\"data\":{}}}",
        DataQueueWait::SIZE,
        dw.data().0,
    );
    println!(
        "{{\"a\":{},\"b\":{},\"own_id\":{},\"context\":{},\"lock\":{},\"task\":{},\"definition\":{},\"ready_head\":{},\"priorities\":{},\"task_slots\":{},\"task_states\":[{}],\"data_queue_config\":{},\"data_queue_control\":{},\"data_queue_wait\":{},\"timer_entry_size\":{},\"timer_arenas\":{{\"a\":{},\"b\":{}}},\"timer_node_bytes\":{},\"state_ready\":{},\"state_wait\":{},\"state_suspended\":{},\"state_sleeping\":{},\"state_object_queued\":{},\"state_owner_lock_queued\":{}}}",
        domain(Domain::A),
        domain(Domain::B),
        DomainId::Own.word(),
        context,
        lock,
        task,
        definition,
        head,
        Priority::COUNT,
        TaskSlot::COUNT,
        states,
        data_queue_config,
        data_queue_control,
        data_queue_wait,
        TIMER_ENTRY_SIZE,
        arena(TIMER_ARENAS.a),
        arena(TIMER_ARENAS.b),
        TIMER_NODE_BYTES,
        TaskState::READY.bits(),
        TaskState::WAIT.bits(),
        TaskState::SUSPENDED.bits(),
        TaskState::SLEEPING.bits(),
        TaskState::OBJECT_QUEUED.bits(),
        TaskState::OWNER_LOCK_QUEUED.bits(),
    );
}
