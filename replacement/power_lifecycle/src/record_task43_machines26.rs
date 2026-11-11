//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0e0f4..f0eb20.

unsafe extern "C" {
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f3aa94(event: u32);
    fn xm1_power_record_helper_f3aab4(value: u32) -> i32;
    fn xm1_power_record_helper_f3adbc(query: u32) -> i32;
    fn xm1_power_record_helper_f3ae2c();
    fn xm1_power_record_helper_f3ae54() -> i32;
    fn xm1_power_record_helper_f3aec0() -> i32;
    fn xm1_power_record_helper_f3aedc() -> i32;
    fn xm1_power_record_helper_f3af14(value: u32);
    fn xm1_power_record_helper_f3af90(value: u32);
    fn xm1_power_record_helper_f3b058();
    fn xm1_power_record_helper_f3b0fc();
    fn xm1_power_record_helper_f3b110();
    fn xm1_power_record_helper_f3b150();
    fn xm1_power_record_helper_f3b168();
    fn xm1_power_record_helper_f3b178();
    fn xm1_power_record_helper_f3b190(value: u32);
    fn xm1_power_record_helper_f3b318(event: u32);
    fn xm1_power_record_helper_f3b370(event: u32);
    fn xm1_power_record_helper_f3b3c8();
    fn xm1_power_record_helper_f3b3f4();
    fn xm1_power_record_helper_f3b420();
    fn xm1_power_record_helper_f3b448();
    fn xm1_power_record_helper_f3b460();
    fn xm1_power_record_helper_f3b5b0();
    fn xm1_power_record_helper_f3b69c();
    fn xm1_power_record_helper_f3b700();
    fn xm1_power_record_helper_f3b774();
    fn xm1_power_record_helper_f3b930();
    fn xm1_power_record_helper_f3b95c();
    fn xm1_power_record_helper_f3bb10(enabled: u32);
    fn xm1_power_record_helper_f3bb44();
    fn xm1_power_record_helper_f3bbac();
    fn xm1_power_record_helper_f3bccc();
    fn xm1_power_record_helper_f3bd8c();
    fn xm1_power_record_helper_f3bed8();
    fn xm1_power_record_helper_f3bf04(request: u32);
    fn xm1_power_record_helper_f3bf44();
    fn xm1_power_record_helper_f3bfa4();
    fn xm1_power_record_helper_f3bfe0();
    fn xm1_power_record_helper_f3c030();
    fn xm1_power_record_helper_f3c08c(status: u32);
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 52) as *mut u16, value) };
}

unsafe fn descriptor(offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((0x016c_0714usize + offset) as *const u32) }
}

unsafe fn submit(status: i32) -> i32 {
    unsafe {
        state(3);
        if status == 0 { xm1_power_record_helper_f3bd8c(); }
        else { xm1_power_record_helper_f3bccc(); }
    }
    2
}

unsafe fn dispatch_query(result: i32) -> Option<i32> {
    unsafe {
        match result {
            1 => { state(9); xm1_power_record_helper_f3b930(); Some(2) }
            2 => { state(9); xm1_power_record_helper_f3b69c(); Some(2) }
            4 => { state(9); xm1_power_record_helper_f3bb44(); Some(2) }
            _ => None,
        }
    }
}

unsafe fn finish_resource(descriptor_mode: bool) -> i32 {
    unsafe {
        if xm1_power_record_helper_f3aec0() == 0 { state(4); }
        else { state(if descriptor_mode && descriptor(12) == 1 { 6 } else { 5 }); }
        xm1_power_record_helper_f3b190(0);
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_prepare26(_event: u32, request: u32) {
    unsafe { xm1_power_record_helper_f3bf04(request) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine26(handler_index: u32, event: u32, _request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 | 4 => {
                xm1_power_record_helper_f3aa94(event);
                let enabled = xm1_power_record_helper_f3ae54();
                let status = xm1_power_record_helper_f3aec0();
                if enabled != 1 { return 1; }
                if handler_index == 2 && core::ptr::read_volatile(0x016b_e00cusize as *const u32) != 1 { return 1; }
                submit(status)
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            5 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3bbac(); 2 }
            6 => 4,
            7 => 1,
            8 | 9 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3af14(if handler_index == 8 { 1 } else { 2 }); 2 }
            10 => { xm1_power_record_helper_f3b370(event); 1 }
            11 | 18 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3af90(if handler_index == 11 { 1 } else { 0 }); xm1_power_record_helper_f3b318(event); 2 }
            12 => { xm1_power_record_helper_f3b058(); 2 }
            13 => 2,
            14 => { xm1_power_record_helper_f3b0fc(); 2 }
            15 => { xm1_power_record_helper_f3b110(); 2 }
            16 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3bfa4(); 2 }
            17 | 19 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b190(0); if handler_index == 17 { 1 } else { 11 } }
            20 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b5b0(); 2 }
            21 => { xm1_power_record_helper_f3b150(); xm1_power_record_helper_f3b318(event); 2 }
            22 => { xm1_power_record_helper_f3b168(); xm1_power_record_helper_f3b318(event); 2 }
            23 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3bed8(); 2 }
            24 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b178(); 2 }
            25 => { xm1_power_record_helper_f3b95c(); 1 }
            26 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b774(); 2 }
            27 => {
                state(9); xm1_power_record_helper_f3aa94(event);
                if xm1_power_record_helper_f3ae54() == 1 { xm1_power_record_helper_f3b774(); }
                else { xm1_power_record_helper_f3af14(2); }
                2
            }
            28 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b700(); 2 }
            29 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b69c(); 2 }
            30 => {
                if xm1_power_record_helper_f3aedc() == 1 {
                    state(3); xm1_power_record_helper_f3aa94(event);
                    let status = xm1_power_record_helper_f3aec0();
                    if status == 0 { xm1_power_record_helper_f3bd8c(); } else { xm1_power_record_helper_f3bccc(); }
                    2
                } else { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3b190(0); 1 }
            }
            31 | 32 => {
                xm1_power_record_helper_f3aa94(event);
                if xm1_power_record_helper_f3aab4(descriptor(0)) != 4 { return 1; }
                state(if handler_index == 31 { 1 } else { 9 }); xm1_power_record_helper_f3bb44(); 2
            }
            33 => {
                xm1_power_record_helper_f3aa94(event);
                dispatch_query(xm1_power_record_helper_f3aab4(descriptor(0))).unwrap_or(1)
            }
            34 => { xm1_power_record_helper_f3aa94(event); xm1_power_record_helper_f3bf44(); 2 }
            35 => { xm1_power_record_helper_f3b190(0); 1 }
            36 => finish_resource(false),
            37 => { xm1_power_record_helper_f3b3c8(); 1 }
            38 | 39 | 40 | 41 => {
                let query = match handler_index { 38 => 2, 39 => 3, 40 => 1, _ => 4 };
                if let Some(result) = dispatch_query(xm1_power_record_helper_f3adbc(query)) { result }
                else { finish_resource(false) }
            }
            42 => { xm1_power_record_helper_f3ae2c(); xm1_power_record_helper_f3b3f4(); 11 }
            43 => { xm1_power_record_helper_f3b420(); 2 }
            44 => { xm1_power_record_helper_f3b448(); 2 }
            45 => { xm1_power_record_helper_f3b460(); 2 }
            46 => finish_resource(true),
            47 => { xm1_power_record_helper_f3bfe0(); 2 }
            48 => { xm1_power_record_helper_f3aec0(); xm1_power_record_helper_f3bfe0(); 2 }
            49 | 50 => { xm1_power_record_helper_f3bb10(if handler_index == 49 { 1 } else { 0 }); 2 }
            51 => { xm1_power_record_helper_f3c030(); 2 }
            52 => { xm1_power_record_helper_f3c08c(0); 2 }
            53 => {
                let status = if xm1_power_record_helper_f3aec0() == 0 { state(4); 0 } else { state(5); 1 };
                xm1_power_record_helper_f3c08c(status); 1
            }
            54 => 3,
            _ => -1,
        }
    }
}
