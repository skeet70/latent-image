use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct MotionCoordinate(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct MotionCost(pub i32);
#[repr(C)]
pub struct MotionSearch {
    pub source_stride: i32,
    pub target_stride: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: i32,
    pub height: i32,
    pub limit_x: i32,
    pub limit_y: i32,
    pub source: u32,
    pub target: u32,
    pub reserved: [u32; 4],
    pub evaluated: i32,
    pub maximum: i32,
    pub sample_x: i32,
    pub sample_y: i32,
    pub flags: u8,
    pub padding: [u8; 3],
}

const _: [(); 76] = [(); core::mem::size_of::<MotionSearch>()];

#[derive(Clone, Copy)]
#[repr(u32)]
enum MotionByteKey {
    Trace = 0x9f6f,
    PrimaryFlags = 0x9f8b,
    AlternateFlags = 0x9fee,
    PrimaryHorizontalRange = 0x9f07,
    PrimaryHorizontalStep = 0x9f08,
    PrimaryVerticalRange = 0x9f09,
    PrimaryVerticalStep = 0x9f0a,
    AlternateHorizontalRange = 0x9fe0,
    AlternateHorizontalStep = 0x9fe1,
    AlternateVerticalRange = 0x9fe2,
    AlternateVerticalStep = 0x9fe3,
    RefineHorizontalRange = 0x9f0b,
    RefineHorizontalStep = 0x9f0c,
    RefineVerticalRange = 0x9f0d,
    RefineVerticalStep = 0x9f0e,
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum MotionHalfKey {
    RefineSample = 0x9f8c,
    RefineDivisor = 0x9f8e,
    CoarseSample = 0x9f90,
    CoarseDivisor = 0x9f92,
}

extern "C" {
    fn xm1_motion_config_byte(key: u32) -> u8;
    fn xm1_motion_config_half(key: u32) -> u16;
    fn xm1_motion_current_task() -> u32;
    fn xm1_motion_wait(event: u32, value: *mut u32, timeout: i32) -> i32;
    fn xm1_motion_debug(a: u32, data: *const u32, length: u32, d: u32, name: u32);
}

unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn store(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn cfg(key: MotionByteKey) -> i32 { unsafe { xm1_motion_config_byte(key as u32) as i32 } }
unsafe fn half(key: MotionHalfKey) -> i32 { unsafe { xm1_motion_config_half(key as u32) as i32 } }
fn rounded_x(x: i32) -> i32 {
    let biased = x.wrapping_add(3);
    (if biased < 0 { x.wrapping_add(6) } else { biased }) & !3
}
unsafe fn seen(search: &MotionSearch, x: i32, y: i32) -> bool {
    let key = (x as u32).wrapping_add((y as u32) << 16);
    let mut index = 0;
    while index < search.evaluated {
        let entry = unsafe { word(0x014e2a80 + index as u32 * 4) };
        if entry == key { return true; }
        if entry == 0 { return false; }
        index += 1;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_motion_cost_hardware(threshold: *const MotionCost, x: MotionCoordinate, y: MotionCoordinate, search: *const MotionSearch) -> MotionCost {
    let search = unsafe { &*search };
    let mut row = 0i32;
    let mut cost = 0i32;
    let mut event = 0;
    while row < search.height {
        unsafe {
            xm1_pipeline_image_motion_configure_candidate(x.0 as u32, y.0.wrapping_add(row) as u32, search.origin_x as u32, search.origin_y.wrapping_add(row) as u32, 0);
            store(0x014e2a30, xm1_motion_current_task());
            xm1_pipeline_image_resample_start(xm1_pipeline_image_motion_complete as usize as u32);
            xm1_motion_wait(0x30, &mut event, -1);
            store(0x014e2a30, 0);
            xm1_pipeline_image_resample_cancel();
        }
        let value = unsafe { xm1_pipeline_image_resample_get_status(0, 0) };
        cost = cost.wrapping_add(value);
        if value < 0 { cost = unsafe { (*threshold).0 }; break; }
        if cost >= unsafe { (*threshold).0 } { break; }
        row = row.wrapping_add(search.sample_y);
    }
    if unsafe { cfg(MotionByteKey::Trace) } & 0x80 != 0 {
        let trace = [unsafe { (*threshold).0 } as u32, x.0 as u32, y.0 as u32, cost as u32];
        unsafe { xm1_motion_debug(0, trace.as_ptr(), 16, 0, 0x00dad710) };
    }
    MotionCost(cost)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_motion_cost(threshold: *const MotionCost, x: MotionCoordinate, y: MotionCoordinate, search: *mut MotionSearch) -> MotionCost {
    let s = unsafe { &mut *search };
    unsafe { store(0x014e2a80u32.wrapping_add((s.evaluated as u32).wrapping_mul(4)), (x.0 as u32).wrapping_add((y.0 as u32) << 16)) };
    s.evaluated = s.evaluated.wrapping_add(1);
    if s.flags & 1 != 0 { return unsafe { xm1_pipeline_image_motion_cost_hardware(threshold, x, y, search) }; }
    let mut cost = 0i32;
    let mut row = 0i32;
    while row < s.height {
        let source_row = unsafe { word(s.source) }.wrapping_add(unsafe { word(s.source + 4) }.wrapping_mul(y.0.wrapping_add(row) as u32));
        let target_row = unsafe { word(s.target) }.wrapping_add(unsafe { word(s.target + 4) }.wrapping_mul(s.origin_y.wrapping_add(row) as u32));
        let mut column = 0i32;
        while column < s.width {
            let sx = x.0.wrapping_add(column);
            let tx = s.origin_x.wrapping_add(column);
            let source = unsafe { core::ptr::read_unaligned(source_row.wrapping_add((sx.wrapping_sub(sx % 2) as u32).wrapping_mul(2)) as *const u32) };
            let target = unsafe { core::ptr::read_unaligned(target_row.wrapping_add((tx.wrapping_sub(tx % 2) as u32).wrapping_mul(2)) as *const u32) };
            cost = cost.wrapping_add(((source & 255) as i32 - (target & 255) as i32).abs()).wrapping_add((((source >> 8) & 255) as i32 - ((target >> 8) & 255) as i32).abs());
            if cost >= unsafe { (*threshold).0 } { return MotionCost(cost); }
            column = column.wrapping_add(s.sample_x);
        }
        row = row.wrapping_add(s.sample_y);
    }
    MotionCost(cost)
}

unsafe fn attempt(s: *mut MotionSearch, point: [i32; 2], best: &mut [i32; 2], cost: &mut MotionCost) {
    if unsafe { (*s).evaluated < (*s).maximum && !seen(&*s, point[0], point[1]) } {
        let candidate = unsafe { xm1_pipeline_image_motion_cost(cost, MotionCoordinate(point[0]), MotionCoordinate(point[1]), s) };
        if candidate.0 < cost.0 { *cost = candidate; *best = point; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_motion_search_local(point: *mut MotionCoordinate, score: *mut MotionCost, search: *mut MotionSearch) {
    let mut best = unsafe { [(*point).0, (*point.add(1)).0] };
    let mut cost = unsafe { *score };
    loop {
        let center = best;
        let initial_cost = cost;
        let mut improved = false;
        for direction in 0..4 {
            let mut candidate = center;
            let mut successful = center;
            let mut local_cost = initial_cost;
            let mut successes = 0;
            let mut step = if direction < 2 { 4i32 } else { 1i32 };
            loop {
                let axis = if direction < 2 { 0 } else { 1 };
                candidate[axis] = candidate[axis].wrapping_add(if direction == 1 || direction == 2 { -step } else { step });
                let within = match direction { 0 => candidate[0] < unsafe { (*search).limit_x }, 1 => candidate[0] >= 0, 2 => candidate[1] >= 0, _ => candidate[1] < unsafe { (*search).limit_y } };
                if !within || unsafe { seen(&*search, candidate[0], candidate[1]) || (*search).evaluated >= (*search).maximum } { break; }
                let value = unsafe { xm1_pipeline_image_motion_cost(&local_cost, MotionCoordinate(candidate[0]), MotionCoordinate(candidate[1]), search) };
                if value.0 >= local_cost.0 { break; }
                local_cost = value;
                successful = candidate;
                successes += 1;
                if successes == 3 { step = if direction < 2 { 24 } else { 6 }; }
            }
            if local_cost.0 < cost.0 {
                best = successful;
                cost = local_cost;
                improved = true;
            }
            if best[0] < unsafe { (*search).origin_x } { break; }
        }
        if !improved { break; }
    }
    unsafe { (*point).0 = best[0]; (*point.add(1)).0 = best[1]; *score = cost; }
}

unsafe fn prepare(s: *mut MotionSearch, best: [i32; 2], sample_key: MotionHalfKey) {
    let mut event = 0;
    unsafe {
        xm1_pipeline_image_resample_initialize();
        xm1_pipeline_image_resample_defaults();
        store(0x014e2a30, xm1_motion_current_task());
        xm1_pipeline_image_resample_start(xm1_pipeline_image_motion_complete as usize as u32);
        xm1_motion_wait(0x30, &mut event, -1);
        store(0x014e2a30, 0);
        xm1_pipeline_image_resample_cancel();
        let p = 0x014e2148;
        store(p + 0x18, (*s).target_stride as u32);
        store(p + 0x3c, (*s).origin_x as u32);
        store(p + 0x1c, (*s).source_stride as u32);
        store(p + 0x14, word((*s).source));
        store(p + 0x38, (*s).origin_y as u32);
        store(p + 0x2c, (*s).width as u32);
        store(p + 0x10, word((*s).target));
        store(p + 0x24, best[0] as u32);
        store(p + 0x20, best[1] as u32);
        store(p + 0x28, half(sample_key) as u32);
        xm1_pipeline_image_resample_set_parameters(p as *const u32);
        if cfg(MotionByteKey::Trace) & 0xc0 != 0 {
            let length = (*s).target_stride.wrapping_mul((*s).source_stride).wrapping_mul(2) as u32;
            xm1_motion_debug(0, word((*s).source) as *const u32, length, 0, 0x00dad714);
            xm1_motion_debug(0, word((*s).target) as *const u32, length, 0, 0x00dad718);
        }
    }
}

unsafe fn bind_inputs(s: *mut MotionSearch, source: *const u32, target: *const u32) {
    unsafe {
        (*s).source = source as u32;
        (*s).target = target as u32;
        for index in 0..100 { store(0x014e2a80 + index * 4, 0); }
    }
}

unsafe fn initialize(s: *mut MotionSearch, refinement: bool) -> [i32; 2] {
    unsafe {
        (*s).evaluated = 0;
        let hardware = (*s).flags & 1 != 0;
        (*s).sample_x = if hardware { 1 } else if refinement { 8 } else { 2 };
        (*s).sample_y = if hardware { half(if refinement { MotionHalfKey::RefineSample } else { MotionHalfKey::CoarseSample }) } else if refinement { 6 } else { 2 };
        let scale = if refinement { 1 } else { 4 };
        let best = [rounded_x((*s).origin_x.wrapping_add((word(0x014e2a28) as i32) / scale)), (*s).origin_y.wrapping_add((word(0x014e2a2c) as i32) / scale)];
        if hardware { prepare(s, best, if refinement { MotionHalfKey::RefineDivisor } else { MotionHalfKey::CoarseDivisor }); }
        best
    }
}

unsafe fn normalize(s: *const MotionSearch, cost: MotionCost, key: MotionHalfKey) -> u32 {
    let s = unsafe { &*s };
    let scaled = ((cost.0 as u32 & 0xffffff) << 7) as i32;
    if s.flags & 1 == 0 {
        return scaled.wrapping_div((s.height / s.sample_y).wrapping_mul(s.width / s.sample_x).wrapping_mul(2)) as u32;
    }
    let divisor = (s.sample_y.wrapping_add(s.height).wrapping_sub(1) / s.sample_y).wrapping_mul(unsafe { half(key) }).wrapping_mul(s.width);
    let result = scaled.wrapping_div(divisor) as u32;
    unsafe { xm1_pipeline_image_resample_release() };
    result
}

unsafe fn axes(s: *mut MotionSearch, best: &mut [i32; 2], cost: &mut MotionCost, range_x: i32, range_y: i32, step_x: i32, step_y: i32) {
    let center = *best;
    let mut distance = 0i32;
    loop {
        let up = center[1].wrapping_add(distance);
        if up < unsafe { (*s).limit_y } { unsafe { attempt(s, [center[0], up], best, cost) }; }
        let down = center[1].wrapping_sub(distance);
        if down >= 0 { unsafe { attempt(s, [center[0], down], best, cost) }; }
        distance = distance.wrapping_add(step_y);
        if distance > range_y { break; }
    }
    let center = *best;
    distance = 0;
    loop {
        let right = center[0].wrapping_add(distance);
        if right < unsafe { (*s).limit_x } { unsafe { attempt(s, [right, center[1]], best, cost) }; }
        let left = center[0].wrapping_sub(distance);
        if left >= unsafe { (*s).origin_x } { unsafe { attempt(s, [left, center[1]], best, cost) }; }
        distance = distance.wrapping_add(step_x);
        if distance > range_x { break; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_find_motion(source: *const u32, target: *const u32, _context: u32, result: *mut u32, variant: u32) -> u32 {
    let s = 0x014e2a34 as *mut MotionSearch;
    unsafe { bind_inputs(s, source, target) };
    let flags = unsafe { cfg(if variant == 0 { MotionByteKey::PrimaryFlags } else { MotionByteKey::AlternateFlags }) } as u8;
    unsafe { store(0x014e29d8, variant); (*s).flags = flags; }
    let range_x = unsafe { cfg(if variant == 0 { MotionByteKey::PrimaryHorizontalRange } else { MotionByteKey::AlternateHorizontalRange }) };
    let range_y = unsafe { cfg(if variant == 0 { MotionByteKey::PrimaryVerticalRange } else { MotionByteKey::AlternateVerticalRange }) };
    let step_x = unsafe { cfg(if variant == 0 { MotionByteKey::PrimaryHorizontalStep } else { MotionByteKey::AlternateHorizontalStep }) };
    let step_y = unsafe { cfg(if variant == 0 { MotionByteKey::PrimaryVerticalStep } else { MotionByteKey::AlternateVerticalStep }) };
    let mut best = unsafe { initialize(s, false) };
    let mut cost = MotionCost(i32::MAX);
    if unsafe { (*s).evaluated < (*s).maximum && !seen(&*s, best[0], best[1]) } {
        cost = unsafe { xm1_pipeline_image_motion_cost(&cost, MotionCoordinate(best[0]), MotionCoordinate(best[1]), s) };
    }
    unsafe {
        axes(s, &mut best, &mut cost, range_x, range_y, step_x, step_y);
        xm1_pipeline_image_motion_search_local(best.as_mut_ptr() as *mut MotionCoordinate, &mut cost, s);
        let x = best[0].wrapping_sub((*s).origin_x).wrapping_mul(4) as u32;
        let y = best[1].wrapping_sub((*s).origin_y).wrapping_mul(4) as u32;
        store(0x014e2a28, x);
        store(0x014e2a2c, y);
        write_volatile(result, x);
        write_volatile(result.add(1), y);
        write_volatile(result.add(2), (*s).evaluated as u32);
        normalize(s, cost, MotionHalfKey::CoarseDivisor)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_refine_motion(source: *const u32, target: *const u32, delta: *mut i32, prediction: *mut u32) -> u32 {
    let s = 0x014e29dc as *mut MotionSearch;
    unsafe { bind_inputs(s, source, target) };
    let variant = unsafe { word(0x014e29d8) };
    let flags = unsafe { cfg(if variant == 0 { MotionByteKey::PrimaryFlags } else { MotionByteKey::AlternateFlags }) } as u8;
    unsafe { (*s).flags = flags; }
    let mut best = unsafe { initialize(s, true) };
    let mut cost = MotionCost(i32::MAX);
    if unsafe { (*s).evaluated < (*s).maximum && !seen(&*s, best[0], best[1]) } {
        cost = unsafe { xm1_pipeline_image_motion_cost(&cost, MotionCoordinate(best[0]), MotionCoordinate(best[1]), s) };
    }
    unsafe {
        refine_axes(s, &mut best, &mut cost);
        xm1_pipeline_image_motion_search_local(best.as_mut_ptr() as *mut MotionCoordinate, &mut cost, s);
        let x = best[0].wrapping_sub((*s).origin_x);
        write_volatile(delta, x);
        write_volatile(delta.add(1), best[1].wrapping_sub((*s).origin_y));
        if x > 0 { store(0x014e2a28, x as u32); store(0x014e2a2c, 0); }
        write_volatile(prediction, word(0x014e2a28));
        write_volatile(prediction.add(1), word(0x014e2a2c));
        write_volatile(prediction.add(3), (*s).evaluated as u32);
        normalize(s, cost, MotionHalfKey::RefineDivisor)
    }
}

unsafe fn refine_axes(s: *mut MotionSearch, best: &mut [i32; 2], cost: &mut MotionCost) {
    let center = *best;
    let mut distance = 0i32;
    let mut range = unsafe { cfg(MotionByteKey::RefineVerticalRange) };
    while distance <= range {
        let up = center[1].wrapping_add(distance);
        if up < unsafe { (*s).limit_y } { unsafe { attempt(s, [center[0], up], best, cost) }; }
        let down = center[1].wrapping_sub(distance);
        if down >= 0 { unsafe { attempt(s, [center[0], down], best, cost) }; }
        distance = distance.wrapping_add(unsafe { cfg(MotionByteKey::RefineVerticalStep) });
        range = unsafe { cfg(MotionByteKey::RefineVerticalRange) };
    }
    let center = *best;
    distance = 0;
    loop {
        if distance > unsafe { cfg(MotionByteKey::RefineHorizontalRange) } { break; }
        let right = center[0].wrapping_add(distance);
        if right < unsafe { (*s).limit_x } { unsafe { attempt(s, [right, center[1]], best, cost) }; }
        let left = center[0].wrapping_sub(distance);
        if left >= unsafe { (*s).origin_x } { unsafe { attempt(s, [left, center[1]], best, cost) }; }
        distance = distance.wrapping_add(unsafe { cfg(MotionByteKey::RefineHorizontalStep) });
    }
}

extern "C" {
    fn xm1_motion_lock() -> u32;
    fn xm1_motion_unlock(lock: u32);
    fn xm1_motion_release_handle(handle: u32) -> i32;
    fn xm1_motion_register_interrupt(interrupt: u32, callback: unsafe extern "C" fn()) -> i32;
    fn xm1_motion_scheduler(a: u32, b: u32) -> u32;
    fn xm1_motion_resource_lock(lock: u32, timeout: i32) -> i32;
    fn xm1_motion_resource_unlock(lock: u32) -> i32;
    fn xm1_motion_acquire_resource(resource: u32, handle: *mut u32) -> i32;
    fn xm1_motion_release_resource(handle: u32) -> i32;
    fn xm1_motion_signal(task: u32, event: u32);
}

const RESAMPLE: u32 = 0x014ce72c;
const REGISTERS: u32 = 0xfff20000;

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_motion_complete() {
    let task = unsafe { word(0x014e2a30) };
    if task != 0 { unsafe { xm1_motion_signal(task, 0x10) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_acquire() {
    unsafe {
        for offset in [0, 4, 8] { store(0x00942aa4 + offset, 0); }
        store(0x00942aac, xm1_motion_resource_lock(94, -1) as u32);
        if xm1_motion_acquire_resource(46, 0x00942aa4 as *mut u32) == 0 { store(0x00942aa8, 1); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_release() {
    unsafe {
        if word(0x00942aa8) != 0 { xm1_motion_release_resource(word(0x00942aa4)); store(0x00942aa8, 0); }
        xm1_motion_resource_unlock(94);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_reset_hardware() {
    unsafe {
        store(REGISTERS, word(REGISTERS) & !1);
        while word(REGISTERS) & 1 == 0 { core::hint::spin_loop(); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_mode(mode: u32) {
    unsafe { store(REGISTERS + 4, (word(REGISTERS + 4) & !7) | if mode > 4 { 2 } else { mode }) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_finish_hardware() -> i32 {
    let status = unsafe { word(REGISTERS + 0x580) };
    unsafe { store(REGISTERS + 0x584, word(REGISTERS + 0x584) | 1) };
    if status & 1 == 0 { -255 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_dimensions(pair: *const u32) {
    unsafe { store(REGISTERS + 0x14, (read_volatile(pair) & 0x1fff) | ((read_volatile(pair.add(1)) & 0x1fff) << 16)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_pair(pair: *const u32, index: u8) {
    if index <= 9 {
        unsafe { store(REGISTERS + 0x20 + u32::from(index) * 4, (read_volatile(pair) & 0x1fff) | ((read_volatile(pair.add(1)) & 0x1fff) << 16)) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_motion_configure_candidate(a: u32, b: u32, c: u32, d: u32, enabled: u32) -> i32 {
    let pair = [b, a];
    let tail = [d, c];
    unsafe {
        xm1_pipeline_image_resample_dimensions(pair.as_ptr());
        xm1_pipeline_image_resample_pair(tail.as_ptr(), enabled as u8);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_get_status(row: u32, column: u32) -> i32 {
    unsafe { word((0xfff200a0u32).wrapping_add((row as u8 as u32).wrapping_mul(40)).wrapping_add((column as u8 as u32).wrapping_mul(4))) as i32 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_stream_flags(flags: *const u32) {
    let f = unsafe { &*(flags as *const [u32; 6]) };
    let value = (f[0] & 255) | ((f[1] & 1) << 8) | ((f[2] & 7) << 12) | ((f[3] & 255) << 16) | ((f[4] & 1) << 24) | ((f[5] & 7) << 28);
    unsafe { store(REGISTERS + 0x570, value) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_plane(configuration: *const u32) {
    let c = unsafe { &*(configuration as *const [u32; 11]) };
    unsafe {
        store(REGISTERS + 8, c[0] & 0x3fffffff);
        store(REGISTERS + 12, c[1] & 0x3fffffff);
        store(REGISTERS + 16, (c[2] & 0x3fff) | ((c[3] & 0x3fff) << 16));
        store(REGISTERS + 20, (c[4] & 0x1fff) | ((c[5] & 0x1fff) << 16));
        store(REGISTERS + 24, (c[6] & 0xfff) | ((c[7] & 0xfff) << 16));
        store(REGISTERS + 28, (c[8] & 3) | ((c[9] & 15) << 8));
        store(REGISTERS + 0x560, c[10] & 0x3fffffff);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_registers(configuration: *const u32) {
    let c = unsafe { &*(configuration as *const [u32; 81]) };
    let control = (c[24] & 3) | ((c[25] & 3) << 2) | ((c[26] & 255) << 16) | ((c[27] & 255) << 24);
    unsafe {
        store(REGISTERS + 4, ((c[1] & 1) << 7) | ((c[2] & 7) << 8) | ((c[3] & 3) << 12));
        for index in 0..10 { store(REGISTERS + 0x20 + index as u32 * 4, (c[4 + index * 2] & 0x1fff) | ((c[5 + index * 2] & 0x1fff) << 16)); }
        store(REGISTERS + 0x48, control);
        store(REGISTERS + 0x4c, (c[28] & 255) | ((c[29] & 255) << 8) | ((c[30] & 255) << 16) | ((c[31] & 255) << 24));
        store(REGISTERS + 0x50, c[32] & 255);
        store(REGISTERS + 0x54, (c[33] & 1) | ((c[34] & 1) << 1) | ((c[35] & 15) << 8) | ((c[36] & 0x1ff) << 16));
        store(REGISTERS + 0x58, (c[37] & 0x3ff) | ((c[38] & 0x3ff) << 10) | ((c[39] & 0x3ff) << 20));
        for index in 0..10 { store(REGISTERS + 0x5c + index as u32 * 4, (c[40 + index * 3] & 255) | ((c[41 + index * 3] & 255) << 8) | ((c[42 + index * 3] & 255) << 16)); }
        store(REGISTERS + 0x84, (c[70] & 127) | ((c[71] & 127) << 8));
        store(REGISTERS + 0x8c, (c[72] & 127) | ((c[73] & 127) << 8) | ((c[74] & 127) << 16) | ((c[75] & 127) << 24));
        store(REGISTERS + 0x90, (c[76] & 127) | ((c[77] & 127) << 8) | ((c[78] & 127) << 16));
        store(REGISTERS + 0x48, control);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_build(parameters: *const u32, targets: *const u32) -> u32 {
    let p = unsafe { &*(parameters as *const [u32; 91]) };
    let t = unsafe { &*(targets as *const [u32; 9]) };
    let plane = [t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], p[12], p[13], t[8]];
    unsafe { xm1_pipeline_image_resample_plane(plane.as_ptr()) };
    let mut registers = [0u32; 81];
    registers[..4].copy_from_slice(&p[..4]);
    registers[24..40].copy_from_slice(&p[34..50]);
    registers[70..81].copy_from_slice(&p[80..91]);
    for index in 0..10 {
        registers[4 + index * 2..6 + index * 2].copy_from_slice(&p[14 + index * 2..16 + index * 2]);
        registers[40 + index * 3..43 + index * 3].copy_from_slice(&p[50 + index * 3..53 + index * 3]);
    }
    unsafe { xm1_pipeline_image_resample_registers(registers.as_ptr()) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_set_parameters(parameters: *const u32) {
    unsafe {
        for index in 0..548 { store(RESAMPLE + 0x2c + index * 4, read_volatile(parameters.add(index as usize))); }
        for (destination, source, align) in [(8, 16, true), (12, 20, true), (20, 28, true), (16, 24, false), (28, 36, false), (24, 32, false), (36, 44, true), (32, 40, false), (40, 0x874, true)] {
            let value = read_volatile((parameters as *const u8).add(source) as *const u32);
            store(RESAMPLE + destination, if align { value.wrapping_add(3) & !3 } else { value });
        }
        xm1_pipeline_image_resample_build((RESAMPLE + 0x2c) as *const u32, (RESAMPLE + 8) as *const u32);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_defaults() {
    unsafe {
        for (offset, value) in [(0x2c,2),(0x30,1),(0x34,0),(0x38,0),(8,0x437cee00),(12,0x437cee00),(20,0x140),(16,0xf0),(28,1),(24,1),(36,8),(32,8),(0x5c,1),(0x60,2),(0xb4,0),(0xb8,0),(0xbc,1),(0xc0,0x40),(0xc4,0),(0xc8,u32::MAX),(0xcc,0),(0xd0,u32::MAX),(0xd4,4),(0xd8,0),(0xdc,0),(0xe0,5),(0xe4,0x80),(0xe8,0),(0xec,0),(0xf0,0),(0x16c,0x20),(0x170,0x3c),(0x174,6),(0x178,12),(0x17c,24),(0x180,6),(0x184,6),(0x188,6),(0x18c,12),(0x190,0),(0x194,0)] { store(RESAMPLE + offset, value); }
        xm1_pipeline_image_resample_build((RESAMPLE + 0x2c) as *const u32, (RESAMPLE + 8) as *const u32);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_initialize() {
    unsafe {
        xm1_pipeline_image_resample_acquire();
        for index in 0..561 { store(RESAMPLE + index * 4, 0); }
        xm1_pipeline_image_resample_reset_hardware();
        store(REGISTERS + 0x584, 0x331);
        xm1_pipeline_image_resample_defaults();
    }
}

unsafe fn release_active() {
    unsafe {
        if word(RESAMPLE + 4) != 0 {
            if xm1_motion_release_handle(word(RESAMPLE)) != 0 { loop { core::hint::spin_loop(); } }
            store(RESAMPLE + 4, 0);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_cancel() -> u32 {
    unsafe {
        let lock = xm1_motion_lock();
        store(RESAMPLE + 0x8c0, 0);
        xm1_motion_unlock(lock);
        release_active();
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_complete() {
    unsafe {
        let lock = xm1_motion_lock();
        store(RESAMPLE + 0x8bc, word(RESAMPLE + 0x8bc).wrapping_add(1));
        xm1_pipeline_image_resample_finish_hardware();
        store(REGISTERS + 0x588, 0);
        release_active();
        let callback = word(RESAMPLE + 0x8c0);
        xm1_motion_unlock(lock);
        if callback != 0 { core::mem::transmute::<u32, unsafe extern "C" fn()>(callback)(); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_start(callback: u32) -> u32 {
    unsafe {
        store(RESAMPLE + 0x8c0, callback);
        let width = word(RESAMPLE + 0x14);
        let format = word(RESAMPLE + 0x34);
        let span = match format { 2 => width.wrapping_mul(3).wrapping_add(31) >> 5, 3 => width.wrapping_add(7) >> 3, _ => width.wrapping_mul(2).wrapping_add(7) >> 3 };
        let alignment = if span & 15 == 0 { 4 } else { span.trailing_zeros().min(3) };
        let flags = [0, 0, alignment, 0, 0, alignment];
        xm1_pipeline_image_resample_stream_flags(flags.as_ptr());
        store(REGISTERS + 0x588, 1);
        store(RESAMPLE + 4, 1);
        if xm1_motion_register_interrupt(0x66, xm1_pipeline_image_resample_complete) != 0 { loop { core::hint::spin_loop(); } }
        xm1_motion_scheduler(0, 0);
        let mode = word(RESAMPLE + 0x2c);
        xm1_pipeline_image_resample_mode(mode);
        mode
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_release_channel() -> u32 {
    unsafe { xm1_pipeline_image_resample_release() };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_read_status(output: *mut u32, row: u8, column: u8) {
    unsafe {
        if row > 9 || column > 9 { write_volatile(output, u32::MAX); }
        write_volatile(output, word((REGISTERS + 0xa0).wrapping_add(u32::from(row) * 40).wrapping_add(u32::from(column) * 4)));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_status_word(output: *mut u32) {
    unsafe { write_volatile(output, word(REGISTERS + 0x580)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_acknowledge() {
    unsafe { store(REGISTERS + 0x584, word(REGISTERS + 0x584) | 1) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_initialize_control() {
    unsafe { store(REGISTERS + 0x584, 0x331) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_command_one() {
    unsafe { store(REGISTERS + 0x588, 1) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_resample_command_zero() {
    unsafe { store(REGISTERS + 0x588, 0) };
}
