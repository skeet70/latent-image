use core::mem::transmute;
use core::ptr::read_volatile;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct TaskSelector(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct TaskPriority(u32);
unsafe fn read(at: u32) -> u32 { unsafe { read_volatile(at as *const u32) } }
unsafe fn root() -> u32 {
    let current: unsafe extern "C" fn() -> u32 = unsafe { transmute(read(read(0x96bd20) + 0x1618) as usize) };
    unsafe { read(0x96bd20 + current() * 4) }
}
unsafe fn resolve(root: u32, task: TaskSelector) -> TaskSelector {
    if task.0 != 0 { return task; }
    let current: unsafe extern "C" fn() -> u32 = unsafe { transmute(read(root + 0x16c) as usize) };
    TaskSelector(unsafe { current() })
}
unsafe fn encoded(task: TaskSelector) -> u32 {
    let slot = task.0 as u16 as u32;
    slot | unsafe { read(0x7cad9cu32.wrapping_add(slot.wrapping_sub(1).wrapping_mul(4))) } << 16
}
extern "C" { fn xm1_power_service_error(operation: u32, id: i32, status: i32); }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_set_task_phase(task: u32, priority: u32) -> i32 {
    let root = unsafe { root() };
    let task = unsafe { resolve(root, TaskSelector(task)) };
    let priority = TaskPriority(priority);
    let change_priority: unsafe extern "C" fn(u32, u32) -> i32 = unsafe { transmute(read(root + 0x1c) as usize) };
    let status = unsafe { change_priority(encoded(task), priority.0) };
    if status != 0 { unsafe { xm1_power_service_error(5, task.0 as i32, status) }; }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_get_task_phase(task: u32, priority: *mut u32) -> i32 {
    let root = unsafe { root() };
    let task = unsafe { resolve(root, TaskSelector(task)) };
    let get_priority: unsafe extern "C" fn(u32, *mut u32) -> i32 = unsafe { transmute(read(root + 0x20) as usize) };
    let status = unsafe { get_priority(encoded(task), priority) };
    if status != 0 { unsafe { xm1_power_service_error(60, task.0 as i32, status) }; }
    status
}
