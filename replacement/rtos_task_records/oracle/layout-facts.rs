//! Prints the task-record tables and field offsets from `xm1_rtos_layout`.

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
mod layout;

use layout::*;

fn domain(domain: Domain) -> String {
    let w = domain.words();
    let (mask_save, mask_restore) = match domain {
        Domain::A => (0x0004_2560u32, 0x0004_2568u32),
        Domain::B => (0x000d_2560u32, 0x000d_2568u32),
    };
    format!(
        "{{\"task_table\":{},\"task_definitions\":{},\"task_slot_ids\":{},\"default_stack_word\":{},\"domain_id\":{},\"mask_save\":{},\"mask_restore\":{}}}",
        w.task_table.0,
        w.task_definitions.0,
        w.task_slot_ids.0,
        w.default_stack_word.address().0,
        domain.id().word(),
        mask_save,
        mask_restore,
    )
}

fn main() {
    let task = Task(GuestAddress(0));
    let definition = TaskDefinition(GuestAddress(0));
    println!(
        "{{\"a\":{},\"b\":{},\"task\":{{\"size\":{},\"next\":{},\"prev\":{},\"definition\":{},\"state\":{},\"priority\":{},\"flags\":{},\"exception_pattern\":{},\"wait\":{},\"saved_sp\":{},\"resume_pc\":{},\"domain\":{},\"terminate_pending\":{},\"priority_change_pending\":{},\"cancel_pending\":{},\"context_discarded\":{},\"pending_priority\":{}}},\"definition\":{{\"size\":{},\"attributes\":{},\"argument\":{},\"entry\":{},\"priority\":{},\"stack_size\":{},\"stack\":{},\"exception_attributes\":{},\"exception_handler\":{},\"slot\":{},\"exists\":{}}},\"task_slots\":{}}}",
        domain(Domain::A),
        domain(Domain::B),
        Task::SIZE,
        task.next().0,
        task.prev().0,
        task.definition().0,
        task.state().0,
        task.priority().0,
        task.flags().0,
        task.exception_pattern().0,
        task.wait().0,
        task.saved_sp().0,
        task.resume_pc().0,
        task.domain().0,
        task.terminate_pending().0,
        task.priority_change_pending().0,
        task.cancel_pending().0,
        task.context_discarded().0,
        task.pending_priority().0,
        TaskDefinition::SIZE,
        definition.attributes().0,
        definition.argument().0,
        definition.entry().0,
        definition.priority().0,
        definition.stack_size().0,
        definition.stack().0,
        definition.exception_attributes().0,
        definition.exception_handler().0,
        definition.slot().0,
        definition.exists().0,
        TaskSlot::COUNT,
    );
}
