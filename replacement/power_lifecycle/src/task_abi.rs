//! Stock task and queue entries. The registry defines them; the executors call them through this
//! one declaration.

use crate::queue_construction::QueueConfiguration;
use core::mem::MaybeUninit;

#[repr(C)]
pub struct TaskDescriptor {
    pub identifier: u32,
    pub name: u32,
    pub priority: u32,
    pub stack: u32,
    pub entry: unsafe extern "C" fn(*const u32) -> !,
    pub argument: *const u32,
    pub reserved: MaybeUninit<u32>,
}

extern "C" {
    pub fn xm1_power_queue_create(queue: u32, configuration: *const QueueConfiguration) -> i32;
    pub fn xm1_power_queue_delete(queue: u32) -> i32;
    pub fn xm1_task_submit(descriptor: u32) -> i32;
    pub fn xm1_task_release(index: u32) -> i32;
}
