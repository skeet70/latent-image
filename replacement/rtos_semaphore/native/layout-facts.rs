//! Prints the `xm1_rtos_layout` addresses and offsets the semaphore oracle builds its cases from, as JSON.

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
mod layout;

use layout::*;

fn domain(domain: Domain) -> String {
    let w = domain.words();
    format!(
        "{{\"id\":{},\"scheduler_lock\":{},\"object_lock\":{},\"contexts\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"dispatch_enabled\":{},\"selected\":{},\"task_table\":{},\"context_a\":{},\"context_b\":{},\"handler_context\":{},\"current_context\":{},\"reschedule\":{},\"timer_base\":{},\"timer_count\":{},\"timer_heap\":{},\"tick_now\":{},\"semaphore_configs\":{},\"semaphore_controls\":{},\"semaphore_waits\":{},\"semaphore_timers\":{}}}",
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
        w.semaphore_configs.0,
        w.semaphore_controls.0,
        w.semaphore_waits.0,
        w.semaphore_timers.0,
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
        "{{\"size\":{},\"id\":{},\"owner\":{},\"scheduler_lock\":{},\"object_lock\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"selected\":{},\"dispatch_enabled\":{},\"first_entry\":{},\"tasks\":{},\"task_count\":{},\"semaphores\":{},\"semaphore_count\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{}}}",
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
        c.first_entry().0,
        c.tasks().0,
        c.task_count().0,
        c.semaphores().0,
        c.semaphore_count().0,
        c.timer_heap().0,
        c.timer_count().0,
        c.timer_base().0,
        c.tick_now().0,
    );
    let lock = format!(
        "{{\"word\":{},\"acquire\":{},\"release\":{}}}",
        l.word().0,
        l.acquire().0,
        l.release().0
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
        "{{\"priority\":{},\"sp_terms\":[{},{}],\"slot\":{}}}",
        d.priority().0,
        sp0.0,
        sp1.0,
        d.slot().0
    );
    let head = format!(
        "{{\"size\":{},\"next\":{},\"prev\":{}}}",
        ReadyHead::SIZE,
        h.next().0,
        h.prev().0
    );
    let states = [
        TaskState::READY,
        TaskState::WAIT,
        TaskState::SUSPENDED,
        TaskState::SLEEPING,
        TaskState::OBJECT_QUEUED,
        TaskState::OWNER_LOCK_QUEUED,
    ]
    .map(|state| state.bits().to_string())
    .join(",");
    let config = SemaphoreConfig(GuestAddress(0));
    let control = SemaphoreControl(GuestAddress(0));
    let wb = WaitBlock(GuestAddress(0));
    let wq = WaitQueue(GuestAddress(0));
    let semaphore_config = format!(
        "{{\"size\":{},\"attributes\":{},\"initial\":{},\"maximum\":{},\"exists\":{},\"priority_order\":{}}}",
        SemaphoreConfig::SIZE,
        config.attributes().0,
        config.initial().0,
        config.maximum().0,
        config.exists().0,
        SemaphoreConfig::PRIORITY_ORDER,
    );
    let semaphore_control = format!(
        "{{\"size\":{},\"queue\":{},\"count\":{}}}",
        SemaphoreControl::SIZE,
        control.queue().0 .0,
        control.count().0,
    );
    let wait_block = format!(
        "{{\"size\":{},\"result\":{},\"queue\":{},\"owner\":{}}}",
        WaitBlock::SIZE,
        wb.result().0,
        wb.queue().0,
        wb.owner().0
    );
    let wait_queue = format!(
        "{{\"size\":{},\"next\":{},\"prev\":{},\"config\":{}}}",
        WaitQueue::SIZE,
        wq.next().0,
        wq.prev().0,
        wq.config().0
    );
    println!(
        "{{\"a\":{},\"b\":{},\"own_id\":{},\"context\":{},\"lock\":{},\"task\":{},\"definition\":{},\"ready_head\":{},\"priorities\":{},\"task_slots\":{},\"semaphore_slots\":{},\"task_states\":[{}],\"semaphore_config\":{},\"semaphore_control\":{},\"wait_block\":{},\"wait_queue\":{},\"timer_entry_size\":{},\"state_ready\":{},\"state_wait\":{},\"state_suspended\":{},\"state_sleeping\":{},\"state_object_queued\":{},\"state_owner_lock_queued\":{},\"flag_queued_activation\":{}}}",
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
        SemaphoreSlot::COUNT,
        states,
        semaphore_config,
        semaphore_control,
        wait_block,
        wait_queue,
        TIMER_ENTRY_SIZE,
        TaskState::READY.bits(),
        TaskState::WAIT.bits(),
        TaskState::SUSPENDED.bits(),
        TaskState::SLEEPING.bits(),
        TaskState::OBJECT_QUEUED.bits(),
        TaskState::OWNER_LOCK_QUEUED.bits(),
        TaskFlags::QUEUED_ACTIVATION.0,
    );
}
