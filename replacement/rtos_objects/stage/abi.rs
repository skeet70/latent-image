use super::{load, Cpu, ENTRY};
use crate::objects::block::{block_locked, Block, Deadline, TimerEntryAddress};
use crate::objects::layout::{FlagConfig, FlagControl, GuestAddress, KernelContext, Task, WaitQueue};
use crate::objects::locked::{check_control, OutWord};
use crate::objects::matching::{MatchMode, Pattern};
use crate::objects::order::QueueOrder;
use crate::objects::wake::{release_with, Selection, WaitCompletion};

fn queue_order(queue: WaitQueue) -> QueueOrder {
    QueueOrder::from_attribute(load(GuestAddress(load(queue.config()))))
}

/// `0x44800` / `0xd47c0`; the caller holds the object and scheduler locks.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.helper_block"]
pub extern "C" fn xm1_rtos_objects_wait_block(owner: u32, queue: u32, wait: u32) {
    let queue = WaitQueue(GuestAddress(queue));
    let task = Cpu.current(ENTRY);
    block_locked(
        &Cpu,
        ENTRY,
        owner,
        task,
        GuestAddress(wait),
        queue,
        queue_order(queue),
        None,
    );
}

/// `0x448e4` / `0xd48a4`; the caller holds the object and scheduler locks.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.helper_block_timed"]
pub extern "C" fn xm1_rtos_objects_wait_block_timed(
    owner: u32,
    queue: u32,
    wait: u32,
    timer: u32,
    deadline: u32,
) {
    let queue = WaitQueue(GuestAddress(queue));
    let task = Cpu.current(ENTRY);
    block_locked(
        &Cpu,
        ENTRY,
        owner,
        task,
        GuestAddress(wait),
        queue,
        queue_order(queue),
        Some((
            TimerEntryAddress(GuestAddress(timer)),
            Deadline(deadline),
        )),
    );
}

/// `0x44744` / `0xd4704`; the caller holds the object and scheduler locks.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.helper_release"]
pub extern "C" fn xm1_rtos_objects_wait_release(owner: KernelContext, task: Task) -> u32 {
    match release_with(&Cpu, owner, task, WaitCompletion::Released) {
        Selection::Unchanged => 0,
        Selection::Changed => 1,
    }
}

/// `0x480cc` / `0xd808c`; the caller holds the object lock.
#[no_mangle]
#[inline(never)]
#[link_section = ".text.helper_check"]
pub extern "C" fn xm1_rtos_objects_check(control: u32, want: u32, mode: u32, out: u32) -> i32 {
    let control = FlagControl(GuestAddress(control));
    let config = FlagConfig(GuestAddress(load(control.0.offset(0x08))));
    match check_control(
        &Cpu,
        control,
        config,
        Pattern(want),
        MatchMode::from_stored(mode),
        OutWord(GuestAddress(out)),
    ) {
        Some(_) => 1,
        None => 0,
    }
}
