// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// Branches and halfword state stores are confirmed at f03584..f03ec0.
extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f1528c();
    fn xm1_power_record_helper_f15528();
    fn xm1_power_record_helper_f154fc(request: u32);
    fn xm1_power_record_helper_f1561c(request: u32);
    fn xm1_power_record_helper_f155a8();
    fn xm1_power_record_helper_f152b4();
    fn xm1_power_record_helper_f15350();
    fn xm1_power_record_helper_f47330(request: u32) -> i32;
    fn xm1_power_record_helper_f15594() -> i32;
    fn xm1_power_record_helper_f15604(request: u32) -> i32;
    fn xm1_power_record_helper_f15378();
    fn xm1_power_record_helper_f153c4();
    fn xm1_power_record_helper_f153ec();
    fn xm1_power_record_helper_f154d4();
    fn xm1_power_record_helper_f15300(request: u32);
    fn xm1_power_record_helper_f152dc() -> i32;
    fn xm1_power_record_helper_f153a0() -> i32;
    fn xm1_power_record_helper_f15414() -> i32;
    fn xm1_power_record_helper_f15438() -> i32;
    fn xm1_power_record_helper_f15460();
    fn xm1_power_record_helper_f15538() -> i32;
    fn xm1_power_record_helper_f15488();
    fn xm1_power_record_helper_f15278();
    fn xm1_power_record_helper_f154e8();
    fn xm1_power_record_helper_f15630();
    fn xm1_power_record_helper_f155dc() -> i32;
    fn xm1_power_record_helper_f154b0() -> i32;
    fn xm1_power_record_helper_f15678() -> i32;
    fn xm1_power_record_helper_f15654();
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile(0x0169d4ba as *mut u16, value) };
}

unsafe fn primary(request: u32, mark: bool) -> i32 {
    unsafe {
        state(1);
        if mark { xm1_power_record_helper_f152b4(); }
        xm1_power_record_helper_f155a8();
        let result = xm1_power_record_task43_event(5, 0xeb, request);
        result
    }
}

unsafe fn secondary(request: u32) -> i32 {
    unsafe {
        state(4);
        xm1_power_record_helper_f15378();
        let result = xm1_power_record_task43_event(18, 0x103, request);
        result
    }
}

unsafe fn tertiary(request: u32) -> i32 {
    unsafe {
        state(5);
        xm1_power_record_helper_f153ec();
        let result = xm1_power_record_task43_event(4, 0x107, request);
        result
    }
}

unsafe fn finish(clear: bool) -> i32 {
    unsafe {
        state(0);
        if clear { xm1_power_record_helper_f15488(); }
        xm1_power_record_helper_f15278();
        xm1_power_record_helper_f154e8();
    }
    1
}

unsafe fn pending(request: u32) -> i32 {
    unsafe {
        state(3);
        let result = xm1_power_record_task43_event(17, 0xf7, request);
        result
    }
}

unsafe fn advance(request: u32, initial: bool) -> i32 {
    unsafe {
        if xm1_power_record_helper_f152dc() != 0 { return primary(request, true); }
        if initial && xm1_power_record_helper_f155dc() != 0 { state(2); return 2; }
        if xm1_power_record_helper_f153a0() != 0 { return secondary(request); }
        if xm1_power_record_helper_f15414() != 0 { return tertiary(request); }
        if xm1_power_record_helper_f154b0() != 0 { return finish(true); }
        if initial { pending(request) } else { 2 }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine3(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => 1,
            3 | 22 => 2,
            4 | 5 => {
                xm1_power_record_helper_f1528c();
                xm1_power_record_helper_f15528();
                xm1_power_record_helper_f154fc(request);
                if handler_index == 4 { xm1_power_record_helper_f1561c(request); }
                2
            }
            6 => primary(request, false),
            7 => {
                xm1_power_record_helper_f1528c();
                let result = xm1_power_record_task43_event(17, 0xf9, request);
                if result == 1 { primary(request, true) } else { result }
            }
            8 => { xm1_power_record_helper_f46c20(); 0 }
            9 => { xm1_power_record_helper_f15350(); 2 }
            10 => {
                xm1_power_record_helper_f15350();
                let next = if xm1_power_record_helper_f47330(request) != 0 { 0xfc }
                    else if xm1_power_record_helper_f15594() != 0 || xm1_power_record_helper_f15604(request) != 0 { 0xf9 }
                    else { 0xf8 };
                let result = xm1_power_record_task43_event(17, next, request);
                if result == 1 { secondary(request) } else { result }
            }
            11 => { xm1_power_record_helper_f153c4(); 2 }
            12 => {
                xm1_power_record_helper_f153c4();
                let result = xm1_power_record_task43_event(17, 0xf9, request);
                if result == 1 { tertiary(request) } else { result }
            }
            13 => {
                xm1_power_record_helper_f154d4();
                xm1_power_record_helper_f15300(request);
                if xm1_power_record_helper_f152dc() != 0 {
                    state(1);
                    xm1_power_record_helper_f152b4();
                    xm1_power_record_helper_f155a8();
                    let mut result = xm1_power_record_task43_event(5, 0xeb, request);
                    if result == 1 {
                        if xm1_power_record_helper_f153a0() != 0 {
                            state(4);
                            xm1_power_record_helper_f15378();
                            result = xm1_power_record_task43_event(18, 0x103, request);
                        } else {
                            state(3);
                            result = xm1_power_record_task43_event(17, 0xf7, request);
                        }
                    }
                    result
                } else if xm1_power_record_helper_f153a0() != 0 { secondary(request) }
                else if xm1_power_record_helper_f15414() != 0 { tertiary(request) }
                else { 2 }
            }
            14 | 15 => {
                if handler_index == 14 && xm1_power_record_helper_f15438() == 0 {
                    xm1_power_record_helper_f46c20();
                    return 0;
                }
                xm1_power_record_helper_f15460();
                if xm1_power_record_helper_f15538() != 0 {
                    let result = xm1_power_record_task43_event(5, 0xec, request);
                    result
                } else { 2 }
            }
            16 => { xm1_power_record_helper_f15460(); 2 }
            17 => {
                xm1_power_record_helper_f15460();
                let result = xm1_power_record_task43_event(17, 0xf9, request);
                if result == 1 { finish(true) } else { result }
            }
            18 | 27 => {
                let result = xm1_power_record_task43_event(if handler_index == 18 { 17 } else { 5 }, event, request);
                result
            }
            19 => xm1_power_record_task43_event(4, event, request),
            20 | 25 => xm1_power_record_task43_event(18, event, request),
            21 => {
                if xm1_power_record_task43_event(17, 0xfa, request) == 1 {
                    state(8);
                    xm1_power_record_task43_event(35, event, request);
                } else { state(7); }
                2
            }
            23 => {
                if xm1_power_record_task43_event(35, event, request) == 1 {
                    state(if xm1_power_record_task43_event(17, 0xfb, request) == 1 { 3 } else { 9 });
                }
                2
            }
            24 => { xm1_power_record_helper_f15630(); 2 }
            26 | 28 => {
                let result = xm1_power_record_task43_event(if handler_index == 26 { 5 } else { 17 }, event, request);
                if result == 1 { advance(request, handler_index == 26) } else { result }
            }
            29 | 30 => {
                let result = xm1_power_record_task43_event(if handler_index == 29 { 18 } else { 4 }, event, request);
                if result == 1 { finish(false) } else { result }
            }
            31 => {
                let result = xm1_power_record_task43_event(18, event, request);
                if result == 1 { state(0); }
                result
            }
            32 => {
                if xm1_power_record_task43_event(17, event, request) == 1 {
                    if xm1_power_record_helper_f15678() == 1 {
                        xm1_power_record_helper_f15654();
                        xm1_power_record_task43_event(35, 0x10, request);
                        state(if xm1_power_record_task43_event(17, 0xfb, request) == 1 { 3 } else { 9 });
                    } else {
                        state(8);
                        xm1_power_record_task43_event(35, 0xa7, request);
                    }
                }
                2
            }
            33 => { xm1_power_record_task43_event(17, event, request); 2 }
            34 => {
                if xm1_power_record_task43_event(17, event, request) == 1 { state(3); }
                2
            }
            _ => 0,
        }
    }
}
