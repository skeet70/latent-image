use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{clear, store, word};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorMode(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorRequest(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorConfiguration(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorRuntime(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorFeatures(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct DriverHandle(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct InterruptCallback(unsafe extern "C" fn());

const MODES: Address32 = Address32(0xda52c0);
const DRIVER: Address32 = Address32(0x9742f8);
const ORDINARY_RUNTIME: SensorRuntime = SensorRuntime(Address32(0x14ce0dc));

unsafe extern "C" {
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn xm1_pipeline_image_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_pipeline_image_route_acquire(index: u32, output: Address32) -> i32;
    fn xm1_pipeline_image_route_release(handle: DriverHandle) -> i32;
    fn xm1_pipeline_image_interrupt_acquire(source: u32, callback: InterruptCallback, output: Address32) -> i32;
    fn xm1_pipeline_image_interrupt_release(handle: DriverHandle) -> i32;
    fn xm1_pipeline_image_wait_reserved(resource: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(resource: u32) -> i32;
    fn strip_cache_sync(address: u32, count: u32);
    fn xm1_pipeline_sensor_program_filters(first: Address32, second: Address32, third: Address32, fourth: Address32, fifth: Address32, option: u32);
    fn xm1_pipeline_sensor_build_configuration(request: SensorRequest, configuration: SensorConfiguration) -> i32;
    fn xm1_pipeline_sensor_configure_channels(request: SensorRequest, configuration: SensorConfiguration) -> i32;
    fn xm1_pipeline_sensor_update_center(vertical: u32, horizontal: u32);
    fn xm1_pipeline_sensor_find_alignment(value: u32, output: Address32);
}

pub(super) fn mode_record(mode: SensorMode) -> Address32 { MODES.offset(ByteOffset(mode.0.wrapping_mul(100))) }
pub(super) unsafe fn runtime(mode: SensorMode) -> SensorRuntime { SensorRuntime(Address32(unsafe { word(mode_record(mode), 0) })) }

#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn xm1_pipeline_sensor_fault() -> ! { loop { core::hint::spin_loop(); } }

unsafe fn clock_selection(features: SensorFeatures) -> u32 {
    let table = Address32(0x7c2d50);
    let mut index = 0;
    while index < 18 && unsafe { word(table, index * 8) } != features.0 { index += 1; }
    unsafe { word(table, index * 8 + 4) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_driver_acquire(features: SensorFeatures) -> i32 {
    unsafe { clear(DRIVER, 12) };
    let clock = unsafe { clock_selection(features) };
    if clock != 1 { unsafe { store(DRIVER, 8, xm1_pipeline_image_wait_reserved(1, u32::MAX) as u32) }; }
    if clock != 0 { unsafe { store(DRIVER, 8, xm1_pipeline_image_wait_reserved(2, u32::MAX) as u32) }; }
    unsafe { xm1_pipeline_image_route_request(0x405, 15, 8) };
    let result = unsafe { xm1_pipeline_image_route_acquire(14, DRIVER) };
    if result == 0 { unsafe { store(DRIVER, 4, 1) }; }
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_driver_release(features: SensorFeatures) {
    if unsafe { word(DRIVER, 4) } != 0 {
        unsafe { xm1_pipeline_image_route_release(DriverHandle(word(DRIVER, 0))); store(DRIVER, 4, 0) };
    }
    unsafe { xm1_pipeline_image_route_request(0x405, 15, 0) };
    let clock = unsafe { clock_selection(features) };
    if clock != 1 { unsafe { store(DRIVER, 8, xm1_pipeline_image_signal_reserved(1) as u32) }; }
    if clock != 0 { unsafe { store(DRIVER, 8, xm1_pipeline_image_signal_reserved(2) as u32) }; }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_stop(mode: SensorMode) {
    let record = mode_record(mode);
    if unsafe { word(record, 0x1c) } == 0 {
        unsafe { store(Address32(0xfff40000), 0, word(Address32(0xfff40000), 0) & !(1 << 16)) };
        while unsafe { word(Address32(0xfff40000), 0) } & (1 << 16) == 0 { core::hint::spin_loop(); }
    }
    if unsafe { word(record, 0x20) } == 0 {
        unsafe { store(Address32(0xfff40000), 0, word(Address32(0xfff40000), 0) & !1) };
        while unsafe { word(Address32(0xfff40000), 0) } & 1 == 0 { core::hint::spin_loop(); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_reset(mode: SensorMode) -> i32 {
    let record = mode_record(mode);
    unsafe { xm1_pipeline_sensor_driver_acquire(SensorFeatures(word(record, 4))) };
    if unsafe { word(record, 0x10) } != 0 { unsafe { xm1_pipeline_sensor_first_reset(); xm1_pipeline_sensor_first_wait(); } }
    if unsafe { word(record, 0x14) } != 0 { unsafe { xm1_pipeline_sensor_second_reset(); xm1_pipeline_sensor_second_wait(); } }
    for offset in [8,12,16,24,28,32,36,40,44,48,52,56,60,64,68,72,76,80,84,88,92,96,100,104,108,112,116,132,136,140,144,160,164,168,176,180,184,188] {
        unsafe { store(Address32(0xfff40000), offset, 0) };
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_sensor_initialize(mode: SensorMode) -> i32 {
    unsafe {
        clear(runtime(mode).0, 0x378);
        xm1_pipeline_sensor_reset(mode);
        xm1_pipeline_sensor_stop(mode);
        xm1_pipeline_sensor_reset_irq();
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_sensor_release(mode: SensorMode) -> i32 {
    let record = mode_record(mode);
    if unsafe { word(record, 0x10) } != 0 {
        if unsafe { word(record, 0x14) } != 0 {
            unsafe { xm1_pipeline_sensor_first_wait(); xm1_pipeline_sensor_second_wait(); xm1_pipeline_sensor_first_start(); xm1_pipeline_sensor_second_start(); }
        } else { unsafe { xm1_pipeline_sensor_first_wait(); xm1_pipeline_sensor_first_start(); } }
    } else if unsafe { word(record, 0x14) } != 0 { unsafe { xm1_pipeline_sensor_second_wait(); xm1_pipeline_sensor_second_start(); } }
    unsafe { xm1_pipeline_sensor_driver_release(SensorFeatures(word(record, 4))) };
    0
}

unsafe fn release_interrupts(state: SensorRuntime) {
    for index in 0..3 {
        if unsafe { word(state.0, 12 + index * 4) } != 0 {
            if unsafe { xm1_pipeline_image_interrupt_release(DriverHandle(word(state.0, index * 4))) } != 0 { xm1_pipeline_sensor_fault(); }
            unsafe { store(state.0, 12 + index * 4, 0) };
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_sensor_wait(mode: SensorMode) -> i32 {
    let state = unsafe { runtime(mode) };
    let token = unsafe { xm1_develop_enter_critical() };
    unsafe { store(state.0, 0x10c, 0); xm1_develop_leave_critical(token); xm1_pipeline_sensor_stop(SensorMode(word(state.0, 0x20))); release_interrupts(state); }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_start_output(features: SensorFeatures, first: u32, second: u32, alternate: u32) {
    let control = Address32(0xfff40004);
    if features.0 == 0x80 {
        let old = unsafe { word(control, 0) };
        unsafe { store(control, 0, (old & !0x18000) | ((first & 1) << 15) | ((second & 1) << 16)); store(Address32(0xfff400bc), 0, 1); }
    } else {
        unsafe { store(control, 0, features.0 & 0x3ff | ((first & 1) << 15) | ((second & 1) << 16) | ((alternate & 1) << 24)) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_sensor_start(mode: SensorMode) {
    let record = mode_record(mode);
    let state = unsafe { runtime(mode) };
    let alternate = unsafe { word(state.0, 0xe8) };
    unsafe { strip_cache_sync(0, 0) };
    if unsafe { word(state.0, 0x18) } == 1 {
        unsafe { xm1_pipeline_sensor_start_output(SensorFeatures(0x40), 1, 1, 0) };
    } else {
        unsafe { xm1_pipeline_sensor_start_output(SensorFeatures(word(record, 4)), word(record, 0x60), word(record, 0x5c), u32::from(alternate != 0)) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_interrupt() {
    let state = ORDINARY_RUNTIME;
    let mode = SensorMode(unsafe { word(state.0, 0x20) });
    let token = unsafe { xm1_develop_enter_critical() };
    if unsafe { xm1_pipeline_sensor_ack_irq() } != 0 { xm1_pipeline_sensor_fault(); }
    unsafe {
        store(state.0, 0x374, word(state.0, 0x374).wrapping_add(1));
        store(state.0, 0x1c, word(state.0, 0x1c).wrapping_sub(1));
        xm1_pipeline_sensor_stop(SensorMode(word(state.0, 0x20)));
        xm1_pipeline_sensor_program_filters(Address32(0xda5298), Address32(0xda5298), Address32(0xda52a8), Address32(0xda52a8), Address32(0xda52a8), word(mode_record(mode), 0x54));
        release_interrupts(state);
    }
    let callback = unsafe { core::ptr::read_volatile(state.0.offset(ByteOffset(0x10c)).0 as *const Option<unsafe extern "C" fn()>) };
    unsafe { store(state.0, 0x10c, 0); xm1_develop_leave_critical(token) };
    if let Some(callback) = callback { unsafe { callback() }; }
}

#[derive(Clone, Copy)]
enum FilterBank { First, Second }
impl FilterBank {
    fn route(self) -> Address32 { Address32(match self { Self::First => 0x9559cc, Self::Second => 0x95c9ac }) }
    fn registers(self) -> Address32 { Address32(match self { Self::First => 0xfff30000, Self::Second => 0xfff38000 }) }
}
unsafe fn acquire_filter(bank: FilterBank) -> i32 {
    let state = bank.route();
    unsafe { clear(state, 12) };
    let result = unsafe { xm1_pipeline_image_route_acquire(7, state) };
    if result == 0 { unsafe { store(state, 4, 1) }; }
    result
}
unsafe fn release_filter(bank: FilterBank) {
    let state = bank.route();
    if unsafe { word(state, 4) } != 0 {
        unsafe { xm1_pipeline_image_route_release(DriverHandle(word(state, 0))); store(state, 4, 0); }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_first_reset() -> i32 { unsafe { acquire_filter(FilterBank::First) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_second_reset() -> i32 { unsafe { acquire_filter(FilterBank::Second) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_first_start() { unsafe { release_filter(FilterBank::First) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_second_start() { unsafe { release_filter(FilterBank::Second) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_first_wait() { unsafe { store(FilterBank::First.registers(), 0, 0) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_second_wait() { unsafe { store(FilterBank::Second.registers(), 0, 0) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_reset_irq() { unsafe { store(Address32(0xfff40084), 0, 0x703f000f) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_ack_irq() -> u32 {
    let register = Address32(0xfff40084);
    unsafe { store(register, 0, word(register, 0)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_sensor_configure(request: SensorRequest) {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    let state = unsafe { runtime(mode) };
    unsafe { super::sensor_requests::copy(state.0.offset(ByteOffset(0x20)), request.0, 240) };
    let features = unsafe { word(record, 4) };
    if features == 0x80 && unsafe { word(request.0, 0x98) } != 0 && unsafe { word(state.0, 0x18) } == 0 {
        unsafe { store(state.0, 0x18, 1); xm1_pipeline_sensor_center_request(request); }
        return;
    }
    let callback = InterruptCallback(if features & 0x80 == 0 { xm1_pipeline_sensor_interrupt } else { xm1_pipeline_sensor_strip_interrupt });
    let configuration = SensorConfiguration(state.0.offset(ByteOffset(0x200)));
    unsafe { xm1_pipeline_sensor_build_configuration(request, configuration) };
    if unsafe { word(record, 0x54) } == 1 {
        unsafe { register_interrupt(state, 0, callback) };
    } else {
        for (index, selector) in [(0,0x30),(1,0x3c),(2,0x48)] {
            if unsafe { word(record, selector) } == 1 { unsafe { register_interrupt(state, index, callback) }; }
        }
    }
    unsafe { xm1_pipeline_sensor_configure_channels(request, configuration) };
}
unsafe fn register_interrupt(state: SensorRuntime, index: u32, callback: InterruptCallback) {
    unsafe { store(state.0, 12 + index * 4, 1) };
    if unsafe { xm1_pipeline_image_interrupt_acquire(54 + index, callback, state.0.offset(ByteOffset(index * 4))) } != 0 {
        unsafe { store(state.0, 12 + index * 4, 0) };
        xm1_pipeline_sensor_fault();
    }
    unsafe { store(state.0, 0x1c, word(state.0, 0x1c).wrapping_add(1)) };
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorSnapshot(pub Address32);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_sensor_read_registers(mode: SensorMode, snapshot: SensorSnapshot) {
    let record = mode_record(mode);
    if unsafe { word(record, 0x10) } == 0 && unsafe { word(record, 0x14) } == 0 { return; }
    let primary = Address32(0xfff30000);
    for index in 0..63 { unsafe { store(snapshot.0, index * 4, word(primary, index * 4)) }; }
    let control = unsafe { word(primary, 0) };
    if control & 0x10000 != 0 {
        unsafe { store(primary, 0, control & !0x10000) };
        for index in 0..64 {
            for (channel, origin) in [0x100,0x104,0x108,0x500,0x504,0x508,0x900,0x904,0x908,0xd00,0xd04,0xd08].into_iter().enumerate() {
                unsafe { store(snapshot.0, 0xfc + channel as u32 * 256 + index * 4, word(primary, origin + index * 16)) };
            }
        }
        unsafe { store(primary, 0, control | 0x10000) };
    }
    let control = unsafe { word(primary, 0) };
    if control & 0x100000 != 0 {
        unsafe { store(primary, 0, control & !0x100000) };
        for index in 0..1536 { unsafe { store(snapshot.0, 0xcfc + index * 4, word(primary, 0x1400 + index * 4)) }; }
        for index in 0..2048 { unsafe { store(snapshot.0, 0x24fc + index * 4, word(primary, 0x2c00 + index * 4)) }; }
        unsafe { store(primary, 0, control | 0x100000) };
    }
    let secondary = Address32(0xfff38000);
    for index in 0..7 { unsafe { store(snapshot.0, 0x44fc + index * 4, word(secondary, index * 4)) }; }
    let control = unsafe { word(secondary, 0) };
    if control & 0x10 != 0 {
        unsafe { store(secondary, 0, control & !0x10) };
        for index in 0..14 { unsafe { store(snapshot.0, 0x4518 + index * 4, word(secondary, 0x1c + index * 4)) }; }
        unsafe { store(secondary, 0, control | 0x10) };
    }
    for index in 0..3 { unsafe { store(snapshot.0, 0x4550 + index * 4, word(secondary, 0x54 + index * 4)) }; }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_center_request(request: SensorRequest) {
    let mode = SensorMode(unsafe { word(request.0, 0) });
    let record = mode_record(mode);
    let state = unsafe { runtime(mode) };
    let centered = SensorRequest(state.0.offset(ByteOffset(0x110)));
    let configuration = SensorConfiguration(state.0.offset(ByteOffset(0x200)));
    unsafe {
        super::sensor_requests::copy(centered.0, request.0, 240);
        store(state.0, 0x110, 22);
        store(state.0, 0x114, word(state.0, 0x13c));
        store(state.0, 0x118, 32);
        store(state.0, 0x11c, 32);
        store(state.0, 0x120, 1);
        store(state.0, 0x124, 2);
        store(state.0, 0x13c, word(state.0, 0x13c).wrapping_add(256));
        store(state.0, 0x140, 16);
        store(state.0, 0x144, 16);
        store(state.0, 0x148, 1);
        for offset in [0x14c,0x150,0x154,0x190,0x198,0x19c] { store(state.0, offset, 0); }
        xm1_pipeline_sensor_build_configuration(centered, configuration);
        store(state.0, 0x20c, 16);
        store(state.0, 0x228, 16);
        store(state.0, 0x210, 16);
        store(state.0, 0x244, 4096);
        store(state.0, 0x248, word(state.0, 0x248).wrapping_mul(word(record, 8)));
        store(state.0, 0x2a4, 16);
        store(state.0, 0x2c0, 16);
        store(state.0, 0x2b4, 16);
        store(state.0, 0x2b8, 0);
        store(state.0, 0x2bc, 0);
        register_interrupt(state, 0, InterruptCallback(xm1_pipeline_sensor_center_interrupt));
        xm1_pipeline_sensor_configure_channels(centered, configuration);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_center_interrupt() {
    let state = SensorRuntime(Address32(0x14cdd64));
    let mode = SensorMode(unsafe { word(state.0, 0x20) });
    let token = unsafe { xm1_develop_enter_critical() };
    if unsafe { xm1_pipeline_sensor_ack_irq() } != 0 { xm1_pipeline_sensor_fault(); }
    unsafe {
        store(state.0, 0x374, word(state.0, 0x374).wrapping_add(1));
        store(state.0, 0x1c, word(state.0, 0x1c).wrapping_sub(1));
        xm1_develop_leave_critical(token);
        xm1_pipeline_sensor_stop(SensorMode(word(state.0, 0x20)));
    }
    if unsafe { word(state.0, 12) } == 0 { xm1_pipeline_sensor_fault(); }
    if unsafe { xm1_pipeline_image_interrupt_release(DriverHandle(word(state.0, 0))) } != 0 { xm1_pipeline_sensor_fault(); }
    let record = mode_record(mode);
    unsafe { store(state.0, 12, 0); store(state.0, 0x374, 0) };
    let token = unsafe { xm1_develop_enter_critical() };
    unsafe {
        store(state.0, 0x18, 2);
        xm1_pipeline_image_sensor_configure(SensorRequest(state.0.offset(ByteOffset(0x20))));
        xm1_develop_leave_critical(token);
        xm1_pipeline_sensor_start_output(SensorFeatures(word(record, 4)), word(record, 0x60), word(record, 0x5c), 0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_strip_interrupt() {
    let state = SensorRuntime(Address32(0x14cdd64));
    let mode = SensorMode(unsafe { word(state.0, 0x20) });
    let token = unsafe { xm1_develop_enter_critical() };
    if unsafe { xm1_pipeline_sensor_ack_irq() } != 0 { xm1_pipeline_sensor_fault(); }
    let record = mode_record(mode);
    unsafe { store(state.0, 0x374, word(state.0, 0x374).wrapping_add(1)) };
    if unsafe { word(state.0, 0x18) } == 2 {
        unsafe {
            store(state.0, 0x244, word(state.0, 0x244).wrapping_add(word(state.0, 0x228)));
            store(state.0, 0x2e8, word(state.0, 0x2e8).wrapping_add(word(state.0, 0x2e4)));
        }
        if unsafe { word(state.0, 0x218).wrapping_sub(1) } == unsafe { word(state.0, 0x374) } {
            unsafe { store(state.0, 0x18, 0) };
            let tail = unsafe { word(state.0, 0x214) };
            if tail != 0 { unsafe { store(state.0, 0x2e4, tail); xm1_pipeline_sensor_find_alignment(tail.wrapping_mul(2), state.0.offset(ByteOffset(0x2d8))); } }
        }
        unsafe { store(state.0, 0x2f0, word(state.0, 0x2f0).wrapping_add(word(state.0, 0x2e4))) };
        if unsafe { word(state.0, 0xb8) } != 0 {
            unsafe {
                store(state.0, 0x234, word(state.0, 0x234).wrapping_sub(word(state.0, 0x228).wrapping_mul(2)));
                xm1_pipeline_sensor_update_center(word(state.0, 0x238) & 0xffff, word(state.0, 0x234) & 0xffff);
            }
        }
        unsafe {
            xm1_develop_leave_critical(token);
            xm1_pipeline_sensor_stop(SensorMode(word(state.0, 0x20)));
            super::sensor_geometry::xm1_pipeline_sensor_program_configuration(SensorRequest(state.0.offset(ByteOffset(0x20))), SensorConfiguration(state.0.offset(ByteOffset(0x200))));
            xm1_pipeline_sensor_start_output(SensorFeatures(word(record, 4)), word(record, 0x60), word(record, 0x5c), 0);
        }
    } else {
        unsafe {
            store(state.0, 0x1c, word(state.0, 0x1c).wrapping_sub(1));
            xm1_pipeline_sensor_stop(SensorMode(word(state.0, 0x20)));
            xm1_pipeline_sensor_program_filters(Address32(0xda5298), Address32(0xda5298), Address32(0xda52a8), Address32(0xda52a8), Address32(0xda52a8), word(record, 0x54));
            release_interrupts(state);
        }
        let callback = unsafe { core::ptr::read_volatile(state.0.offset(ByteOffset(0x10c)).0 as *const Option<unsafe extern "C" fn()>) };
        unsafe { store(state.0, 0x10c, 0); xm1_develop_leave_critical(token) };
        if let Some(callback) = callback { unsafe { callback() }; }
    }
}
