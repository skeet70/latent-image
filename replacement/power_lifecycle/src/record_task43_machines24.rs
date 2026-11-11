//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0dd24..f0def0.

unsafe extern "C" {
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f339f8();
    fn xm1_power_record_helper_f33a18();
    fn xm1_power_record_helper_f33ef4();
    fn xm1_power_record_helper_f34710();
    fn xm1_power_record_helper_f34c70();
    fn xm1_power_record_helper_f34c98();
    fn xm1_power_record_helper_f3515c();
    fn xm1_power_record_helper_f3557c();
    fn xm1_power_record_helper_f3579c();
    fn xm1_power_record_helper_f35d14() -> i32;
    fn xm1_power_record_helper_f363b8() -> i32;
    fn xm1_power_record_helper_f363d8(request: u32);
    fn xm1_power_record_helper_f36658();
    fn xm1_power_record_helper_f36eac();
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 48) as *mut u16, value) };
}

unsafe fn prepare_stage() {
    unsafe {
        match xm1_power_record_helper_f363b8() {
            0 => xm1_power_record_helper_f33ef4(),
            1 => xm1_power_record_helper_f33a18(),
            _ => xm1_power_record_helper_f34710(),
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_prepare24(_event: u32, request: u32) {
    unsafe { xm1_power_record_helper_f363d8(request) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine24(handler_index: u32, _event: u32, _request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { xm1_power_record_helper_f339f8(); prepare_stage(); 1 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => { xm1_power_record_helper_f339f8(); state(1); prepare_stage(); xm1_power_record_helper_f34c70(); 1 }
            5 => 1,
            6 => { xm1_power_record_helper_f34c98(); 1 }
            7 => 2,
            8 => 12,
            9 => { xm1_power_record_helper_f3579c(); 2 }
            10 => { xm1_power_record_helper_f36eac(); 2 }
            11 => { xm1_power_record_helper_f3515c(); 12 }
            12 => { xm1_power_record_helper_f3515c(); xm1_power_record_helper_f34c98(); 1 }
            13 => {
                state(1);
                xm1_power_record_helper_f3557c();
                xm1_power_record_helper_f36658();
                if xm1_power_record_helper_f35d14() == 1 { 6 } else { 12 }
            }
            14 => {
                xm1_power_record_helper_f3557c();
                xm1_power_record_helper_f36658();
                xm1_power_record_helper_f35d14();
                xm1_power_record_helper_f34c98();
                1
            }
            15 => 3,
            _ => -1,
        }
    }
}
