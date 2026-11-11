// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// NONMATCHING branches and omitted slots are confirmed at f03ff0..f042e8.
extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f46c4c(status: u32);
    fn xm1_power_record_helper_f156e8(request: u32);
    fn xm1_power_record_helper_f16ffc(value: u32);
    fn xm1_power_record_helper_f16640(value: u32);
    fn xm1_power_record_helper_f16664() -> i32;
    fn xm1_power_record_helper_f1667c();
    fn xm1_power_record_helper_f17a84();
    fn xm1_power_record_helper_f15708() -> i32;
    fn xm1_power_record_helper_f165fc() -> i32;
    fn xm1_power_record_helper_f1661c() -> i32;
    fn xm1_power_record_helper_f156d4();
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile(0x0169d4be as *mut u16, value) };
}

unsafe fn finish(result: i32) -> i32 {
    if result == 1 {
        unsafe { state(0); xm1_power_record_helper_f17a84(); }
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine5(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                xm1_power_record_helper_f156e8(request);
                xm1_power_record_helper_f16ffc(0);
                xm1_power_record_helper_f16640(0);
                if xm1_power_record_helper_f16664() == 0 {
                    state(5);
                    xm1_power_record_helper_f1667c();
                    return finish(xm1_power_record_task43_event(20, 0xf1, request));
                }
                if xm1_power_record_helper_f15708() == 0 {
                    xm1_power_record_helper_f17a84();
                    return 1;
                }
                if xm1_power_record_helper_f165fc() != 0 && xm1_power_record_helper_f1661c() != 0 {
                    state(1);
                    xm1_power_record_helper_f16640(1);
                } else { state(3); }
                let result = xm1_power_record_task43_event(7, 0xed, request);
                xm1_power_record_helper_f156d4();
                result
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => {
                let result = xm1_power_record_task43_event(20, event, request);
                xm1_power_record_helper_f156d4();
                result
            }
            5 => { xm1_power_record_helper_f46c4c(0); 2 }
            6 => 3,
            7 | 9 => {
                let result = xm1_power_record_task43_event(7, event, request);
                if result != 1 { return result; }
                state(if handler_index == 7 { 2 } else { 4 });
                let result = xm1_power_record_task43_event(21, 0xef, request);
                xm1_power_record_helper_f156d4();
                result
            }
            8 => {
                let result = xm1_power_record_task43_event(21, event, request);
                if result != 1 { return result; }
                state(3);
                xm1_power_record_helper_f16640(0);
                let result = xm1_power_record_task43_event(7, 0xed, request);
                xm1_power_record_helper_f156d4();
                result
            }
            10 => {
                let result = xm1_power_record_task43_event(21, event, request);
                if result != 1 { return result; }
                state(5);
                finish(xm1_power_record_task43_event(20, 0xf1, request))
            }
            11 => finish(xm1_power_record_task43_event(20, event, request)),
            _ => 0,
        }
    }
}
