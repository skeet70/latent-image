//! Source authority: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Handler slots and NONMATCHING branches are confirmed against image5 bytes at f0edb8..f10808.

use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(u32)]
enum Machine {
    Channel = 28,
    Values = 30,
    Primary = 32,
    Pair = 33,
    Output = 34,
    Route = 35,
    Runtime = 36,
}

#[derive(Clone, Copy)]
struct State(u16);

unsafe fn state(machine: Machine, value: State) {
    write_volatile((0x0169_d4b4 + 2 * machine as u32) as *mut u16, value.0);
}

extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f20628(request: u32) -> i32;
    fn xm1_power_record_helper_f207f8();
    fn xm1_power_record_helper_f208d8();
    fn xm1_power_record_helper_f2095c();
    fn xm1_power_record_helper_f2096c();
    fn xm1_power_record_helper_f20980();
    fn xm1_power_record_helper_f20994();
    fn xm1_power_record_helper_f209a8() -> u32;
    fn xm1_power_record_helper_f209e4();
    fn xm1_power_record_helper_f20a88();
    fn xm1_power_record_helper_f20b38();
    fn xm1_power_record_helper_f20b60();
    fn xm1_power_record_helper_f20ea0();
    fn xm1_power_record_helper_f20ec0();
    fn xm1_power_record_helper_f21074();
    fn xm1_power_record_helper_f212e8();
    fn xm1_power_record_helper_f21304();
    fn xm1_power_record_helper_f2131c();
    fn xm1_power_record_helper_f21378();
    fn xm1_power_record_helper_f213d4() -> u32;
    fn xm1_power_record_helper_f216fc() -> u32;
    fn xm1_power_record_helper_f21744();
    fn xm1_power_record_helper_f21a5c(value: u32);
    fn xm1_power_record_helper_f21ae4();
    fn xm1_power_record_helper_f21b20() -> u32;
    fn xm1_power_record_helper_f21b40() -> u32;
    fn xm1_power_record_helper_f21b60(value: u32);
    fn xm1_power_record_helper_f21b80() -> u32;
    fn xm1_power_record_helper_f21ba0(value: u32);
    fn xm1_power_record_helper_f21bc0() -> u32;
    fn xm1_power_record_helper_f21be0(value: u32);
    fn xm1_power_record_helper_f21c00() -> u32;
    fn xm1_power_record_helper_f21c20() -> u32;
    fn xm1_power_record_helper_f21c40(value: u32);
    fn xm1_power_record_helper_f21c60() -> u32;
    fn xm1_power_record_helper_f21c80() -> u32;
    fn xm1_power_record_helper_f21cc8(request: u32, mode: u32);
    fn xm1_power_record_helper_f21de4(result: u32) -> u32;
    fn xm1_power_record_helper_f21e08() -> u32;
    fn xm1_power_record_helper_f21e18();
    fn xm1_power_record_helper_f21e40();
    fn xm1_power_record_helper_f22150();
    fn xm1_power_record_helper_f2216c() -> u32;
    fn xm1_power_record_helper_f2218c();
    fn xm1_power_record_helper_f221d4();
    fn xm1_power_record_helper_f221f8();
    fn xm1_power_record_helper_f22224();
    fn xm1_power_record_helper_f22238();
    fn xm1_power_record_helper_f2225c();
    fn xm1_power_record_helper_f22298() -> u32;
    fn xm1_power_record_helper_f222c4() -> u32;
    fn xm1_power_record_helper_f2230c();
    fn xm1_power_record_helper_f2267c() -> u32;
    fn xm1_power_record_helper_f226a4() -> u32;

    fn xm1_power_record_helper_f387c0();
    fn xm1_power_record_helper_f38d54();
    fn xm1_power_record_helper_f38d78();
    fn xm1_power_record_helper_f38de0();
    fn xm1_power_record_helper_f38e0c() -> i32;
    fn xm1_power_record_helper_f39748();
    fn xm1_power_record_helper_f39784() -> i32;
    fn xm1_power_record_helper_f3a440();
    fn xm1_power_record_helper_f3a464();
    fn xm1_power_record_helper_f3a48c();
    fn xm1_power_record_helper_f3a4c0();
    fn xm1_power_record_helper_f3a564();
    fn xm1_power_record_helper_f397e8(request: u32);
    fn xm1_power_record_helper_f3a748(request: u32);

    fn xm1_power_record_helper_f2421c(request: u32);
    fn xm1_power_record_helper_f242ec(request: u32);
    fn xm1_power_record_helper_f2457c(request: u32);
    fn xm1_power_record_helper_f2481c(request: u32) -> i32;
    fn xm1_power_record_helper_f24bd0(request: u32);
    fn xm1_power_record_helper_f25334(request: u32);
    fn xm1_power_record_helper_f254bc(request: u32);
    fn xm1_power_record_helper_f255ac(request: u32);
    fn xm1_power_record_helper_f25784();
    fn xm1_power_record_helper_f257e4() -> i32;
    fn xm1_power_record_helper_f2580c(request: u32);
    fn xm1_power_record_helper_f25860();
    fn xm1_power_record_helper_f258e0();
    fn xm1_power_record_helper_f259b8();
    fn xm1_power_record_helper_f25afc() -> i32;
    fn xm1_power_record_helper_f25b3c() -> i32;
    fn xm1_power_record_helper_f25b54();
    fn xm1_power_record_helper_f25bb0();
    fn xm1_power_record_helper_f25be0(enabled: u32);
    fn xm1_power_record_helper_f267b0(request: u32) -> i32;
    fn xm1_power_record_helper_f267f8(request: u32);
    fn xm1_power_record_helper_f26a60(request: u32) -> i32;
    fn xm1_power_record_helper_f26fbc(request: u32);
    fn xm1_power_record_helper_f26ff0(value: u32);
    fn xm1_power_record_helper_f2711c(request: u32);
    fn xm1_power_record_helper_f2751c(request: u32) -> i32;
    fn xm1_power_record_helper_f27fcc(request: u32) -> i32;
    fn xm1_power_record_helper_f287e8(request: u32) -> i32;
    fn xm1_power_record_helper_f28edc(request: u32, mode: u32) -> i32;
    fn xm1_power_record_helper_f28f4c(status: i32);
    fn xm1_power_record_helper_f28fac(request: u32) -> i32;
    fn xm1_power_record_helper_f28fd0(value: u32);
    fn xm1_power_record_helper_f28fe0() -> i32;
    fn xm1_power_record_helper_f28ff0(value: u32);
    fn xm1_power_record_helper_f29000();

    fn xm1_power_record_helper_f19634();
    fn xm1_power_record_helper_f1e468();
    fn xm1_power_record_helper_f1fd60(enabled: u32);
    fn xm1_power_record_helper_f2ef7c(request: u32);
    fn xm1_power_record_helper_f2ef9c();
    fn xm1_power_record_helper_f2efe8();
    fn xm1_power_record_helper_f2f4a0() -> i32;
    fn xm1_power_record_helper_f2f0f4();
    fn xm1_power_record_helper_f2f054();
    fn xm1_power_record_helper_f2f278() -> i32;
    fn xm1_power_record_helper_f2f2c0();
    fn xm1_power_record_helper_f2f314();
    fn xm1_power_record_helper_f2f3a0();
}

unsafe fn channel_finish(request: u32, waiting: bool) -> i32 {
    let result = xm1_power_record_task43_event(6, if waiting { 0xf2 } else { 0xf3 }, request);
    if waiting {
        xm1_power_record_helper_f22224();
    }
    result
}

unsafe fn channel_apply(request: u32, prepare: bool, mode: u32) {
    if prepare {
        xm1_power_record_helper_f221d4();
    }
    xm1_power_record_helper_f21cc8(request, mode);
    xm1_power_record_helper_f2131c();
}

unsafe fn channel_reselect(request: u32) {
    xm1_power_record_helper_f221f8();
    channel_apply(request, false, 0);
    xm1_power_record_helper_f212e8();
}

unsafe fn channel_begin() {
    xm1_power_record_helper_f207f8();
    xm1_power_record_helper_f209e4();
    xm1_power_record_helper_f2230c();
    xm1_power_record_helper_f208d8();
    xm1_power_record_helper_f22238();
    xm1_power_record_helper_f21ae4();
}

unsafe fn channel_secondary_idle() -> bool {
    read_volatile(0x016f_1028 as *const u32) == 0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine28(handler_index: u32, event: u32, request: u32) -> i32 {
    match handler_index {
        2 => {
            xm1_power_record_helper_f20628(request);
            if xm1_power_record_helper_f222c4() != 0 {
                state(Machine::Channel, State(if channel_secondary_idle() { 17 } else { 15 }));
                return channel_finish(request, true);
            }
            if xm1_power_record_helper_f21c80() == 0 {
                let waiting = xm1_power_record_helper_f22298() == 0;
                if waiting { state(Machine::Channel, State(17)); }
                channel_apply(request, false, 1);
                return channel_finish(request, waiting);
            }
            if xm1_power_record_helper_f2216c() != 0 {
                let waiting = xm1_power_record_helper_f22298() == 0;
                if waiting { state(Machine::Channel, State(17)); }
                channel_apply(request, true, 0);
                return channel_finish(request, waiting);
            }
            if xm1_power_record_helper_f21e08() != 0 {
                state(Machine::Channel, State(1));
                xm1_power_record_helper_f20a88();
            } else {
                state(Machine::Channel, State(3));
                channel_begin();
            }
        }
        3 => { xm1_power_record_helper_f46c20(); return 0; }
        4 => { xm1_power_record_helper_f2218c(); }
        5 => {
            if xm1_power_record_helper_f22298() != 0 {
                xm1_power_record_helper_f21cc8(request, 0);
            }
        }
        6 => { xm1_power_record_helper_f20b38(); }
        7 => {}
        8 => {
            if xm1_power_record_helper_f21b40() != 0 {
                xm1_power_record_helper_f21ba0(1);
                xm1_power_record_helper_f20b60();
            }
        }
        9 => {
            let ready = if xm1_power_record_helper_f21c20() != 0 { true } else {
                let status = xm1_power_record_helper_f209a8();
                xm1_power_record_helper_f21c40(u32::from(status == 0));
                xm1_power_record_helper_f21c60() != 0
            };
            if ready {
                state(Machine::Channel, State(9));
                xm1_power_record_helper_f20b38();
                xm1_power_record_helper_f20980();
            } else {
                state(Machine::Channel, State(5));
                xm1_power_record_helper_f20ec0();
            }
        }
        10 => {
            xm1_power_record_helper_f20b38();
            xm1_power_record_helper_f2096c();
        }
        11 => {
            if xm1_power_record_helper_f213d4() != 0 {
                state(Machine::Channel, State(11));
                xm1_power_record_helper_f20b38();
            } else {
                state(Machine::Channel, State(4));
                xm1_power_record_helper_f21e40();
            }
        }
        12 | 13 => {
            if xm1_power_record_helper_f21b20() != 0 {
                state(Machine::Channel, State(if handler_index == 12 { 7 } else { 9 }));
            } else {
                state(Machine::Channel, State(13));
                channel_reselect(request);
            }
        }
        14 => { xm1_power_record_helper_f21074(); }
        15 => {
            let status = xm1_power_record_helper_f226a4();
            if status == 0 {
                state(Machine::Channel, State(13));
                xm1_power_record_helper_f21a5c(status as u32);
                channel_reselect(request);
                xm1_power_record_helper_f20994();
            } else {
                state(Machine::Channel, State(12));
                xm1_power_record_helper_f21a5c(0);
                xm1_power_record_helper_f21074();
            }
        }
        16 => {
            let status = xm1_power_record_helper_f226a4();
            if status == 0 {
                state(Machine::Channel, State(10));
                xm1_power_record_helper_f21a5c(status as u32);
                xm1_power_record_helper_f20994();
            } else {
                state(Machine::Channel, State(12));
                xm1_power_record_helper_f21a5c(0);
            }
        }
        17 => {
            xm1_power_record_helper_f21a5c(0);
            channel_reselect(request);
        }
        18 => { xm1_power_record_helper_f21a5c(0); }
        19 => {
            xm1_power_record_helper_f2230c();
            if xm1_power_record_helper_f216fc() != 0 {
                state(Machine::Channel, State(4));
                xm1_power_record_helper_f2225c();
                xm1_power_record_helper_f2095c();
                xm1_power_record_helper_f208d8();
                xm1_power_record_helper_f22238();
                xm1_power_record_helper_f21ae4();
            } else {
                state(Machine::Channel, State(13));
                xm1_power_record_helper_f21744();
                xm1_power_record_helper_f2131c();
                xm1_power_record_helper_f212e8();
            }
        }
        20 | 21 | 22 => {
            if handler_index == 21 { state(Machine::Channel, State(0)); }
            xm1_power_record_helper_f21ba0(0);
            xm1_power_record_helper_f21b60(1);
            if handler_index == 21 { return channel_finish(request, false); }
            if handler_index == 22 { xm1_power_record_helper_f20ea0(); }
        }
        23 => {
            xm1_power_record_helper_f21b60(0);
            xm1_power_record_helper_f21304();
        }
        24 => {
            xm1_power_record_helper_f2225c();
            xm1_power_record_helper_f2095c();
            if xm1_power_record_helper_f22298() == 0 {
                state(Machine::Channel, State(15));
                return channel_finish(request, true);
            }
            if xm1_power_record_helper_f21bc0() == 0 {
                state(Machine::Channel, State(0));
                return channel_finish(request, false);
            }
            state(Machine::Channel, State(14));
        }
        25 => {
            if xm1_power_record_helper_f21c00() != 0 {
                state(Machine::Channel, State(0));
                xm1_power_record_helper_f21378();
                return 1;
            }
            state(Machine::Channel, State(17));
        }
        26 | 27 => {
            if handler_index == 26 && xm1_power_record_helper_f2267c() != 0 {
                state(Machine::Channel, State(2));
                xm1_power_record_helper_f21e18();
            } else {
                channel_apply(request, true, 0);
                let waiting = xm1_power_record_helper_f22298() == 0;
                state(Machine::Channel, State(if waiting { 17 } else { 0 }));
                return channel_finish(request, waiting);
            }
        }
        28 => {
            xm1_power_record_helper_f22150();
            channel_begin();
        }
        29 => return 3,
        30 => {
            let result = xm1_power_record_task43_event(6, event, request);
            if result == 1 { xm1_power_record_helper_f21be0(1); }
            if xm1_power_record_helper_f21de4(result as u32) != 0 {
                state(Machine::Channel, State(16));
                if xm1_power_record_helper_f21b80() != 0 {
                    xm1_power_record_helper_f20ea0();
                } else if xm1_power_record_helper_f21bc0() == 0 {
                    xm1_power_record_helper_f21304();
                }
            }
        }
        31 => {
            if xm1_power_record_task43_event(6, event, request) == 1 {
                xm1_power_record_helper_f21be0(1);
            }
        }
        32 => {
            if xm1_power_record_task43_event(6, event, request) == 1 {
                state(Machine::Channel, State(0));
                xm1_power_record_helper_f21378();
                return 1;
            }
        }
        _ => return -1,
    }
    2
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine29(handler_index: u32, _event: u32, _request: u32) -> i32 {
    match handler_index {
        2 => { xm1_power_record_helper_f3a440(); 1 }
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 => { xm1_power_record_helper_f3a440(); xm1_power_record_helper_f3a464(); 1 }
        5 => 1,
        6 => { xm1_power_record_helper_f3a48c(); 1 }
        7 => 2,
        8 => 12,
        9 => { xm1_power_record_helper_f3a564(); 2 }
        10 => { xm1_power_record_helper_f3a4c0(); 12 }
        11 => { xm1_power_record_helper_f3a4c0(); xm1_power_record_helper_f3a48c(); 1 }
        12 => 3,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine30(handler_index: u32, _event: u32, _request: u32) -> i32 {
    match handler_index {
        2 => { xm1_power_record_helper_f387c0(); 1 }
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 => { xm1_power_record_helper_f387c0(); xm1_power_record_helper_f38d54(); 1 }
        5 => 1,
        6 => { xm1_power_record_helper_f38d78(); 1 }
        7 => 2,
        8 => 12,
        9 => {
            if xm1_power_record_helper_f39784() == 0 { return 12; }
            state(Machine::Values, State(2));
            xm1_power_record_helper_f38de0();
            xm1_power_record_helper_f39748();
            2
        }
        10 => {
            state(Machine::Values, State(1));
            if xm1_power_record_helper_f38e0c() == 1 { 7 } else { 12 }
        }
        11 => { xm1_power_record_helper_f38e0c(); xm1_power_record_helper_f38d78(); 1 }
        12 => 3,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine31(handler_index: u32, _event: u32, request: u32) -> i32 {
    match handler_index {
        2 => { xm1_power_record_helper_f2421c(request); 1 }
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 => 1,
        5 => { xm1_power_record_helper_f25334(request); 1 }
        6 => 2,
        7 => { xm1_power_record_helper_f242ec(request); 2 }
        8 => { xm1_power_record_helper_f2457c(request); 2 }
        9 => { xm1_power_record_helper_f2457c(request); xm1_power_record_helper_f24bd0(request); 2 }
        10 => { xm1_power_record_helper_f2457c(request); xm1_power_record_helper_f25334(request); 1 }
        11 | 12 => {
            xm1_power_record_helper_f255ac(request);
            xm1_power_record_helper_f254bc(request);
            if handler_index == 11 { xm1_power_record_helper_f2481c(request); 2 }
            else { xm1_power_record_helper_f25334(request); 1 }
        }
        13 => 3,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine32(handler_index: u32, _event: u32, _request: u32) -> i32 {
    match handler_index {
        2 | 5 | 8 | 15 => 1,
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 | 6 | 7 | 13 | 16 => 2,
        9 | 10 => { state(Machine::Primary, State(6)); 12 }
        11 | 14 | 17 => { state(Machine::Primary, State(0)); 1 }
        12 => 12,
        18 => 3,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine33(handler_index: u32, event: u32, request: u32) -> i32 {
    match handler_index {
        2 => {
            xm1_power_record_task43_event(23, event, request);
            xm1_power_record_task43_event(32, event, request);
            1
        }
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 => {
            state(Machine::Pair, State(1));
            xm1_power_record_task43_event(32, 0xca, request);
            xm1_power_record_task43_event(23, event, request);
            1
        }
        5 => 1,
        6 | 8 => {
            let result = xm1_power_record_task43_event(if handler_index == 6 { 23 } else { 32 }, event, request);
            state(Machine::Pair, State(if result == 1 { 0 } else if handler_index == 6 { 3 } else { 6 }));
            result
        }
        7 => 2,
        9 => 3,
        10 => xm1_power_record_task43_event(23, event, request),
        11 => {
            let result = xm1_power_record_task43_event(23, event, request);
            if result != 1 { return result; }
            state(Machine::Pair, State(4));
            xm1_power_record_task43_event(23, 0xca, request);
            xm1_power_record_task43_event(32, 0xcb, request);
            12
        }
        12 | 15 => {
            let result = xm1_power_record_task43_event(if handler_index == 12 { 23 } else { 32 }, event, request);
            if result == 1 { state(Machine::Pair, State(0)); }
            result
        }
        13 | 14 => {
            let result = xm1_power_record_task43_event(32, event, request);
            let result = if handler_index == 13 {
                xm1_power_record_task43_event(32, 0xcc, request)
            } else { result };
            if result != 1 {
                if handler_index == 13 { state(Machine::Pair, State(5)); }
                return result;
            }
            state(Machine::Pair, State(1));
            xm1_power_record_task43_event(32, 0xca, request);
            xm1_power_record_task43_event(23, 0xcb, request);
            12
        }
        _ => -1,
    }
}

unsafe fn output_enable() {
    xm1_power_record_helper_f259b8();
    xm1_power_record_helper_f25be0(1);
}

unsafe fn output_complete_selection() {
    if xm1_power_record_helper_f25b3c() == 1 {
        state(Machine::Output, State(14));
        xm1_power_record_helper_f26ff0(18);
        xm1_power_record_helper_f258e0();
        output_enable();
    } else {
        state(Machine::Output, State(13));
        xm1_power_record_helper_f26ff0(14);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine34(handler_index: u32, _event: u32, request: u32) -> i32 {
    match handler_index {
        2 => {
            let result = xm1_power_record_helper_f26a60(request);
            state(Machine::Output, State(match result { 16 => 1, 13 => 2, _ => 0 }));
            return 1;
        }
        3 => { xm1_power_record_helper_f46c20(); return 0; }
        4 => return 1,
        5 | 7 => {
            if xm1_power_record_helper_f25afc() == 1 {
                state(Machine::Output, State(if handler_index == 5 { 11 } else { 9 }));
                xm1_power_record_helper_f25be0(0);
            } else {
                state(Machine::Output, State(if handler_index == 5 { 10 } else { 8 }));
            }
        }
        6 => {}
        8 => { xm1_power_record_helper_f25784(); return 1; }
        9 => {
            if xm1_power_record_helper_f25afc() != 1 {
                state(Machine::Output, State(12));
            } else {
                output_complete_selection();
            }
        }
        10 => {
            if xm1_power_record_helper_f257e4() == 1 {
                xm1_power_record_helper_f2711c(request);
                xm1_power_record_helper_f25bb0();
            }
        }
        11 | 12 | 13 | 17 => {
            xm1_power_record_helper_f267f8(request);
            xm1_power_record_helper_f2580c(request);
            xm1_power_record_helper_f25860();
            let result = match handler_index {
                11 => xm1_power_record_helper_f2751c(request),
                12 => xm1_power_record_helper_f27fcc(request),
                _ => xm1_power_record_helper_f287e8(request),
            };
            xm1_power_record_helper_f26ff0(result as u32);
            if handler_index == 17 {
                state(Machine::Output, State(if result == 14 { 13 } else { 14 }));
                if result != 14 { xm1_power_record_helper_f258e0(); }
                output_enable();
            } else {
                let selected = match (handler_index, result) {
                    (11, 14) | (12, 14) => Some(State(6)),
                    (11, 16) | (13, 16) => Some(State(5)),
                    (12, 13) | (13, 13) => Some(State(4)),
                    (_, 18) => Some(State(7)),
                    _ => None,
                };
                if let Some(selected) = selected { state(Machine::Output, selected); }
                if result == 18 {
                    xm1_power_record_helper_f258e0();
                    output_enable();
                } else if selected.is_none() {
                    output_enable();
                }
            }
        }
        14 | 15 => {
            xm1_power_record_helper_f267f8(request);
            if xm1_power_record_helper_f25afc() == 1 {
                state(Machine::Output, State(if handler_index == 14 { 9 } else { 11 }));
                xm1_power_record_helper_f25be0(0);
            }
        }
        16 => {
            xm1_power_record_helper_f267f8(request);
            if xm1_power_record_helper_f25afc() == 1 { output_complete_selection(); }
        }
        18 => { xm1_power_record_helper_f25b54(); }
        19 => {
            xm1_power_record_helper_f25b54();
            if xm1_power_record_helper_f267b0(request) == 1 {
                state(Machine::Output, State(5));
            } else {
                xm1_power_record_helper_f258e0();
                output_enable();
            }
        }
        20 | 22 => {
            xm1_power_record_helper_f25b54();
            if xm1_power_record_helper_f25afc() == 1 {
                state(Machine::Output, State(if handler_index == 20 { 9 } else { 11 }));
                xm1_power_record_helper_f25be0(0);
            }
        }
        21 => { xm1_power_record_helper_f25b54(); return 1; }
        23 => {
            xm1_power_record_helper_f25b54();
            xm1_power_record_helper_f26fbc(request);
            return 1;
        }
        24 => {
            xm1_power_record_helper_f25b54();
            if xm1_power_record_helper_f25afc() == 1 { output_complete_selection(); }
        }
        25 => {
            xm1_power_record_helper_f25b54();
            if xm1_power_record_helper_f267b0(request) == 1 {
                state(Machine::Output, State(15));
                xm1_power_record_helper_f25b54();
                xm1_power_record_helper_f25be0(0);
            } else {
                xm1_power_record_helper_f258e0();
                output_enable();
            }
        }
        26 => return 3,
        _ => return -1,
    }
    2
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine35(handler_index: u32, _event: u32, request: u32) -> i32 {
    match handler_index {
        2 => {
            if xm1_power_record_helper_f28edc(request, 1) == 0 {
                state(Machine::Route, State(1));
                2
            } else {
                xm1_power_record_helper_f28f4c(-98);
                0
            }
        }
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 => { xm1_power_record_helper_f28fd0(20); 2 }
        5 => {
            let result = xm1_power_record_helper_f28fac(request);
            xm1_power_record_helper_f28fd0(20);
            if result == 0 { 2 } else { 0 }
        }
        6 => {
            xm1_power_record_helper_f28ff0(20);
            xm1_power_record_helper_f28f4c(0);
            1
        }
        7 => {
            let result = xm1_power_record_helper_f28fe0();
            xm1_power_record_helper_f28ff0(20);
            if result == 0 { 2 } else { 0 }
        }
        8 => { xm1_power_record_helper_f28f4c(0); 1 }
        9 => { xm1_power_record_helper_f29000(); 2 }
        10 => 3,
        _ => -1,
    }
}

unsafe fn runtime_continue(resumed: bool) -> i32 {
    xm1_power_record_helper_f2f054();
    if xm1_power_record_helper_f2f278() != 0 {
        state(Machine::Runtime, State(2));
        xm1_power_record_helper_f2f2c0();
        2
    } else {
        if resumed { state(Machine::Runtime, State(0)); }
        xm1_power_record_helper_f1fd60(0);
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine36(handler_index: u32, _event: u32, request: u32) -> i32 {
    match handler_index {
        2 => {
            xm1_power_record_helper_f2ef7c(request);
            xm1_power_record_helper_f2ef9c();
            xm1_power_record_helper_f1fd60(1);
            xm1_power_record_helper_f2efe8();
            xm1_power_record_helper_f1e468();
            xm1_power_record_helper_f19634();
            if xm1_power_record_helper_f2f4a0() != 0 {
                state(Machine::Runtime, State(1));
                xm1_power_record_helper_f2f0f4();
                2
            } else { runtime_continue(false) }
        }
        3 => { xm1_power_record_helper_f46c20(); 0 }
        4 => {
            xm1_power_record_helper_f1e468();
            xm1_power_record_helper_f19634();
            runtime_continue(true)
        }
        5 => { xm1_power_record_helper_f2f314(); xm1_power_record_helper_f2f3a0(); 2 }
        6 => { xm1_power_record_helper_f1fd60(0); 1 }
        7 => 3,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_prepare29(_event: u32, request: u32) {
    xm1_power_record_helper_f3a748(request);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_prepare30(_event: u32, request: u32) {
    xm1_power_record_helper_f397e8(request);
}
