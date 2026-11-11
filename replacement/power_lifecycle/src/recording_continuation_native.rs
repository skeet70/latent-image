#[cfg(target_arch = "arm")]
mod arm {
    use crate::recording_continuation::{self as cpu, AuxiliaryRequest, CallbackAddress, CaptureRequest, Controller, Field, FinalizeRequest, OutputKind, Runtime, StepRequest};
    use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
    use crate::shutdown_rtos::NativeShutdownRtos;
    use core::ptr::{read_volatile, write_volatile};

    const RECORDING: usize = 0x01526a78;
    const PREFIX: usize = 0x01545790;
    const RESULTS: usize = 0x01545850;
    const SAVED: usize = RECORDING + 0x3100;

    unsafe extern "C" {
        fn xm1_power_recording_step_request(request: *const u32, output: *mut u8) -> i32;
        fn xm1_power_recording_output_request(kind: u32, settings: u32) -> i32;
        fn xm1_power_recording_finalize_request(kind: u32) -> i32;
        fn xm1_power_recording_capture_request(kind: u32) -> i32;
        fn xm1_power_output_route_set_requested_level(slot: u32, level: u32, unused: u32) -> i32;
        fn xm1_power_persistence_shutdown_callback() -> i32;
        fn xm1_power_service_error(operation: u32, id: i32, status: i32);
        fn xm1_power_recording_auxiliary_request(kind: u32, first: u32, second: u32) -> i32;
        fn xm1_power_recording_auxiliary_finish() -> i32;
        fn xm1_power_recording_auxiliary_state() -> u32;
    }

    fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
    fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
    fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) }; }
    fn copy(destination: usize, source: usize, size: usize) {
        for index in 0..size {
            let value = unsafe { read_volatile((source + index) as *const u8) };
            unsafe { write_volatile((destination + index) as *mut u8, value) };
        }
    }
    fn clear(address: usize, size: usize) {
        for index in 0..size { unsafe { write_volatile((address + index) as *mut u8, 0) }; }
    }
    fn fail(status: i32) -> ! {
        unsafe { xm1_power_service_error(0x13e7, 3, status) };
        loop { core::hint::spin_loop(); }
    }
    fn post(operation: u16, destination: u8, payload: &[u8]) -> i32 {
        let rtos = &mut NativeShutdownRtos;
        let sender = rtos.current_task();
        match rtos.post(ServiceMessage { operation: RequestId(operation), sender, destination: QueueId(destination), payload }) {
            Ok(()) => 0,
            Err(status) => fail(status.0),
        }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_halt_error(status: i32) -> ! { fail(status) }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_report_missing_state() {
        if word(0x9134f8) != 1 { xm1_power_recording_post_notification(0x91b, half(RECORDING) as u32); }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_post_notification(operation: u32, value: u32) -> i32 {
        let operation = operation & 0xffff;
        let destination = if word(0x9134f8) == 1 && operation & 0x900 != 0 { 7 } else { 1 };
        if operation == 0x82e && destination == 1 {
            let count = word(0x9121bc);
            if count != word(0x9134c4) { return 0; }
            put(0x9121bc, count.wrapping_add(1));
        }
        let mut packet = [0; 76];
        packet[..4].copy_from_slice(&operation.to_le_bytes());
        packet[4..8].copy_from_slice(&value.to_le_bytes());
        let result = post(0, destination, &packet);
        if operation == 0x91b && unsafe { read_volatile(0x954884 as *const u8) } == 0 {
            unsafe { write_volatile(0x954884 as *mut u8, NativeShutdownRtos.current_task().0) };
        }
        result
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_repeat_current_notification() -> i32 {
        xm1_power_recording_post_notification(0x91b, half(RECORDING) as u32)
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_defer_current() -> i32 {
        let operation = half(RECORDING);
        if operation & 0x7f00 != 0x200 {
            return xm1_power_recording_post_notification(0x91b, operation as u32);
        }
        let offset = match operation {
            0x200 | 0x201 | 0x208 | 0x209 => Some(0x1c),
            0x202 | 0x20c | 0x20d | 0x20f | 0x215 => Some(0x10),
            0x203 | 0x204 | 0x20b => Some(0x18),
            0x205 | 0x214 => Some(0x0c),
            0x206 | 0x207 => Some(0x24),
            0x20e => Some(0x20),
            0x210 | 0x212 | 0x213 => Some(0x14),
            _ => None,
        };
        xm1_power_recording_post_notification(0x838, offset.map_or(0xffff, |offset| word(RECORDING + offset)))
    }

    /// # Safety
    /// `payload` covers `size` readable bytes until the queue copies the message.
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_recording_send_task_message(operation: u32, size: u32, payload: *const u8) -> i32 {
        let bytes = if size == 0 { &[] } else { unsafe { core::slice::from_raw_parts(payload, size as usize) } };
        post(operation as u16, 3, bytes)
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_service_stopped_callback(_status: u32) { post(0x13ea, 3, &[]); }

    struct NativeRuntime;
    impl Runtime for NativeRuntime {
        fn word(&self, field: Field) -> u32 { word(RECORDING + field as usize) }
        fn set_word(&mut self, field: Field, value: u32) { put(RECORDING + field as usize, value); }
        fn state(&self, controller: Controller) -> u16 { half(RESULTS + controller as usize * 2) }
        fn set_state(&mut self, controller: Controller, state: u16) { unsafe { write_volatile((RESULTS + controller as usize * 2) as *mut u16, state) }; }
        fn set_event(&mut self, controller: Controller, event: u16) { unsafe { write_volatile((PREFIX + controller as usize * 2) as *mut u16, event) }; }
        fn current_operation(&self) -> u16 { half(RECORDING) }
        fn defer_current(&mut self) { xm1_power_recording_defer_current(); }
        fn channel_enabled(&mut self, enabled: bool) { unsafe { xm1_power_output_route_set_requested_level(11, if enabled { 0 } else { 8 }, 0) }; }
        fn save_current_request(&mut self) {
            copy(SAVED, RECORDING, 12);
            copy(SAVED + 12, RECORDING + 12, word(RECORDING + 4) as usize);
        }
        fn clear_saved_request(&mut self) { clear(SAVED, 12 + 0x2c); }
        fn step_request(&mut self, request: StepRequest) -> i32 {
            let request = [match request { StepRequest::Start => 2, StepRequest::Poll => 3 }, 0];
            unsafe { xm1_power_recording_step_request(request.as_ptr(), core::ptr::null_mut()) }
        }
        fn capture_request(&mut self, request: CaptureRequest) -> i32 { unsafe { xm1_power_recording_capture_request(match request { CaptureRequest::Start => 4, CaptureRequest::Poll => 11 }) } }
        fn output_request(&mut self, kind: OutputKind) -> i32 { unsafe { xm1_power_recording_output_request(kind as u32, 0) } }
        fn finalize_request(&mut self, request: FinalizeRequest) -> i32 { unsafe { xm1_power_recording_finalize_request(match request { FinalizeRequest::Start => 2, FinalizeRequest::Poll => 25 }) } }
        fn auxiliary_request(&mut self, request: AuxiliaryRequest) -> i32 { unsafe { xm1_power_recording_auxiliary_request(match request { AuxiliaryRequest::StartFirst => 1, AuxiliaryRequest::StartSecond => 7 }, 0, 0) } }
        fn auxiliary_state(&mut self) -> u32 { unsafe { xm1_power_recording_auxiliary_state() } }
        fn finish_auxiliary(&mut self) -> i32 { unsafe { xm1_power_recording_auxiliary_finish() } }
        fn request_service_stop(&mut self) {
            let mut packet = [0; 24];
            packet[..4].copy_from_slice(&(xm1_power_recording_service_stopped_callback as *const () as u32).to_le_bytes());
            post(0x7302, 99, &packet);
        }
        fn callback(&mut self, callback: CallbackAddress) {
            if callback.0 != xm1_power_persistence_shutdown_callback as *const () as u32 { fail(-30); }
            unsafe { xm1_power_persistence_shutdown_callback() };
        }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_continuation_publish() {
        if cpu::publish_status4(&mut NativeRuntime).is_err() { fail(-30); }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_recording_continuation_event(request: u32) -> i32 {
        NativeRuntime.set_word(Field::Request, request);
        match cpu::continue_status(&mut NativeRuntime) { Ok(status) => status, Err(_) => fail(-30) }
    }
}
