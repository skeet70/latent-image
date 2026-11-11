use core::ptr::write_volatile;

extern "C" {
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f37cbc();
    fn xm1_power_record_helper_f37e90();
    fn xm1_power_record_helper_f37f40();
    fn xm1_power_record_helper_f38048();
    fn xm1_power_record_helper_f3857c();
    fn xm1_power_record_helper_f38110() -> i32;
    fn xm1_power_record_helper_f46b44(mode: u32) -> i32;
    fn xm1_power_record_helper_f46b5c();
    fn xm1_power_record_helper_f46b9c();
    fn xm1_power_record_helper_f46bb0() -> i32;
    fn xm1_power_record_helper_f46bc8(mode: u32) -> i32;
    fn xm1_power_record_helper_f3d490() -> i32;
    fn xm1_power_record_helper_f2e754(request: u32);
    fn xm1_power_record_helper_f2e774();
    fn xm1_power_record_helper_f2e7bc() -> i32;
    fn xm1_power_record_helper_f2ed50() -> i32;
    fn xm1_power_record_helper_f2edac();
    fn xm1_power_record_helper_f2ea60() -> i32;
    fn xm1_power_record_helper_f2ebb0();
    fn xm1_power_record_helper_f2ec94() -> i32;
    fn xm1_power_record_helper_f2ecfc();
    fn xm1_power_record_helper_f2ee18() -> i32;
    fn xm1_power_record_helper_f2ee94();
    fn xm1_power_record_helper_f2ed74();
    fn xm1_power_record_helper_f2ea8c();
    fn xm1_power_record_helper_f2ecb8();
    fn xm1_power_record_helper_f2ee50();
    fn xm1_power_record_helper_f2edc0();
    fn xm1_power_record_helper_f2ea4c();
    fn xm1_power_record_helper_f2ebc4();
    fn xm1_power_record_helper_f2ed10();
    fn xm1_power_record_helper_f2eea8();
}
unsafe fn state(machine: u32, value: u16) {
    unsafe { write_volatile((0x169d4b4 + machine * 2) as *mut u16, value); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine14(handler_index: u32, _: u32, _: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { xm1_power_record_helper_f37cbc(); 1 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => { xm1_power_record_helper_f37cbc(); xm1_power_record_helper_f37e90(); 1 }
            5 => 1,
            6 => { xm1_power_record_helper_f37f40(); 1 }
            7 => 2,
            8 => 12,
            9 => { xm1_power_record_helper_f38048(); xm1_power_record_helper_f3857c(); 2 }
            10 => { state(14, 1); if xm1_power_record_helper_f38110() == 1 { 5 } else { 12 } }
            11 => { xm1_power_record_helper_f38110(); xm1_power_record_helper_f37f40(); 1 }
            12 => 3,
            _ => 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine15(handler_index: u32, _: u32, _: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                let ready = xm1_power_record_helper_f46b44(0) == 1;
                xm1_power_record_helper_f46b5c();
                state(15, if ready { 1 } else { 3 });
                20
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => 1,
            5 => {
                if xm1_power_record_helper_f46b44(1) != 1 { state(15, 3); return 20; }
                xm1_power_record_helper_f46b9c();
                let result = xm1_power_record_helper_f46bb0();
                xm1_power_record_helper_f46b5c();
                if xm1_power_record_helper_f3d490() == 1 { state(15, 0); return 13; }
                if result == 14 { state(15, 0); return 14; }
                20
            }
            6 | 7 => {
                let mode = handler_index - 4;
                if xm1_power_record_helper_f46b44(mode) == 1 { state(15, 1); return 20; }
                let next = xm1_power_record_helper_f46bc8(mode);
                let result = xm1_power_record_helper_f46bb0();
                xm1_power_record_helper_f46b5c();
                if xm1_power_record_helper_f3d490() == 1 { state(15, 0); return 13; }
                if result == 14 { state(15, 0); return 14; }
                if next == if mode == 2 { 3 } else { 2 } { state(15, next as u16); }
                20
            }
            8 => 3,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy)]
enum Direction { Primary, Alternate }
#[derive(Clone, Copy)]
enum Stage { Geometry, Output, Final }
unsafe fn begin_stage(stage: Stage, direction: Direction) -> bool {
    unsafe {
        match stage {
            Stage::Geometry => {
                if xm1_power_record_helper_f2ea60() == 0 { return false; }
                match direction { Direction::Primary => xm1_power_record_helper_f2ea8c(), Direction::Alternate => xm1_power_record_helper_f2ebb0() }
            }
            Stage::Output => {
                if xm1_power_record_helper_f2ec94() == 0 { return false; }
                match direction { Direction::Primary => xm1_power_record_helper_f2ecb8(), Direction::Alternate => xm1_power_record_helper_f2ecfc() }
            }
            Stage::Final => {
                if xm1_power_record_helper_f2ee18() == 0 { return false; }
                match direction { Direction::Primary => xm1_power_record_helper_f2ee50(), Direction::Alternate => xm1_power_record_helper_f2ee94() }
            }
        }
        true
    }
}
unsafe fn resume(stages: &[Stage], direction: Direction) -> i32 {
    for &stage in stages { if unsafe { begin_stage(stage, direction) } { return 2; } }
    unsafe { state(16, 0); }
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine16(handler_index: u32, _: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => {
                xm1_power_record_helper_f2e754(request);
                xm1_power_record_helper_f2e774();
                let direction = if xm1_power_record_helper_f2e7bc() == 0 { Direction::Alternate } else { Direction::Primary };
                if xm1_power_record_helper_f2ed50() != 0 {
                    state(16, match direction { Direction::Primary => 1, Direction::Alternate => 2 });
                    match direction { Direction::Primary => xm1_power_record_helper_f2ed74(), Direction::Alternate => xm1_power_record_helper_f2edac() }
                    return 2;
                }
                if xm1_power_record_helper_f2ea60() != 0 {
                    state(16, match direction { Direction::Primary => 1, Direction::Alternate => 2 });
                    match direction { Direction::Primary => xm1_power_record_helper_f2ea8c(), Direction::Alternate => xm1_power_record_helper_f2ebb0() }
                    return 2;
                }
                if xm1_power_record_helper_f2ec94() != 0 {
                    state(16, match direction { Direction::Primary => 1, Direction::Alternate => 2 });
                    match direction { Direction::Primary => xm1_power_record_helper_f2ecb8(), Direction::Alternate => xm1_power_record_helper_f2ecfc() }
                    return 2;
                }
                if xm1_power_record_helper_f2ee18() != 0 {
                    state(16, match direction { Direction::Primary => 1, Direction::Alternate => 2 });
                    match direction { Direction::Primary => xm1_power_record_helper_f2ee50(), Direction::Alternate => xm1_power_record_helper_f2ee94() }
                    return 2;
                }
                1
            }
            3 => 2,
            4 => { xm1_power_record_helper_f46c20(); 0 }
            5 | 6 => {
                xm1_power_record_helper_f2edc0();
                resume(&[Stage::Geometry, Stage::Output, Stage::Final], if handler_index == 5 { Direction::Primary } else { Direction::Alternate })
            }
            7 => { xm1_power_record_helper_f2ea4c(); 2 }
            8 | 9 => {
                xm1_power_record_helper_f2ebc4();
                resume(&[Stage::Output, Stage::Final], if handler_index == 8 { Direction::Primary } else { Direction::Alternate })
            }
            10 | 11 => {
                xm1_power_record_helper_f2ed10();
                resume(&[Stage::Final], if handler_index == 10 { Direction::Primary } else { Direction::Alternate })
            }
            12 => { xm1_power_record_helper_f2eea8(); state(16, 0); 1 }
            13 => 3,
            _ => 0,
        }
    }
}
