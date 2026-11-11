#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationRequest(pub u32);
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationArgument(pub u32);

pub fn decimal_pair(value: u32) -> Option<[u8; 2]> {
    (value <= 99).then(|| [b'0' + (value / 10) as u8, b'0' + (value % 10) as u8])
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(include_str!("mode_events_residue.S"), options(raw));

#[cfg(target_arch = "arm")]
mod native {
    use super::{ApplicationArgument, ApplicationRequest, decimal_pair};
    use crate::shutdown::{post_application_record, KernelStatus, TaskId, UiEvents, UiRouting};
    use crate::shutdown_rtos::NativeShutdownRtos;
    use core::ptr::{read_volatile, write_volatile};

    unsafe extern "C" {
        fn xm1_power_current_task() -> u32;
        fn xm1_power_append_stream_text(task: u32, text: u32);
        fn xm1_power_owned_service_trace(header: u32);
        pub fn xm1_power_application_post(event: u32, argument: u32) -> i32;
    }
    unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
    unsafe fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
    unsafe fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
    unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

    pub unsafe fn post_application(event: ApplicationRequest, argument: ApplicationArgument) -> KernelStatus {
        KernelStatus(unsafe { xm1_power_application_post(event.0, argument.0) })
    }

    unsafe fn text(message: *const u8) {
        unsafe { xm1_power_append_stream_text(xm1_power_current_task() & 0xff, message as u32) };
    }

    #[repr(C)]
    struct TraceHeader { operation: u16, sender: u8, destination: u8, length: u32, payload: *const u8 }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_application_post_backed(
        event: u32,
        argument: u32,
        trace: *mut [u8; 76],
        application: *mut [u8; 76],
    ) -> i32 {
        unsafe { text(0x7818ec as *const u8) };
        let trace = unsafe { &mut *trace };
        trace[..4].copy_from_slice(&(event & !0xc0).to_le_bytes());
        trace[4..8].copy_from_slice(&argument.to_le_bytes());
        let header = TraceHeader {
            operation: 0,
            sender: unsafe { xm1_power_current_task() } as u8,
            destination: 1,
            length: 76,
            payload: trace.as_ptr(),
        };
        unsafe { xm1_power_owned_service_trace(&header as *const TraceHeader as u32) };
        let application = unsafe { &mut *application };
        application[..4].copy_from_slice(&event.to_le_bytes());
        application[4..8].copy_from_slice(&argument.to_le_bytes());
        let routing = if unsafe { word(0x9134f8) } == 1 { UiRouting::Primary } else { UiRouting::Other };
        let first_task = TaskId(unsafe { byte(0x954884) });
        let mut events = UiEvents::from_runtime(routing, first_task);
        let mut rtos = NativeShutdownRtos;
        let result = post_application_record(&mut rtos, &mut events, application);
        if event == 0x91b && first_task.0 == 0 {
            if let Some(task) = events.first_request_task() {
                unsafe { write_volatile(0x954884 as *mut u8, task.0) };
            }
        }
        let status = match result { Ok(()) => 0, Err(status) => status.0 };
        unsafe { put(0x954d68, status as u32) };
        if status != 0 { unsafe { xm1_power_frontend_failure() }; }
        status
    }

    unsafe fn append(destination: *mut u8, source: *const u8) {
        let mut end = destination;
        while unsafe { read_volatile(end) } != 0 { end = unsafe { end.add(1) }; }
        let mut source = source;
        loop {
            let value = unsafe { read_volatile(source) };
            unsafe { write_volatile(end, value) };
            if value == 0 { break; }
            end = unsafe { end.add(1) };
            source = unsafe { source.add(1) };
        }
    }
    unsafe fn append_decimal(destination: *mut u8, value: u32) {
        let Some(digits) = decimal_pair(value) else { return; };
        let mut end = destination;
        while unsafe { read_volatile(end) } != 0 { end = unsafe { end.add(1) }; }
        unsafe {
            write_volatile(end, digits[0]);
            write_volatile(end.add(1), digits[1]);
            write_volatile(end.add(2), 0);
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_frontend_failure() {
        let mut buffer = [0u8; 32];
        let status = unsafe { word(0x954d68) };
        if status != 0 {
            unsafe {
                append(buffer.as_mut_ptr(), 0x7818fc as *const u8);
                append_decimal(buffer.as_mut_ptr(), status.wrapping_neg());
                put(0x954d68, 0);
                text(buffer.as_ptr());
            }
        }
        buffer.fill(0);
        let machine = unsafe { half(0x95037e) };
        let name = unsafe { word(0x81c344 + usize::from(machine) * 4) };
        unsafe {
            append(buffer.as_mut_ptr(), name as *const u8);
            append(buffer.as_mut_ptr(), 0x781904 as *const u8);
            append_decimal(buffer.as_mut_ptr(), u32::from(half(0x950382)));
            append(buffer.as_mut_ptr(), 0x781908 as *const u8);
            append_decimal(buffer.as_mut_ptr(), u32::from(half(0x950380)));
            append(buffer.as_mut_ptr(), 0x78195c as *const u8);
            text(buffer.as_ptr());
            let _ = post_application(ApplicationRequest(0xa81), ApplicationArgument(0));
        }
    }
}

#[cfg(target_arch = "arm")]
pub use native::{post_application, xm1_power_frontend_failure};

