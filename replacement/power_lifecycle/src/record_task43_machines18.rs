//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0ba44..f0bd4c.

unsafe extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f290a4();
    fn xm1_power_record_helper_f290b8(request: u32);
    fn xm1_power_record_helper_f29120();
    fn xm1_power_record_helper_f29160();
    fn xm1_power_record_helper_f29194() -> i32;
    fn xm1_power_record_helper_f29260();
    fn xm1_power_record_helper_f292a4();
    fn xm1_power_record_helper_f292c4();
    fn xm1_power_record_helper_f292f0();
    fn xm1_power_record_helper_f29304();
    fn xm1_power_record_helper_f29318();
    fn xm1_power_record_helper_f2a11c();
    fn xm1_power_record_helper_f2a12c();
    fn xm1_power_record_helper_f2a1cc();
    fn xm1_power_record_helper_f2a240() -> i32;
    fn xm1_power_record_helper_f2a468();
    fn xm1_power_record_helper_f2ab5c(event: u32) -> i32;
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 36) as *mut u16, value) };
}

unsafe fn choose_channel(request: u32) -> i32 {
    let result = if unsafe { xm1_power_record_helper_f29194() } == 0 {
        unsafe { state(4) };
        unsafe { xm1_power_record_task43_event(22, 0x106, request) }
    } else {
        unsafe { state(3); xm1_power_record_helper_f29260() };
        unsafe { xm1_power_record_task43_event(19, 0x105, request) }
    };
    unsafe { xm1_power_record_helper_f290a4() };
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine18(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { xm1_power_record_helper_f290b8(request); xm1_power_record_helper_f29160(); state(1); 2 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => { xm1_power_record_helper_f29304(); 2 }
            5 => { let result = xm1_power_record_task43_event(22, event, request); xm1_power_record_helper_f290a4(); result }
            6 => {
                let result = xm1_power_record_task43_event(22, event, request);
                if result == 1 { state(0); xm1_power_record_helper_f292c4(); }
                result
            }
            7 => { xm1_power_record_helper_f292f0(); 2 }
            8 => {
                if xm1_power_record_helper_f2a240() != 0 {
                    let result = xm1_power_record_task43_event(36, 0x104, request);
                    if result != 1 { state(2); return result; }
                }
                xm1_power_record_helper_f29120();
                xm1_power_record_helper_f2a468();
                let result = choose_channel(request);
                xm1_power_record_helper_f292a4();
                result
            }
            9 => xm1_power_record_task43_event(19, event, request),
            10 => { xm1_power_record_helper_f290b8(request); xm1_power_record_helper_f2a11c(); xm1_power_record_helper_f2a12c(); 2 }
            11 => { xm1_power_record_helper_f2a1cc(); 1 }
            12 => xm1_power_record_helper_f2ab5c(event),
            13 => {
                let mut result = xm1_power_record_task43_event(36, event, request);
                if result == 1 {
                    xm1_power_record_helper_f29120();
                    xm1_power_record_helper_f2a468();
                    result = choose_channel(request);
                }
                xm1_power_record_helper_f292a4();
                result
            }
            14 => {
                let mut result = xm1_power_record_task43_event(19, event, request);
                if result == 1 {
                    state(4);
                    result = xm1_power_record_task43_event(22, 0x106, request);
                    xm1_power_record_helper_f290a4();
                }
                result
            }
            15 => {
                let result = xm1_power_record_task43_event(22, event, request);
                if result == 1 {
                    state(0); xm1_power_record_helper_f29318(); xm1_power_record_helper_f292c4();
                } else if result == 0 {
                    state(0); xm1_power_record_helper_f29318(); return 1;
                }
                result
            }
            16 => 3,
            _ => -1,
        }
    }
}
