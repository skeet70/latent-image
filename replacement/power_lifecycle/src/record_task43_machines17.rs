use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_f22960(argument0: u32);
    fn xm1_power_record_helper_f22f3c();
    fn xm1_power_record_helper_f23070();
    fn xm1_power_record_helper_f23088();
    fn xm1_power_record_helper_f232f4();
    fn xm1_power_record_helper_f23314();
    fn xm1_power_record_helper_f23338();
    fn xm1_power_record_helper_f23384();
    fn xm1_power_record_helper_f234d8() -> u32;
    fn xm1_power_record_helper_f234f8();
    fn xm1_power_record_helper_f2351c();
    fn xm1_power_record_helper_f23540();
    fn xm1_power_record_helper_f23564(argument0: u32);
    fn xm1_power_record_helper_f23584();
    fn xm1_power_record_helper_f23624();
    fn xm1_power_record_helper_f23648() -> u32;
    fn xm1_power_record_helper_f23668() -> u32;
    fn xm1_power_record_helper_f2368c(argument0: u32);
    fn xm1_power_record_helper_f236ac() -> u32;
    fn xm1_power_record_helper_f236cc(argument0: u32);
    fn xm1_power_record_helper_f236f0(argument0: u32);
    fn xm1_power_record_helper_f23880(argument0: u32);
    fn xm1_power_record_helper_f238a0() -> u32;
    fn xm1_power_record_helper_f23900() -> u32;
    fn xm1_power_record_helper_f23944();
    fn xm1_power_record_helper_f239a4();
    fn xm1_power_record_helper_f23a04();
    fn xm1_power_record_helper_f23a38();
    fn xm1_power_record_helper_f23a64();
    fn xm1_power_record_helper_f23a84();
    fn xm1_power_record_helper_f23ac4();
    fn xm1_power_record_helper_f23aec();
    fn xm1_power_record_helper_f23b14();
    fn xm1_power_record_helper_f23b3c() -> u32;
    fn xm1_power_record_helper_f23b68();
    fn xm1_power_record_helper_f23cb8(argument0: u32, argument1: u32) -> u32;
    fn xm1_power_record_helper_f24a94(argument0: u32) -> u32;
    fn xm1_power_record_helper_f32360() -> u32;
    fn xm1_power_record_helper_f323e4(argument0: u32);
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f46c4c(argument0: u32);
    fn xm1_power_record_helper_f46cf8(status: u32);
    fn xm1_power_record_helper_f470d0(argument0: u32, argument1: u32);
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine17(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => handler_f0ae80(event, request),
            3 => handler_f0aeb8(event, request),
            4 => handler_f0aecc(event, request),
            5 => handler_f0aef8(event, request),
            6 => handler_f0af20(event, request),
            7 => handler_f0af4c(event, request),
            8 => handler_f0afe4(event, request),
            9 => handler_f0b08c(event, request),
            10 => handler_f0b09c(event, request),
            11 => handler_f0b0c0(event, request),
            12 => handler_f0b15c(event, request),
            13 => handler_f0b170(event, request),
            14 => handler_f0b188(event, request),
            15 => handler_f0b19c(event, request),
            16 => handler_f0b1c4(event, request),
            17 => handler_f0b298(event, request),
            18 => handler_f0b2ac(event, request),
            19 => handler_f0b2c0(event, request),
            20 => handler_f0b2d4(event, request),
            21 => handler_f0b3dc(event, request),
            22 => handler_f0b3f0(event, request),
            23 => handler_f0b430(event, request),
            24 => handler_f0b448(event, request),
            25 => handler_f0b45c(event, request),
            26 => handler_f0b474(event, request),
            27 => handler_f0b4fc(event, request),
            28 => handler_f0b510(event, request),
            29 => handler_f0b5a0(event, request),
            30 => handler_f0b630(event, request),
            31 => handler_f0b690(event, request),
            32 => handler_f0b760(event, request),
            33 => handler_f0b79c(event, request),
            34 => handler_f0b7b8(event, request),
            35 => handler_f0b7cc(event, request),
            36 => handler_f0b7e4(event, request),
            37 => handler_f0b800(event, request),
            38 => handler_f0b828(event, request),
            39 => handler_f0b850(event, request),
            40 => handler_f0b868(event, request),
            41 => handler_f0b87c(event, request),
            42 => handler_f0b8d0(event, request),
            43 => handler_f0b914(event, request),
            44 => handler_f0b964(event, request),
            45 => handler_f0b978(event, request),
            46 => handler_f0b9bc(event, request),
            47 => handler_f0ba0c(event, request),
            48 => handler_f0ba2c(event, request),
            _ => 0,
        }
    }
}

unsafe fn handler_f0ae80(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r1;
        r0 = 253u32;
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = 256u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        xm1_power_record_helper_f2351c();
        r0 = r4;
        xm1_power_record_helper_f22960(r0);
        xm1_power_record_helper_f22f3c();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0aeb8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f46c20();
        r0 = 0u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0aecc(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r1;
        r0 = 254u32;
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = 0x101u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        xm1_power_record_helper_f23070();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0aef8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r1;
        r0 = 254u32;
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = 0x101u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0af20(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r1;
        r0 = 254u32;
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = 0x101u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        xm1_power_record_helper_f234f8();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0af4c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = 254u32;
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23aec();
                    r0 = 0x101u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23b14();
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f232f4();
                    next = Flow::B6;
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 10u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    next = Flow::B6;
                    continue;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 10u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 0x101u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    xm1_power_record_helper_f23b14();
                    next = Flow::B6;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0afe4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = 254u32;
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23aec();
                    r0 = 0x101u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 0u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23b14();
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f23314();
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 11u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 11u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 0x101u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    xm1_power_record_helper_f23b14();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b08c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b09c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    unsafe {
        r4 = r0;
        r5 = r1;
        xm1_power_record_helper_f23b68();
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b0c0(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r0;
                    r5 = r1;
                    r0 = 254u32;
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23aec();
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23b14();
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f232f4();
                    next = Flow::B6;
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 10u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    next = Flow::B6;
                    continue;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 10u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    xm1_power_record_helper_f23b14();
                    next = Flow::B6;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b15c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23088();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b170(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b188(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23540();
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b19c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r1;
        r0 = 253u32;
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = r4;
        xm1_power_record_helper_f22960(r0);
        xm1_power_record_helper_f22f3c();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b1c4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = xm1_power_record_helper_f234d8();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 3u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23338();
                    next = Flow::B10;
                    continue;
                }
                Flow::B2 => {
                    r0 = xm1_power_record_helper_f23648();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 12u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23584();
                    next = Flow::B10;
                    continue;
                }
                Flow::B4 => {
                    r0 = xm1_power_record_helper_f236ac();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    next = Flow::B10;
                    continue;
                }
                Flow::B6 => {
                    r0 = xm1_power_record_helper_f238a0();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B9; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f24a94(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 0u32;
                    xm1_power_record_helper_f23880(r0);
                    next = Flow::B10;
                    continue;
                }
                Flow::B9 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    next = Flow::B10;
                }
                Flow::B10 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b298(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f232f4();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b2ac(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23314();
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b2c0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23944();
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b2d4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10, B11 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = xm1_power_record_helper_f234d8();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 3u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23338();
                    next = Flow::B11;
                    continue;
                }
                Flow::B2 => {
                    r0 = xm1_power_record_helper_f23648();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 12u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23584();
                    next = Flow::B11;
                    continue;
                }
                Flow::B4 => {
                    r0 = xm1_power_record_helper_f236ac();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B7; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 7u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f232f4();
                    r3 = 0x5a3cu32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = 0u32;
                    write_volatile((r3) as *mut u8, r2 as u8);
                    r0 = 254u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B11; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r3 = 0x5a3cu32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3) as *const u8) as u32;
                    r2 = r2 | 2u32;
                    write_volatile((r3) as *mut u8, r2 as u8);
                    next = Flow::B11;
                    continue;
                }
                Flow::B7 => {
                    r0 = xm1_power_record_helper_f238a0();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f24a94(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B11; } else { next = Flow::B9; }
                    continue;
                }
                Flow::B9 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 0u32;
                    xm1_power_record_helper_f23880(r0);
                    next = Flow::B11;
                    continue;
                }
                Flow::B10 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    next = Flow::B11;
                }
                Flow::B11 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b3dc(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23070();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b3f0(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r3 = 0x5a3cu32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r2 = read_volatile((r3) as *const u8) as u32;
                    r2 = r2 | 1u32;
                    write_volatile((r3) as *mut u8, r2 as u8);
                    equal = r2 == 3u32; lower_or_same = r2 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23070();
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b430(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 0u32;
        xm1_power_record_helper_f46cf8(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b448(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f239a4();
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b45c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 0u32;
        xm1_power_record_helper_f46c4c(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b474(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = xm1_power_record_helper_f23648();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 12u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23584();
                    next = Flow::B6;
                    continue;
                }
                Flow::B2 => {
                    r0 = xm1_power_record_helper_f238a0();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B5; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f24a94(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 0u32;
                    xm1_power_record_helper_f23880(r0);
                    next = Flow::B6;
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    next = Flow::B6;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b4fc(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23384();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b510(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r0;
                    r0 = r1;
                    xm1_power_record_helper_f236cc(r0);
                    r0 = r4;
                    xm1_power_record_helper_f23564(r0);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f23cb8(r0, r1);
                    r4 = r0;
                    equal = r0 == 3u32; lower_or_same = r0 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23624();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = xm1_power_record_helper_f23668();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 15u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2368c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r4 == 4u32; lower_or_same = r4 <= 4u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f23880(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b5a0(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r0;
                    r0 = r1;
                    xm1_power_record_helper_f236cc(r0);
                    r0 = r4;
                    xm1_power_record_helper_f23564(r0);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f23cb8(r0, r1);
                    r4 = r0;
                    equal = r0 == 3u32; lower_or_same = r0 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23624();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = xm1_power_record_helper_f23668();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 16u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2368c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r4 == 4u32; lower_or_same = r4 <= 4u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f23880(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b630(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r0;
                    r0 = r1;
                    xm1_power_record_helper_f236cc(r0);
                    r0 = r4;
                    xm1_power_record_helper_f23564(r0);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f23cb8(r0, r1);
                    equal = r0 == 3u32; lower_or_same = r0 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23624();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 4u32; lower_or_same = r0 <= 4u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f23880(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b690(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r0;
                    r5 = r1;
                    r0 = r1;
                    xm1_power_record_helper_f236cc(r0);
                    r0 = r4;
                    xm1_power_record_helper_f23564(r0);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f23cb8(r0, r1);
                    r4 = r0;
                    equal = r0 == 3u32; lower_or_same = r0 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 12u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23584();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = xm1_power_record_helper_f23668();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 7u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2368c(r0);
                    xm1_power_record_helper_f232f4();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r4 == 4u32; lower_or_same = r4 <= 4u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f23880(r0);
                    r0 = r5;
                    r0 = xm1_power_record_helper_f24a94(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 0u32;
                    xm1_power_record_helper_f23880(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B7 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f46c4c(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B8 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b760(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
    let mut r0: u32 = event;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    xm1_power_record_helper_f23a04();
                    r0 = xm1_power_record_helper_f23900();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23a38();
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 13u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23a64();
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b79c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = r1;
        xm1_power_record_helper_f22960(r0);
        xm1_power_record_helper_f23088();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b7b8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f23a64();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b7cc(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = r1;
        xm1_power_record_helper_f236f0(r0);
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b7e4(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = r1;
        r1 = 1u32;
        xm1_power_record_helper_f470d0(r0, r1);
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b800(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        xm1_power_record_helper_f23a84();
        r0 = xm1_power_record_helper_f32360();
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { r0 = 0u32; }
        xm1_power_record_helper_f323e4(r0);
        xm1_power_record_helper_f23ac4();
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b828(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r1;
        r0 = 255u32;
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = 0x102u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b850(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = 0x102u32;
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b868(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b87c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if equal { r3 = 0x5a3cu32; }
                    if equal { r3 = (r3 & 0xffff) | (0x16bu32 << 16); }
                    if equal { r2 = read_volatile((r3) as *const u8) as u32; }
                    if equal { r2 = r2 | 2u32; }
                    if equal { write_volatile((r3) as *mut u8, r2 as u8); }
                    r3 = 0x5a3cu32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f23070();
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b8d0(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23aec();
                    r0 = xm1_power_record_helper_f23b3c();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f232f4();
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b914(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(31, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { r0 = 2u32; }
                    if !equal { return r0 as i32; }
                    xm1_power_record_helper_f23aec();
                    r0 = xm1_power_record_helper_f23b3c();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 0u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f23314();
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b964(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0b978(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f23b14();
                    r0 = xm1_power_record_helper_f23b3c();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f232f4();
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0b9bc(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(34, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { r0 = 2u32; }
                    if !equal { return r0 as i32; }
                    xm1_power_record_helper_f23b14();
                    r0 = xm1_power_record_helper_f23b3c();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 0u32;
                    write_volatile((r3.wrapping_add(34u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f234f8();
                    xm1_power_record_helper_f23314();
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0ba0c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 0u32;
        xm1_power_record_helper_f46c4c(r0);
        r0 = 0u32;
        xm1_power_record_helper_f23880(r0);
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0ba2c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 3u32;
        return r0 as i32;
    }
}
