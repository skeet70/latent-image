// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// NONMATCHING and omitted handlers are confirmed at f02fb4..f0347c.
extern "C" {
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f14ccc(request: u32, kind: u32) -> i32;
    fn xm1_power_record_helper_f14d20(request: u32, kind: u32) -> i32;
    fn xm1_power_record_helper_f14d74(request: u32, kind: u32) -> i32;
    fn xm1_power_record_helper_f14dc8(request: u32, mode: u32) -> i32;
    fn xm1_power_record_helper_f14f2c(status: u32);
    fn xm1_power_record_helper_f14ecc(status: u32);
    fn xm1_power_record_helper_f14eac(status: u32);
    fn xm1_power_record_helper_f14c20();
    fn xm1_power_record_helper_f14e38(request: u32) -> i32;
    fn xm1_power_record_helper_f14ebc(status: u32);
    fn xm1_power_record_helper_f14e5c() -> i32;
    fn xm1_power_record_helper_f14e6c() -> i32;
    fn xm1_power_record_helper_f14fc0(request: u32) -> i32;
    fn xm1_power_record_helper_f14c8c(request: u32);
    fn xm1_power_record_helper_f15050() -> i32;
    fn xm1_power_record_helper_f15060(status: u32);
    fn xm1_power_record_helper_f14f7c(status: u32);
    fn xm1_power_record_helper_f15000(status: u32);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(request: u32) -> i32;
    fn xm1_power_record_helper_f47100(request: u32) -> i32;
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile(0x0169d4b6 as *mut u16, value) };
}

unsafe fn wait(result: i32, next: u16) -> i32 {
    if result == 0 { unsafe { state(next); } 2 } else { 0 }
}

unsafe fn alternate(event: u32, request: u32) -> bool {
    unsafe {
        let requested = xm1_power_record_helper_69f8fc(0x88ef) & 8 != 0
            || (xm1_power_record_helper_69f8fc(0x88ef) & 2 != 0 && xm1_power_record_helper_74e5c0(request) == 1)
            || (xm1_power_record_helper_69f8fc(0x88ef) & 4 != 0 && xm1_power_record_helper_74e5c0(request) == 0);
        !(requested && event != 0xa7 && xm1_power_record_helper_f47100(request) == 0)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine1(handler_index: u32, event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => wait(xm1_power_record_helper_f14ccc(request, 2), 2),
            3 | 6 => { xm1_power_record_helper_f46c20(); 0 }
            4 => wait(xm1_power_record_helper_f14ccc(request, 3), 3),
            5 => wait(xm1_power_record_helper_f14d20(request, 13), 4),
            7 => wait(xm1_power_record_helper_f14d74(request, 13), 8),
            8 => 2,
            9 => wait(xm1_power_record_helper_f14ccc(request, 13), 10),
            10 => {
                let mode = alternate(event, request);
                if xm1_power_record_helper_f14dc8(request, u32::from(mode)) == 0 {
                    state(11);
                    2
                } else {
                    if mode { xm1_power_record_helper_f14ecc((-98i32) as u32); }
                    else { xm1_power_record_helper_f14f2c((-98i32) as u32); }
                    0
                }
            }
            11 => { xm1_power_record_helper_f14eac(21); 1 }
            12 => { xm1_power_record_helper_f14c20(); xm1_power_record_helper_f14eac(20); 2 }
            13 => {
                let result = xm1_power_record_helper_f14e38(request);
                xm1_power_record_helper_f14eac(20);
                if result == 0 { 2 } else { 0 }
            }
            14 => { xm1_power_record_helper_f14ebc(21); 1 }
            15 => 1,
            16 => {
                let result = xm1_power_record_helper_f14e5c();
                xm1_power_record_helper_f14ebc(20);
                if result == 0 { 2 } else { 0 }
            }
            17 => {
                if alternate(event, request) { xm1_power_record_helper_f14ecc(0); }
                else { xm1_power_record_helper_f14f2c(0); }
                1
            }
            18 => {
                if xm1_power_record_helper_f14e6c() == 0 { state(12); 2 }
                else { xm1_power_record_helper_f14f7c((-98i32) as u32); 0 }
            }
            19 => { xm1_power_record_helper_f14f7c(0); 1 }
            20 => wait(xm1_power_record_helper_f14d74(request, 13), 9),
            21 => {
                if xm1_power_record_helper_f14fc0(request) == 0 {
                    state(13);
                    xm1_power_record_helper_f14c8c(request);
                    2
                } else { xm1_power_record_helper_f15000((-98i32) as u32); 0 }
            }
            22 => { xm1_power_record_helper_f15060(21); 1 }
            23 => {
                if xm1_power_record_helper_f15050() == 0 { xm1_power_record_helper_f15060(20); 2 }
                else { xm1_power_record_helper_f15060((-98i32) as u32); 0 }
            }
            24 => { xm1_power_record_helper_f15000(0); 1 }
            25 => 3,
            _ => 0,
        }
    }
}
