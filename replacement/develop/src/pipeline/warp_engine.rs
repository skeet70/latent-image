use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct WarpConfiguration(pub(super) Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct WarpParameters(pub(super) Address32);

unsafe fn get(a: Address32, word: u32) -> u32 { unsafe { a.offset(ByteOffset(word * 4)).word() } }
unsafe fn put(a: Address32, word: u32, value: u32) { unsafe { a.offset(ByteOffset(word * 4)).write_word(value) } }
fn require(condition: bool) { if !condition { loop { core::hint::spin_loop(); } } }
fn left(value: u32, shift: u32) -> u32 { value.checked_shl(shift & 255).unwrap_or(0) }
fn right(value: u32, shift: u32) -> u32 { value.checked_shr(shift & 255).unwrap_or(0) }
fn divide(numerator: u32, denominator: u32) -> u32 {
    require(denominator != 0);
    numerator / denominator
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_radial_select_format(configuration: WarpConfiguration, parameters: WarpParameters) {
    unsafe {
        let c = configuration.0;
        let p = parameters.0;
        let correction = Address32(get(c, 18));
        if correction.0 != 0 { put(p, 0, get(correction, 0)); return; }
        let format = get(c, 1);
        let mut mode = u32::from(format == 2);
        loop {
            let scaled = get(p, 16).wrapping_mul(64 >> mode);
            if scaled >> 12 <= (if format == 2 { 80 } else { 160 }) && get(p, 17).wrapping_shl(3) < 0x21000 {
                put(p, 0, mode);
                return;
            }
            require(mode != 2);
            mode += 1;
        }
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_build_parameters(configuration: WarpConfiguration, parameters: WarpParameters) -> u32 {
    unsafe {
        let c = configuration.0;
        let p = parameters.0;
        require(get(c, 10) & 15 == 0);
        require((get(c, 5) | get(c, 12) | get(c, 13)) & 7 == 0);
        require((get(c, 11) | get(c, 14)) & 15 == 0);
        let shift = get(c, 1);
        let width = get(c, 6);
        let height = get(c, 7);
        let stride = get(c, 4);
        if get(c, 8) == 0 && get(c, 9) == 0 {
            put(p, 1, stride & !15);
            put(p, 2, (stride & 15) >> 1);
            put(p, 3, 0);
        } else {
            require(stride & 15 == 0);
            put(p, 1, stride);
            put(p, 2, get(c, 8).wrapping_sub(width).wrapping_add(left(1, shift)) >> 1);
            put(p, 3, get(c, 9).wrapping_sub(height).wrapping_add(left(1, shift)) >> 1);
        }
        put(p, 10, get(p, 2).wrapping_mul(2).wrapping_add(left(right(width, shift).wrapping_sub(1), shift)));
        put(p, 11, get(p, 3).wrapping_mul(2).wrapping_add(left(right(height, shift).wrapping_sub(1), shift)));
        if get(c, 2) == 0 && get(c, 3) == 0 {
            put(p, 12, get(c, 14).wrapping_sub(1).wrapping_add(get(c, 12).wrapping_mul(2)));
            put(p, 13, get(c, 15).wrapping_sub(1).wrapping_add(get(c, 13).wrapping_mul(2)));
            put(p, 16, divide(right(width, shift).wrapping_sub(1).wrapping_shl(12), get(c, 14).wrapping_sub(1)));
            put(p, 17, divide(right(height, shift).wrapping_sub(1).wrapping_shl(12), get(c, 15).wrapping_sub(1)));
            put(p, 7, get(c, 15).wrapping_add(7) & !7);
        } else {
            put(p, 12, get(c, 16).wrapping_add(get(c, 12).wrapping_mul(2)));
            put(p, 13, get(c, 17).wrapping_add(get(c, 13).wrapping_mul(2)));
            put(p, 16, get(c, 2));
            put(p, 17, get(c, 3));
            put(p, 7, get(c, 15));
        }
        put(p, 4, width.wrapping_add(7).wrapping_add(get(p, 2).wrapping_mul(2)) & !7);
        put(p, 5, height.wrapping_add(get(p, 3).wrapping_mul(2)));
        for index in [16, 17] { if get(p, index) == 0x1000 { put(p, index, 0xfff); } }
        let correction = Address32(get(c, 18));
        if correction.0 == 0 {
            put(p, 20, 0x1000); put(p, 21, 0x1000);
            put(p, 22, 0); put(p, 23, 0x00010001);
            put(p, 24, get(p, 10)); put(p, 25, get(p, 11));
            for index in 26..57 { put(p, index, 0x10000); }
        } else {
            for index in 1..38 { put(p, index + 19, get(correction, index)); }
        }
        let padding = if correction.0 == 0 { 0 } else { 4 };
        put(p, 8, padding); put(p, 9, padding);
        xm1_pipeline_image_radial_select_format(configuration, parameters);
        put(p, 18, 0); put(p, 19, 0);
    }
    0
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct WarpSetup(pub(super) Address32);

unsafe fn mmio(offset: u32, value: u32) { unsafe { Address32(0xfff65000 + offset).write_word(value) } }
fn bounded(value: u32, maximum: u32, alignment: u32) -> u32 {
    require(value <= maximum && value & (alignment - 1) == 0);
    value
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_write_geometry(setup: WarpSetup) {
    unsafe {
        let s = setup.0;
        mmio(0x10, get(s, 0) & 0x3fffffff);
        mmio(0x14, bounded(get(s, 1), 0x7fff, 8));
        let width = bounded(get(s, 2), 0x7fff, 1);
        let height = bounded(get(s, 3), 0x7fff, 1);
        require(width & 7 == 0);
        mmio(0x18, width | height << 16);
        mmio(0x1c, get(s, 4) & 0x3fffffff);
        mmio(0x20, bounded(get(s, 5), 0x7fff, 8));
        let width = bounded(get(s, 6), 0x1fff, 1);
        let height = bounded(get(s, 7), 0x1fff, 1);
        require(width & 7 == 0);
        mmio(0x24, width | height << 16);
        let width = bounded(get(s, 8), 0x7fff, 1);
        let height = bounded(get(s, 9), 0x7fff, 1);
        require(width & 7 == 0);
        mmio(0x28, width | height << 16);
        mmio(0x2c, get(s, 10) & 3);
        let x = bounded(get(s, 11), 15, 1);
        let y = bounded(get(s, 12), 15, 1);
        mmio(0x30, x | y << 16);
        let x = bounded(get(s, 13), 0xffff, 1);
        let y = bounded(get(s, 14), 0xffff, 1);
        mmio(0x50, x | y << 16);
        let x = bounded(get(s, 15), 0xffff, 1);
        let y = bounded(get(s, 16), 0xffff, 1);
        mmio(0x54, x | y << 16);
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_write_filter(parameters: WarpParameters) {
    unsafe {
        let p = parameters.0;
        mmio(0x40, bounded(get(p, 0), 0x1fff, 1));
        mmio(0x44, bounded(get(p, 1), 0x1fff, 1));
        mmio(0x48, get(p, 2) & 0x7fff7fff);
        let scales = get(p, 3);
        bounded(scales & 0xffff, 7, 1); bounded(scales >> 16, 7, 1);
        mmio(0x4c, scales);
        let x = bounded(get(p, 4), 0xffff, 1);
        let y = bounded(get(p, 5), 0xffff, 1);
        mmio(0x58, x | y << 16);
        for index in 0..31 { mmio(0x60 + index * 4, get(p, index + 6) & 0x1ffff); }
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_program(configuration: WarpConfiguration, parameters: WarpParameters) -> u32 {
    unsafe {
        let c = configuration.0;
        let p = parameters.0;
        mmio(4, (get(c, 0) & 1) << 6);
        mmio(0x34, get(c, 1) & 3);
        mmio(0x38, bounded(get(p, 16), 0xffff, 1));
        mmio(0x3c, bounded(get(p, 17), 0xffff, 1));
        let setup = [get(p, 1), get(c, 5), get(p, 4), get(p, 5), get(c, 10), get(c, 11), get(c, 12), get(c, 13), get(c, 14), get(p, 7), get(p, 0), get(p, 8), get(p, 9), get(p, 10), get(p, 11), get(p, 12), get(p, 13)];
        xm1_pipeline_image_warp_write_geometry(WarpSetup(Address32(setup.as_ptr() as u32)));
        xm1_pipeline_image_warp_write_filter(WarpParameters(p.offset(ByteOffset(80))));
        0
    }
}

const RUNTIME: Address32 = Address32(0x942c04);
const DRIVER: Address32 = Address32(0x942d64);

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_configure(configuration: WarpConfiguration) -> u32 {
    unsafe {
        let target = RUNTIME.offset(ByteOffset(8));
        for index in 0..20 { put(target, index, get(configuration.0, index)); }
        let parameters = WarpParameters(RUNTIME.offset(ByteOffset(88)));
        xm1_pipeline_image_warp_build_parameters(WarpConfiguration(target), parameters);
        xm1_pipeline_image_warp_program(WarpConfiguration(target), parameters)
    }
}

extern "C" {
    #[link_name = "xm1_pipeline_image_wait_reserved"]
    fn xm1_warp_clock_acquire(channel: u32, timeout: u32) -> u32;
    #[link_name = "xm1_pipeline_image_signal_reserved"]
    fn xm1_warp_clock_release(channel: u32);
    #[link_name = "xm1_pipeline_image_route_request"]
    fn xm1_warp_clock_configure(channel: u32, field: u32, value: u32);
    #[link_name = "xm1_pipeline_image_route_acquire"]
    fn xm1_warp_driver_acquire(identifier: u32, state: Address32) -> u32;
    #[link_name = "xm1_pipeline_image_route_release"]
    fn xm1_warp_driver_release(resource: u32);
    #[link_name = "xm1_pipeline_image_interrupt_acquire"]
    fn xm1_warp_resource_acquire(identifier: u32, callback: unsafe extern "C" fn(), resource: Address32) -> u32;
    #[link_name = "xm1_pipeline_image_interrupt_release"]
    fn xm1_warp_resource_release(resource: u32) -> u32;
    #[link_name = "xm1_develop_enter_critical"]
    fn xm1_warp_enter() -> u32;
    #[link_name = "xm1_develop_leave_critical"]
    fn xm1_warp_leave(context: u32);
    fn xm1_warp_barrier(value: u32, option: u32);
}

unsafe fn reset() {
    unsafe {
        let control = Address32(0xfff65000);
        control.write_word(control.word() & !0x10000);
        while control.word() & 0x10000 == 0 { core::hint::spin_loop(); }
    }
}
unsafe fn release_resource() {
    unsafe {
        if get(RUNTIME, 1) != 0 {
            require(xm1_warp_resource_release(get(RUNTIME, 0)) == 0);
            put(RUNTIME, 1, 0);
        }
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_initialize() -> u32 {
    unsafe {
        for index in 0..3 { put(DRIVER, index, 0); }
        put(DRIVER, 2, xm1_warp_clock_acquire(4, u32::MAX));
        xm1_warp_clock_configure(0x508, 20, 10);
        xm1_warp_clock_configure(0x609, 20, 8);
        if xm1_warp_driver_acquire(42, DRIVER) == 0 { put(DRIVER, 1, 1); }
        for index in 0..80 { put(RUNTIME, index, 0); }
        reset(); mmio(0x144, 0x3f);
        0
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_release() -> u32 {
    unsafe {
        if get(DRIVER, 1) != 0 { xm1_warp_driver_release(get(DRIVER, 0)); put(DRIVER, 1, 0); }
        xm1_warp_clock_configure(0x508, 20, 0);
        xm1_warp_clock_configure(0x609, 20, 0);
        xm1_warp_clock_release(4);
        0
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_cancel() -> u32 {
    unsafe {
        let context = xm1_warp_enter(); put(RUNTIME, 21, 0); xm1_warp_leave(context);
        reset(); release_resource(); 0
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_start() {
    unsafe {
        mmio(0x140, 0); mmio(0x148, 0x21); put(RUNTIME, 1, 1);
        require(xm1_warp_resource_acquire(57, xm1_pipeline_image_warp_completion, RUNTIME) == 0);
        xm1_warp_barrier(0, 0);
        mmio(0, Address32(0xfff65000).word() | 1);
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_warp_completion() {
    unsafe {
        let context = xm1_warp_enter();
        put(RUNTIME, 79, get(RUNTIME, 79).wrapping_add(1));
        let status = Address32(0xfff65144).word();
        mmio(0x144, status);
        if status & 1 != 0 && get(RUNTIME, 2) == 1 { put(RUNTIME, 21, 0); }
        if get(RUNTIME, 2) == 0 { reset(); }
        mmio(0x148, 0);
        release_resource();
        let callback = get(RUNTIME, 21); put(RUNTIME, 21, 0);
        xm1_warp_leave(context);
        if callback != 0 { core::mem::transmute::<u32, unsafe extern "C" fn()>(callback)(); }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DriverHandle(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DriverSlot(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DriverCapacity(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct RouteSelector(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct RouteLevel(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct InterruptSource(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct InterruptCallback(pub(super) Address32);

const ROUTES: Address32 = Address32(0x409c1e30);
const ROUTE_CLOCKS: Address32 = Address32(0x7c86d4);
const ROUTE_SELECTS: Address32 = Address32(0x7a20d4);
const SERIAL: Address32 = Address32(0x409c2804);
const INTERRUPTS: Address32 = Address32(0x940e9c);
const INTERRUPT_SOURCES: Address32 = Address32(0x7c9124);

extern "C" {
    fn xm1_warp_save_irq() -> u32;
    fn xm1_warp_restore_irq(saved: u32);
    #[link_name = "xm1_pipeline_image_irq_configure"]
    fn xm1_warp_configure_irq(line: u32, priority: u32, mode: u32, callback: InterruptCallback) -> i32;
    #[link_name = "xm1_pipeline_image_irq_release"]
    fn xm1_warp_release_irq(line: u32) -> i32;
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_driver_allocate(slot: DriverSlot, capacity: DriverCapacity, handles: Address32) -> i32 {
    unsafe {
        if slot.0 >= capacity.0 { return -30; }
        if get(handles, slot.0) != 0 { return -20; }
        loop {
            let mut serial = SERIAL.word().wrapping_add(1);
            if serial == 0 { serial = 1; }
            SERIAL.write_word(serial);
            if !(0..capacity.0).any(|index| get(handles, index) == serial) {
                put(handles, slot.0, serial);
                return 0;
            }
        }
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_driver_find(handle: DriverHandle, capacity: DriverCapacity, handles: Address32, output: Address32) -> i32 {
    unsafe {
        if handle.0 != 0 {
            for index in 0..capacity.0 {
                if get(handles, index) == handle.0 { output.write_word(index); return 0; }
            }
        }
        -30
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_driver_clear(slot: DriverSlot, capacity: DriverCapacity, handles: Address32) -> i32 {
    unsafe {
        if slot.0 >= capacity.0 { return -30; }
        put(handles, slot.0, 0);
        0
    }
}

unsafe fn route_channel(channel: DriverSlot) -> Address32 {
    ROUTES.offset(ByteOffset(0x54 + channel.0.wrapping_mul(0xa0)))
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_route_resolve(channel: DriverSlot) -> RouteLevel {
    unsafe {
        let state = route_channel(channel);
        let mut level = get(state, 1);
        for slot in 2..39 { level = level.max(get(state, slot)); }
        RouteLevel(level.min(get(state, 0)))
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_route_apply(channel: DriverSlot, level: RouteLevel) -> i32 {
    unsafe {
        let selection = get(ROUTE_SELECTS, channel.0.wrapping_mul(12).wrapping_add(level.0));
        if selection == u32::MAX { return -30; }
        let state = route_channel(channel);
        if get(state, 39) != level.0 {
            if channel.0 > 9 { return -30; }
            let irq = xm1_warp_save_irq();
            let register = Address32(0xfff88000 + channel.0 * 4);
            let previous = register.word();
            register.write_word(if channel.0 == 9 { (previous & 0x8fffffff) | selection.wrapping_shl(28) } else { (previous & !7) | selection });
            xm1_warp_restore_irq(irq);
        }
        put(state, 39, level.0);
        0
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_route_request(selector: RouteSelector, slot: DriverSlot, level: RouteLevel) -> i32 {
    unsafe {
        let saved = xm1_warp_enter();
        let status = if slot.0 > 36 || level.0 > 11 || selector.0 & 255 > 18 { -30 } else {
            let channel = DriverSlot((selector.0 >> 8) & 255);
            put(route_channel(channel), slot.0 + 2, level.0);
            xm1_pipeline_image_route_apply(channel, xm1_pipeline_image_route_resolve(channel))
        };
        xm1_warp_leave(saved);
        status
    }
}

unsafe fn clock_register(index: DriverSlot) -> Address32 {
    unsafe { Address32(if get(ROUTE_CLOCKS, index.0 * 2) == 0 { 0xfff8802c } else { 0xfff88030 }) }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_route_acquire(index: DriverSlot, output: Address32) -> i32 {
    unsafe {
        let saved = xm1_warp_enter();
        let handles = ROUTES.offset(ByteOffset(0x758));
        let status = if index.0 > 46 { -30 } else if xm1_pipeline_image_driver_allocate(index, DriverCapacity(47), handles) != 0 { -20 } else { 0 };
        if status == 0 {
            output.write_word(get(handles, index.0));
            put(ROUTES.offset(ByteOffset(0x69c)), index.0, 1);
            let mask = get(ROUTE_CLOCKS, index.0 * 2 + 1);
            let irq = xm1_warp_save_irq();
            let register = clock_register(index);
            register.write_word(register.word() | mask);
            xm1_warp_restore_irq(irq);
        }
        xm1_warp_leave(saved);
        status
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_route_release(handle: DriverHandle) -> i32 {
    unsafe {
        let saved = xm1_warp_enter();
        let handles = ROUTES.offset(ByteOffset(0x758));
        let mut index = 0;
        let found = xm1_pipeline_image_driver_find(handle, DriverCapacity(47), handles, Address32((&mut index as *mut u32) as u32));
        let status = if found != 0 { -20 } else {
            let enabled = ROUTES.offset(ByteOffset(0x69c));
            put(enabled, index, 0);
            let bank = get(ROUTE_CLOCKS, index * 2);
            let mut shared = 0;
            for scan in 0..47 {
                if get(enabled, scan) == 1 && get(ROUTE_CLOCKS, scan * 2) == bank { shared |= get(ROUTE_CLOCKS, scan * 2 + 1); }
            }
            let mask = get(ROUTE_CLOCKS, index * 2 + 1) & !shared;
            if Address32(0x4089e810).word() != 0x19790515 {
                let irq = xm1_warp_save_irq();
                let register = clock_register(DriverSlot(index));
                register.write_word(register.word() & !mask);
                xm1_warp_restore_irq(irq);
            }
            xm1_pipeline_image_driver_clear(DriverSlot(index), DriverCapacity(47), handles);
            0
        };
        xm1_warp_leave(saved);
        status
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_interrupt_acquire(source: InterruptSource, callback: InterruptCallback, output: Address32) -> i32 {
    unsafe {
        if source.0 > 122 || callback.0.0 == 0 {
            xm1_pipeline_image_diagnostic_event(DiagnosticCategory(2), DiagnosticEvent(15));
            return -30;
        }
        let descriptor = INTERRUPT_SOURCES.offset(ByteOffset(source.0 * 12));
        let line = get(descriptor, 0);
        let priority = get(descriptor, 1);
        let mode = get(descriptor, 2);
        if line > 127 || priority > 15 || mode > 3 { return -30; }
        let saved = xm1_warp_enter();
        let status = xm1_pipeline_image_driver_allocate(DriverSlot(line), DriverCapacity(128), INTERRUPTS);
        xm1_warp_leave(saved);
        if status != 0 { xm1_pipeline_image_diagnostic_event(DiagnosticCategory(2), DiagnosticEvent(15)); return status; }
        let saved = xm1_warp_enter();
        let configured = xm1_warp_configure_irq(line, priority, mode, callback);
        put(INTERRUPTS, line + 128, source.0);
        let status = if configured == 0 { output.write_word(get(INTERRUPTS, line)); 0 } else { -255 };
        xm1_warp_leave(saved);
        status
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_interrupt_release(handle: DriverHandle) -> i32 {
    unsafe {
        let mut line = 0;
        if xm1_pipeline_image_driver_find(handle, DriverCapacity(128), INTERRUPTS, Address32((&mut line as *mut u32) as u32)) != 0 { return -20; }
        let status = if xm1_warp_release_irq(line) == 0 { 0 } else { -255 };
        put(INTERRUPTS, line + 128, u32::MAX);
        xm1_pipeline_image_driver_clear(DriverSlot(line), DriverCapacity(128), INTERRUPTS);
        status
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DiagnosticCategory(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct DiagnosticEvent(pub(super) u32);

extern "C" {
    fn xm1_warp_kernel_diagnostic(category: DiagnosticCategory, event: DiagnosticEvent);
    #[link_name = "xm1_pipeline_image_current_task"]
    fn xm1_warp_current_task() -> u32;
    fn xm1_develop_append_stream_text(task: u32, text: Address32);
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_diagnostic_trigger(category: DiagnosticCategory, event: DiagnosticEvent) {
    unsafe {
        let configuration = Address32(0x4089ea9c);
        if get(configuration, 0) == category.0 && get(configuration, 1) == event.0 { put(configuration, 2, 1); }
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_diagnostic_driver_text(_category: DiagnosticCategory, event: DiagnosticEvent) {
    unsafe {
        let mut message = *b"errno_set(ERRLVL_DRIVER,0x####)\0";
        let digits = b"0123456789ABCDEF";
        let mut shift = 12u32;
        for byte in &mut message {
            if *byte == b'#' { *byte = digits[((event.0 >> shift) & 15) as usize]; shift = shift.wrapping_sub(4); }
        }
        xm1_develop_append_stream_text(xm1_warp_current_task() & 255, Address32(message.as_ptr() as u32));
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_diagnostic_selected(category: DiagnosticCategory, event: DiagnosticEvent) {
    unsafe {
        if category.0 == 2 && (3..80).any(|index| get(Address32(0x4089ea9c), index) == event.0) {
            xm1_pipeline_image_diagnostic_driver_text(category, event);
        }
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_diagnostic_event(category: DiagnosticCategory, event: DiagnosticEvent) {
    unsafe {
        let errors = Address32(0x40a5b660);
        if category.0 == 1 || category.0 == 2 {
            let slot = category.0 - 1;
            if get(errors, slot) == 0 { put(errors, slot, event.0); }
        }
        xm1_pipeline_image_diagnostic_trigger(category, event);
        xm1_warp_kernel_diagnostic(category, event);
        xm1_pipeline_image_diagnostic_selected(category, event);
    }
}

#[repr(C)]
struct IrqConfiguration {
    reserved: u32,
    callback: InterruptCallback,
    priority: u32,
    enabled: u32,
    kind: u32,
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_irq_configure(line: DriverSlot, priority: RouteLevel, mode: u32, callback: InterruptCallback) -> i32 {
    unsafe {
        let table = super::image_planes::interface(super::image_planes::current_interface());
        let enter: unsafe extern "C" fn() -> u32 = core::mem::transmute(table.offset(ByteOffset(0x164)).word());
        let configure: unsafe extern "C" fn(DriverSlot, *const IrqConfiguration) = core::mem::transmute(table.offset(ByteOffset(0x160)).word());
        let activate: unsafe extern "C" fn(DriverSlot) = core::mem::transmute(table.offset(ByteOffset(0x158)).word());
        let leave: unsafe extern "C" fn(u32) = core::mem::transmute(table.offset(ByteOffset(0x168)).word());
        let saved = enter();
        let configuration = IrqConfiguration {
            reserved: 0, callback, priority: priority.0,
            enabled: u32::from(mode > 1),
            kind: match mode { 0 | 2 => 2, 1 | 3 => 1, 4 => 3, _ => 0 },
        };
        configure(line, &configuration);
        activate(line);
        leave(saved);
        0
    }
}

#[no_mangle]
pub(super) unsafe extern "C" fn xm1_pipeline_image_irq_release(line: DriverSlot) -> i32 {
    unsafe {
        let table = super::image_planes::interface(super::image_planes::current_interface());
        let enter: unsafe extern "C" fn() -> u32 = core::mem::transmute(table.offset(ByteOffset(0x164)).word());
        let prepare: unsafe extern "C" fn(DriverSlot) = core::mem::transmute(table.offset(ByteOffset(0x15c)).word());
        let configure: unsafe extern "C" fn(DriverSlot, *const IrqConfiguration) = core::mem::transmute(table.offset(ByteOffset(0x160)).word());
        let leave: unsafe extern "C" fn(u32) = core::mem::transmute(table.offset(ByteOffset(0x168)).word());
        let saved = enter();
        prepare(line);
        configure(line, core::ptr::null());
        leave(saved);
        0
    }
}
