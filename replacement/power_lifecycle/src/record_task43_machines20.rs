//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0bf38..f0c06c.

unsafe extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f201dc();
    fn xm1_power_record_helper_f201f8(request: u32);
    fn xm1_power_record_helper_f20300() -> i32;
    fn xm1_power_record_helper_f2032c() -> i32;
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 40) as *mut u16, value) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine20(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                xm1_power_record_helper_f201dc();
                xm1_power_record_helper_f201f8(request);
                let (machine, pending) = if xm1_power_record_helper_f20300() != 0 { (27, 1) } else { (28, 2) };
                let result = xm1_power_record_task43_event(machine, event, request);
                if result != 1 { state(pending); }
                result
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => xm1_power_record_task43_event(28, event, request),
            5 => 3,
            6 => {
                let result = xm1_power_record_task43_event(27, event, request);
                if result != 1 { return result; }
                if xm1_power_record_helper_f2032c() == 0 {
                    state(0);
                    return 1;
                }
                let result = xm1_power_record_task43_event(28, 0xf1, request);
                state(if result == 1 { 0 } else { 2 });
                result
            }
            7 => {
                let result = xm1_power_record_task43_event(28, event, request);
                if result == 1 { state(0); }
                result
            }
            _ => -1,
        }
    }
}
