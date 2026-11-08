//! Prints the timer heap's stock words, record offsets and the owned reservation from `xm1_rtos_layout`.

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

fn main() {
    let context = KernelContext(GuestAddress(0));
    let lock = LockRecord(GuestAddress(0));
    println!(
        "{{\"a\":{},\"b\":{},\"context\":{{\"scheduler_lock\":{},\"timer_heap\":{},\"timer_count\":{},\"timer_base\":{},\"tick_now\":{}}},\"lock\":{{\"word\":{},\"acquire\":{},\"release\":{}}},\"timer_entry_size\":{},\"task_slots\":{},\"reservation\":{{\"task_pool\":{{\"guest\":{},\"physical\":{},\"allocatable\":{},\"allocatable_physical\":{},\"reserved\":{}}},\"timer_arenas\":{{\"a\":{},\"b\":{}}},\"timer_node_bytes\":{}}}}}",
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
