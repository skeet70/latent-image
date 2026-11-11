//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0def0..f0e0f4.

unsafe extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f156e8(request: u32);
    fn xm1_power_record_helper_f16ffc(enabled: u32);
    fn xm1_power_record_helper_f3c0bc();
    fn xm1_power_record_helper_f3c0d0(request: u32);
    fn xm1_power_record_helper_f3c0f0();
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 50) as *mut u16, value) };
}

unsafe fn finish_primary(event: u32, request: u32, mark_pending: bool) -> i32 {
    unsafe {
        let result = xm1_power_record_task43_event(6, event, request);
        if result == 1 {
            state(0);
            xm1_power_record_helper_f3c0f0();
        } else if result == 2 && mark_pending {
            state(3);
        }
        result
    }
}

unsafe fn finish_secondary(event: u32, request: u32, mark_pending: bool) -> i32 {
    unsafe {
        let result = xm1_power_record_task43_event(21, event, request);
        if result == 1 {
            finish_primary(0xf6, request, true)
        } else {
            if result == 2 && mark_pending { state(2); }
            result
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_prepare25(_event: u32, request: u32) {
    unsafe { xm1_power_record_helper_f3c0d0(request) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine25(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                xm1_power_record_helper_f3c0bc();
                xm1_power_record_helper_f156e8(request);
                xm1_power_record_helper_f16ffc(1);
                let result = xm1_power_record_task43_event(7, 0xee, request);
                if result == 1 {
                    finish_secondary(0xf0, request, true)
                } else {
                    if result == 2 { state(1); }
                    result
                }
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => 3,
            5 => {
                let result = xm1_power_record_task43_event(7, event, request);
                if result == 1 { finish_secondary(0xf0, request, true) } else { result }
            }
            6 => finish_secondary(event, request, false),
            7 => finish_primary(event, request, false),
            _ => -1,
        }
    }
}
