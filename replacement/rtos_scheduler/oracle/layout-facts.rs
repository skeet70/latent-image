//! Prints the `xm1_rtos_layout` addresses and offsets the scheduler oracle builds its cases from, as JSON.

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
mod layout;

use layout::*;

fn domain(domain: Domain) -> String {
    let w = domain.words();
    format!(
        "{{\"id\":{},\"scheduler_lock\":{},\"object_lock\":{},\"contexts\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"dispatch_enabled\":{},\"selected\":{},\"task_table\":{},\"context_a\":{},\"context_b\":{}}}",
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
        "{{\"size\":{},\"id\":{},\"owner\":{},\"scheduler_lock\":{},\"object_lock\":{},\"ready_heads\":{},\"ready_bitmap\":{},\"running\":{},\"selected\":{},\"dispatch_enabled\":{},\"first_entry\":{}}}",
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
    );
    let lock = format!("{{\"word\":{},\"acquire\":{},\"release\":{}}}", l.word().0, l.acquire().0, l.release().0);
    let task = format!(
        "{{\"size\":{},\"next\":{},\"prev\":{},\"definition\":{},\"state\":{},\"priority\":{},\"flags\":{},\"exception_pattern\":{},\"saved_sp\":{},\"resume_pc\":{},\"domain\":{},\"terminate_pending\":{},\"priority_change_pending\":{},\"cancel_pending\":{}}}",
        Task::SIZE,
        t.next().0,
        t.prev().0,
        t.definition().0,
        t.state().0,
        t.priority().0,
        t.flags().0,
        t.exception_pattern().0,
        t.saved_sp().0,
        t.resume_pc().0,
        t.domain().0,
        t.terminate_pending().0,
        t.priority_change_pending().0,
        t.cancel_pending().0,
    );
    let definition = format!("{{\"priority\":{},\"sp_terms\":[{},{}]}}", d.priority().0, sp0.0, sp1.0);
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
    println!(
        "{{\"a\":{},\"b\":{},\"own_id\":{},\"context\":{},\"lock\":{},\"task\":{},\"definition\":{},\"ready_head\":{},\"priorities\":{},\"task_slots\":{},\"task_states\":[{}]}}",
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
    );
}
