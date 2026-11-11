use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_74e020(argument0: u32, argument1: u32, argument2: u32, argument3: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(argument0: u32) -> u32;
    fn xm1_power_record_helper_750ac0(argument0: u32) -> u32;
    fn xm1_power_record_helper_75569c(argument0: u32) -> u32;
    fn xm1_power_record_helper_f2f7f0(argument0: u32);
    fn xm1_power_record_helper_f2fb7c(argument0: u32);
    fn xm1_power_record_helper_f2fba4();
    fn xm1_power_record_helper_f2fbc8();
    fn xm1_power_record_helper_f2fbf0(argument0: u32);
    fn xm1_power_record_helper_f2fc50();
    fn xm1_power_record_helper_f2fc74() -> u32;
    fn xm1_power_record_helper_f2fc90(argument0: u32);
    fn xm1_power_record_helper_f2fcd0() -> u32;
    fn xm1_power_record_helper_f2fcf0() -> u32;
    fn xm1_power_record_helper_f2fd34() -> u32;
    fn xm1_power_record_helper_f2fd58(argument0: u32);
    fn xm1_power_record_helper_f2fd78();
    fn xm1_power_record_helper_f2fdb4();
    fn xm1_power_record_helper_f2fde0();
    fn xm1_power_record_helper_f2fe10();
    fn xm1_power_record_helper_f2fe44();
    fn xm1_power_record_helper_f2ff10();
    fn xm1_power_record_helper_f2ff80();
    fn xm1_power_record_helper_f2ff94();
    fn xm1_power_record_helper_f2ffa8() -> u32;
    fn xm1_power_record_helper_f30014() -> u32;
    fn xm1_power_record_helper_f30050(argument0: u32, argument1: u32, argument2: u32, argument3: u32) -> u32;
    fn xm1_power_record_helper_f30094() -> u32;
    fn xm1_power_record_helper_f307f0() -> u32;
    fn xm1_power_record_helper_f3089c();
    fn xm1_power_record_helper_f308d4(argument0: u32, argument1: u32);
    fn xm1_power_record_helper_f30f64();
    fn xm1_power_record_helper_f31548();
    fn xm1_power_record_helper_f315bc();
    fn xm1_power_record_helper_f321b0(argument0: u32) -> u32;
    fn xm1_power_record_helper_f322dc();
    fn xm1_power_record_helper_f322fc();
    fn xm1_power_record_helper_f3232c();
    fn xm1_power_record_helper_f32350();
    fn xm1_power_record_helper_f32360() -> u32;
    fn xm1_power_record_helper_f32394();
    fn xm1_power_record_helper_f323bc();
    fn xm1_power_record_helper_f323e4(argument0: u32);
    fn xm1_power_record_helper_f32404() -> u32;
    fn xm1_power_record_helper_f32484() -> u32;
    fn xm1_power_record_helper_f324a0();
    fn xm1_power_record_helper_f324c4();
    fn xm1_power_record_helper_f324e8() -> u32;
    fn xm1_power_record_helper_f3aef8() -> u32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine12(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => handler_f07c6c(event, request),
            3 => handler_f07c98(event, request),
            4 => handler_f07cac(event, request),
            5 => handler_f07cc4(event, request),
            6 => handler_f07dd0(event, request),
            7 => handler_f07de8(event, request),
            8 => handler_f07e38(event, request),
            9 => handler_f07e8c(event, request),
            10 => handler_f07f10(event, request),
            11 => handler_f07f64(event, request),
            12 => handler_f07fb0(event, request),
            13 => handler_f07fe8(event, request),
            14 => handler_f08084(event, request),
            15 => handler_f08180(event, request),
            16 => handler_f08278(event, request),
            17 => handler_f08358(event, request),
            18 => handler_f083cc(event, request),
            19 => handler_f083e8(event, request),
            20 => handler_f08430(event, request),
            21 => handler_f08488(event, request),
            22 => handler_f084c0(event, request),
            23 => handler_f084f8(event, request),
            24 => handler_f08508(event, request),
            25 => handler_f0852c(event, request),
            26 => handler_f08550(event, request),
            27 => handler_f08560(event, request),
            28 => handler_f085b8(event, request),
            29 => handler_f08790(event, request),
            30 => handler_f087c8(event, request),
            31 => handler_f087ec(event, request),
            32 => handler_f08840(event, request),
            33 => handler_f08894(event, request),
            34 => handler_f08984(event, request),
            35 => handler_f08a5c(event, request),
            36 => handler_f08ad8(event, request),
            37 => handler_f08be0(event, request),
            38 => handler_f08dcc(event, request),
            39 => handler_f08de0(event, request),
            40 => handler_f08e08(event, request),
            41 => handler_f08eec(event, request),
            42 => handler_f08f14(event, request),
            43 => handler_f08f30(event, request),
            44 => handler_f08f88(event, request),
            45 => handler_f08f9c(event, request),
            46 => handler_f090e0(event, request),
            47 => handler_f09110(event, request),
            48 => handler_f09134(event, request),
            49 => handler_f091b4(event, request),
            50 => handler_f091cc(event, request),
            51 => handler_f0928c(event, request),
            52 => handler_f092c4(event, request),
            53 => handler_f09308(event, request),
            54 => handler_f09368(event, request),
            55 => handler_f093a8(event, request),
            56 => handler_f093e8(event, request),
            57 => handler_f09470(event, request),
            58 => handler_f094c0(event, request),
            59 => handler_f09594(event, request),
            60 => handler_f095ac(event, request),
            61 => handler_f095c0(event, request),
            62 => handler_f09620(event, request),
            63 => handler_f09698(event, request),
            64 => handler_f096d0(event, request),
            65 => handler_f096f4(event, request),
            66 => handler_f09718(event, request),
            67 => handler_f09770(event, request),
            68 => handler_f09784(event, request),
            69 => handler_f097a8(event, request),
            70 => handler_f097c0(event, request),
            71 => handler_f097e4(event, request),
            72 => handler_f09874(event, request),
            73 => handler_f09894(event, request),
            74 => handler_f098b4(event, request),
            75 => handler_f098d8(event, request),
            76 => handler_f0991c(event, request),
            77 => handler_f09940(event, request),
            78 => handler_f09984(event, request),
            79 => handler_f099e8(event, request),
            80 => handler_f09a2c(event, request),
            81 => handler_f09a70(event, request),
            82 => handler_f09a98(event, request),
            83 => handler_f09ad8(event, request),
            84 => handler_f09af8(event, request),
            85 => handler_f09b2c(event, request),
            86 => handler_f09bc0(event, request),
            87 => handler_f09be4(event, request),
            88 => handler_f09c48(event, request),
            89 => handler_f09d48(event, request),
            90 => handler_f09e48(event, request),
            91 => handler_f09f80(event, request),
            92 => handler_f09fb8(event, request),
            93 => handler_f09fd8(event, request),
            94 => handler_f0a028(event, request),
            _ => 0,
        }
    }
}

unsafe fn handler_f07c6c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r0;
        xm1_power_record_helper_f308d4(r0, r1);
        xm1_power_record_helper_f2fba4();
        r0 = r4;
        xm1_power_record_helper_f2fbf0(r0);
        xm1_power_record_helper_f2ff80();
        xm1_power_record_helper_f2fd78();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f07c98(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f46c20();
        r0 = 0u32;
        return r0 as i32;
    }
}

unsafe fn handler_f07cac(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f07cc4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = r4;
                    xm1_power_record_helper_f2fc90(r0);
                    r0 = xm1_power_record_helper_f324e8();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f324c4();
                    r0 = xm1_power_record_helper_f32484();
                    r4 = r0;
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    r0 = 174u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { r0 = 1u32; }
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B7 => {
                    equal = r4 == 2u32; lower_or_same = r4 <= 2u32;
                    if !equal { r0 = r4; }
                    if !equal { return r0 as i32; }
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B9; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B9 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 23u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    if !equal { r0 = 2u32; }
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f07dd0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 0u32;
        return r0 as i32;
    }
}

unsafe fn handler_f07de8(event: u32, request: u32) -> i32 {
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
                    r0 = 185u32;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2ff10();
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f07e38(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
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
                    r0 = 185u32;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 7u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    next = Flow::B4;
                    continue;
                }
                Flow::B2 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 7u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2ff10();
                    next = Flow::B4;
                }
                Flow::B4 => {
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f07e8c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
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
                    r0 = xm1_power_record_helper_f3aef8();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 9u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 231u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = 186u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 8u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 8u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f3089c();
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f07f10(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
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
                    r0 = 186u32;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 11u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    next = Flow::B4;
                    continue;
                }
                Flow::B2 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 11u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f3089c();
                    next = Flow::B4;
                }
                Flow::B4 => {
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f07f64(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = r4;
                    xm1_power_record_helper_f2fbf0(r0);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r0 = xm1_power_record_helper_f30014();
                    r4 = r0;
                    xm1_power_record_helper_f2ff80();
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fd78();
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fe10();
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

unsafe fn handler_f07fb0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r0;
        r5 = r1;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 14u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f07fe8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r0 = r4;
                    xm1_power_record_helper_f2fc90(r0);
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(12u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32;
                    if equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 24u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fdb4();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B3 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08084(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10, B11 }
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
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r0 = r4;
                    xm1_power_record_helper_f2fc90(r0);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(12u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(4u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r0 = r4;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fc50();
                    xm1_power_record_helper_f2fba4();
                    r0 = 175u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B5;
                    continue;
                }
                Flow::B4 => {
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { return r0 as i32; }
                    next = Flow::B5;
                }
                Flow::B5 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B8;
                    continue;
                }
                Flow::B7 => {
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { return r0 as i32; }
                    next = Flow::B8;
                }
                Flow::B8 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32;
                    if !equal { next = Flow::B11; } else { next = Flow::B9; }
                    continue;
                }
                Flow::B9 => {
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 14u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
                Flow::B11 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08180(event: u32, request: u32) -> i32 {
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
                    r5 = r0;
                    r4 = r1;
                    r0 = 176u32;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 4u32; lower_or_same = r0 <= 4u32;
                    if !equal { return r0 as i32; }
                    r0 = r5;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = 174u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B3 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 17u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08278(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r5 = r0;
                    r6 = r1;
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = xm1_power_record_helper_f2fc74();
                    r4 = r0;
                    r0 = r5;
                    xm1_power_record_helper_f2fc90(r0);
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B9; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = r5;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = 177u32;
                    r1 = r6;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B4;
                    continue;
                }
                Flow::B3 => {
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { return r0 as i32; }
                    next = Flow::B4;
                }
                Flow::B4 => {
                    r0 = 184u32;
                    r1 = r6;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B7;
                    continue;
                }
                Flow::B6 => {
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { return r0 as i32; }
                    next = Flow::B7;
                }
                Flow::B7 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B9 => {
                    r0 = r5;
                    r1 = r6;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 14u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08358(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 2u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = xm1_power_record_helper_f30014();
                    r4 = r0;
                    xm1_power_record_helper_f2ff80();
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fd78();
                    next = Flow::B5;
                    continue;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f2fe10();
                    next = Flow::B5;
                    continue;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 24u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fdb4();
                    next = Flow::B5;
                }
                Flow::B5 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f083cc(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        xm1_power_record_helper_f308d4(r0, r1);
        r0 = 0u32;
        xm1_power_record_helper_f2f7f0(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f083e8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 1u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 11u32; lower_or_same = r0 <= 11u32;
                    if equal { r0 = 1u32; }
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08430(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r5 = r0;
                    r4 = r1;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r0 = r5;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 15u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 11u32; lower_or_same = r0 <= 11u32;
                    if !equal { return r0 as i32; }
                    r0 = 187u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08488(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r0;
        r5 = r1;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 7u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f084c0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r0;
        r5 = r1;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 11u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f084f8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08508(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    unsafe {
        r4 = r0;
        r5 = r1;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        return r0 as i32;
    }
}

unsafe fn handler_f0852c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 16u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f08550(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08560(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 1u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f085b8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10, B11, B12, B13, B14, B15 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    equal = r0 == 4u32; lower_or_same = r0 <= 4u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fba4();
                    r0 = 174u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B5 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B7 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 17u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
                Flow::B8 => {
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B9; }
                    continue;
                }
                Flow::B9 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B10 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(8u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32;
                    if !equal { next = Flow::B15; } else { next = Flow::B11; }
                    continue;
                }
                Flow::B11 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(4u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32;
                    if !equal { next = Flow::B15; } else { next = Flow::B12; }
                    continue;
                }
                Flow::B12 => {
                    r0 = r4;
                    xm1_power_record_helper_f2fc90(r0);
                    xm1_power_record_helper_f2fba4();
                    xm1_power_record_helper_f2fc50();
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B14; } else { next = Flow::B13; }
                    continue;
                }
                Flow::B13 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B14 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B15 => {
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 16u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08790(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r0;
        r5 = r1;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 1u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f087c8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 1u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f087ec(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = r4;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r0 = xm1_power_record_helper_f32360();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    r0 = r4;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 0u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08840(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r0;
        r5 = r1;
        xm1_power_record_helper_f308d4(r0, r1);
        r0 = r4;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = xm1_power_record_helper_f32360();
        r6 = r0;
        r0 = r4;
        r1 = r5;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        r4 = r0;
        equal = r6 == 1u32; lower_or_same = r6 <= 1u32;
        if equal { r0 = 2u32; }
        if !equal { r0 = 0u32; }
        xm1_power_record_helper_f323e4(r0);
        r0 = 0u32;
        xm1_power_record_helper_f2f7f0(r0);
        r0 = r4;
        return r0 as i32;
    }
}

unsafe fn handler_f08894(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r6 = r0;
                    r5 = r1;
                    r0 = xm1_power_record_helper_f2ffa8();
                    r4 = r0;
                    r0 = r6;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = r6;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = 174u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    next = Flow::B10;
                    continue;
                }
                Flow::B4 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B10;
                    continue;
                }
                Flow::B5 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B10;
                    continue;
                }
                Flow::B7 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 18u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    r0 = 175u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B9; }
                    continue;
                }
                Flow::B9 => {
                    xm1_power_record_helper_f2fe44();
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

unsafe fn handler_f08984(event: u32, request: u32) -> i32 {
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
                    r5 = r0;
                    r4 = r1;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    equal = r0 == 4u32; lower_or_same = r0 <= 4u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = r5;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = 174u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B5 => {
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B7 => {
                    r0 = r5;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 14u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
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

unsafe fn handler_f08a5c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
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
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = xm1_power_record_helper_f2fcd0();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fba4();
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r4 = 2u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r4 as u16);
                    xm1_power_record_helper_f2ff80();
                    xm1_power_record_helper_f2fd78();
                    r0 = r4;
                    return r0 as i32;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 24u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fdb4();
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08ad8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
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
                    r5 = r0;
                    r4 = r1;
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = xm1_power_record_helper_f2fcd0();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B9; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fba4();
                    r0 = r5;
                    xm1_power_record_helper_f2fc90(r0);
                    r0 = 174u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r5 = r0;
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r5 == 1u32; lower_or_same = r5 <= 1u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B5 => {
                    equal = r5 == 2u32; lower_or_same = r5 <= 2u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 23u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B8 => {
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 23u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
                Flow::B9 => {
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = r5;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08be0(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10, B11, B12, B13, B14, B15, B16 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut r6: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r6 = r0;
                    r5 = r1;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    r4 = r0;
                    r0 = xm1_power_record_helper_f30050(r0, r1, r2, r3);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 20u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { r0 = 2u32; }
                    if !equal { return r0 as i32; }
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(52u32)) as *const u8) as u32;
                    equal = r3 == 2u32; lower_or_same = r3 <= 2u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f32394();
                    r0 = 26u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 1u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B3;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r4 == 4u32; lower_or_same = r4 <= 4u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = r6;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f2fba4();
                    r0 = 174u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B9; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B8 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B9 => {
                    r0 = 184u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { r0 = 2u32; }
                    if !equal { return r0 as i32; }
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = r6;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r0 = r6;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B12; } else { next = Flow::B11; }
                    continue;
                }
                Flow::B11 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 14u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B12 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r2 = 32u32;
                    r3 = 0u32;
                    r0 = xm1_power_record_helper_74e020(r0, r1, r2, r3);
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B14; } else { next = Flow::B13; }
                    continue;
                }
                Flow::B13 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 14u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 162u32;
                    xm1_power_record_helper_f308d4(r0, r1);
                    xm1_power_record_helper_f31548();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B14 => {
                    r0 = xm1_power_record_helper_f2fd34();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B16; } else { next = Flow::B15; }
                    continue;
                }
                Flow::B15 => {
                    r0 = xm1_power_record_helper_f2fcf0();
                    equal = r0 == 4u32; lower_or_same = r0 <= 4u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 12u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 181u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B16 => {
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f08dcc(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08de0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r0;
        xm1_power_record_helper_f308d4(r0, r1);
        r0 = r4;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = 0u32;
        xm1_power_record_helper_f2f7f0(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08e08(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
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
                    r5 = r0;
                    r4 = r1;
                    xm1_power_record_helper_f308d4(r0, r1);
                    r0 = r5;
                    r0 = xm1_power_record_helper_f321b0(r0);
                    equal = r0 == 4u32; lower_or_same = r0 <= 4u32;
                    if !equal { next = Flow::B7; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 17u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fba4();
                    r0 = 174u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    xm1_power_record_helper_f2fe44();
                    next = Flow::B10;
                    continue;
                }
                Flow::B4 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B10;
                    continue;
                }
                Flow::B5 => {
                    r0 = 184u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B10; } else { next = Flow::B6; }
                    continue;
                }
                Flow::B6 => {
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    next = Flow::B10;
                    continue;
                }
                Flow::B7 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 14u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B9; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    xm1_power_record_helper_f31548();
                    next = Flow::B10;
                    continue;
                }
                Flow::B9 => {
                    r0 = 182u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
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

unsafe fn handler_f08eec(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    unsafe {
        r4 = r0;
        xm1_power_record_helper_f308d4(r0, r1);
        r0 = r4;
        r0 = xm1_power_record_helper_f321b0(r0);
        r0 = 0u32;
        xm1_power_record_helper_f2f7f0(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08f14(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        xm1_power_record_helper_f308d4(r0, r1);
        xm1_power_record_helper_f2fba4();
        xm1_power_record_helper_f322dc();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08f30(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r1;
        xm1_power_record_helper_f308d4(r0, r1);
        xm1_power_record_helper_f2fba4();
        xm1_power_record_helper_f2ff80();
        xm1_power_record_helper_f2fbc8();
        r0 = 183u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        r3 = 0xd4b4u32;
        r3 = (r3 & 0xffff) | (0x169u32 << 16);
        r2 = 4u32;
        write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
        xm1_power_record_helper_f322fc();
        xm1_power_record_helper_f30f64();
        r0 = 2u32;
        xm1_power_record_helper_f2fb7c(r0);
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08f88(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f2ff94();
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f08f9c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8 }
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
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(40u32)) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f32350();
                    r0 = 172u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(25, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 3u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B3 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    xm1_power_record_helper_f2fbc8();
                    r0 = 183u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f322fc();
                    xm1_power_record_helper_f30f64();
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B5 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r4 = 0u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r4 as u16);
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    xm1_power_record_helper_f2fbc8();
                    r0 = 183u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f322fc();
                    xm1_power_record_helper_f30f64();
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B8 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r4 = 0u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r4 as u16);
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f090e0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r1;
        xm1_power_record_helper_f2fbc8();
        r0 = 183u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f30f64();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09110(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = 183u32;
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f30f64();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09134(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fbc8();
                    r0 = 183u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f30f64();
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r4 = 0u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r4 as u16);
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f091b4(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 0u32;
        xm1_power_record_helper_f2f7f0(r0);
        r0 = 1u32;
        return r0 as i32;
    }
}

unsafe fn handler_f091cc(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8 }
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
                    r0 = 4u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(52u32)) as *const u8) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = r4;
                    r0 = xm1_power_record_helper_750ac0(r0);
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f323bc();
                    r0 = 26u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 2u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B3;
                }
                Flow::B3 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 7u32; lower_or_same = r3 <= 7u32;
                    if !equal { next = Flow::B8; } else { next = Flow::B4; }
                    continue;
                }
                Flow::B4 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 182u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    next = Flow::B7;
                    continue;
                }
                Flow::B6 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r4 = 1u32;
                    next = Flow::B7;
                }
                Flow::B7 => {
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
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

unsafe fn handler_f0928c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = 29u32;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2ff94();
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f092c4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
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
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(52u32)) as *const u8) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f323bc();
                    r0 = 26u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 2u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2ff94();
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09308(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
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
                    xm1_power_record_helper_f2fba4();
                    xm1_power_record_helper_f3232c();
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(52u32)) as *const u8) as u32;
                    equal = r3 == 2u32; lower_or_same = r3 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f32394();
                    r0 = 26u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 1u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r4 = 2u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r4 as u16);
                    xm1_power_record_helper_f2ff80();
                    xm1_power_record_helper_f2fd78();
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09368(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
    let mut r0: u32 = event;
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
                    r0 = xm1_power_record_helper_f30014();
                    r4 = r0;
                    xm1_power_record_helper_f2ff80();
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 19u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fde0();
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fe10();
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

unsafe fn handler_f093a8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3 }
    let mut r0: u32 = event;
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
                    r0 = xm1_power_record_helper_f30094();
                    r4 = r0;
                    xm1_power_record_helper_f2ff80();
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 21u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    equal = r4 == 1u32; lower_or_same = r4 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fde0();
                    next = Flow::B3;
                    continue;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fe10();
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

unsafe fn handler_f093e8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4 }
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
                    xm1_power_record_helper_f3232c();
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(52u32)) as *const u8) as u32;
                    equal = r3 == 2u32; lower_or_same = r3 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f32394();
                    r0 = 26u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 1u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(12u32)) as *const u32) as u32;
                    equal = r3 == 0u32; lower_or_same = r3 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 24u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2fdb4();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r0 = 0u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r0 as u16);
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09470(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_helper_f3aef8();
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 10u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 232u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f094c0(event: u32, request: u32) -> i32 {
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
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(20u32)) as *const u32) as u32;
                    equal = r3 == 9u32; lower_or_same = r3 <= 9u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 176u32;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    next = Flow::B5;
                    continue;
                }
                Flow::B2 => {
                    r0 = r1;
                    r0 = xm1_power_record_helper_f307f0();
                    equal = r0 == 0u32; lower_or_same = r0 <= 0u32;
                    if equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    xm1_power_record_helper_f324a0();
                    r0 = xm1_power_record_helper_f32404();
                    next = Flow::B6;
                    continue;
                }
                Flow::B4 => {
                    r0 = 173u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    next = Flow::B5;
                }
                Flow::B5 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    next = Flow::B6;
                }
                Flow::B6 => {
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 7u32; lower_or_same = r3 <= 7u32;
                    if !equal { next = Flow::B11; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B9; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    r0 = 182u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    next = Flow::B10;
                    continue;
                }
                Flow::B9 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r4 = 1u32;
                    next = Flow::B10;
                }
                Flow::B10 => {
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
                    return r0 as i32;
                }
                Flow::B11 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09594(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f095ac(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        xm1_power_record_helper_f31548();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f095c0(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 182u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if equal { r3 = 0xd4b4u32; }
                    if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if equal { r2 = 5u32; }
                    if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    return r0 as i32;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09620(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 182u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09698(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f096d0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f2fd58(r0);
        xm1_power_record_helper_f315bc();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f096f4(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 5u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f09718(event: u32, request: u32) -> i32 {
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f31548();
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09770(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09784(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 14u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f097a8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if equal { r0 = 2u32; }
        return r0 as i32;
    }
}

unsafe fn handler_f097c0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 5u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f097e4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(25, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3) as *const u32) as u32;
                    equal = r3 == 1u32; lower_or_same = r3 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fbc8();
                    r0 = 183u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 4u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f322fc();
                    xm1_power_record_helper_f30f64();
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r4 = 0u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r4 as u16);
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
                    xm1_power_record_helper_f2f7f0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09874(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f2ff10();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09894(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f3089c();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f098b4(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if equal { r3 = 0xd4b4u32; }
        if equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if equal { r2 = 5u32; }
        if equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f098d8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r0 = 2u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
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

unsafe fn handler_f0991c(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if !equal { r3 = 0xd4b4u32; }
        if !equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if !equal { r2 = 0u32; }
        if !equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f09940(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 6u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 11u32; lower_or_same = r0 <= 11u32;
                    if !equal { return r0 as i32; }
                    r0 = 187u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09984(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r0 = 186u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 8u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 8u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f3089c();
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f099e8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 8u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 11u32; lower_or_same = r0 <= 11u32;
                    if !equal { return r0 as i32; }
                    r0 = 187u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09a2c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 11u32; lower_or_same = r0 <= 11u32;
                    if !equal { return r0 as i32; }
                    r0 = 187u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09a70(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r1;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 11u32; lower_or_same = r0 <= 11u32;
        if !equal { return r0 as i32; }
        r0 = 187u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09a98(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    xm1_power_record_helper_f2fb7c(r0);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 3u32; lower_or_same = r3 <= 3u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
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

unsafe fn handler_f09ad8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f2fe44();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09af8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
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
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    xm1_power_record_helper_f2ff94();
                    next = Flow::B2;
                }
                Flow::B2 => {
                    r0 = r4;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09b2c(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5 }
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
                    r5 = r1;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { r0 = r0; }
                    if !equal { return r0 as i32; }
                    r0 = 1u32;
                    xm1_power_record_helper_f2fb7c(r0);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(28u32)) as *const u8) as u32;
                    equal = r3 == 7u32; lower_or_same = r3 <= 7u32;
                    if !equal { next = Flow::B5; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_75569c(r0);
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B3; } else { next = Flow::B2; }
                    continue;
                }
                Flow::B2 => {
                    r0 = 182u32;
                    r1 = r5;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r4 = r0;
                    next = Flow::B4;
                    continue;
                }
                Flow::B3 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 0u32;
                    xm1_power_record_helper_f2f7f0(r0);
                    next = Flow::B4;
                }
                Flow::B4 => {
                    xm1_power_record_helper_f2ff94();
                    r0 = r4;
                    return r0 as i32;
                }
                Flow::B5 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09bc0(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r2: u32 = 0;
    let mut r3: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
        if !equal { r3 = 0xd4b4u32; }
        if !equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
        if !equal { r2 = 0u32; }
        if !equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
        return r0 as i32;
    }
}

unsafe fn handler_f09be4(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6 }
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
                    r4 = r1;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 5u32; lower_or_same = r0 <= 5u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r0 = 178u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 6u32; lower_or_same = r0 <= 6u32;
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r0 = 179u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 7u32; lower_or_same = r0 <= 7u32;
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 180u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    equal = r0 == 12u32; lower_or_same = r0 <= 12u32;
                    if equal { r0 = 2u32; }
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09c48(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f2ff10();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 8u32; lower_or_same = r0 <= 8u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 8u32; lower_or_same = r0 <= 8u32;
                    if !equal { equal = r0 == 5u32; lower_or_same = r0 <= 5u32; }
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r0 = 178u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { r0 = 2u32; }
                    if equal { return r0 as i32; }
                    xm1_power_record_helper_f2ff10();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 9u32; lower_or_same = r0 <= 9u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 9u32; lower_or_same = r0 <= 9u32;
                    if !equal { equal = r0 == 6u32; lower_or_same = r0 <= 6u32; }
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 179u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { r0 = 2u32; }
                    if equal { return r0 as i32; }
                    xm1_power_record_helper_f2ff10();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    equal = r0 == 10u32; lower_or_same = r0 <= 10u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 10u32; lower_or_same = r0 <= 10u32;
                    if !equal { equal = r0 == 7u32; lower_or_same = r0 <= 7u32; }
                    if !equal { next = Flow::B9; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r0 = 180u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    xm1_power_record_helper_f2ff10();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B9 => {
                    equal = r0 == 12u32; lower_or_same = r0 <= 12u32;
                    if equal { r0 = 2u32; }
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09d48(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut r5: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    let mut next = Flow::B0;
    unsafe {
        loop {
            match next {
                Flow::B0 => {
                    r4 = r1;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f3089c();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 8u32; lower_or_same = r0 <= 8u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 8u32; lower_or_same = r0 <= 8u32;
                    if !equal { equal = r0 == 5u32; lower_or_same = r0 <= 5u32; }
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r0 = 178u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { r0 = 2u32; }
                    if equal { return r0 as i32; }
                    xm1_power_record_helper_f3089c();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 9u32; lower_or_same = r0 <= 9u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 9u32; lower_or_same = r0 <= 9u32;
                    if !equal { equal = r0 == 6u32; lower_or_same = r0 <= 6u32; }
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 179u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { r0 = 2u32; }
                    if equal { return r0 as i32; }
                    xm1_power_record_helper_f3089c();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B6 => {
                    equal = r0 == 10u32; lower_or_same = r0 <= 10u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 10u32; lower_or_same = r0 <= 10u32;
                    if !equal { equal = r0 == 7u32; lower_or_same = r0 <= 7u32; }
                    if !equal { next = Flow::B9; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r0 = 180u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    xm1_power_record_helper_f3089c();
                    r0 = 2u32;
                    return r0 as i32;
                }
                Flow::B9 => {
                    equal = r0 == 12u32; lower_or_same = r0 <= 12u32;
                    if equal { r0 = 2u32; }
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09e48(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2, B3, B4, B5, B6, B7, B8, B9, B10 }
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
                    r4 = r1;
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r0 = read_volatile((r3.wrapping_add(24u32)) as *const u32) as u32;
                    r0 = xm1_power_record_helper_74e5c0(r0);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B2 => {
                    equal = r0 == 8u32; lower_or_same = r0 <= 8u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 8u32; lower_or_same = r0 <= 8u32;
                    if !equal { equal = r0 == 5u32; lower_or_same = r0 <= 5u32; }
                    if !equal { next = Flow::B4; } else { next = Flow::B3; }
                    continue;
                }
                Flow::B3 => {
                    r0 = 178u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { r0 = 2u32; }
                    if !equal { r3 = 0xd4b4u32; }
                    if !equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if !equal { r2 = 5u32; }
                    if !equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    if !equal { r0 = 1u32; }
                    return r0 as i32;
                }
                Flow::B4 => {
                    equal = r0 == 9u32; lower_or_same = r0 <= 9u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 9u32; lower_or_same = r0 <= 9u32;
                    if !equal { equal = r0 == 6u32; lower_or_same = r0 <= 6u32; }
                    if !equal { next = Flow::B6; } else { next = Flow::B5; }
                    continue;
                }
                Flow::B5 => {
                    r0 = 179u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { r0 = 2u32; }
                    if !equal { r3 = 0xd4b4u32; }
                    if !equal { r3 = (r3 & 0xffff) | (0x169u32 << 16); }
                    if !equal { r2 = 5u32; }
                    if !equal { write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16); }
                    if !equal { r0 = 1u32; }
                    return r0 as i32;
                }
                Flow::B6 => {
                    equal = r0 == 10u32; lower_or_same = r0 <= 10u32;
                    if !equal { r5 = 0u32; }
                    if equal { r5 = 1u32; }
                    equal = r0 == 10u32; lower_or_same = r0 <= 10u32;
                    if !equal { equal = r0 == 7u32; lower_or_same = r0 <= 7u32; }
                    if !equal { next = Flow::B9; } else { next = Flow::B7; }
                    continue;
                }
                Flow::B7 => {
                    r0 = 180u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    equal = r0 == 2u32; lower_or_same = r0 <= 2u32;
                    if !equal { return r0 as i32; }
                    equal = r5 == 0u32; lower_or_same = r5 <= 0u32;
                    if equal { next = Flow::B10; } else { next = Flow::B8; }
                    continue;
                }
                Flow::B8 => {
                    r3 = 0xd4b4u32;
                    r3 = (r3 & 0xffff) | (0x169u32 << 16);
                    r2 = 5u32;
                    write_volatile((r3.wrapping_add(24u32)) as *mut u16, r2 as u16);
                    r0 = 1u32;
                    return r0 as i32;
                }
                Flow::B9 => {
                    equal = r0 == 12u32; lower_or_same = r0 <= 12u32;
                    if equal { r0 = 2u32; }
                    return r0 as i32;
                }
                Flow::B10 => {
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f09f80(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut r4: u32 = 0;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r4 = r1;
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        r0 = 175u32;
        r1 = r4;
        r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f2fe44();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09fb8(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    let mut r1: u32 = request;
    let mut equal = false;
    let mut lower_or_same = false;
    unsafe {
        r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
        equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
        if !equal { return r0 as i32; }
        xm1_power_record_helper_f30f64();
        r0 = 2u32;
        return r0 as i32;
    }
}

unsafe fn handler_f09fd8(event: u32, request: u32) -> i32 {
    enum Flow { B0, B1, B2 }
    let mut r0: u32 = event;
    let mut r1: u32 = request;
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
                    r0 = xm1_power_record_task43_event(13, r0, r1) as u32;
                    equal = r0 == 1u32; lower_or_same = r0 <= 1u32;
                    if !equal { return r0 as i32; }
                    r3 = 0xe004u32;
                    r3 = (r3 & 0xffff) | (0x16bu32 << 16);
                    r3 = read_volatile((r3.wrapping_add(52u32)) as *const u8) as u32;
                    equal = r3 == 2u32; lower_or_same = r3 <= 2u32;
                    if !equal { next = Flow::B2; } else { next = Flow::B1; }
                    continue;
                }
                Flow::B1 => {
                    xm1_power_record_helper_f32394();
                    r0 = 26u32;
                    r1 = r4;
                    r0 = xm1_power_record_task43_event(26, r0, r1) as u32;
                    r0 = 1u32;
                    xm1_power_record_helper_f323e4(r0);
                    next = Flow::B2;
                }
                Flow::B2 => {
                    xm1_power_record_helper_f2fe44();
                    r0 = 2u32;
                    return r0 as i32;
                }
            }
        }
    }
}

unsafe fn handler_f0a028(event: u32, request: u32) -> i32 {
    let mut r0: u32 = event;
    unsafe {
        r0 = 3u32;
        return r0 as i32;
    }
}
