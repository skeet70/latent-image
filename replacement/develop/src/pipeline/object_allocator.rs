use core::ptr::{read_volatile, write_volatile};
use super::buffers::{xm1_develop_object_group, xm1_develop_canonical_object_group, xm1_develop_enter_critical, xm1_develop_leave_critical, xm1_develop_trace_object_action};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Frame(u32);
impl Frame {
    unsafe fn word(self, offset: u32) -> u32 {
        unsafe { read_volatile(self.0.wrapping_add(offset) as *const u32) }
    }
}
#[derive(Clone, Copy)]
#[repr(transparent)]
struct CriticalToken(u32);
unsafe fn enter_critical() -> CriticalToken { CriticalToken(unsafe { xm1_develop_enter_critical() }) }
unsafe fn leave_critical(token: CriticalToken) { unsafe { xm1_develop_leave_critical(token.0) }; }

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Plane {
    format: u32,
    address: u32,
    length: u32,
    stride: u32,
    line_size: u32,
    lines: u32,
    flags: u32,
}
#[derive(Clone, Copy)]
#[repr(C)]
struct ObjectRecord {
    owner: i16,
    owner_reserved: u16,
    group: u32,
    kind: u32,
    slot: i16,
    slot_reserved: u16,
    plane: Plane,
}
unsafe extern "C" {
    #[link_name = "xm1_pipeline_image_wait_reserved"]
    fn xm1_develop_lock_object_semaphore(handle: u32, timeout: u32) -> u32;
    #[link_name = "xm1_pipeline_image_signal_reserved"]
    fn xm1_develop_unlock_object_semaphore(handle: u32) -> u32;
    #[link_name = "xm1_pipeline_image_wait_all"]
    fn xm1_develop_wait_current(flags: u32, result: *mut u32, timeout: u32) -> u32;
    #[link_name = "xm1_pipeline_image_signal_current_zero"]
    fn xm1_develop_signal_current();
    #[link_name = "xm1_pipeline_image_send_notification"]
    fn xm1_develop_notify_task(task: u32, flags: u32) -> u32;
    fn xm1_develop_append_stream_text(task: u32, text: u32);
    fn xm1_develop_trace_event(text: u32);
}

fn reference_count(shared: u32, slot: i32) -> *mut u16 {
    0x01548a4cu32.wrapping_add(shared.wrapping_sub(1).wrapping_mul(201).wrapping_add(slot as u32).wrapping_mul(2)) as *mut u16
}

unsafe fn bitmap_update(canonical: u32, first: u32, count: u32, set: bool) {
    for offset in 0..count {
        let bit = first.wrapping_add(offset);
        let address = 0x015480dcu32.wrapping_add(canonical.wrapping_mul(26)).wrapping_add(bit >> 3) as *mut u8;
        let value = unsafe { read_volatile(address) };
        let mask = 0x80u8 >> (bit & 7);
        unsafe { write_volatile(address, if set { value | mask } else { value & !mask }) };
    }
}

fn allocation_blocks(size: u32, unit: u32) -> u32 {
    let whole = size / unit;
    if whole == 0 { 1 } else { whole.wrapping_add(u32::from(size % unit != 0)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_release_object(group: u32, slot: i32, size: u32) -> u32 {
    let descriptor = Frame(unsafe { xm1_develop_object_group(group) });
    let canonical = unsafe { xm1_develop_canonical_object_group(group) };
    let token = unsafe { enter_critical() };
    let shared = unsafe { descriptor.word(8) };
    if shared != 0 {
        let address = reference_count(shared, slot);
        let remaining = unsafe { read_volatile(address) }.wrapping_sub(1);
        unsafe { write_volatile(address, remaining) };
        if remaining != 0 {
            unsafe { leave_critical(token); xm1_develop_trace_object_action(group, slot, 2); }
            return 0;
        }
    }
    let first = (slot.wrapping_sub(1) as i16 as i32) as u32;
    let count = if group == 33 { allocation_blocks(size, unsafe { Frame(descriptor.word(12)).word(8) }) } else { 1 };
    unsafe { bitmap_update(canonical, first, count, false); leave_critical(token); }
    let result = unsafe { xm1_develop_unlock_object_semaphore(descriptor.word(0)) };
    if result != 0 {
        let task = unsafe { super::image_planes::current_task() };
        unsafe { xm1_develop_append_stream_text(task.0 & 255, 0xe498b8); xm1_develop_trace_event(0xe498c8); }
        return result;
    }
    unsafe { xm1_develop_trace_object_action(group, slot, 2) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_notify_object_waiters() {
    let token = unsafe { enter_critical() };
    let mut waiters = [(0u32, 0u32); 15];
    for (index, entry) in waiters.iter_mut().enumerate() {
        let source = Frame(0x0154c32c + index as u32 * 8);
        *entry = (unsafe { source.word(0) }, unsafe { source.word(4) });
    }
    unsafe { leave_critical(token) };
    for (group, task) in waiters {
        if group != 0 && unsafe { xm1_develop_notify_task(task, 16) } != 0 { return; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_range_available(plane: *const Plane, except_owner: i32) -> u32 {
    let candidate = unsafe { read_volatile(plane) };
    let start = candidate.address | 0x40000000;
    let end = candidate.address.wrapping_add(if candidate.lines == 0 || candidate.stride == 0 { candidate.length } else { candidate.lines.wrapping_mul(candidate.stride) }).wrapping_sub(1);
    let token = unsafe { enter_critical() };
    let entries = 0x0154ad2c as *const ObjectRecord;
    let mut available = true;
    for index in 0..128 {
        let record = unsafe { read_volatile(entries.add(index)) };
        if record.owner == 0 { break; }
        if except_owner != 0 && i32::from(record.owner) == except_owner { continue; }
        if candidate.format != record.plane.format { continue; }
        let other_start = record.plane.address | 0x40000000;
        let other_end = other_start.wrapping_add(if record.plane.lines == 0 || record.plane.stride == 0 { record.plane.length } else { record.plane.lines.wrapping_mul(record.plane.stride) }).wrapping_sub(1);
        if if start < other_start { end >= other_start } else { start <= other_end } {
            available = false;
            break;
        }
    }
    unsafe { leave_critical(token) };
    u32::from(available)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_find_object_range(group: u32, owner: i32, size: u32, ascending: u32) -> i32 {
    let descriptor = Frame(unsafe { xm1_develop_object_group(group) });
    let canonical = unsafe { xm1_develop_canonical_object_group(group) };
    let token = unsafe { enter_critical() };
    for index in 0..26u32 {
        let source = 0x015480dcu32.wrapping_add(canonical.wrapping_mul(26)).wrapping_add(index) as *const u8;
        unsafe { write_volatile((0x015480b8 + index) as *mut u8, read_volatile(source)) };
    }
    unsafe { leave_critical(token) };
    if unsafe { descriptor.word(0) } == 0 { return -1; }
    let canonical = Frame(unsafe { xm1_develop_object_group(canonical) });
    let capacity = unsafe { read_volatile(canonical.0.wrapping_add(4) as *const i16) };
    let planes = unsafe { descriptor.word(12) };
    let count = allocation_blocks(size, unsafe { Frame(planes).word(8) });
    let limit = (i32::from(capacity).wrapping_sub(count as i32)) as u32;
    let mut first = if ascending == 1 { 0u16 } else { limit as u16 };
    if ascending != 1 && (first as i16) < 0 { return -1; }
    loop {
        let start = first as i16 as i32 as u32;
        let end = start.wrapping_add(count);
        let mut available = true;
        for bit in start..end {
            let byte = unsafe { read_volatile((0x015480b8u32.wrapping_add(bit >> 3)) as *const u8) };
            if byte & (0x80 >> (bit & 7)) != 0 { available = false; break; }
        }
        if available {
            for index in start..end {
                if unsafe { xm1_develop_object_range_available(planes.wrapping_add(index.wrapping_mul(28)) as *const Plane, owner) } == 0 { available = false; break; }
            }
        }
        if available { return i32::from(first as i16); }
        if ascending == 1 {
            first = first.wrapping_add(1);
            if first as i16 as i32 as u32 > limit { return -1; }
        } else {
            first = first.wrapping_sub(1);
            if first as i16 == -1 { return -1; }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_priority(index: u32) -> u32 {
    unsafe { object_priority(index) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_priority_index(operation: u32) -> u32 {
    unsafe { object_priority_index(operation) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_operation(group: u32) -> u32 {
    match group { 0 => 1, 2 => 2, 0x1a => 4, 0x21 => 5, 0x12 => 6, _ => 0 }
}

unsafe fn object_priority(index: u32) -> u32 {
    let mode = unsafe { read_volatile(0x015480d4 as *const u32) };
    unsafe { read_volatile(0x00e49320u32.wrapping_add(mode.wrapping_mul(28)).wrapping_add(index.wrapping_mul(4)) as *const u32) }
}

unsafe fn object_priority_index(operation: u32) -> u32 {
    for index in 0..7 {
        if unsafe { object_priority(index) } == operation { return index; }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_order_object(group: u32, owner: i32, notify: *mut u32) -> u32 {
    let operation = unsafe { xm1_develop_object_operation(group) };
    unsafe { write_volatile(notify, 0) };
    let priority = unsafe { object_priority_index(operation) };
    if priority == 0 { return 1; }
    let task = unsafe { super::image_planes::current_task() };
    let token = unsafe { enter_critical() };
    let count = unsafe { read_volatile(0x0154a9b8 as *const i32) };
    let mut position = 0i32;
    while position < count {
        if i32::from(unsafe { read_volatile((0x0154a04cu32.wrapping_add(position as u32 * 12)) as *const i16) }) == owner { break; }
        position += 1;
    }
    let mut granted = true;
    if position < count {
        for index in 0..position {
            let entry = Frame(0x0154a04c + index as u32 * 12);
            let prior = unsafe { entry.word(4) };
            if unsafe { object_priority_index(prior) } < priority || (prior == operation && unsafe { entry.word(8) } != 0) {
                granted = false;
                break;
            }
        }
    }
    let entry = Frame(0x0154a04cu32.wrapping_add(position as u32 * 12));
    unsafe { write_volatile((entry.0 + 4) as *mut u32, operation); write_volatile((entry.0 + 8) as *mut u32, if granted { 0 } else { task.0 }); }
    if granted {
        for index in 0..count {
            let entry = Frame(0x0154a04c + index as u32 * 12);
            let prior = unsafe { entry.word(4) };
            let rank = unsafe { object_priority_index(prior) };
            if rank < priority { break; }
            let waiting = unsafe { entry.word(8) };
            if prior == operation && waiting != 0 {
                if unsafe { object_priority_index(1) } < rank {
                    unsafe { write_volatile((entry.0 + 4) as *mut u32, object_priority(rank - 1)) };
                }
                unsafe { write_volatile(notify, waiting); write_volatile((entry.0 + 8) as *mut u32, 0); }
                break;
            }
        }
    }
    unsafe { leave_critical(token) };
    u32::from(granted)
}

unsafe fn object_error(text: u32, event: u32) {
    let task = unsafe { super::image_planes::current_task() };
    unsafe { xm1_develop_append_stream_text(task.0 & 255, text); xm1_develop_trace_event(event); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_wait_for_object(group: u32, owner: i32, ordered: i32) -> u32 {
    let descriptor = Frame(unsafe { xm1_develop_object_group(group) });
    let mut notify = 0;
    loop {
        unsafe { xm1_develop_trace_object_action(group, owner, 0) };
        let result = unsafe { xm1_develop_lock_object_semaphore(descriptor.word(0), u32::MAX) };
        if result != 0 {
            unsafe { object_error(0xe4989c, 0xe498ac) };
            return result;
        }
        if ordered == 0 || unsafe { xm1_develop_order_object(group, owner, &mut notify) } != 0 {
            return if notify == 0 { 0 } else { unsafe { xm1_develop_notify_task(notify, 16) } };
        }
        let result = unsafe { xm1_develop_unlock_object_semaphore(descriptor.word(0)) };
        if result != 0 {
            unsafe { object_error(0xe498b8, 0xe498c8) };
            return result;
        }
        unsafe { xm1_develop_trace_object_action(group, owner, 2); xm1_develop_trace_object_action(group, owner, 3); }
        let mut received = 0;
        let result = unsafe { xm1_develop_wait_current(16, &mut received, u32::MAX) };
        if result != 0 { return result; }
        unsafe { xm1_develop_signal_current() };
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ObjectWaiter {
    group: u32,
    task: u32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_add_object_waiter(waiter: *const ObjectWaiter) {
    let waiter = unsafe { read_volatile(waiter) };
    unsafe { add_object_waiter(waiter.group, waiter.task) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_remove_object_waiter(waiter: *const ObjectWaiter) {
    let waiter = unsafe { read_volatile(waiter) };
    unsafe { remove_object_waiter(waiter.group, waiter.task) };
}

unsafe fn add_object_waiter(group: u32, task: u32) {
    let token = unsafe { enter_critical() };
    for index in 0..15u32 {
        let address = 0x0154c32c + index * 8;
        if unsafe { read_volatile(address as *const u32) } == 0 {
            unsafe { write_volatile(address as *mut u32, group); write_volatile((address + 4) as *mut u32, task); }
            break;
        }
    }
    unsafe { leave_critical(token) };
}

unsafe fn remove_object_waiter(group: u32, task: u32) {
    let token = unsafe { enter_critical() };
    for index in 0..15u32 {
        let address = Frame(0x0154c32c + index * 8);
        if unsafe { address.word(0) } == group && unsafe { address.word(4) } == task {
            unsafe { write_volatile(address.0 as *mut u32, 0); write_volatile((address.0 + 4) as *mut u32, 0); }
            break;
        }
    }
    unsafe { leave_critical(token) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_acquire_object(group: u32, kind: u32, owner: i32, inherited: i32, slot: *mut i16, ordered: i32, size: u32) -> u32 {
    let descriptor = Frame(unsafe { xm1_develop_object_group(group) });
    let canonical = unsafe { xm1_develop_canonical_object_group(group) };
    let canonical_descriptor = Frame(unsafe { xm1_develop_object_group(canonical) });
    let shared = unsafe { descriptor.word(8) };
    let mut first_shared = 0;
    if shared as i32 > 0 {
        while first_shared <= 92 {
            if unsafe { Frame(xm1_develop_object_group(first_shared)).word(8) } == shared { break; }
            first_shared += 1;
        }
    }
    let capacity = unsafe { read_volatile((canonical_descriptor.0 + 4) as *const i16) };
    let (selected, token) = if group != first_shared && shared != 0 {
        let token = unsafe { enter_critical() };
        (if capacity <= 1 || inherited <= 0 { 1i16 } else { inherited as i16 }, token)
    } else {
        let result = unsafe { xm1_develop_wait_for_object(group, owner, ordered) };
        if result != 0 { return result; }
        let selected = if group < 20 {
            let bitmap = 0x015480dcu32.wrapping_add(canonical.wrapping_mul(26));
            let mut candidate = 0i16;
            if capacity > 0 {
                while candidate != capacity {
                    let index = candidate as i32;
                    let byte = unsafe { read_volatile(bitmap.wrapping_add((index / 8) as u32) as *const u8) };
                    if byte & (0x80u8 >> (index % 8)) == 0 { break; }
                    candidate = candidate.wrapping_add(1);
                }
            }
            candidate
        } else {
            loop {
                let found = unsafe { xm1_develop_find_object_range(group, owner, size, u32::from(kind <= 8)) };
                if found != -1 { break found as i16; }
                let task = unsafe { super::image_planes::current_task() };
                unsafe { xm1_develop_append_stream_text(task.0 & 255, 0xe498d0); xm1_develop_trace_event(0xe498e0); }
                let task = unsafe { super::image_planes::current_task() };
                unsafe { add_object_waiter(group, task.0) };
                let mut received = 0u32;
                let result = unsafe { xm1_develop_wait_current(16, &mut received, u32::MAX) };
                if result != 0 { return result; }
                unsafe { xm1_develop_signal_current(); remove_object_waiter(group, task.0); }
            }
        };
        let token = unsafe { enter_critical() };
        let count = if group == 33 { allocation_blocks(size, unsafe { Frame(descriptor.word(12)).word(8) }) } else { 1 };
        unsafe { bitmap_update(canonical, selected as i32 as u32, count, true) };
        (selected.wrapping_add(1), token)
    };
    if shared as i32 > 0 {
        let address = reference_count(shared, i32::from(selected));
        unsafe { write_volatile(address, read_volatile(address).wrapping_add(1)) };
    }
    unsafe { leave_critical(token); write_volatile(slot, selected); xm1_develop_trace_object_action(group, i32::from(selected), 1); }
    0
}
