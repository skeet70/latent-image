#[cfg(target_arch = "arm")]
mod native {
    use core::ptr::{read_volatile, write_volatile};
    use crate::primitives::{QueueDiagnostic, QueueDiagnostics, ServiceStatus};

    extern "C" {
        fn xm1_power_save_irq() -> u32;
        fn xm1_power_restore_irq(saved: u32);
        fn xm1_power_current_task() -> u32;
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
        fn xm1_power_diagnostic_calendar_header(out: *mut u8);
    }
    unsafe fn word(at: u32) -> u32 { read_volatile(at as *const u32) }
    unsafe fn put(at: u32, value: u32) { write_volatile(at as *mut u32, value); }
    unsafe fn half(at: u32) -> u16 { read_volatile(at as *const u16) }
    unsafe fn put_half(at: u32, value: u16) { write_volatile(at as *mut u16, value); }
    unsafe fn byte(at: u32) -> u8 { read_volatile(at as *const u8) }
    unsafe fn put_byte(at: u32, value: u8) { write_volatile(at as *mut u8, value); }
    unsafe fn copy(to: u32, from: u32, count: u32) { for offset in 0..count { put_byte(to + offset, byte(from + offset)); } }
    fn unowned_callback(address: u32) -> ! {
        unsafe { core::arch::asm!("2: b 2b", in("r0") address, options(noreturn, nostack)); }
    }
    unsafe fn thread() -> u32 {
        let first: u32;
        core::arch::asm!("mrc p15,0,{value},c13,c0,2", value = out(reg) first, options(nostack));
        if (first as i32) < 0 { return 0; }
        let second: u32;
        core::arch::asm!("mrc p15,0,{value},c13,c0,2", value = out(reg) second, options(nostack));
        second & 0x7fffffff
    }
    unsafe fn timer() -> u32 { word(0xfff91004).wrapping_sub(word(0xfff91000)) }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_configuration_byte(offset: u32) -> u32 { u32::from(byte(word(0x96ca50) + offset)) }

    unsafe fn trace(header: u32, entry: u32) {
        if xm1_power_configuration_byte(0xf7) != 2 || word(0x89edd4) == 0 || word(0x409c1e1c) == 1 { return; }
        let mut payload = [0u8; 52];
        let mut length = if word(0x409c1e24) == 0 {
            for offset in 0..36 { payload[offset] = byte(header + offset as u32); }
            for offset in 0..16 { payload[36 + offset] = byte(entry + offset as u32); }
            52u32
        } else {
            for offset in 0..16 { payload[offset] = byte(entry + offset as u32); }
            16u32
        };
        loop {
            let cursor = word(0x409c1e10);
            let limit = word(0x409c1e14);
            let mut wrap = 0;
            let mut tail = 0;
            if word(0x409c1e18) == 0 {
                let end = length.wrapping_add(cursor);
                if end > 0x2000 {
                    wrap = end - 0x2000;
                    if limit == 0x2000 {
                        if cursor != 0x2000 { put(0x409c1e1c, 1); return; }
                    } else {
                        if limit < wrap { put(0x409c1e1c, 1); return; }
                        tail = 0x2000 - cursor;
                    }
                }
            } else if limit < length.wrapping_add(cursor) { put(0x409c1e1c, 1); return; }
            let buffer = word(0x409c1e2c);
            let source = payload.as_ptr() as u32;
            if tail != 0 {
                copy(buffer + cursor, source, tail);
                put(0x409c1e10, 0);
                copy(buffer, source + tail, wrap);
                put(0x409c1e10, wrap);
                put(0x409c1e18, 1);
            } else if wrap != 0 {
                put(0x409c1e10, 0);
                copy(buffer, source, wrap);
                put(0x409c1e10, wrap);
                put(0x409c1e18, 1);
            } else {
                copy(buffer + cursor, source, length);
                put(0x409c1e10, cursor.wrapping_add(length));
            }
            match word(0x409c1e24) {
                0 => { put(0x409c1e24, 1); put(0x409c1e28, 0x34); }
                1 => {
                    let total = word(0x409c1e28).wrapping_add(0x10);
                    put(0x409c1e28, total);
                    if total > 0x1ffff { put(0x409c1e24, 2); payload[..4].copy_from_slice(&0xaaaaaaaau32.to_le_bytes()); length = 4; }
                }
                _ => { put(0x409c1e24, 0); put(0x409c1e28, 0); }
            }
            if word(0x409c1e24) != 2 { return; }
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_owned_log_trace(header: u32, entry: u32) { trace(header, entry); }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_install_log_trace() { put(0x4004f15c, xm1_power_owned_log_trace as *const () as u32); }
    unsafe fn publish() {
        let hook = word(0x4004f15c);
        if hook != 0 {
            if hook != 0x6470fc && hook != xm1_power_owned_log_trace as *const () as u32 { unowned_callback(hook); }
            trace(0x400a0800, 0x400a0834 + u32::from(half(0x400a081e)) * 16);
        }
        let next = half(0x400a081e).wrapping_add(1);
        put_half(0x400a081e, next);
        if half(0x400a081c) <= next { put_half(0x400a081e, 0); }
        let next = 0x400a0834 + u32::from(half(0x400a081e)) * 16;
        for offset in [0, 4, 8, 12] { put(next + offset, 0); }
    }
    unsafe fn service_event(operation: u32, id: i32, status: i32) {
        if byte(0x4004f5e4 + operation) == 0 { return; }
        let saved = xm1_power_save_irq();
        let thread = thread();
        let index = u32::from(half(0x400a081e));
        let entry = 0x400a0834 + index * 16;
        if word(0x4004f25c) == 1 {
            put_byte(entry, 9);
            put_byte(entry + 1, operation as u8);
            put_half(entry + 2, id as u16);
            put(entry + 4, u32::from(status as u16) | thread << 16);
            put(entry + 8, word(0x400c0800));
            put(entry + 12, timer());
            publish();
        }
        if word(0x400a0824) == 0 {
            for offset in [0, 4, 8, 12] { put(0x400a0824 + offset, word(entry + offset)); }
        }
        xm1_power_restore_irq(saved);
    }
    unsafe fn kernel_command(category: u32, code: u32) {
        let saved = xm1_power_save_irq();
        if word(0x4004f25c) == 1 {
            let entry = 0x400a0834 + u32::from(half(0x400a081e)) * 16;
            put_byte(entry, 0x81);
            put_byte(entry + 1, thread() as u8);
            put_half(entry + 2, timer() as u16);
            put(entry + 4, word(0x400c0800));
            put(entry + 8, category);
            put(entry + 12, code);
            publish();
        }
        xm1_power_restore_irq(saved);
    }

    unsafe fn stream_error(code: u32) {
        let mut message = [0u8; 32];
        let template = b"errno_set(ERRLVL_DRIVER,0x####)";
        message[..template.len()].copy_from_slice(template);
        for (index, shift) in [12, 8, 4, 0].into_iter().enumerate() {
            message[26 + index] = b"0123456789ABCDEF"[((code >> shift) & 15) as usize];
        }
        let task = xm1_power_current_task() as u8;
        publish_text(task, &message);
    }
    unsafe fn publish_text(task: u8, message: &[u8; 32]) {
        let packet = 0x40a5b5e0;
        let buffer = word(packet + 12);
        if buffer == 0 { return; }
        let saved = xm1_power_interrupt_lock();
        let mut cursor = word(packet + 4);
        if (cursor.wrapping_add(0x2b) & !3) > 0x1ffc { put(buffer + cursor, 0xaaaaaaaa); cursor = 8; put(packet + 4, cursor); }
        put_half(buffer + cursor, 0xdeae);
        put_byte(buffer + cursor + 2, task);
        put_byte(buffer + cursor + 3, task);
        put(buffer + cursor + 4, 32);
        cursor += 8;
        put(packet + 4, cursor);
        for (offset, value) in message.iter().copied().enumerate() { put_byte(buffer + cursor + offset as u32, value); }
        cursor += 32;
        put(packet + 4, cursor);
        xm1_power_diagnostic_calendar_header(buffer as *mut u8);
        put(buffer + cursor, 0x55555555);
        xm1_power_interrupt_unlock(saved);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_append_stream_text(task: u32, text: u32) {
        if word(0x40a5b5ec) == 0 { return; }
        let mut message = [0u8; 32];
        for (offset, output) in message.iter_mut().enumerate() {
            let value = byte(text + offset as u32);
            if value == 0 { break; }
            *output = value;
        }
        publish_text(task as u8, &message);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32 {
        let state = match category { 1 => Some(0x40a5b660), 2 => Some(0x40a5b664), _ => None };
        if let Some(state) = state { if word(state) == 0 { put(state, code); } }
        if word(0x4089ea9c) == category && word(0x4089eaa0) == code { put(0x4089eaa4, 1); }
        kernel_command(category, code);
        if category == 2 {
            for address in (0x4089eaa8..=0x4089ebd8).step_by(4) { if word(address) == code { stream_error(code); break; } }
        }
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_diagnostic_report_status(status: i32) {
        let code = match status { -62 | -34 | -33 | -5 => 0x91, -50 => 0x90, -51 | -42 => 0x94, -18 => 0x93, -41 => 0x95, -25 => 0x92, -60 => 0x98, _ => 0xa0 };
        let _ = xm1_power_command_diagnostic(2, code);
    }
    unsafe fn dispatch_error(status: i32) {
        let handler = word(0x4089ea8c);
        if handler == 0x723330 { return; }
        if handler == 0x72835c || handler == xm1_power_diagnostic_report_status as *const () as u32 { xm1_power_diagnostic_report_status(status); return; }
        unowned_callback(handler);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_service_event(operation: u32, id: i32, status: i32) {
        service_event(operation, id, status);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_service_error(status: i32) {
        put(0x40a0f82c, status as u32);
        dispatch_error(status);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_service_error(operation: u32, id: i32, status: i32) {
        service_event(operation, id, status);
        xm1_power_record_service_error(status);
    }

    unsafe fn timed_pair(command: u32, first: u32, second: u32) {
        let saved = xm1_power_save_irq();
        if word(0x4004f25c) == 1 {
            let entry = 0x400a0834 + u32::from(half(0x400a081e)) * 16;
            put_byte(entry, 0x0f);
            put_byte(entry + 1, thread() as u8);
            put_half(entry + 2, timer() as u16);
            put(entry + 4, command);
            put(entry + 8, (first << 16) | u32::from(second as u16));
            put(entry + 12, word(0x400c0800));
            publish();
        }
        xm1_power_restore_irq(saved);
    }
    unsafe fn event_level(event: u16) -> u16 {
        if event == 0x1900 || event == 0x1600 { return 0; }
        let mut lower = 0u32;
        let mut upper = 0x6b4u32;
        while lower < upper {
            let middle = (lower + upper) >> 1;
            if half(0x81485c + middle * 4) < event { lower = middle + 1; } else { upper = middle; }
        }
        if half(0x81485c + lower * 4) == event { half(0x81485e + lower * 4) } else { 0 }
    }
    #[derive(Clone, Copy)]
    struct ServiceTraceHeader { address: u32, event: u16, command: u16, length: u32, payload: u32 }
    impl ServiceTraceHeader {
        unsafe fn read(address: u32) -> Self {
            Self { address, event: half(address), command: u16::from(byte(address + 2)) << 8 | u16::from(byte(address + 3)), length: word(address + 4), payload: word(address + 8) }
        }
    }
    unsafe fn append_service_event(address: u32) {
        let packet = 0x40a5b5e0;
        let buffer = word(packet + 12);
        if buffer == 0 { return; }
        if byte(packet + 1) != 0 && event_level(half(address) & 0x7fff) < u16::from(byte(packet + 2)) { return; }
        let header = ServiceTraceHeader::read(address);
        let (payload_bytes, header_bytes) = if header.event == 0 {
            timed_pair(u32::from(header.command), word(header.payload), word(header.payload + 4));
            (12, 4)
        } else if byte(packet) == 0 {
            timed_pair(u32::from(header.command) | u32::from(header.event) << 16, 0, 0);
            (0, 4)
        } else {
            timed_pair(u32::from(header.command) | u32::from(header.event) << 16, header.length, header.payload);
            (header.length, 8)
        };
        let saved = xm1_power_interrupt_lock();
        let mut cursor = word(packet + 4);
        let space = cursor.wrapping_add(3).wrapping_add(header_bytes).wrapping_add(payload_bytes) & !3;
        if space > 0x1ffc { put(buffer + cursor, 0xaaaaaaaa); cursor = 8; put(packet + 4, cursor); }
        copy(buffer + cursor, header.address, header_bytes);
        cursor += header_bytes;
        put(packet + 4, cursor);
        if payload_bytes != 0 && header.payload != 0 {
            copy(buffer + cursor, header.payload, payload_bytes);
            cursor += payload_bytes;
            put(packet + 4, cursor);
        }
        xm1_power_diagnostic_calendar_header(buffer as *mut u8);
        let remainder = payload_bytes & 3;
        if remainder != 0 {
            for offset in 0..4-remainder { put_byte(buffer + cursor + offset, 0); }
            cursor += 4-remainder;
            put(packet + 4, cursor);
        }
        put(buffer + cursor, 0x55555555);
        xm1_power_interrupt_unlock(saved);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_service_post_trace(header: u32) {
        let installed = word(0x4089ea94);
        if installed == 0x723330 { return; }
        if installed != 0x726478 && installed != xm1_power_owned_service_trace as *const () as u32 { unowned_callback(installed); }
        append_service_event(header);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_owned_service_trace(header: u32) { append_service_event(header); }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_install_service_trace() { put(0x4089ea94, xm1_power_owned_service_trace as *const () as u32); }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_install_status_handler() { put(0x4089ea8c, xm1_power_diagnostic_report_status as *const () as u32); }

    pub struct NativeQueueDiagnostics;
    impl QueueDiagnostics for NativeQueueDiagnostics {
        fn report(&mut self, operation: QueueDiagnostic, channel: u32, status: ServiceStatus) { unsafe { service_event(operation.operation(), channel as i32, status.0); } }
        fn dispatch_error(&mut self, status: ServiceStatus) { unsafe { dispatch_error(status.0); } }
    }
}
#[cfg(target_arch = "arm")]
pub use native::NativeQueueDiagnostics;
