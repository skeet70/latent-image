// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// All handler slots, including omitted C bodies, are confirmed at f04840..f05d74.
extern "C" {
    fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_helper_f32530(mask: u32);
    fn xm1_power_record_helper_f32558(mask: u32);
    fn xm1_power_record_helper_f32580(mask: u32);
    fn xm1_power_record_helper_f325a8(mask: u32) -> u32;
    fn xm1_power_record_helper_f32cc0(flags: u32, first: u32, second: u32);
    fn xm1_power_record_helper_f32dd8(status: u32, stage: u32, output: u32);
    fn xm1_power_record_helper_f32e60(category: u32) -> u32;
    fn xm1_power_record_helper_f330e4() -> u32;
    fn xm1_power_record_helper_f3300c(first: u32, second: u32) -> i32;
}

#[derive(Clone, Copy)]
enum Category { First, Second, Mode, Primary, Auxiliary, Secondary, Tertiary, Output }

impl Category {
    fn mask(self) -> u32 {
        match self { Self::First | Self::Second => 1, Self::Mode => 2, Self::Primary => 4, Self::Auxiliary => 8, Self::Secondary => 32, Self::Tertiary => 64, Self::Output => 16 }
    }
    fn machine(self) -> u32 {
        match self { Self::First | Self::Second => 24, Self::Mode => 33, Self::Primary => 14, Self::Auxiliary => 9, Self::Secondary => 29, Self::Tertiary => 30, Self::Output => 10 }
    }
    fn ordinal(self) -> u32 {
        match self { Self::First => 0, Self::Second => 1, Self::Mode => 2, Self::Primary => 3, Self::Auxiliary => 4, Self::Secondary => 5, Self::Tertiary => 6, Self::Output => 7 }
    }
    fn start_event(self) -> u32 {
        match self { Self::First | Self::Second => 0xc7, Self::Mode => 0xcc, Self::Primary => 0xd5, Self::Auxiliary => 0xdd, Self::Secondary => 0xe5, Self::Tertiary => 0xd9, Self::Output => 0xe1 }
    }
    fn select_event(self, selected: bool) -> u32 {
        match self { Self::First | Self::Second => if selected { 0xc6 } else { 0xc5 }, Self::Mode => if selected { 0xcb } else { 0xca }, Self::Primary => if selected { 0xd4 } else { 0xd3 }, Self::Auxiliary => if selected { 0xdc } else { 0xdb }, Self::Secondary => if selected { 0xe4 } else { 0xe3 }, Self::Tertiary => if selected { 0xd8 } else { 0xd7 }, Self::Output => if selected { 0xdf } else { 0xe0 } }
    }
}

const ORDER: [Category; 7] = [Category::First, Category::Mode, Category::Primary, Category::Auxiliary, Category::Secondary, Category::Tertiary, Category::Output];

unsafe fn flags(offset: usize) -> u8 { unsafe { core::ptr::read_volatile((0x016be044 + offset) as *const u8) } }
unsafe fn state(value: u16) { unsafe { core::ptr::write_volatile(0x0169d4c4 as *mut u16, value); } }

unsafe fn complete(result: i32, category: Category, preserve: bool) -> i32 {
    unsafe {
        if result != 1 { return if preserve { map(result) } else { 2 }; }
        xm1_power_record_helper_f325a8(category.mask());
        if flags(2) == 0 { state(0); 1 } else { 2 }
    }
}
fn map(result: i32) -> i32 { if result == 12 { 2 } else { result } }

unsafe fn category_event(category: Category, event: u32, request: u32) -> i32 {
    unsafe { xm1_power_record_task43_event(category.machine(), event, request) }
}

unsafe fn report(category: Category, result: i32) {
    unsafe {
        let output = xm1_power_record_helper_f32e60(category.ordinal());
        xm1_power_record_helper_f32dd8(result as u32, 7, output);
    }
}

unsafe fn pair_stage(stage: u32, request: u32) -> (i32, bool) {
    let (category, event) = match stage {
        0 => (Category::First, 0xc8), 1 => (Category::Second, 0xc9),
        2 => (Category::Mode, 0xcf), 3 => (Category::Primary, 0xd6),
        4 => (Category::Auxiliary, 0xde), 5 => (Category::Secondary, 0xe6),
        6 => (Category::Tertiary, 0xda), _ => return (2, true),
    };
    (unsafe { category_event(category, event, request) }, false)
}

unsafe fn pair(request: u32) -> i32 {
    unsafe {
        loop {
            let mut first = core::mem::MaybeUninit::<u32>::uninit();
            let mut second = core::mem::MaybeUninit::<u32>::uninit();
            xm1_power_record_helper_f32cc0(u32::from(flags(0)), first.as_mut_ptr() as u32, second.as_mut_ptr() as u32);
            let first = first.assume_init();
            let second = second.assume_init();
            let (a, a_done) = if flags(3) == 0 {
                let result = pair_stage(first, request);
                xm1_power_record_helper_f32dd8(result.0 as u32, first, 0x016be047);
                result
            } else { (2, false) };
            let (b, b_done) = if flags(4) == 0 {
                let result = pair_stage(second, request);
                xm1_power_record_helper_f32dd8(result.0 as u32, second, 0x016be048);
                result
            } else { (2, false) };
            if a == 2 && b == 2 { return if a_done && b_done { 1 } else { 2 }; }
        }
    }
}

#[derive(Clone, Copy)]
enum Completion { Report, ReportComplete, ReportCompletePreserving, Complete, CompletePreserving, Map }

unsafe fn completion(category: Category, behavior: Completion, event: u32, request: u32) -> i32 {
    unsafe {
        let result = category_event(category, event, request);
        match behavior {
            Completion::Report => { report(category, result); map(result) }
            Completion::ReportComplete => { report(category, result); complete(result, category, false) }
            Completion::ReportCompletePreserving => { report(category, result); complete(result, category, true) }
            Completion::Complete => complete(result, category, false),
            Completion::CompletePreserving => complete(result, category, true),
            Completion::Map => map(result),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_machine8(handler_index: u32, event: u32, request: u32) -> i32 {
    use Category::*;
    use Completion::*;
    unsafe {
        match handler_index {
            2 => {
                for category in ORDER {
                    let selected = u32::from(flags(0)) & category.mask() != 0;
                    if category_event(category, category.select_event(selected), request) != 1 { return 0; }
                }
                if flags(0) & 0x7f == 0 { 1 } else { 2 }
            }
            3 => { xm1_power_record_helper_f46c20(); 0 }
            4 | 6 => {
                if handler_index == 4 { xm1_power_record_helper_f32580(0x7f); }
                else if flags(2) & 0x7f == 0 { return 1; }
                for category in ORDER {
                    if handler_index == 6 && u32::from(flags(2)) & category.mask() == 0 { continue; }
                    let result = category_event(category, category.start_event(), request);
                    if result == 1 { xm1_power_record_helper_f325a8(category.mask()); }
                    else if result != 2 { return 0; }
                }
                if flags(2) == 0 { 1 } else { state(if handler_index == 4 { 2 } else { 1 }); 2 }
            }
            5 => {
                if flags(1) & 0x7f == 0 { return 1; }
                for category in ORDER {
                    if u32::from(flags(1)) & category.mask() == 0 { continue; }
                    if matches!(category, Mode | Auxiliary) && core::ptr::read_volatile(0x016be04c as *const u32) != 1 { continue; }
                    if category_event(category, category.select_event(true), request) != 1 { return 0; }
                    xm1_power_record_helper_f32558(category.mask());
                }
                if flags(1) == 0 { 1 } else { 2 }
            }
            7 => 2,
            8 => category_event(Mode, if xm1_power_record_helper_f330e4() == 1 { 0xcd } else { 0xcc }, request),
            9 => {
                let next = if xm1_power_record_helper_f330e4() == 1 { 0xce }
                    else { Mode.select_event(flags(0) & 2 != 0) };
                category_event(Mode, next, request)
            }
            10 => {
                let mut first = core::mem::MaybeUninit::<u8>::uninit();
                let mut second = core::mem::MaybeUninit::<u8>::uninit();
                if xm1_power_record_helper_f3300c(first.as_mut_ptr() as u32, second.as_mut_ptr() as u32) != 1 { return 1; }
                let first = u32::from(first.assume_init());
                let second = u32::from(second.assume_init());
                for category in ORDER {
                    let mask = category.mask();
                    if first & mask != 0 { xm1_power_record_helper_f32530(mask); }
                    else if second & mask != 0 { xm1_power_record_helper_f32580(mask); }
                }
                12
            }
            11 => pair(request),
            12 => category_event(Output, 0xe2, request),
            13 => category_event(Auxiliary, event, request),
            14 => completion(First, Report, event, request),
            15 | 16 => completion(First, ReportComplete, event, request),
            17 => completion(Second, Report, event, request),
            18 | 19 => completion(Second, ReportComplete, event, request),
            20 => completion(Mode, Report, event, request),
            21 => completion(Mode, ReportCompletePreserving, event, request),
            22 => completion(Mode, ReportComplete, event, request),
            23 => completion(Primary, Report, event, request),
            24 => completion(Primary, ReportCompletePreserving, event, request),
            25 => completion(Primary, ReportComplete, event, request),
            26 => completion(Output, Map, event, request),
            27 => completion(Output, CompletePreserving, event, request),
            28 => completion(Output, Complete, event, request),
            29 => completion(Auxiliary, Report, event, request),
            30 => completion(Auxiliary, ReportCompletePreserving, event, request),
            31 => completion(Auxiliary, ReportComplete, event, request),
            32 => completion(Secondary, Report, event, request),
            33 => completion(Secondary, ReportCompletePreserving, event, request),
            34 => completion(Secondary, ReportComplete, event, request),
            35 => completion(Tertiary, Report, event, request),
            36 => completion(Tertiary, ReportCompletePreserving, event, request),
            37 => completion(Tertiary, ReportComplete, event, request),
            38 => completion(Mode, Map, event, request),
            39 => completion(Mode, CompletePreserving, event, request),
            40 => completion(Mode, Complete, event, request),
            41 => completion(Auxiliary, Map, event, request),
            42 => completion(Auxiliary, CompletePreserving, event, request),
            43 => completion(Auxiliary, Complete, event, request),
            44 => 3,
            _ => 0,
        }
    }
}
