//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0c06c..f0c0dc.

unsafe extern "C" {
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f1d1c0(request: u32);
    fn xm1_power_record_helper_f1a190(request: u32);
    fn xm1_power_record_helper_f1e468();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine21(handler_index: u32, _event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { xm1_power_record_helper_f1d1c0(request); 2 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => { xm1_power_record_helper_f1a190(request); 2 }
            5 => { xm1_power_record_helper_f1e468(); 1 }
            6 => 3,
            _ => -1,
        }
    }
}
