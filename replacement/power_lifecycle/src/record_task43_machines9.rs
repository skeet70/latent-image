use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_f39b8c();
    fn xm1_power_record_helper_f39f48();
    fn xm1_power_record_helper_f39fd0();
    fn xm1_power_record_helper_f3a050();
    fn xm1_power_record_helper_f3a090();
    fn xm1_power_record_helper_f3a0c4();
    fn xm1_power_record_helper_f3a160(failed: u32);
    fn xm1_power_record_helper_f3a1b8(failed: u32);
    fn xm1_power_record_helper_f3a210();
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_task43_enabled(command: u32, enabled: u32);
}

unsafe fn load(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn state(machine: u32, value: u16) {
    unsafe { write_volatile((0x169d4b4 + machine * 2) as *mut u16, value); }
}
unsafe fn clear_selection() -> i32 {
    if unsafe { load(0x16c06f0) } != 0 || unsafe { load(0x16c06f4) } != 0 { return 2; }
    unsafe { state(9, 0); }
    1
}

// The pinned f39c5c helper returns zero; f3a2ac..f3a3e0 have no effects and their queries return zero.
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine9(handler_index: u32, _: u32, _: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { xm1_power_record_helper_f39b8c(); 1 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => 1,
            5 => 2,
            6 => { xm1_power_record_helper_f3a090(); 2 }
            7 | 8 | 9 => 12,
            10 => { xm1_power_record_helper_f3a210(); xm1_power_record_helper_f39f48(); 2 }
            11 => { xm1_power_record_task43_enabled(21, 0); 1 }
            12 => { xm1_power_record_helper_f3a050(); xm1_power_record_task43_enabled(20, 0); 2 }
            13 => { xm1_power_record_task43_enabled(21, 1); 1 }
            14 => { xm1_power_record_helper_f3a090(); xm1_power_record_task43_enabled(20, 1); 2 }
            15 => { write_volatile(0x16c06f0 as *mut u32, 0); xm1_power_record_helper_f3a0c4(); 12 }
            16 => { write_volatile(0x16c06f0 as *mut u32, 0); clear_selection() }
            17 | 18 | 21 | 22 => {
                write_volatile(0x16c06f4 as *mut u32, 0);
                write_volatile(0x16c06f8 as *mut u32, load(0x169d380));
                state(9, if handler_index == 17 || handler_index == 21 { 1 } else { 2 });
                let failed = u32::from(load(0x16c06f8) != 0);
                if handler_index < 21 { xm1_power_record_helper_f3a160(failed); }
                else { xm1_power_record_helper_f3a1b8(failed); }
                1
            }
            19 => {
                write_volatile(0x16c06f4 as *mut u32, 0);
                write_volatile(0x16c06f8 as *mut u32, load(0x169d380));
                clear_selection()
            }
            20 => { xm1_power_record_helper_f3a210(); xm1_power_record_helper_f39fd0(); 2 }
            23 => 3,
            _ => 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine10(handler_index: u32, _: u32, _: u32) -> i32 {
    unsafe {
        match handler_index {
            2 | 4 | 5 => 1,
            3 => { xm1_power_record_helper_f46c20(); 0 }
            6 => 2,
            7 | 10 => 12,
            8 => {
                if load(0x16c0704) != 1 || load(0x16c0708) == 3 { return 12; }
                state(10, match load(0x16c0708) { 0 => 2, 1 => 3, _ => 4 });
                2
            }
            9 => {
                if load(0x16c0704) == 1 && load(0x16c0708) == 3 { state(10, 1); }
                12
            }
            11 => 3,
            _ => 0,
        }
    }
}
