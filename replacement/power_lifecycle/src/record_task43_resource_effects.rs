// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// NONMATCHING remap/session paths are confirmed at f46cf8..f4738c.
use core::mem::MaybeUninit;

extern "C" {
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_74e020(resource: u32, ignored: u32, low: u32, high: u32) -> u32;
    fn xm1_power_record_helper_74e648(resource: u32) -> u32;
    fn xm1_power_record_helper_74f784(resource: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(resource: u32) -> u32;
    fn xm1_power_record_helper_74fa2c(resource: u32) -> u32;
    fn xm1_power_record_helper_74f9a4(resource: u32) -> u32;
    fn xm1_power_record_helper_7503ec(resource: u32) -> u32;
    fn xm1_power_record_helper_7569a4(resource: u32) -> u32;
    fn xm1_power_record_helper_74fab4(output: u32, resource: u32);
    fn xm1_power_record_helper_752278(output: *mut Pair, resource: u32) -> *mut Pair;
    fn xm1_power_record_helper_758c34(resource: u32, session: *const Session);
    fn xm1_power_record_helper_76da38() -> u32;
    fn xm1_power_record_helper_76d9a8() -> u32;
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_76f340() -> u32;
    fn xm1_power_record_helper_746018() -> u32;
    fn xm1_power_record_helper_76fbac(lens_value: u32, adjusted: u32);
    fn xm1_power_record_helper_76fc4c() -> u32;
    fn xm1_power_record_helper_76da84() -> u32;
    fn xm1_power_record_helper_76dad4() -> u32;
    fn xm1_power_record_helper_76dbb4() -> u32;
    fn xm1_power_record_helper_76f5dc(callback: u32, enabled: u32) -> u32;
    fn xm1_power_record_helper_76fb54(output: u32);
    fn xm1_power_record_helper_749fac(output: u32, position: u32, lens_value: u32, adjusted: u32, property: u32, flags: u32, direction: u32, selector: u32);
    fn xm1_power_record_helper_749e98(output: u32, position: u32, lens_value: u32, property: u32, flags: u32, direction: u32, selector: u32);
    fn xm1_power_record_helper_748730(a: u32, b: u32, value: u32, c: u32, flags: u32, d: u32) -> u32;
    fn xm1_power_record_helper_efb73c(status: u32);
    fn xm1_power_record_helper_efb984(status: u32, triplet: u32, extra: u32);
    fn xm1_power_record_helper_efb9f0(triplet: u32, extra: u32);
    fn xm1_power_record_helper_efba90(status: u32);
    fn xm1_power_record_helper_f15070(code: u32, value: u32);
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Resource(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Position(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Direction(u32);
#[repr(C)]
struct Remap { values: [u32; 3] }
#[repr(C)]
struct Pair { first: u32, second: u32 }
#[repr(C)]
struct LensSnapshot { position: i16, padding: MaybeUninit<u16>, lens_value: u32, value: u32, mode: u32 }
#[repr(C)]
struct Session { enabled: u32, pair: Pair, lens: LensSnapshot }

unsafe fn word(address: usize) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
unsafe fn put(address: usize, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value); } }

unsafe fn remap(resource: Resource, position: Position, direction: Direction) -> Remap {
    unsafe {
        let mut output = Remap { values: [u32::MAX; 3] };
        let kind = xm1_power_record_helper_74e648(resource.0).wrapping_sub(46);
        let property = if kind <= 10 && 0x503 & (1u32 << kind) != 0 {
            xm1_power_record_helper_74f784(resource.0)
        } else { 0 };
        let category = xm1_power_record_helper_76da38();
        if category == 2 { return output; }
        let category = xm1_power_record_helper_76d9a8();
        if category == 2 { return output; }
        let mut lens_value = xm1_power_record_helper_76d9c8();
        let flags = xm1_power_record_helper_76f340();
        if xm1_power_record_helper_746018() != 0 {
            let mut adjusted = 0u16;
            xm1_power_record_helper_76fbac(&mut lens_value as *mut u32 as u32, &mut adjusted as *mut u16 as u32);
            let selector = xm1_power_record_helper_76fc4c();
            xm1_power_record_helper_749fac(&mut output as *mut Remap as u32, position.0, lens_value, u32::from(adjusted), property, flags, direction.0, selector);
        } else {
            let selector = xm1_power_record_helper_76fc4c();
            xm1_power_record_helper_749e98(&mut output as *mut Remap as u32, position.0, lens_value, property, flags, direction.0, selector);
        }
        output
    }
}

unsafe fn publish(resource: Resource, position: Position, direction: Direction) {
    unsafe {
        if word(0x00954ddc) == 0 { return; }
        let output = remap(resource, position, direction);
        xm1_power_record_helper_efb9f0(&output as *const Remap as u32, 0);
    }
}

unsafe fn current_direction() -> Direction {
    unsafe {
        if xm1_power_record_helper_76da38() != 0 { Direction(1) }
        else { Direction(xm1_power_record_helper_76dad4()) }
    }
}

unsafe fn pair(resource: Resource, direct: bool) -> Pair {
    unsafe {
        let mut result = MaybeUninit::<Pair>::uninit();
        if direct { xm1_power_record_helper_752278(result.as_mut_ptr(), resource.0); }
        else if xm1_power_record_helper_74fa2c(resource.0) == 2 {
            xm1_power_record_helper_74fab4(result.as_mut_ptr() as u32, resource.0);
        } else { return Pair { first: 3, second: 3 }; }
        result.assume_init()
    }
}

unsafe fn session(resource: Resource, enabled: u32, direct: u32) {
    unsafe {
        let session = if enabled == 1 {
            let position = xm1_power_record_helper_76da84() as i16;
            let lens_value = xm1_power_record_helper_76d9c8();
            let value = xm1_power_record_helper_76dbb4();
            let mode = xm1_power_record_helper_76dad4();
            Session {
                enabled: 1,
                pair: pair(resource, direct == 1),
                lens: LensSnapshot { position, padding: MaybeUninit::uninit(), lens_value, value, mode },
            }
        } else {
            Session { enabled: 0, pair: Pair { first: 3, second: 3 }, lens: LensSnapshot { position: 0, padding: MaybeUninit::uninit(), lens_value: 0, value: 128, mode: 2 } }
        };
        xm1_power_record_helper_758c34(resource.0, &session);
    }
}

unsafe fn finish_session(resource: Resource, enabled: u32) {
    unsafe {
        let lens = if enabled == 1 {
            let mut lens = MaybeUninit::<LensSnapshot>::uninit();
            xm1_power_record_helper_76fb54(lens.as_mut_ptr() as u32);
            let mut lens = lens.assume_init();
            if lens.position == i16::MIN {
                let mut value = xm1_power_record_helper_74f9a4(resource.0);
                if xm1_power_record_helper_7503ec(resource.0) != 0 { value = 0; }
                let flags = xm1_power_record_helper_76f340();
                lens.position = xm1_power_record_helper_748730(0, 0, value, 0, flags, 0) as i16;
                lens.lens_value = 0;
                lens.value = 0;
                lens.mode = 1;
            }
            lens
        } else { LensSnapshot { position: 0, padding: MaybeUninit::uninit(), lens_value: 0, value: 128, mode: 2 } };
        let selected = if enabled == 1 { pair(resource, false) } else { Pair { first: 3, second: 3 } };
        let session = Session { enabled: u32::from(enabled == 1), pair: selected, lens };
        xm1_power_record_helper_758c34(resource.0, &session);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f46cf8(status: u32) {
    unsafe { xm1_power_record_helper_efb73c(status); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f46ea8(resource: u32) {
    unsafe {
        if word(0x00954ddc) == 0 { return; }
        let first = xm1_power_record_helper_74e020(resource, 0, 0x200000, 0);
        let second = xm1_power_record_helper_74e020(resource, 0, 0x2000000, 0);
        let third = xm1_power_record_helper_74e020(resource, 0, 2, 0);
        if first == 0 && second == 0 && third == 0 { return; }
        let position = Position(xm1_power_record_helper_76da84());
        let direction = current_direction();
        publish(Resource(resource), position, direction);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f46f28(resource: u32) {
    unsafe {
        let position = core::ptr::read_volatile(0x0169d380 as *const i16) as i32 as u32;
        let direction = word(0x0169d384);
        publish(Resource(resource), Position(position), Direction(direction));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f46f4c(resource: u32) {
    unsafe {
        put(0x00954dd8, word(0x0169d300));
        put(0x00954ddc, word(0x0169d304));
        xm1_power_record_helper_76f5dc(xm1_power_record_helper_f15070 as *const () as u32, word(0x0169d304));
        let position = Position(xm1_power_record_helper_76da84());
        let direction = current_direction();
        let output = remap(Resource(resource), position, direction);
        xm1_power_record_helper_efb984(0, &output as *const Remap as u32, 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f470d0(resource: u32, direct: u32) {
    unsafe {
        session(Resource(resource), word(0x0169d314), direct);
        xm1_power_record_helper_efba90(0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f47100(resource: u32) -> i32 {
    unsafe { i32::from(xm1_power_record_helper_7569a4(resource) & 2 == 0) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f47234(resource: u32) {
    unsafe {
        finish_session(Resource(resource), word(0x0169d314));
        xm1_power_record_helper_efba90(0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f47330(resource: u32) -> i32 {
    unsafe {
        let enabled = xm1_power_record_helper_69f8fc(0x887c);
        let category = xm1_power_record_helper_74e5c0(resource);
        let kind = xm1_power_record_helper_74fa2c(resource);
        i32::from(enabled != 0 && category == 0 && kind.wrapping_sub(4) <= 2)
    }
}
