//! Prints the cyclic family's stock words and record offsets from `xm1_rtos_layout`.

#[allow(unused_attributes)]
#[path = "../../rtos_layout/src/lib.rs"]
mod layout;

use layout::*;

fn domain(domain: Domain) -> String {
    let w = domain.words();
    format!(
        "{{\"handler_context\":{},\"current_context\":{},\"scheduler_lock\":{},\"contexts\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{},\"cyclic_configs\":{},\"cyclic_controls\":{}}}",
        w.handler_context.0,
        w.current_context.0,
        w.scheduler_lock.0,
        w.contexts.0,
        w.timer_heap.0,
        w.timer_count.0,
        w.timer_base.0,
        w.tick_now.0,
        w.cyclic_configs.0,
        w.cyclic_controls.0,
    )
}

fn main() {
    let context = KernelContext(GuestAddress(0));
    let lock = LockRecord(GuestAddress(0));
    println!(
        "{{\"a\":{},\"b\":{},\"cyclic_count\":{{\"a\":{},\"b\":{}}},\"context\":{{\"scheduler_lock\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{},\"cyclics\":{},\"cyclic_count\":{},\"cyclic_callback\":{}}},\"lock\":{{\"word\":{},\"acquire\":{},\"release\":{}}},\"cyclic_config_size\":{},\"cyclic_control_size\":{},\"cyclic_slots\":{},\"timer_entry_size\":{}}}",
        domain(Domain::A),
        domain(Domain::B),
        0x0004_daccu32,
        0x000d_da90u32,
        context.scheduler_lock().0 .0,
        context.timer_heap().0,
        context.timer_count().0,
        context.timer_base().0,
        context.tick_now().0,
        context.cyclics().0,
        context.cyclic_count().0,
        context.cyclic_callback().0,
        lock.word().0,
        lock.acquire().0,
        lock.release().0,
        CyclicConfig::SIZE,
        CyclicControl::SIZE,
        CyclicSlot::COUNT,
        TIMER_ENTRY_SIZE,
    );
}
