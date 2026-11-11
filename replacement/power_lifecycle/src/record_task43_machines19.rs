//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0bd4c..f0bf38.

unsafe extern "C" {
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f2afc8(request: u32);
    fn xm1_power_record_helper_f2afe8();
    fn xm1_power_record_helper_f2b014();
    fn xm1_power_record_helper_f2be34() -> i32;
    fn xm1_power_record_helper_f2be60() -> i32;
    fn xm1_power_record_helper_f2becc();
    fn xm1_power_record_helper_f2beec();
    fn xm1_power_record_helper_f2c05c();
    fn xm1_power_record_helper_f2c128();
    fn xm1_power_record_helper_f2c188();
    fn xm1_power_record_helper_f2c36c();
    fn xm1_power_record_helper_f2c3f8() -> i32;
    fn xm1_power_record_helper_f2c434();
    fn xm1_power_record_helper_f2c4bc();
    fn xm1_power_record_helper_f2c898();
    fn xm1_power_record_helper_f2c8ac() -> i32;
    fn xm1_power_record_helper_f2c8d4();
    fn xm1_power_record_helper_f2c908();
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 38) as *mut u16, value) };
}

unsafe fn finish_selection() -> i32 {
    unsafe {
        xm1_power_record_helper_f2c188();
        if xm1_power_record_helper_f2c8ac() != 0 {
            state(6);
            xm1_power_record_helper_f2c8d4();
            2
        } else {
            state(0);
            xm1_power_record_helper_f2c908();
            1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine19(handler_index: u32, _event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                xm1_power_record_helper_f2afc8(request);
                xm1_power_record_helper_f2c128();
                xm1_power_record_helper_f2afe8();
                xm1_power_record_helper_f2b014();
                2
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => {
                if xm1_power_record_helper_f2be60() != 1 {
                    state(5);
                } else if xm1_power_record_helper_f2be34() != 0 {
                    xm1_power_record_helper_f2becc();
                } else if xm1_power_record_helper_f2c3f8() != 0 {
                    state(4);
                    xm1_power_record_helper_f2c434();
                    xm1_power_record_helper_f2c898();
                } else {
                    state(2);
                    xm1_power_record_helper_f2beec();
                    xm1_power_record_helper_f2c898();
                }
                2
            }
            5 => 2,
            6 => { xm1_power_record_helper_f2c36c(); xm1_power_record_helper_f2c898(); xm1_power_record_helper_f2c188(); 1 }
            7 => finish_selection(),
            8 => { xm1_power_record_helper_f2c4bc(); 2 }
            9 => { xm1_power_record_helper_f2c4bc(); finish_selection() }
            10 => { xm1_power_record_helper_f2c05c(); 2 }
            11 => 1,
            12 => 3,
            _ => -1,
        }
    }
}
