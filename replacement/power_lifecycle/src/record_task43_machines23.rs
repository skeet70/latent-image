//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; bytes f0c99c..f0dd24.

unsafe extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f3c17c();
    fn xm1_power_record_helper_f3c428();
    fn xm1_power_record_helper_f3d3e0(value: u32);
    fn xm1_power_record_helper_f3d400(value: u32);
    fn xm1_power_record_helper_f3d420();
    fn xm1_power_record_helper_f3d45c();
    fn xm1_power_record_helper_f3d490() -> i32;
    fn xm1_power_record_helper_f3d500();
    fn xm1_power_record_helper_f3d544();
    fn xm1_power_record_helper_f3d904() -> i32;
    fn xm1_power_record_helper_f3da6c() -> i32;
    fn xm1_power_record_helper_f3e280(item: u32) -> i32;
    fn xm1_power_record_helper_f3e2f8() -> i32;
    fn xm1_power_record_helper_f3e32c();
    fn xm1_power_record_helper_f3e380();
    fn xm1_power_record_helper_f3e3c8();
    fn xm1_power_record_helper_f3e6f0() -> i32;
    fn xm1_power_record_helper_f3e9cc(value: u32);
    fn xm1_power_record_helper_f3eaf0() -> i32;
    fn xm1_power_record_helper_f3eb3c() -> i32;
    fn xm1_power_record_helper_f3ebd4() -> i32;
    fn xm1_power_record_helper_f3ec34();
    fn xm1_power_record_helper_f400fc() -> i32;
    fn xm1_power_record_helper_f40300(mode: i32, profile: i32) -> i32;
    fn xm1_power_record_helper_f40944();
    fn xm1_power_record_helper_f429d0() -> i32;
    fn xm1_power_record_helper_f438c4() -> i32;
    fn xm1_power_record_helper_f43d94();
    fn xm1_power_record_helper_f44fec() -> u32;
    fn xm1_power_record_helper_f46074() -> i32;
    fn xm1_power_record_helper_f46754() -> i32;
    fn xm1_power_record_helper_f467cc() -> i32;
}

unsafe fn state(value: u16) {
    unsafe { core::ptr::write_volatile((0x0169_d4b4usize + 46) as *mut u16, value) };
}

unsafe fn runtime(offset: usize) -> i32 {
    unsafe { core::ptr::read_volatile((0x016c_21fcusize + offset) as *const i32) }
}

unsafe fn transition(offset: usize) -> i32 {
    unsafe { core::ptr::read_volatile((0x016c_0fb8usize + offset) as *const i32) }
}

unsafe fn active() -> bool {
    unsafe { runtime(4) == 1 || runtime(8) == 1 }
}

unsafe fn special_mode() -> bool {
    matches!(unsafe { transition(0xf70) }, 2 | 16)
}

unsafe fn query_pending(reset: bool) {
    unsafe {
        if reset { xm1_power_record_helper_f3ec34(); }
        if xm1_power_record_helper_f3eaf0() != 0 { xm1_power_record_helper_f3d544(); }
    }
}

unsafe fn publish() {
    unsafe { xm1_power_record_helper_f3c428(); xm1_power_record_helper_f3e9cc(2); }
}

unsafe fn start_selection() -> i32 {
    unsafe { xm1_power_record_helper_f40944(); xm1_power_record_helper_f3d500(); }
    2
}

#[derive(Clone, Copy)]
enum Classifier { Current, Resume, Begin, Restart, Primary, Alternate, Saved, BeginPending, RestartPending }

unsafe fn classify(kind: Classifier, request: u32) -> i32 {
    unsafe {
        xm1_power_record_helper_f3d3e0(0);
        let item = xm1_power_record_helper_f44fec();
        match xm1_power_record_helper_f3e280(item) {
            13 => { state(2); return 12; }
            20 => { xm1_power_record_task43_event(15, 0xd0, request); state(15); publish(); return 12; }
            _ => {}
        }
        xm1_power_record_helper_f43d94();
        let profile = match kind {
            Classifier::Current => xm1_power_record_helper_f429d0(),
            Classifier::Resume => xm1_power_record_helper_f438c4(),
            Classifier::Begin | Classifier::Restart => xm1_power_record_helper_f3da6c(),
            Classifier::Primary => xm1_power_record_helper_f46754(),
            Classifier::Alternate => xm1_power_record_helper_f400fc(),
            Classifier::Saved => xm1_power_record_helper_f46074(),
            Classifier::BeginPending | Classifier::RestartPending => xm1_power_record_helper_f3d904(),
        };
        let mode = xm1_power_record_helper_f3d490();
        let class = xm1_power_record_helper_f40300(mode, profile);
        match kind {
            Classifier::Current => match class { 14 => state(3), 16 => state(6), 18 => state(9), _ => publish() },
            Classifier::Resume => match class {
                13 => { state(2); query_pending(false); }
                15 => { state(4); query_pending(false); }
                18 => state(9),
                _ => {
                    if xm1_power_record_helper_f3eaf0() != 0 { xm1_power_record_helper_f3d544(); }
                    else if xm1_power_record_helper_f3eb3c() == 0 { publish(); }
                }
            },
            Classifier::Begin | Classifier::Restart => match class {
                13 => state(2),
                17 => { state(7); publish(); }
                16 => state(6),
                18 => state(9),
                _ => { if matches!(kind, Classifier::Begin) { state(5); } publish(); }
            },
            Classifier::Primary | Classifier::Saved => match class { 13 => state(2), 14 => state(3), 18 => state(9), _ => publish() },
            Classifier::Alternate => match class { 13 => state(2), 19 => state(8), 15 => state(4), 18 => state(9), _ => {} },
            Classifier::BeginPending | Classifier::RestartPending => {
                if class == 13 { state(2); }
                else {
                    if class == 17 { state(7); }
                    else if matches!(kind, Classifier::BeginPending) { state(10); }
                    publish();
                }
            }
        }
        12
    }
}

unsafe fn reset_classification() {
    unsafe {
        xm1_power_record_helper_f3d3e0(0);
        let item = xm1_power_record_helper_f44fec();
        xm1_power_record_helper_f3e280(item);
        xm1_power_record_helper_f43d94();
        let profile = xm1_power_record_helper_f467cc();
        xm1_power_record_helper_f40300(0, profile);
    }
}

unsafe fn clear_first() {
    unsafe { xm1_power_record_helper_f3d400(0); xm1_power_record_helper_f3d420(); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_task43_machine23(handler_index: u32, _event: u32, request: u32) -> i32 {
    unsafe {
        match handler_index {
            2 => { xm1_power_record_helper_f3c17c(); 1 }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 => { state(if xm1_power_record_helper_f3e6f0() == 17 { 7 } else { 2 }); xm1_power_record_helper_f3c17c(); 1 }
            5 => 1,
            6 | 7 | 8 | 9 => {
                if handler_index == 9 { xm1_power_record_task43_event(15, 0xd1, request); }
                if active() {
                    state(18);
                    if handler_index == 7 || (handler_index == 8 && !special_mode()) { query_pending(true); }
                } else { state(19); xm1_power_record_helper_f3e9cc(1); }
                2
            }
            10 => 2,
            11 | 12 => {
                if active() { state(11); if handler_index == 12 { query_pending(true); } 2 }
                else { state(12); xm1_power_record_helper_f3e32c(); 1 }
            }
            13 => {
                xm1_power_record_helper_f3e3c8();
                if active() { state(13); if !special_mode() { query_pending(true); } 2 }
                else { state(2); xm1_power_record_helper_f3e380(); 1 }
            }
            14 => 12,
            15 => if runtime(8) == 0 { start_selection() } else { 12 },
            16 => if xm1_power_record_helper_f3ebd4() != 0 { start_selection() } else { 12 },
            17 => if xm1_power_record_helper_f3e2f8() != 0 { start_selection() } else { 12 },
            18 => {
                if special_mode() { if runtime(8) == 0 { start_selection() } else { 12 } }
                else if xm1_power_record_helper_f3ebd4() != 0 { start_selection() } else { 12 }
            }
            19 => classify(Classifier::Current, request),
            20 => classify(Classifier::Resume, request),
            21 => classify(Classifier::Begin, request),
            22 => classify(Classifier::Restart, request),
            23 => classify(Classifier::Primary, request),
            24 => classify(Classifier::Alternate, request),
            25 => classify(Classifier::Saved, request),
            26 => classify(Classifier::BeginPending, request),
            27 => classify(Classifier::RestartPending, request),
            28 | 30 => {
                xm1_power_record_helper_f3d3e0(0);
                if runtime(8) != 0 { return 12; }
                if handler_index == 28 { state(12); xm1_power_record_helper_f3e32c(); }
                else { state(2); xm1_power_record_helper_f3e380(); }
                1
            }
            29 => {
                reset_classification();
                if transition(0xf84) != 0 {
                    if special_mode() { publish(); }
                    else if xm1_power_record_helper_f3eaf0() != 0 { xm1_power_record_helper_f3d544(); }
                    else if xm1_power_record_helper_f3eb3c() == 0 && transition(0xf7c) != 1 { publish(); }
                }
                12
            }
            31 => {
                reset_classification();
                if transition(0xf7c) == 1 {
                    if xm1_power_record_helper_f3eaf0() != 0 { state(13); xm1_power_record_helper_f3d544(); 12 }
                    else { state(2); xm1_power_record_helper_f3e380(); 1 }
                } else {
                    if xm1_power_record_helper_f3eaf0() != 0 { state(13); xm1_power_record_helper_f3d544(); }
                    else if xm1_power_record_helper_f3eb3c() == 0 { publish(); }
                    12
                }
            }
            32 => {
                xm1_power_record_helper_f3d3e0(0);
                let item = xm1_power_record_helper_f44fec();
                match xm1_power_record_helper_f3e280(item) {
                    13 => { state(2); xm1_power_record_task43_event(15, 0xd1, request); }
                    20 => match xm1_power_record_task43_event(15, 0xd2, request) { 13 => state(2), 14 => state(3), _ => publish() },
                    _ => {}
                }
                12
            }
            33 => { xm1_power_record_helper_f3d3e0(0); xm1_power_record_helper_f3e9cc(1); 12 }
            34 => { xm1_power_record_helper_f3d3e0(0); if runtime(8) == 0 { state(19); xm1_power_record_helper_f3e9cc(1); } 2 }
            35 | 38 => {
                clear_first();
                if runtime(12) == -94 {
                    if runtime(4) == 1 {
                        state(16);
                        if handler_index == 38 { xm1_power_record_task43_event(15, 0xd1, request); }
                    } else {
                        state(17);
                        if handler_index == 38 { xm1_power_record_task43_event(15, 0xd1, request); }
                        xm1_power_record_helper_f3e9cc(1);
                    }
                } else { xm1_power_record_helper_f3d45c(); }
                12
            }
            36 | 37 => {
                clear_first(); xm1_power_record_helper_f3d45c();
                if runtime(4) != 0 { return 12; }
                if handler_index == 36 { state(12); xm1_power_record_helper_f3e32c(); }
                else { state(2); xm1_power_record_helper_f3e380(); }
                1
            }
            39 => { clear_first(); 12 }
            40 => { clear_first(); if runtime(4) == 0 { state(19); xm1_power_record_helper_f3e9cc(1); } 2 }
            41 => {
                clear_first();
                if runtime(12) == 21 { xm1_power_record_helper_f3e9cc(1); 2 }
                else { state(0); clear_first(); 1 }
            }
            42 => 3,
            _ => -1,
        }
    }
}
