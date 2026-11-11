//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0eb20..f0edb8.

unsafe extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 54) as *mut u16, value) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine27(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { state(1); 2 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => xm1_power_record_task43_event(6, 0xf2, request),
            5 | 8 | 9 | 10 | 16 | 18 | 19 => 2,
            6 | 11 => { state(3); 2 }
            7 | 12 => { state(5); 2 }
            13 => { state(9); 2 }
            14 => { state(8); xm1_power_record_task43_event(6, 0xf2, request) }
            15 => { state(0); 1 }
            17 => xm1_power_record_task43_event(6, 0xf3, request),
            20 => 3,
            21 => { xm1_power_record_task43_event(6, event, request); state(9); 2 }
            22 => { xm1_power_record_task43_event(6, event, request); 2 }
            23 => {
                if xm1_power_record_task43_event(6, event, request) == 1 { state(0); 1 } else { 2 }
            }
            _ => -1,
        }
    }
}
