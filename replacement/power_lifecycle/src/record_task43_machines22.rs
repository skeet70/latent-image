//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0c0dc..f0c99c.

unsafe extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f2cb3c(request: u32);
    fn xm1_power_record_helper_f2cb5c() -> i32;
    fn xm1_power_record_helper_f2cb90() -> i32;
    fn xm1_power_record_helper_f2cbb4() -> i32;
    fn xm1_power_record_helper_f2cc00();
    fn xm1_power_record_helper_f2d000();
    fn xm1_power_record_helper_f2d094();
    fn xm1_power_record_helper_f2d0b8() -> i32;
    fn xm1_power_record_helper_f2d0e4();
    fn xm1_power_record_helper_f2d108();
    fn xm1_power_record_helper_f2d130();
    fn xm1_power_record_helper_f2d154() -> i32;
    fn xm1_power_record_helper_f2d180();
    fn xm1_power_record_helper_f2d1a4();
    fn xm1_power_record_helper_f2d1bc();
    fn xm1_power_record_helper_f2d338();
    fn xm1_power_record_helper_f2d358();
    fn xm1_power_record_helper_f2d440() -> i32;
    fn xm1_power_record_helper_f2d480();
    fn xm1_power_record_helper_f2d4d0();
    fn xm1_power_record_helper_f2d4e0();
    fn xm1_power_record_helper_f2d5d0() -> i32;
    fn xm1_power_record_helper_f2d624();
    fn xm1_power_record_helper_f2d67c();
    fn xm1_power_record_helper_f2d68c();
    fn xm1_power_record_helper_f2d6fc(value: u32);
    fn xm1_power_record_helper_f2d71c(value: u32);
    fn xm1_power_record_helper_f2d73c(value: u32);
    fn xm1_power_record_helper_f2d75c(value: u32);
    fn xm1_power_record_helper_f2d77c() -> i32;
    fn xm1_power_record_helper_f2d79c() -> u32;
    fn xm1_power_record_helper_f2d7b4();
    fn xm1_power_record_helper_f2d8b0();
    fn xm1_power_record_helper_f2dcf4() -> i32;
    fn xm1_power_record_helper_f2dd20();
    fn xm1_power_record_helper_f2ddd4() -> i32;
    fn xm1_power_record_helper_f2e30c();
    fn xm1_power_record_helper_f2e360();
    fn xm1_power_record_helper_f2e5b8();
    fn xm1_power_record_helper_f2e5d4(mode: u32);
    fn xm1_power_record_helper_f2e65c() -> i32;
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 44) as *mut u16, value) };
}

unsafe fn finish_pending(notify_secondary: bool) -> i32 {
    unsafe {
        if xm1_power_record_helper_f2ddd4() != 0 { state(7); return 2; }
        state(0);
        xm1_power_record_helper_f2d0e4();
        if notify_secondary { xm1_power_record_helper_f2d180(); }
        1
    }
}

unsafe fn finish_ready(publish: bool) -> i32 {
    unsafe {
        if xm1_power_record_helper_f2d79c() != 0 {
            if xm1_power_record_helper_f2d77c() == 0 { state(6); return 2; }
            if publish { xm1_power_record_helper_f2e30c(); }
        }
        finish_pending(true)
    }
}

unsafe fn invalid_selection() -> bool {
    unsafe {
        if xm1_power_record_helper_f2e65c() == 13 {
            xm1_power_record_helper_f2e5d4(13);
            true
        } else if xm1_power_record_helper_f2e65c() == 14 {
            xm1_power_record_helper_f2e5d4(14);
            true
        } else { false }
    }
}

unsafe fn schedule_primary() -> bool {
    unsafe {
        if xm1_power_record_helper_f2d5d0() == 0 { return false; }
        xm1_power_record_helper_f2d4e0();
        xm1_power_record_helper_f2d624();
        true
    }
}

unsafe fn schedule_secondary() -> bool {
    unsafe {
        if xm1_power_record_helper_f2d440() == 0 { return false; }
        xm1_power_record_helper_f2d480();
        true
    }
}

unsafe fn mapped_channel(event: u32, request: u32, next: u16, restart: bool) -> i32 {
    unsafe {
        let result = xm1_power_record_task43_event(16, event, request);
        if result != 1 { return result; }
        state(next);
        xm1_power_record_helper_f2e30c();
        xm1_power_record_helper_f2dd20();
        if restart { xm1_power_record_helper_f2d338(); }
        2
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine22(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                xm1_power_record_helper_f2cb3c(request);
                xm1_power_record_helper_f2d6fc(0);
                xm1_power_record_helper_f2d73c(0);
                xm1_power_record_helper_f2d71c(0);
                xm1_power_record_helper_f2d75c(0);
                xm1_power_record_helper_f2e360();
                xm1_power_record_helper_f2d358();
                if xm1_power_record_helper_f2cb5c() == 0 {
                    if xm1_power_record_helper_f2cb90() == 0 {
                        state(4);
                        if schedule_primary() { return 2; }
                        xm1_power_record_helper_f2d67c();
                        if !schedule_secondary() { xm1_power_record_helper_f2d4d0(); }
                        return 2;
                    }
                    state(3);
                } else {
                    state(if xm1_power_record_helper_f2cb90() == 0 { 2 } else { 1 });
                }
                xm1_power_record_helper_f2d8b0();
                if xm1_power_record_helper_f2d0b8() != 0 { xm1_power_record_helper_f2d108(); }
                if xm1_power_record_helper_f2d154() != 0 { xm1_power_record_helper_f2d1a4(); }
                2
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => { xm1_power_record_helper_f2d094(); 2 }
            5 => { xm1_power_record_helper_f2d108(); 2 }
            6 => 2,
            7 => { xm1_power_record_helper_f2d130(); 2 }
            8 => { xm1_power_record_helper_f2d1a4(); 2 }
            9 => { if xm1_power_record_helper_f2dcf4() != 0 { xm1_power_record_helper_f2cc00(); } 2 }
            10 | 12 => {
                if xm1_power_record_helper_f2cbb4() == 0 {
                    xm1_power_record_helper_f2d000(); xm1_power_record_helper_f2dd20(); 2
                } else {
                    mapped_channel(event, request, if handler_index == 10 { 2 } else { 4 }, handler_index == 12)
                }
            }
            11 => { xm1_power_record_helper_f2d000(); xm1_power_record_helper_f2dd20(); 2 }
            13 => { xm1_power_record_helper_f2d1bc(); 2 }
            14 => { xm1_power_record_helper_f2d1bc(); xm1_power_record_helper_f2e30c(); xm1_power_record_helper_f2dd20(); 2 }
            15 => { xm1_power_record_helper_f2d1bc(); xm1_power_record_helper_f2dd20(); xm1_power_record_helper_f2d338(); 2 }
            16 => { xm1_power_record_helper_f2e5b8(); 2 }
            17 => { xm1_power_record_helper_f2e5b8(); xm1_power_record_helper_f2e30c(); 2 }
            18 => { xm1_power_record_helper_f2e5b8(); xm1_power_record_helper_f2e30c(); finish_pending(false) }
            19 => if invalid_selection() { 0 } else { 2 },
            20 => {
                xm1_power_record_helper_f2d68c();
                if invalid_selection() { return 0; }
                if !schedule_primary() && !schedule_secondary() { state(5); }
                2
            }
            21 => {
                if invalid_selection() { return 0; }
                if schedule_primary() || schedule_secondary() { 2 } else { finish_ready(false) }
            }
            22 => finish_ready(false),
            23 => { if !schedule_secondary() { state(5); } 2 }
            24 => if schedule_secondary() { 2 } else { finish_ready(false) },
            25 | 26 | 27 => {
                xm1_power_record_helper_f2d71c(1);
                xm1_power_record_helper_f2d7b4();
                if handler_index >= 26 { xm1_power_record_helper_f2dd20(); }
                if handler_index == 27 { xm1_power_record_helper_f2d0e4(); xm1_power_record_helper_f2d180(); 1 } else { 2 }
            }
            28 => 3,
            29 => mapped_channel(event, request, 2, false),
            30 => xm1_power_record_task43_event(16, event, request),
            31 => mapped_channel(event, request, 4, true),
            32 => {
                let result = xm1_power_record_task43_event(16, event, request);
                if result != 1 { return result; }
                xm1_power_record_helper_f2dd20();
                xm1_power_record_helper_f2d338();
                finish_ready(true)
            }
            _ => -1,
        }
    }
}
