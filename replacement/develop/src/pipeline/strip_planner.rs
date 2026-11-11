use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct GeometryRequest(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct GeometryMargins(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct GeometryDimensions(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct StripPlan(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct GeometryKind(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PixelColumn(pub u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct StripSelectionCode(pub u32);

#[derive(Clone, Copy)]
enum StripSelection { Main, Tail }

impl StripSelectionCode {
    fn parse(self) -> StripSelection { match self.0 { 1 => StripSelection::Tail, _ => StripSelection::Main } }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Engine { Paired = 1, Extended = 2, Primary = 3, Secondary = 4, Simple = 5, Profile = 6, Alternate = 7 }

#[derive(Clone, Copy)]
struct Mode { input: u32, output: u32, engine: Option<Engine>, subsampling: u32, profile: u32, filter: u32, scale: u32, alternate: u32 }

unsafe fn get(p: Address32, index: u32) -> u32 { unsafe { p.offset(ByteOffset(index * 4)).word() } }
unsafe fn put(p: Address32, index: u32, value: u32) { unsafe { p.offset(ByteOffset(index * 4)).write_word(value) } }
unsafe fn mode(kind: GeometryKind) -> Mode {
    let p = Address32(0xda93c8).offset(ByteOffset(kind.0.wrapping_mul(40)));
    let raw = unsafe { get(p, 2) };
    Mode { input: unsafe { get(p, 0) }, output: unsafe { get(p, 1) }, engine: match raw { 1 => Some(Engine::Paired), 2 => Some(Engine::Extended), 3 => Some(Engine::Primary), 4 => Some(Engine::Secondary), 5 => Some(Engine::Simple), 6 => Some(Engine::Profile), 7 => Some(Engine::Alternate), _ => None }, subsampling: unsafe { get(p, 3) }, profile: unsafe { get(p, 4) }, filter: unsafe { get(p, 5) }, scale: unsafe { get(p, 6) }, alternate: unsafe { get(p, 8) } }
}
fn factor(m: Mode) -> u32 { match m.subsampling { 2 => 4, 1 => 2, _ => 1 } }
fn shift(n: u32) -> u32 { if n & 255 < 32 { 1 << (n & 255) } else { 0 } }
unsafe extern "C" { fn image_operation_divide_fault(value: u32) -> !; }
fn divide_fault(a: u32) -> ! { unsafe { image_operation_divide_fault(if a == 0 { 0 } else { u32::MAX }) } }
fn div(a: u32, b: u32) -> u32 { if b == 0 { divide_fault(a) } else { a / b } }
fn rem(a: u32, b: u32) -> u32 { if b == 0 { divide_fault(a) } else { a % b } }
#[inline(never)]
#[export_name = "xm1_pipeline_image_geometry_fault"]
fn fault() -> ! { loop { core::hint::spin_loop() } }
fn round(a: u32, b: u32) -> u32 { div(a.wrapping_sub(1).wrapping_add(b), b).wrapping_mul(b) }
fn margin_round(a: u32, b: u32) -> u32 { let n = a.wrapping_add(b - 1); n.wrapping_add(if (n as i32) < 0 { b - 1 } else { 0 }) & !(b - 1) }
fn alignment(a: u32) -> u32 { let mut n = 128; while rem(a, n) != 0 { n >>= 1; } n }
fn dividing_strip(total: u32, maximum: u32, unit: u32) -> Option<u32> { let mut n = div(maximum, unit).wrapping_mul(unit); while n >= unit { if rem(total, n) == 0 { return Some(n); } n -= unit; } None }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_margins(request: GeometryRequest, margins: GeometryMargins) {
    let m = unsafe { mode(GeometryKind(get(request.0, 0))) };
    for i in 0..6 { unsafe { put(margins.0, i, 0) }; }
    match m.engine {
        Some(Engine::Paired) => for i in 0..5 { unsafe { put(margins.0, i, 9) }; },
        Some(Engine::Primary) => unsafe { put(margins.0, 5, factor(m) - 1) },
        Some(Engine::Simple | Engine::Profile | Engine::Alternate) => for i in 0..4 { unsafe { put(margins.0, i, 8) }; },
        _ => (),
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_dimensions(request: GeometryRequest, margins: GeometryMargins, dimensions: GeometryDimensions) {
    let r = request.0; let d = dimensions.0; let g = margins.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) };
    let f = factor(m);
    let unit = if m.input == 15 || m.output == 15 { 16 } else if m.input == 10 || m.output == 10 { 8 } else if m.input == 20 || m.output == 40 { 2 } else { 4 };
    let x = margin_round(unsafe { get(g, 0).max(get(g, 1)) }, unit);
    let y = unsafe { get(g, 2).max(get(g, 3)) };
    unsafe { put(d, 10, x); put(d, 11, y); put(d, 14, margin_round(get(g, 5), unit)); }
    let padded = matches!(m.engine, Some(Engine::Extended | Engine::Primary));
    let px = if padded { round(x, f * 8).wrapping_sub(x) } else { 0 };
    let py = if padded { round(y, f).wrapping_sub(y) } else { 0 };
    unsafe { put(d, 12, px); put(d, 13, py); }
    let centered = matches!(m.engine, Some(Engine::Paired | Engine::Primary | Engine::Secondary));
    let cx = if centered { x.wrapping_add(px).wrapping_add(div(unsafe { get(r, 23) }.wrapping_mul(10), m.output) >> 1).wrapping_add(if m.engine == Some(Engine::Primary) { 0 } else { div(unsafe { get(r, 25) }.wrapping_mul(10), m.input) }) } else { 0 };
    let cy = if centered { y.wrapping_add(py).wrapping_add(unsafe { get(r, 24) } >> 1) } else { 0 };
    unsafe { put(d, 5, cx); put(d, 6, cy); put(d, 7, if centered { cx.wrapping_mul(m.input) / 10 * 2 } else { get(r, 20) }); put(d, 8, if centered { cy.wrapping_mul(2) } else { get(r, 15) }); }
    if centered {
        for i in 0..3 { unsafe { put(d, i, get(r, i + 1).wrapping_sub(get(r, i + 10).wrapping_mul(cy).wrapping_add(m.input.wrapping_mul(cx) / 10))) }; }
        let base = unsafe { get(r, 4) };
        let fourth = if base == 0 { 0 } else if m.engine == Some(Engine::Primary) { base.wrapping_sub(div(cx, f)).wrapping_sub(unsafe { get(r, 13) }.wrapping_mul(div(cy, f))) } else { base.wrapping_sub(unsafe { get(r, 13) }.wrapping_mul(cy).wrapping_add(m.input.wrapping_mul(cx) / 10)) };
        unsafe { put(d, 3, fourth) };
    } else { for i in 0..5 { unsafe { put(d, i, get(r, i + 5)) }; } }
    let width = unsafe { get(r, 23) };
    let a = shift(6u32.wrapping_sub(m.scale));
    let b = shift(m.filter);
    let output_width = match m.engine {
        Some(Engine::Paired | Engine::Primary | Engine::Secondary) => if unsafe { get(r, 25) } == 0 { width.wrapping_add(px.wrapping_mul(m.output) / 10) } else { let combined = width.wrapping_add(unsafe { get(r, 25) }); if m.output == 15 { round(combined, 24) } else { combined.wrapping_add(7) & !7 } },
        Some(Engine::Extended) => round(width, if matches!(unsafe { get(r, 0) }, 1..=3) { 64 } else { 8 }),
        Some(Engine::Simple | Engine::Profile) => { let scale = if m.engine == Some(Engine::Simple) { a } else { b }; m.output.wrapping_mul(div(div(unsafe { get(d, 7) }.wrapping_mul(10), m.input).wrapping_sub(17).wrapping_add(scale), scale)) / 10 },
        Some(Engine::Alternate) => round(m.output.wrapping_mul(div(unsafe { get(d, 7) }.wrapping_mul(10), m.input).wrapping_sub(16)) / 10, if unsafe { get(r, 0) } == 10 { 32 } else { 8 }),
        None => unsafe { get(d, 9) },
    };
    if m.engine.is_some() { unsafe { put(d, 9, output_width) }; }
    if m.engine == Some(Engine::Simple) {
        let parameters = Address32(unsafe { get(r, 31) });
        unsafe { put(parameters, 7, rem(a.wrapping_sub(rem(div(get(d, 7).wrapping_mul(10), m.input).wrapping_sub(16), a)), a)); put(parameters, 8, rem(a.wrapping_sub(rem(get(d, 8).wrapping_sub(16), a)), a)); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_strips(request: GeometryRequest, _margins: GeometryMargins, dimensions: GeometryDimensions, plan: StripPlan) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let kind = GeometryKind(unsafe { get(r, 0) }); let m = unsafe { mode(kind) }; let f = factor(m);
    let a = shift(6u32.wrapping_sub(m.scale)); let b = shift(m.filter); let c = shift(m.alternate);
    let mut budget = m.input.wrapping_mul(if matches!(m.engine, Some(Engine::Paired | Engine::Extended | Engine::Primary | Engine::Secondary)) { 512 } else { 256 }) / 10;
    let border = unsafe { get(d, 10) }.wrapping_mul(m.input) / 10;
    let borders = border.wrapping_mul(2);
    if matches!(m.engine, Some(Engine::Extended | Engine::Primary)) { budget = div(budget.wrapping_sub(borders), f * 16).wrapping_mul(f * 16); }
    else if matches!(m.engine, Some(Engine::Paired | Engine::Secondary | Engine::Simple | Engine::Profile)) { budget = budget.wrapping_sub(borders); }
    let total = unsafe { get(d, 9) };
    let width = match m.engine {
        Some(Engine::Paired) => Some(total.min(m.output.wrapping_mul(div(div(budget, f).wrapping_mul(10), m.input)) / 10)),
        Some(Engine::Primary) => { let n = total.min(m.output.wrapping_mul(div(budget.wrapping_mul(10), m.input).wrapping_sub(unsafe { get(d, 14) })) / 10); Some(div(div(n.wrapping_mul(10), m.output), f * 8).wrapping_mul(f * m.output * 8) / 10) },
        Some(Engine::Extended) => dividing_strip(total, m.output.wrapping_mul(div(budget.wrapping_mul(10).wrapping_sub(960), m.input)) / 10, if matches!(kind.0, 1..=3) { 64 } else { 8 }),
        Some(Engine::Simple) => Some(total.min((div(a.wrapping_sub(1).wrapping_add(div(budget.wrapping_mul(10), m.input)), a) >> 1).wrapping_mul(m.output.wrapping_mul(2)) / 10)),
        Some(Engine::Profile) => { let mut n = (div(a.wrapping_sub(1).wrapping_add(div(budget.wrapping_mul(10), m.input)), a) & !1).min(div(total.wrapping_mul(10), m.output)); let mut rows = div(a.wrapping_mul(n), b); if rows > 64 && m.filter != 0 { n = div(b.wrapping_mul(64), a).wrapping_add(1) & !1; rows = div(a.wrapping_mul(n), b); } unsafe { put(p, 5, n.wrapping_mul(40) / 10) }; Some(rows) },
        Some(Engine::Alternate) => { let available = div(budget.wrapping_mul(10), m.input); let mut n = div(available, c); let edge = div(borders.wrapping_mul(10), m.input); if available.wrapping_sub(borders) < n { n = available.wrapping_sub(edge.wrapping_mul(2)); } else { n = n.wrapping_sub(edge); } n = if n < edge { n.wrapping_add(edge) } else { n.wrapping_add(edge) & !15 }; unsafe { put(p, 5, n) }; let raw = n.wrapping_sub(edge).wrapping_mul(m.output.wrapping_mul(c)) / 10; unsafe { put(p, 7, raw) }; let selected = dividing_strip(total, raw, if kind.0 == 10 { 32 } else { 8 }); if let Some(width) = selected { unsafe { put(p, 7, width) }; } unsafe { put(p, 5, edge.wrapping_add(div(div(get(p, 7).wrapping_mul(10), m.output), c))) }; None },
        _ => Some(total.min(m.output.wrapping_mul(div(budget.wrapping_mul(10), m.input).wrapping_sub(unsafe { get(d, 14) })) / 10)),
    };
    if let Some(width) = width { unsafe { put(p, 7, width) }; }
    match m.engine {
        Some(Engine::Paired | Engine::Extended | Engine::Primary | Engine::Secondary) => unsafe { put(p, 9, get(r, 24).wrapping_add(get(d, 11).wrapping_mul(2))) },
        Some(Engine::Simple) => unsafe { put(p, 9, div(get(d, 8).wrapping_sub(1).wrapping_sub(get(d, 11).wrapping_mul(2)).wrapping_add(a), a)) },
        Some(Engine::Profile) => { let rows = div(unsafe { get(d, 8).wrapping_sub(1).wrapping_sub(get(d, 11).wrapping_mul(2)) }.wrapping_add(b), b); unsafe { put(p, 9, rows); put(p, 6, div(b.wrapping_mul(rows), a).wrapping_add(1) & !1); } },
        Some(Engine::Alternate) => unsafe { put(p, 9, get(d, 8).wrapping_sub(get(d, 11).wrapping_mul(2))) },
        _ => (),
    }
    let width = unsafe { get(p, 7) };
    let align = if m.output == 15 { let mut n = 128; loop { let divisor = if n >= 4 { n * 3 } else { 0 }; if rem(width, divisor) == 0 { break n; } n >>= 1; } } else { alignment(width) };
    unsafe { put(p, 12, align); put(p, 2, div(total, width)); }
    if matches!(m.engine, Some(Engine::Extended | Engine::Alternate)) && rem(total, width) != 0 { fault(); }
    let input_bytes = match m.engine {
        Some(Engine::Paired) => borders.wrapping_add(m.input.wrapping_mul(div(f.wrapping_mul(width).wrapping_mul(10), m.output)) / 10),
        Some(Engine::Extended) => borders.wrapping_add(m.input.wrapping_mul(div(width.wrapping_mul(10).wrapping_add(960), m.output)) / 10),
        Some(Engine::Simple) => borders.wrapping_add(a.wrapping_mul(m.input).wrapping_mul(div(width.wrapping_mul(10), m.output)) / 10),
        Some(Engine::Profile) => borders.wrapping_add(b.wrapping_mul(m.input).wrapping_mul(div(width.wrapping_mul(10), m.output)) / 10),
        Some(Engine::Alternate) => { let n = unsafe { get(p, 5) }.wrapping_mul(c); if div(border.wrapping_mul(20), m.input).wrapping_add(n) <= div(budget.wrapping_mul(10), m.input) { borders.wrapping_add(m.input.wrapping_mul(n) / 10) } else { budget } },
        _ => borders.wrapping_add(m.input.wrapping_mul(div(width.wrapping_mul(10), m.output).wrapping_add(unsafe { get(d, 14) })) / 10),
    };
    let input_bytes = if matches!(m.engine, Some(Engine::Extended | Engine::Primary)) { round(input_bytes, f * 16) } else { input_bytes.wrapping_add(7) & !7 };
    if input_bytes & 7 != 0 { fault(); }
    unsafe { put(p, 3, input_bytes); put(p, 4, if matches!(m.engine, Some(Engine::Paired | Engine::Extended | Engine::Primary | Engine::Secondary)) { get(p, 9) } else { get(d, 8) }); }
    let input_alignment = match m.engine { Some(Engine::Primary) => alignment(div(input_bytes, f) >> 1), Some(Engine::Paired | Engine::Secondary) => alignment(input_bytes), _ => 128 };
    unsafe { put(p, 11, input_alignment); put(p, 8, if matches!(m.engine, Some(Engine::Paired | Engine::Primary | Engine::Secondary)) { rem(total, width) } else { 0 }); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_adjust(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan, tail: StripPlan) {
    let m = unsafe { mode(GeometryKind(get(request.0, 0))) }; let f = factor(m);
    for i in 0..13 { unsafe { put(tail.0, i, get(plan.0, i)) }; }
    let width = unsafe { get(plan.0, 8) };
    if width & 7 != 0 { fault(); }
    unsafe { put(tail.0, 7, width); put(tail.0, 12, alignment(width)); put(tail.0, 2, 1); }
    let scaled = div((if m.engine == Some(Engine::Paired) { f.wrapping_mul(unsafe { get(plan.0, 7) }) } else { width }).wrapping_mul(10), m.output);
    let input = (unsafe { get(dimensions.0, 10) }.wrapping_mul(m.input) / 10).wrapping_mul(2).wrapping_add(m.input.wrapping_mul(scaled) / 10);
    let input = if matches!(m.engine, Some(Engine::Extended | Engine::Primary)) { round(input, f * 16) } else { input.wrapping_add(7) & !7 };
    if input & 7 != 0 { fault(); }
    unsafe { put(tail.0, 3, input); put(tail.0, 11, alignment(if matches!(m.engine, Some(Engine::Extended | Engine::Primary)) { div(input, f) >> 1 } else { input })); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_build_strip_request(request: GeometryRequest, plan: StripPlan, tail: StripPlan) -> i32 {
    let margins = GeometryMargins(Address32(0x14ce600)); let dimensions = GeometryDimensions(Address32(0x14ce680));
    unsafe { xm1_pipeline_image_geometry_margins(request, margins); xm1_pipeline_image_geometry_dimensions(request, margins, dimensions); xm1_pipeline_image_geometry_strips(request, margins, dimensions, plan); }
    if unsafe { get(plan.0, 8) } != 0 { unsafe { xm1_pipeline_image_geometry_adjust(request, dimensions, plan, tail) }; }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_mode(kind: GeometryKind) -> u32 { unsafe { get(Address32(0xda93c8).offset(ByteOffset(kind.0.wrapping_mul(40))), 2) } }

#[repr(C)]
struct DevicePayload<const N: usize> { words: [u32; N] }
unsafe extern "C" {
    fn strip_device_paired(payload: *const DevicePayload<27>);
    fn strip_device_extended(payload: *const DevicePayload<26>);
    fn strip_device_primary(payload: *const DevicePayload<24>);
    fn strip_device_secondary(payload: *const DevicePayload<28>);
    fn strip_device_simple(payload: *const DevicePayload<33>);
    fn strip_device_profile(payload: *const DevicePayload<66>);
    fn strip_device_alternate(payload: *const DevicePayload<52>);
}
fn burst_code(bytes: u32) -> u32 { match bytes >> 3 { 16 => 4, 8 => 3, 4 => 2, 2 => 1, _ => 0 } }
unsafe fn plane(r: Address32, d: Address32, index: u32, offset: PixelColumn, scale: u32) -> [u32; 2] {
    if unsafe { get(r, index + 1) } == 0 { [0, 0] } else { [unsafe { get(d, index) }.wrapping_add(scale.wrapping_mul(offset.0) / 10), div(unsafe { get(r, index + 10) }.wrapping_mul(10), scale)] }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_paired(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan, offset: PixelColumn) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) };
    let parameters = Address32(unsafe { get(r, 28) });
    if parameters.0 == 0 || unsafe { get(r, 1) } == 0 || unsafe { get(r, 2) } == 0 { fault(); }
    let rows = div(unsafe { get(p, 7) }.wrapping_mul(10), m.output);
    let first = unsafe { plane(r, d, 0, offset, m.input) }; let second = unsafe { plane(r, d, 1, offset, m.input) }; let third = unsafe { plane(r, d, 2, offset, m.input) };
    let payload = DevicePayload { words: [
        first[0], first[1], second[0], second[1], third[0], third[1],
        unsafe { get(r, 21) }.wrapping_add(m.output.wrapping_mul(offset.0) / 10), div(unsafe { get(r, 22) }.wrapping_mul(10), m.output),
        unsafe { get(p, 2) }.wrapping_mul(rows).wrapping_add(unsafe { get(d, 10) }.wrapping_mul(2)),
        unsafe { get(d, 8) }, unsafe { get(d, 10) }, unsafe { get(d, 11) },
        burst_code(unsafe { get(p, 11) }), burst_code(unsafe { get(p, 12) }), 0, 0, 0, 0,
        div(unsafe { get(p, 3) }.wrapping_mul(10), m.input), rows, unsafe { get(r, 26) },
        unsafe { get(parameters, 4) }, unsafe { get(parameters, 5) }, unsafe { get(parameters, 2) }, unsafe { get(parameters, 3) }, unsafe { get(parameters, 1) }, unsafe { get(parameters, 0) },
    ] };
    unsafe { strip_device_paired(&payload) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_extended(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan, offset: PixelColumn) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) };
    let parameters = unsafe { get(r, 29) };
    if parameters == 0 { fault(); }
    let input_offset = m.input.wrapping_mul(offset.0) / 10;
    let payload = DevicePayload { words: [
        unsafe { get(d, 0) }.wrapping_add(input_offset), div(unsafe { get(r, 10) }.wrapping_mul(10), m.input),
        unsafe { get(d, 1) }.wrapping_add(input_offset), div(unsafe { get(r, 11) }.wrapping_mul(10), m.input),
        unsafe { get(d, 2) }.wrapping_add(input_offset), div(unsafe { get(r, 12) }.wrapping_mul(10), m.input),
        unsafe { get(d, 3) }.wrapping_add(input_offset), div(unsafe { get(r, 13) }.wrapping_mul(10), m.input),
        unsafe { get(r, 21) }.wrapping_add(m.output.wrapping_mul(offset.0) / 10), div(unsafe { get(r, 22) }.wrapping_mul(10), m.output),
        unsafe { get(d, 4) }.wrapping_add(div(offset.0, factor(m))), unsafe { get(r, 14) },
        div(unsafe { get(d, 7) }.wrapping_mul(10), m.input), unsafe { get(d, 8) }, unsafe { get(d, 10) }, unsafe { get(d, 11) },
        burst_code(unsafe { get(p, 11) }), burst_code(unsafe { get(p, 12) }), 0, 0, 0, 0,
        div(unsafe { get(p, 3) }.wrapping_mul(10), m.input), div(unsafe { get(p, 7) }.wrapping_mul(10), m.output), m.subsampling, parameters,
    ] };
    unsafe { strip_device_extended(&payload) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_primary(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan, offset: PixelColumn) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) };
    if unsafe { get(r, 1) } == 0 || unsafe { get(r, 2) } == 0 { fault(); }
    let rows = div(unsafe { get(p, 7) }.wrapping_mul(10), m.output);
    let first = unsafe { plane(r, d, 0, offset, m.input) }; let second = unsafe { plane(r, d, 1, offset, m.input) }; let third = unsafe { plane(r, d, 2, offset, m.input) };
    let fourth = if unsafe { get(r, 4) } == 0 { [0, 0] } else { [unsafe { get(d, 3) }.wrapping_add(div(offset.0, factor(m)).wrapping_mul(if m.subsampling == 1 || m.subsampling == 2 { 1 } else { 2 })), unsafe { get(r, 13) }] };
    let payload = DevicePayload { words: [
        first[0], first[1], second[0], second[1], third[0], third[1], fourth[0], fourth[1],
        unsafe { get(r, 21) }.wrapping_add(m.output.wrapping_mul(offset.0) / 10), div(unsafe { get(r, 22) }.wrapping_mul(10), m.output),
        unsafe { get(p, 2) }.wrapping_mul(rows).wrapping_add(unsafe { get(d, 10) }.wrapping_mul(2)),
        unsafe { get(d, 8) }, unsafe { get(d, 10) }, unsafe { get(d, 11) },
        burst_code(unsafe { get(p, 11) }), burst_code(unsafe { get(p, 12) }), 0, 0, 0, 0,
        div(unsafe { get(p, 3) }.wrapping_mul(10), m.input), rows, unsafe { get(r, 26) }, m.subsampling,
    ] };
    unsafe { strip_device_primary(&payload) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_secondary(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan, offset: PixelColumn) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) };
    let parameters = Address32(unsafe { get(r, 30) }); if parameters.0 == 0 { fault(); }
    let rows = div(unsafe { get(p, 7) }.wrapping_mul(10), m.output);
    let first = unsafe { plane(r, d, 0, offset, m.input) }; let second = unsafe { plane(r, d, 1, offset, m.input) }; let third = unsafe { plane(r, d, 2, offset, m.input) }; let fourth = unsafe { plane(r, d, 3, offset, m.input) };
    let payload = DevicePayload { words: [
        first[0], first[1], second[0], second[1], third[0], third[1], fourth[0], fourth[1],
        unsafe { get(r, 21) }.wrapping_add(m.output.wrapping_mul(offset.0) / 10), div(unsafe { get(r, 22) }.wrapping_mul(10), m.output),
        unsafe { get(p, 2) }.wrapping_mul(rows).wrapping_add(unsafe { get(d, 10) }.wrapping_mul(2)),
        unsafe { get(d, 8) }, unsafe { get(d, 10) }, unsafe { get(d, 11) },
        burst_code(unsafe { get(p, 11) }), burst_code(unsafe { get(p, 12) }), 0, 0, 0, 0,
        div(unsafe { get(p, 3) }.wrapping_mul(10), m.input), rows, unsafe { get(r, 26) }, m.profile,
        unsafe { get(parameters, 0) }, unsafe { get(parameters, 1) }, unsafe { get(parameters, 2) }, unsafe { get(parameters, 3) },
    ] };
    unsafe { strip_device_secondary(&payload) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_simple(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) }; let parameters = Address32(unsafe { get(r, 31) });
    if parameters.0 == 0 { fault(); }
    let payload = DevicePayload { words: [
        unsafe { get(d, 0) }, div(unsafe { get(r, 10) }.wrapping_mul(10), m.input), unsafe { get(d, 1) }, div(unsafe { get(r, 11) }.wrapping_mul(10), m.input),
        unsafe { get(r, 21) }, div(unsafe { get(r, 22) }.wrapping_mul(10), m.output),
        div(unsafe { get(d, 7) }.wrapping_mul(10), m.input), unsafe { get(d, 8) }, div(unsafe { get(d, 9) }.wrapping_mul(10), m.output), unsafe { get(p, 9) },
        4, 0, 0, 0, 0, 0, 4, burst_code(unsafe { get(p, 12) }), 4, 0,
        div(unsafe { get(p, 3) }.wrapping_mul(10), m.input), div(unsafe { get(p, 7) }.wrapping_mul(10), m.output),
        8, unsafe { get(parameters, 1) }, unsafe { get(parameters, 2) }, unsafe { get(parameters, 3) },
        unsafe { get(Address32(0xda93c8).offset(ByteOffset(get(r, 0).wrapping_mul(40))), 7) }, m.scale,
        unsafe { get(parameters, 4) }, unsafe { get(parameters, 5) }, unsafe { get(parameters, 6) }, unsafe { get(parameters, 8) }, unsafe { get(parameters, 7) },
    ] };
    unsafe { strip_device_simple(&payload) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_profile(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) }; let parameters = Address32(unsafe { get(r, 32) });
    if parameters.0 == 0 { fault(); }
    let mut payload = DevicePayload { words: [0; 66] };
    let w = &mut payload.words;
    w[0] = unsafe { get(d, 0) }; w[1] = div(unsafe { get(r, 10) }.wrapping_mul(10), m.input);
    w[2] = unsafe { get(d, 1) }; w[3] = div(unsafe { get(r, 11) }.wrapping_mul(10), m.input);
    w[4] = unsafe { get(d, 2) }; w[5] = (unsafe { get(r, 12) }.wrapping_mul(10) as i32 / 40) as u32;
    w[6] = unsafe { get(r, 21) }; w[7] = div(unsafe { get(r, 22) }.wrapping_mul(10), m.output);
    w[8] = div(unsafe { get(d, 7) }.wrapping_mul(10), m.input); w[9] = unsafe { get(d, 8) };
    w[12] = div(unsafe { get(d, 9) }.wrapping_mul(10), m.output);
    w[10] = div(w[12].wrapping_mul(shift(m.filter)), shift(6u32.wrapping_sub(m.scale))).wrapping_add(1) & !1;
    w[11] = unsafe { get(p, 6) }; w[13] = unsafe { get(p, 9) };
    w[14] = 4; w[20] = 4; w[22] = 4; w[23] = burst_code(unsafe { get(p, 12) });
    w[24] = div(unsafe { get(p, 3) }.wrapping_mul(10), m.input); w[25] = div(unsafe { get(p, 7) }.wrapping_mul(10), m.output);
    w[26] = unsafe { get(p, 5) }.wrapping_mul(10) / 40; w[27] = 8;
    for i in 0..4 { w[28 + i] = unsafe { get(parameters, (i + 1) as u32) }; }
    w[32] = m.filter; w[33] = m.scale;
    for i in 0..32 { w[34 + i] = unsafe { get(parameters, (i + 5) as u32) }; }
    unsafe { strip_device_profile(&payload) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_program_alternate(request: GeometryRequest, dimensions: GeometryDimensions, plan: StripPlan) {
    let r = request.0; let d = dimensions.0; let p = plan.0;
    let m = unsafe { mode(GeometryKind(get(r, 0))) }; let parameters = Address32(unsafe { get(r, 33) });
    if parameters.0 == 0 { fault(); }
    let mut payload = [core::mem::MaybeUninit::<u32>::uninit(); 52];
    let mut set = |index: usize, value: u32| { payload[index].write(value); };
    // The original alternate request leaves words 12 and 13 indeterminate.
    for index in 0..52 { if index != 12 && index != 13 { set(index, 0); } }
    set(0, unsafe { get(d, 0) }); set(1, div(unsafe { get(r, 10) }.wrapping_mul(10), m.input));
    set(2, unsafe { get(d, 1) }); set(3, div(unsafe { get(r, 11) }.wrapping_mul(10), m.input));
    set(4, unsafe { get(d, 2) }); set(5, unsafe { get(r, 12) });
    set(6, unsafe { get(r, 21) }); set(7, div(unsafe { get(r, 22) }.wrapping_mul(10), m.output));
    set(8, div(unsafe { get(d, 7) }.wrapping_mul(10), m.input)); set(9, unsafe { get(d, 8) });
    set(10, div(div(unsafe { get(d, 9) }, shift(m.alternate)).wrapping_mul(10), m.input)); set(11, div(unsafe { get(p, 9) }, shift(m.alternate)));
    set(14, burst_code(unsafe { get(p, 11) })); set(15, burst_code(unsafe { get(p, 12) })); set(20, 4); set(22, 4);
    set(24, div(unsafe { get(p, 3) }.wrapping_mul(10), m.input)); set(25, div(unsafe { get(p, 7) }.wrapping_mul(10), m.output)); set(26, unsafe { get(p, 5) });
    set(27, 8); set(28, 16); set(29, m.profile); set(30, unsafe { get(parameters, 0) }); set(31, m.alternate);
    set(32, unsafe { get(parameters, 1) }); set(33, unsafe { get(parameters, 2) });
    for i in 3..20 { set(32 + i, unsafe { get(parameters, i as u32) }); }
    unsafe { strip_device_alternate(payload.as_ptr().cast()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_dispatch(request: GeometryRequest, selection: StripSelectionCode) -> i32 {
    let state = Address32(0x14ce570); let m = unsafe { mode(GeometryKind(get(request.0, 0))) };
    let (plan, offset) = match selection.parse() { StripSelection::Tail => (StripPlan(Address32(0x14ce64c)), PixelColumn(unsafe { get(state, 44) }.wrapping_mul(div(unsafe { get(state, 49) }.wrapping_mul(10), m.output)))), StripSelection::Main => (StripPlan(Address32(0x14ce618)), PixelColumn(0)) };
    let dimensions = GeometryDimensions(Address32(0x14ce680));
    unsafe { match m.engine {
        Some(Engine::Paired) => xm1_pipeline_image_geometry_program_paired(request, dimensions, plan, offset),
        Some(Engine::Extended) => xm1_pipeline_image_geometry_program_extended(request, dimensions, plan, offset),
        Some(Engine::Primary) => xm1_pipeline_image_geometry_program_primary(request, dimensions, plan, offset),
        Some(Engine::Secondary) => xm1_pipeline_image_geometry_program_secondary(request, dimensions, plan, offset),
        Some(Engine::Simple) => xm1_pipeline_image_geometry_program_simple(request, dimensions, plan),
        Some(Engine::Profile) => xm1_pipeline_image_geometry_program_profile(request, dimensions, plan),
        Some(Engine::Alternate) => xm1_pipeline_image_geometry_program_alternate(request, dimensions, plan),
        None => (),
    } Address32(0xfff480c8).write_word(0); }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_finish() -> u32 {
    let m = unsafe { xm1_pipeline_image_geometry_mode(GeometryKind(get(Address32(0x14ce570), 2))) };
    (unsafe { Address32(0xfff48004).word() } & !7) | (m & 7)
}

unsafe extern "C" {
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn xm1_pipeline_image_wait_reserved(resource: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(resource: u32) -> i32;
    fn xm1_pipeline_image_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_pipeline_image_route_acquire(index: u32, handle: Address32) -> i32;
    fn xm1_pipeline_image_route_release(handle: u32) -> i32;
    fn xm1_pipeline_image_interrupt_acquire(source: u32, handler: u32, handle: Address32) -> i32;
    fn xm1_pipeline_image_interrupt_release(handle: u32) -> i32;
    fn strip_cache_sync(address: u32, length: u32);
    fn strip_device_reset();
}

unsafe fn clear_words(address: Address32, count: u32) { for index in 0..count { unsafe { put(address, index, 0) }; } }
unsafe fn reset_device() { unsafe { strip_device_reset() }; }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_initialize() -> i32 {
    let hardware = Address32(0x96eb9c);
    unsafe {
        clear_words(hardware, 3);
        put(hardware, 2, xm1_pipeline_image_wait_reserved(44, u32::MAX) as u32);
        xm1_pipeline_image_route_request(0x405, 17, 8);
        let status = xm1_pipeline_image_route_acquire(22, hardware);
        if status == 0 { put(hardware, 1, 1); }
        reset_device();
        Address32(0xfff480c4).write_word(0x331);
        clear_words(Address32(0x14ce570), 83);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_release() -> i32 {
    let hardware = Address32(0x96eb9c);
    unsafe {
        reset_device();
        if get(hardware, 1) != 0 { xm1_pipeline_image_route_release(get(hardware, 0)); put(hardware, 1, 0); }
        xm1_pipeline_image_route_request(0x405, 17, 0);
        xm1_pipeline_image_signal_reserved(44);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_begin() {
    let engine = unsafe { xm1_pipeline_image_geometry_mode(GeometryKind(get(Address32(0x14ce570), 2))) };
    unsafe {
        strip_cache_sync(0, 0);
        Address32(0x942d8c).write_word(engine);
        Address32(0xfff480c8).write_word(1);
        let control = Address32(0xfff48004);
        control.write_word((control.word() & !7) | (engine & 7));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_wait() -> i32 {
    let state = Address32(0x14ce570);
    unsafe { reset_device() };
    if unsafe { get(state, 1) } == 0 { return 0; }
    let status = unsafe { xm1_pipeline_image_interrupt_release(get(state, 0)) };
    if status == 0 { unsafe { put(state, 1, 0) }; }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_irq() {
    let state = Address32(0x14ce570);
    let token = unsafe { xm1_develop_enter_critical() };
    unsafe { put(state, 42, get(state, 42).wrapping_add(1)) };
    let tail = unsafe { get(state, 50) } != 0;
    if tail { unsafe { put(state, 50, 0) }; }
    unsafe {
        let pending = Address32(0xfff480c0).word();
        Address32(0xfff480c4).write_word(pending);
        xm1_develop_leave_critical(token);
        reset_device();
    }
    if tail {
        unsafe { xm1_pipeline_image_geometry_dispatch(GeometryRequest(Address32(0x14ce578)), StripSelectionCode(1)); xm1_pipeline_image_geometry_begin(); }
    } else {
        let token = unsafe { xm1_develop_enter_critical() };
        let callback = unsafe { get(state, 29) };
        unsafe { put(state, 29, 0); xm1_develop_leave_critical(token); }
        if callback != 0 {
            let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback as usize) };
            unsafe { callback() };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_geometry_configure(request: GeometryRequest) -> i32 {
    let state = Address32(0x14ce570);
    for index in 0..34 { unsafe { put(Address32(0x14ce578), index, get(request.0, index)) }; }
    unsafe { xm1_pipeline_image_geometry_build_strip_request(request, StripPlan(Address32(0x14ce618)), StripPlan(Address32(0x14ce64c))); }
    let status = unsafe { xm1_pipeline_image_geometry_dispatch(request, StripSelectionCode(0)) };
    if status != 0 { return status; }
    let status = unsafe { xm1_pipeline_image_interrupt_acquire(61, xm1_pipeline_image_geometry_irq as *const () as u32, state) };
    if status != 0 { return status; }
    unsafe { put(state, 1, 1) };
    0
}
