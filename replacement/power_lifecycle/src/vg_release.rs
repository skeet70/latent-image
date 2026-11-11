use core::ptr::{read_volatile, write_volatile};
use crate::callback_registry::registry as callbacks;
use crate::callback_slots;
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
use crate::shutdown_rtos::NativeShutdownRtos;
use crate::resource_registry::xm1_power_resource_release;
use crate::resource_route::xm1_power_resource_route_request;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ReleaseError(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DisplayEngine { Primary = 0, Secondary = 1 }

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct EglHandle(u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct VglibObject(u32);


const DISPLAY: usize = 0x955af8;
const RENDER: usize = 0x8e2098;
const VGLIB: usize = 0x8fa5dc;
type Completion = unsafe extern "C" fn(i32);

unsafe extern "C" {
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn power_owned_vglib_destroy_object(object: VglibObject, operations: u32);
    fn xm1_heap_free_r(context: u32, payload: u32);
    fn xm1_power_delay_us(ticks: u32);
    fn power_owned_egl_bind(display: EglHandle, draw: EglHandle, read: EglHandle, context: EglHandle) -> i32;
    fn power_owned_egl_destroy_surface(display: EglHandle, surface: EglHandle) -> i32;
    fn power_owned_egl_destroy_context(display: EglHandle, context: EglHandle) -> i32;
    fn power_owned_egl_terminate(display: EglHandle) -> i32;
    fn power_owned_egl_release_thread() -> i32;
}

unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn put_word(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn put_byte(address: usize, value: u8) { unsafe { write_volatile(address as *mut u8, value) } }
unsafe fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) } }

/// The caller owns the stock display and VGLIB lifecycle serialization.
///
/// # Safety
/// Stock lifecycle RAM, device registers, and the imported owned runtime must be mapped.
pub unsafe fn release_output() -> Result<(), ReleaseError> {
    let status = unsafe { release_output_status() };
    if status == 0 { Ok(()) } else { Err(ReleaseError(status)) }
}

unsafe fn release_output_status() -> i32 {
    unsafe {
        let remaining = byte(DISPLAY + 0x34).wrapping_sub(1);
        put_byte(DISPLAY + 0x34, remaining);
        if remaining != 0 { return -12; }
        let secondary = word(DISPLAY + 0x28) == 0 && word(DISPLAY + 0x10) == 1;
        let engine = if secondary { DisplayEngine::Secondary } else { DisplayEngine::Primary };
        let status = stop_output(engine);
        let offset = if secondary { 0x54 } else { 0x58 };
        let registration = word(DISPLAY + offset);
        if registration != 0 {
            let _ = xm1_power_resource_release(registration);
            put_word(DISPLAY + offset, 0);
        }
        let _ = release_vglib(None);
        let _ = xm1_power_resource_route_request(0x304, 0x15, 0);
        status
    }
}

unsafe fn clear_output_masks(engine: DisplayEngine) {
    unsafe {
        let registers = match engine { DisplayEngine::Primary => 0xfff6b000, DisplayEngine::Secondary => 0xfff6e000 };
        let control = (word(registers) & !1) | 6;
        let span = word(registers + 0x108) & !0xffff;
        let range = word(registers + 0x40) & !0xffff;
        let origin = word(registers + 0x90) & !0xffff;
        let format = word(registers + 0x34) & !7;
        let options = word(registers + 0x84) & !7;
        put_byte(0x9554b8, byte(0x9554b8) & !1);
        for offset in [0x40, 4, 8] { put_half(0x9554b8 + offset, 0); }
        put_byte(0x9554b8 + 0x23c, byte(0x9554b8 + 0x23c) & !1);
        for offset in [0x27c, 0x240, 0x244] { put_half(0x9554b8 + offset, 0); }
        for (offset, value) in [(0, control), (0x108, span), (0x40, range), (0x90, origin), (0x34, format), (0x84, options)] {
            put_word(registers + offset, value);
        }
    }
}

unsafe fn reset_register_bank(engine: DisplayEngine) {
    unsafe {
        let registers = match engine { DisplayEngine::Primary => 0xfff6b000, DisplayEngine::Secondary => 0xfff6e000 };
        let saved = word(registers);
        put_word(registers, 0x10000);
        put_word(registers, word(registers) & !0x10000);
        while word(registers) & 0x10000 == 0 { core::hint::spin_loop(); }
        put_word(registers, saved | 0x10000);
    }
}

unsafe fn stop_output(engine: DisplayEngine) -> i32 {
    unsafe {
        clear_output_masks(engine);
        reset_register_bank(engine);
        let runtime = 0x9553cc + engine as usize * 0x58;
        if byte(runtime + 0x54) == 0 { return -10; }
        let remaining = byte(runtime + 0x55).wrapping_sub(1);
        put_byte(runtime + 0x55, remaining);
        if remaining != 0 { return -12; }
        let remaining = byte(runtime + 0x54).wrapping_sub(1);
        put_byte(runtime + 0x54, remaining);
        if remaining != 0 { return -12; }
        let handle = word(runtime + 0x14);
        if handle != 0 {
            let status = match engine {
                DisplayEngine::Primary => release_primary_runtime(handle),
                DisplayEngine::Secondary => xm1_power_timer_release_handle(handle),
            };
            if status == 0 { put_word(runtime + 0x14, 0); }
        }
        0
    }
}

unsafe extern "C" fn api_complete(status: i32) {
    unsafe {
        put_byte(VGLIB + 0x204, 1);
        put_word(VGLIB + 0x1c, status as u32);
    }
}

unsafe fn send_completion() -> i32 {
    let callback = (api_complete as *const () as usize as u32).to_le_bytes();
    let mut rtos = NativeShutdownRtos;
    let sender = rtos.current_task();
    let message = ServiceMessage {
        operation: RequestId(0x5901),
        sender,
        destination: QueueId(0x49),
        payload: &callback,
    };
    if rtos.post(message).is_ok() { 0 } else { -255 }
}

/// # Safety
/// The VGLIB task must consume messages through the owned task dispatcher.
pub unsafe fn release_vglib(completion: Option<Completion>) -> i32 {
    unsafe {
        let remaining = word(VGLIB).wrapping_sub(1);
        put_word(VGLIB, remaining);
        if remaining != 0 { return 0; }
        release_objects();
        for index in 0..6144 { put_word(0x8e22a4 + index * 16 + 12, 0); }
        put_byte(VGLIB + 0x204, 0);
        put_word(VGLIB + 0x1c, 0);
        let _ = send_completion();
        while byte(VGLIB + 0x204) == 0 { xm1_power_delay_us(5); }
        let _ = xm1_power_resource_release(word(VGLIB + 4));
        put_word(VGLIB + 4, 0);
        let status = word(VGLIB + 0x1c) as i32;
        if let Some(completion) = completion { completion(status); }
        status
    }
}

unsafe fn initialize_list(head: usize) {
    unsafe {
        put_word(head, head as u32);
        put_word(head + 4, head as u32);
        put_word(head + 8, 0);
    }
}

unsafe fn clear_list(head: usize) {
    unsafe {
        let mut node = word(head) as usize;
        while node != head {
            let next = word(node) as usize;
            put_word(node + 12, 0);
            node = next;
        }
        initialize_list(head);
    }
}

unsafe fn append_snapshot(head: usize, object: u32) -> bool {
    unsafe {
        for index in 0..6144 {
            let node = 0x8e22a4 + index * 16;
            if word(node + 12) != 0 { continue; }
            put_word(node + 12, 1);
            put_word(node, head as u32);
            let previous = word(head + 4) as usize;
            put_word(node + 4, previous as u32);
            put_word(previous, node as u32);
            put_word(head + 4, node as u32);
            put_word(node + 8, object);
            return true;
        }
        false
    }
}

unsafe fn remove_object(object: VglibObject) {
    unsafe {
        for group in 0..24 {
            let head = 0x8fa2a4 + group * 12;
            let mut node = word(head) as usize;
            while node != head {
                if word(node + 8) == object.0 {
                    let next = word(node) as usize;
                    let previous = word(node + 4) as usize;
                    put_word(next + 4, previous as u32);
                    put_word(previous, next as u32);
                    put_word(node + 12, 0);
                    let count = 0x8fa2a4 + 0x120 + group;
                    put_byte(count, byte(count).wrapping_sub(1));
                    return;
                }
                node = word(node) as usize;
            }
        }
    }
}

unsafe fn release_objects() {
    unsafe {
        for group in (0..24).rev() {
            let source = 0x8fa2a4 + group * 12;
            let mut temporary = [0u32; 3];
            let head = temporary.as_mut_ptr() as usize;
            initialize_list(head);
            let mut node = word(source) as usize;
            let mut copied = true;
            while node != source {
                if !append_snapshot(head, word(node + 8)) {
                    copied = false;
                    break;
                }
                node = word(node) as usize;
            }
            if !copied { continue; }
            node = word(head) as usize;
            let mut found_null = false;
            while node != head {
                let object = VglibObject(word(node + 8));
                if object.0 == 0 {
                    found_null = true;
                    break;
                }
                remove_object(object);
                let operations = word(object.0 as usize + 0xc0);
                power_owned_vglib_destroy_object(object, operations);
                xm1_heap_free_r(word(0x89c970), object.0);
                node = word(node) as usize;
            }
            clear_list(head);
            if found_null { return; }
        }
    }
}

unsafe fn release_render_context() -> i32 {
    unsafe {
        let display = EglHandle(word(RENDER + 0x84));
        if power_owned_egl_bind(display, EglHandle(0), EglHandle(0), EglHandle(0)) == 0 { return -255; }
        for stage in 0..6 {
            for buffer in 0..2 {
                let slot = RENDER + 0x90 + stage * 8 + buffer * 4;
                let _ = power_owned_egl_destroy_surface(EglHandle(word(RENDER + 0x84)), EglHandle(word(slot)));
                put_word(slot, 0);
            }
        }
        if power_owned_egl_destroy_context(EglHandle(word(RENDER + 0x84)), EglHandle(word(RENDER + 0x88))) == 0 { return -255; }
        if power_owned_egl_terminate(EglHandle(word(RENDER + 0x84))) != 1 { return -255; }
        put_word(RENDER + 0x84, 0);
        if power_owned_egl_release_thread() != 1 { return -255; }
        0
    }
}

unsafe fn complete_waiters() {
    unsafe {
        for index in 0..6 {
            let slot = RENDER + 0x100 + index * 4;
            let target = word(slot);
            if target != 0 {
                put_word(slot, 0);
                let _ = callback_slots::call_signed_argument(callbacks(), target, word(RENDER + 0x4c) as i32);
            }
        }
        put_word(RENDER + 0x4c, 0);
    }
}

/// Handles command 0x5901 after the task73 receive operation.
///
/// # Safety
/// `payload` is the received four-byte completion payload. Task73 owns render state.
pub unsafe fn consume_release_message(payload: *const u32) {
    unsafe {
        if word(RENDER + 0x3c) == 0 {
            for index in 0..6 { put_word(RENDER + 0x100 + index * 4, 0); }
            put_word(RENDER + 0x100, read_volatile(payload));
            put_word(RENDER + 0x48, 0x7cfc4c);
            put_word(RENDER + 0x44, 2);
            put_word(RENDER + 0x40, 0);
            put_word(RENDER + 0x3c, 1);
            put_word(RENDER + 0x4c, 0);
            let status = release_render_context();
            if status == 0 {
                put_word(RENDER + 0x40, 1);
                complete_waiters();
            }
            put_word(RENDER + 0x3c, 0);
            put_word(RENDER + 0x40, u32::MAX);
            put_word(RENDER + 0x48, 0);
            put_word(RENDER + 0x44, 0);
            put_word(RENDER + 0x4c, status as u32);
        }
        if word(RENDER + 0x4c) != 0 { complete_waiters(); }
    }
}

/// # Safety
/// The stock lifecycle memory and owned runtime imports must be initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn power_vg_release_output() -> i32 { unsafe { release_output_status() } }



unsafe fn release_primary_runtime(handle: u32) -> i32 {
    unsafe {
        if handle == 0 { return -30; }
        let saved = xm1_power_interrupt_lock();
        let mut found = None;
        for table in [0x917ee4usize, 0x917f24] {
            for index in 0..4 {
                if word(table + index * 16) == handle {
                    found = Some((table, index));
                    break;
                }
            }
            if found.is_some() { break; }
        }
        let status = if let Some((table, index)) = found {
            let kind = word(table + index * 16 + 12);
            let mut destination = index;
            while destination + 1 < 4 {
                let source = table + (destination + 1) * 16;
                let target = table + destination * 16;
                for offset in [12, 0, 4, 8] { put_word(target + offset, word(source + offset)); }
                if word(source) == 0 { break; }
                destination += 1;
            }
            for offset in [12, 0, 4, 8] { put_word(table + destination * 16 + offset, 0); }
            if kind == 1 { put_word(0x917ee0, word(0x917ee0).wrapping_sub(1)); }
            0
        } else { -30 };
        xm1_power_interrupt_unlock(saved);
        status
    }
}
