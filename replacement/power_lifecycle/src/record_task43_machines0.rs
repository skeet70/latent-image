// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// NONMATCHING bodies and omitted handlers are confirmed at f01c48..f02fb4.
extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f14668();
    fn xm1_power_record_helper_f147bc(resource: u32, event: u32);
    fn xm1_power_record_helper_f14824(resource: u32);
    fn xm1_power_record_helper_f146c0();
    fn xm1_power_record_helper_f146d4(resource: u32);
    fn xm1_power_record_helper_f14704();
    fn xm1_power_record_helper_f14990(resource: u32);
    fn xm1_power_record_helper_f14964(resource: u32);
    fn xm1_power_record_helper_f149e8(bank: u32, kind: u32, event: u32) -> u32;
    fn xm1_power_record_helper_f14aac(bank: u32);
    fn xm1_power_record_helper_f1473c();
    fn xm1_power_record_helper_f14718();
    fn xm1_power_record_helper_f14760() -> u32;
    fn xm1_power_record_helper_f148fc();
    fn xm1_power_record_helper_f14944() -> u32;
    fn xm1_power_record_helper_f14920();
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f46c4c(status: u32);
    fn xm1_power_record_helper_f46ea8(resource: u32);
    fn xm1_power_record_helper_f46f4c(resource: u32);
    fn xm1_power_record_helper_f46f28(resource: u32);
    fn xm1_power_record_helper_f47234(resource: u32);
    fn xm1_power_record_helper_f470d0(resource: u32, value: u32);
    fn xm1_power_record_helper_f47330(resource: u32) -> i32;
    fn xm1_power_record_helper_74e5c0(resource: u32) -> i32;
    fn xm1_power_record_helper_75ace4(bitmap: u32, resource: u32, low: u32, high: u32) -> i32;
    fn xm1_power_record_helper_759250(resource: u32, source: u32);
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Bank { Root = 0, Primary = 1, Secondary = 2, Tertiary = 3 }
#[derive(Clone, Copy)]
#[repr(u32)]
enum Mapping { Start = 3, Resume = 4 }

unsafe fn resource(bank: Bank, kind: Mapping, event: u32) -> u32 {
    unsafe { xm1_power_record_helper_f149e8(bank as u32, kind as u32, event) }
}
unsafe fn release(bank: u32) { unsafe { xm1_power_record_helper_f14aac(bank); } }
unsafe fn route(machine: u32, event: u32, resource: u32) -> i32 {
    unsafe { xm1_power_record_task43_event(machine, event, resource) }
}
unsafe fn state(value: u16) { unsafe { core::ptr::write_volatile(0x0169d4b4 as *mut u16, value); } }
unsafe fn word(address: usize) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
unsafe fn failure() -> i32 { unsafe { xm1_power_record_helper_f46c20(); } 0 }
unsafe fn accepted(result: i32) -> i32 { if result == 1 || result == 2 { 2 } else { unsafe { failure() } } }
unsafe fn mapped(bank: Bank, machine: u32, event: u32) -> i32 {
    unsafe { route(machine, event, resource(bank, Mapping::Resume, event)) }
}

unsafe fn combined(event: u32, full: bool) -> i32 {
    unsafe {
        let first = mapped(Bank::Primary, 2, event);
        let mut flags = match first {
            1 => { release(1); 1 },
            2 => 0,
            3 => 4,
            _ => return failure(),
        };
        match mapped(Bank::Secondary, 11, event) {
            1 => { if full { flags |= 2; } }
            2 => (),
            3 => flags |= 8,
            _ => return failure(),
        }
        if flags == 1 || flags == 9 { state(if full { 6 } else { 4 }); }
        else if full && (flags == 2 || flags == 6) { state(2); }
        else if full && flags == 3 { state(4); }
        2
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine0(handler_index: u32, event: u32, _request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 | 3 => {
                xm1_power_record_helper_f14668();
                let selected = resource(Bank::Primary, Mapping::Start, event);
                xm1_power_record_helper_f147bc(selected, event);
                xm1_power_record_helper_f14824(selected);
                xm1_power_record_helper_f146c0();
                release(1);
                release(6);
                if handler_index == 2 {
                    state(if word(0x00954dcc) == 2 { 4 } else { 1 });
                    core::ptr::write_volatile(0x00954dcc as *mut u32, 0);
                }
                2
            }
            4 => failure(),
            5 => {
                state(0);
                xm1_power_record_helper_f146d4(resource(Bank::Primary, Mapping::Resume, event));
                xm1_power_record_helper_f14704();
                for bank in [1, 6, 2, 7, 3, 8] { release(bank); }
                2
            }
            6 | 7 | 8 | 9 => {
                if handler_index == 7 || handler_index == 9 {
                    if mapped(Bank::Secondary, 11, event) == 0 { return failure(); }
                }
                let selected = resource(Bank::Primary, Mapping::Resume, event);
                if handler_index == 6 { xm1_power_record_helper_f14990(selected); }
                match route(2, event, selected) {
                    1 => {
                        state(if handler_index <= 7 { 4 } else { 1 });
                        if handler_index == 9 { release(2); }
                        release(1);
                        2
                    }
                    2 => {
                        state(if handler_index <= 7 { 2 } else { 3 });
                        if handler_index == 9 { release(2); }
                        2
                    }
                    _ => failure(),
                }
            }
            10 | 11 | 14 | 25 => {
                match mapped(Bank::Secondary, 11, event) {
                    1 => 2,
                    2 => { state(match handler_index { 10 => 5, 11 => 6, 14 => 7, _ => 8 }); 2 }
                    _ => failure(),
                }
            }
            12 | 19 => {
                let selected = resource(Bank::Tertiary, Mapping::Resume, event);
                if handler_index == 19 { xm1_power_record_helper_f46ea8(selected); }
                accepted(route(3, event, selected))
            }
            13 => {
                let result = mapped(Bank::Secondary, 11, event);
                if result == 1 { release(2); }
                accepted(result)
            }
            15 | 22 => { mapped(Bank::Tertiary, 3, event); 2 }
            16 => {
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                xm1_power_record_helper_759250(selected, 0x0169d130);
                xm1_power_record_helper_f46ea8(selected);
                xm1_power_record_helper_f46c4c(0);
                release(2);
                2
            }
            17 | 20 => {
                let selected = resource(Bank::Secondary, if handler_index == 17 { Mapping::Start } else { Mapping::Resume }, event);
                if handler_index == 17 { xm1_power_record_helper_f46ea8(selected); }
                match route(11, event, selected) {
                    1 => { release(7); 2 }
                    2 => { state(7); 2 }
                    _ => failure(),
                }
            }
            18 => {
                if xm1_power_record_helper_75ace4(0x0169d388, event, 0x200000, 0) == 0
                    && ((word(0x0169d408) | word(0x0169d40c)) != 0 || (word(0x0169d410) | word(0x0169d414)) != 0) {
                    return failure();
                }
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                xm1_power_record_helper_f46ea8(selected);
                route(11, event, selected);
                2
            }
            21 => {
                mapped(Bank::Tertiary, 3, event);
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                match route(11, 0xab, selected) {
                    1 => { state(10); route(3, 0xe9, selected); 2 }
                    2 => { state(9); 2 }
                    _ => failure(),
                }
            }
            23 => { xm1_power_record_helper_f1473c(); mapped(Bank::Secondary, 11, event); 2 }
            24 => {
                xm1_power_record_helper_f14718();
                let selected = resource(Bank::Tertiary, Mapping::Resume, event);
                route(3, 0xea, selected);
                2
            }
            26 | 36 | 38 | 42 | 43 | 44 => {
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                match route(11, event, selected) {
                    1 => {
                        state(match handler_index { 38 => 1, 44 => 10, _ => 4 });
                        if handler_index == 42 && xm1_power_record_helper_f14944() != 0 {
                            xm1_power_record_helper_f14920();
                            route(12, 0x2a, selected);
                        }
                        if handler_index == 43 { release(7); }
                        if handler_index == 44 {
                            let selected = resource(Bank::Tertiary, Mapping::Resume, event);
                            route(3, 0xe9, selected);
                        }
                        2
                    }
                    2 => 2,
                    _ => failure(),
                }
            }
            27 => {
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                let result = route(3, 0x108, selected);
                let classified = xm1_power_record_helper_f47330(selected);
                let result = if result == 2 && classified == 1 { route(3, 0xa7, selected) }
                    else if xm1_power_record_helper_74e5c0(selected) == 1 {
                        match route(3, 0x108, selected) {
                            1 => route(11, 0xe, selected),
                            2 => route(11, 0xa7, selected),
                            _ => return failure(),
                        }
                    } else { route(11, 0xa7, selected) };
                accepted(result)
            }
            28 => {
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                let result = route(3, 0x108, selected);
                let classified = xm1_power_record_helper_f47330(selected);
                accepted(route(if result == 2 && classified == 1 { 3 } else { 11 }, event, selected))
            }
            29 => accepted(mapped(Bank::Secondary, 11, event)),
            30 | 31 => {
                if word(0x0169d384) != 1 { return failure(); }
                mapped(if handler_index == 30 { Bank::Secondary } else { Bank::Tertiary }, 11, event);
                2
            }
            32 | 33 => {
                let selected = resource(Bank::Root, Mapping::Resume, event);
                if handler_index == 32 { xm1_power_record_helper_f46f4c(selected); }
                else { xm1_power_record_helper_f46f28(selected); }
                release(0);
                1
            }
            34 | 35 => {
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                if handler_index == 34 { xm1_power_record_helper_f47234(selected); }
                else { xm1_power_record_helper_f470d0(selected, 0); }
                1
            }
            37 => {
                xm1_power_record_helper_f148fc();
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                xm1_power_record_helper_f14964(selected);
                match route(11, 0xab, selected) {
                    1 => { route(12, event, selected); 2 }
                    2 => { state(6); 2 }
                    _ => failure(),
                }
            }
            39 | 41 => combined(event, handler_index == 41),
            40 => {
                if mapped(Bank::Primary, 2, event) == 1 { state(1); release(1); }
                else { state(3); }
                2
            }
            45 => {
                let selected = resource(Bank::Secondary, Mapping::Resume, event);
                let result = route(3, 0x108, selected);
                let classified = xm1_power_record_helper_f47330(selected);
                let result = if result == 2 && classified == 1 { route(3, event, selected) }
                    else if matches!(event, 0x33 | 0x34 | 0x43 | 0x3d | 0x44 | 0x82 | 0x37 | 0x77 | 0x8b | 0x47 | 0x66 | 0x39) {
                        mapped(Bank::Secondary, 11, event);
                        return 2;
                    } else { mapped(Bank::Tertiary, 3, event) };
                if result == 1 {
                    state(4);
                    if xm1_power_record_helper_f14760() != 0 {
                        xm1_power_record_helper_f1473c();
                        release(3);
                        let selected = resource(Bank::Secondary, Mapping::Resume, event);
                        route(11, 0xb, selected);
                    } else { release(3); }
                    2
                } else { accepted(result) }
            }
            _ => 0,
        }
    }
}
