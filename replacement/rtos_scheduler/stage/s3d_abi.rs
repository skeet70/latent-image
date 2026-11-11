//! A32 adapter for the six S3d lifecycle entries over the shared Rust core.
#![cfg_attr(not(test), no_std)]

#[path = "../src/s3d.rs"]
mod s3d;

#[path = "../src/s3d_cpu.rs"]
mod cpu;
use cpu::Cpu;


use s3d::layout::{Domain, GuestAddress, KernelContext, Task};
use s3d::{
    change_priority, delay, interrupt_release_wait, release_wait, terminate, timed_sleep,
    timer_heap_active, EncodedId, PriorityArgument, ServiceError, TimerEntry, Ticks,
};

#[cfg(feature = "domain-a")]
const EXECUTING: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const EXECUTING: Domain = Domain::B;

const CPU: Cpu = unsafe { Cpu::claim(EXECUTING) };

#[cfg(all(feature = "domain-a", feature = "domain-b"))]
compile_error!("feature=\"domain-a\" and feature=\"domain-b\" are exclusive");
#[cfg(all(not(feature = "domain-a"), not(feature = "domain-b")))]
compile_error!("select one of feature=\"domain-a\" or feature=\"domain-b\"");


#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_timer_active"]
pub extern "C" fn s3d_timer_active(context: u32, entry: u32) -> bool {
    timer_heap_active(
        &CPU,
        KernelContext(GuestAddress(context)),
        TimerEntry(GuestAddress(entry)),
    )
}

#[inline(always)]
fn status(result: Result<(), ServiceError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => error.status(),
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_terminate_body"]
pub extern "C" fn s3d_terminate_body(id: i32) -> i32 {
    status(terminate(&CPU, EXECUTING, EncodedId(id)))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_change_priority_body"]
pub extern "C" fn s3d_change_priority_body(id: i32, priority: u32) -> i32 {
    status(change_priority(
        &CPU,
        EXECUTING,
        EncodedId(id),
        PriorityArgument(priority),
    ))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_timed_sleep_body"]
pub extern "C" fn s3d_timed_sleep_body(timeout: i32) -> i32 {
    timed_sleep(&CPU, EXECUTING, Ticks(timeout))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_release_core"]
pub extern "C" fn s3d_release_core(id: i32, handler: u32) -> i32 {
    status(s3d::release(&CPU, EXECUTING, EncodedId(id), handler != 0))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_release_wait_body"]
pub extern "C" fn s3d_release_wait_body(id: i32) -> i32 {
    s3d_release_core(id, 0)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_interrupt_release_wait_body"]
pub extern "C" fn s3d_interrupt_release_wait_body(id: i32) -> i32 {
    s3d_release_core(id, 1)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.s3d_delay_body"]
pub extern "C" fn s3d_delay_body(delay_ticks: i32) -> i32 {
    delay(&CPU, EXECUTING, Ticks(delay_ticks))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.scheduler_timeout_body"]
pub extern "C" fn xm1_rtos_scheduler_timeout_body(task: u32) -> i32 {
    s3d::timeout::timeout(&CPU, EXECUTING, Task(GuestAddress(task)))
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.scheduler_delay_completion_body"]
pub extern "C" fn xm1_rtos_scheduler_delay_completion_body(task: u32) -> i32 {
    s3d::timeout::delay(&CPU, EXECUTING, Task(GuestAddress(task)))
}

core::arch::global_asm!(
    ".section .text.s3d_terminate_entry,\"ax\",%progbits",
    ".global s3d_terminate",
    ".type s3d_terminate, %function",
    "s3d_terminate: b s3d_terminate_body",
    ".section .text.s3d_change_priority_entry,\"ax\",%progbits",
    ".global s3d_change_priority",
    ".type s3d_change_priority, %function",
    "s3d_change_priority: b s3d_change_priority_body",
    ".section .text.s3d_timed_sleep_entry,\"ax\",%progbits",
    ".global s3d_timed_sleep",
    ".type s3d_timed_sleep, %function",
    "s3d_timed_sleep: b s3d_timed_sleep_body",
    ".section .text.s3d_release_wait_entry,\"ax\",%progbits",
    ".global s3d_release_wait",
    ".type s3d_release_wait, %function",
    "s3d_release_wait: b s3d_release_wait_body",
    ".section .text.s3d_interrupt_release_wait_entry,\"ax\",%progbits",
    ".global s3d_interrupt_release_wait",
    ".type s3d_interrupt_release_wait, %function",
    "s3d_interrupt_release_wait: b s3d_interrupt_release_wait_body",
    ".section .text.s3d_delay_entry,\"ax\",%progbits",
    ".global s3d_delay",
    ".type s3d_delay, %function",
    "s3d_delay: b s3d_delay_body",
    ".section .text.scheduler_timeout_entry,\"ax\",%progbits",
    ".global xm1_rtos_scheduler_timeout",
    ".type xm1_rtos_scheduler_timeout, %function",
    "xm1_rtos_scheduler_timeout: b xm1_rtos_scheduler_timeout_body",
    ".section .text.scheduler_delay_completion_entry,\"ax\",%progbits",
    ".global xm1_rtos_scheduler_delay_completion",
    ".type xm1_rtos_scheduler_delay_completion, %function",
    "xm1_rtos_scheduler_delay_completion: b xm1_rtos_scheduler_delay_completion_body",
);

#[cfg(all(target_arch = "arm", not(test), not(feature = "component")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
