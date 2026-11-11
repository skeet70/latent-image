use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Configuration(Address32);
impl Configuration {
    unsafe fn byte(self, offset: u32) -> u32 { unsafe { self.0.offset(ByteOffset(offset)).byte() as u32 } }
    unsafe fn half(self, offset: u32) -> u32 { unsafe { self.0.offset(ByteOffset(offset)).half() as u32 } }
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.0.offset(ByteOffset(offset)).word() } }
}
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Workspace(Address32);
#[derive(Clone, Copy)]
#[repr(u32)]
enum Plane { SourceX = 0, SourceY = 0x800, TargetX = 0x1000, TargetY = 0x1800, RoundedX = 0x2000, RoundedY = 0x2800, RoundedU = 0x3000, RoundedV = 0x3800, SelectedX = 0x4000, SelectedY = 0x4800, SelectedU = 0x5000, SelectedV = 0x5800, OriginalIndex = 0x6400 }
const RAW: [Plane; 4] = [Plane::SourceX, Plane::SourceY, Plane::TargetX, Plane::TargetY];
const ROUNDED: [Plane; 4] = [Plane::RoundedX, Plane::RoundedY, Plane::RoundedU, Plane::RoundedV];
const SELECTED: [Plane; 4] = [Plane::SelectedX, Plane::SelectedY, Plane::SelectedU, Plane::SelectedV];
impl Workspace {
    unsafe fn get(self, plane: Plane, index: u32) -> u32 { unsafe { self.0.offset(ByteOffset((plane as u32).wrapping_add(index.wrapping_mul(4)))).word() } }
    unsafe fn put(self, plane: Plane, index: u32, value: u32) { unsafe { self.0.offset(ByteOffset((plane as u32).wrapping_add(index.wrapping_mul(4)))).write_word(value) } }
    unsafe fn flag(self, index: u32, bits: u8) { let p = self.0.offset(ByteOffset(0x6200 + index)); unsafe { p.write_byte(p.byte() | bits) }; }
    unsafe fn invalidate(self, index: u32) { for plane in ROUNDED { unsafe { self.put(plane, index, 0xffff) }; } }
    unsafe fn select(self, destination: u32, source: u32) { for i in 0..4 { unsafe { self.put(SELECTED[i], destination, self.get(ROUNDED[i], source)) }; } }
}
fn magnitude(value: u32) -> u32 { if (value as i32) < 0 { value.wrapping_neg() } else { value } }
fn distance(a: u32, b: u32) -> u32 { magnitude(a.wrapping_sub(b)) }
fn clamp(value: u32) -> u32 { (value as i32).clamp(-0x1ffff, 0x1ffff) as u32 }
unsafe fn identity(matrix: Address32, scale: u32) { for i in 0..6 { unsafe { matrix.offset(ByteOffset(i * 4)).write_word(if i == 0 || i == 3 { scale } else { 0 }) }; } }
unsafe fn read_matrix(matrix: Address32) -> [u32; 6] { let mut result = [0; 6]; for (i, value) in result.iter_mut().enumerate() { *value = unsafe { matrix.offset(ByteOffset(i as u32 * 4)).word() }; } result }
unsafe fn store_matrix(matrix: Address32, values: [u32; 6]) { for (i, value) in values.into_iter().enumerate() { unsafe { matrix.offset(ByteOffset(i as u32 * 4)).write_word(value) }; } }
unsafe fn residual(work: Workspace, index: u32, matrix: &[u32; 6], shift: u32) -> [u32; 2] {
    let x = unsafe { work.get(Plane::RoundedX, index) }; let y = unsafe { work.get(Plane::RoundedY, index) };
    [unsafe { work.get(Plane::RoundedU, index) }.wrapping_shl(shift).wrapping_sub(x.wrapping_mul(matrix[0])).wrapping_sub(y.wrapping_mul(matrix[1])).wrapping_sub(matrix[4]), unsafe { work.get(Plane::RoundedV, index) }.wrapping_shl(shift).wrapping_sub(x.wrapping_mul(matrix[2])).wrapping_sub(y.wrapping_mul(matrix[3])).wrapping_sub(matrix[5])]
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_affine_grid_valid(source: Address32, target: Address32, matrix: Address32, workspace: Address32, options: Address32) -> i32 {
    let config = Configuration(options); let work = Workspace(workspace);
    let count = unsafe { config.byte(0) * config.byte(1) };
    let bit = unsafe { config.byte(40) }; let mask = if bit < 32 { (1u32 << bit) as u8 } else { 0 };
    let mut used = 0u32;
    for index in 0..count {
        if unsafe { workspace.offset(ByteOffset(0x6200 + index)).byte() } & mask == 0 { for plane in RAW { unsafe { work.put(plane, index, 0xffff) }; } }
        else {
            let a = unsafe { source.offset(ByteOffset(index * 4)).word() }; let b = unsafe { target.offset(ByteOffset(index * 4)).word() };
            for (plane, value) in RAW.into_iter().zip([a >> 16, a & 0xffff, b >> 16, b & 0xffff]) { unsafe { work.put(plane, index, (value + 4) >> 3) }; }
            used += 1;
        }
    }
    let m = unsafe { read_matrix(matrix) }; let mut tight = 0; let mut broad = 0;
    for index in 0..count {
        let x = unsafe { work.get(Plane::SourceX, index) }; if x == 0xffff { continue; }
        let y = unsafe { work.get(Plane::SourceY, index) };
        let ex = magnitude(unsafe { work.get(Plane::TargetX, index) }.wrapping_shl(16).wrapping_sub(x.wrapping_mul(m[0]).wrapping_add(y.wrapping_mul(m[1])).wrapping_add(m[4]))) as i32;
        let ey = magnitude(unsafe { work.get(Plane::TargetY, index) }.wrapping_shl(16).wrapping_sub(x.wrapping_mul(m[2]).wrapping_add(y.wrapping_mul(m[3])).wrapping_add(m[5]))) as i32;
        if ex < unsafe { config.byte(41) } as i32 * 0xffff && ey < unsafe { config.byte(41) } as i32 * 0xffff { tight += 1; }
        if ex < unsafe { config.byte(42) } as i32 * 0xffff && ey < unsafe { config.byte(42) } as i32 * 0xffff { broad += 1; }
    }
    i32::from(tight * 100 >= unsafe { config.byte(43) } * used && broad * 100 >= unsafe { config.byte(44) } * used)
}

unsafe fn estimate(source: Address32, target: Address32, offset_x: i32, offset_y: i32, matrix: Address32, workspace: Address32, options: Address32, completion: &mut i32) -> bool {
    let config = Configuration(options); let work = Workspace(workspace);
    let rows = unsafe { config.byte(0) }; let columns = unsafe { config.byte(1) }; let count = rows * columns;
    let selected_rows = unsafe { config.byte(4) }; let selected_columns = unsafe { config.byte(5) }; let selected_count = selected_rows * selected_columns;
    if count > 512 || selected_count > 512 { return false; }
    for index in 0..512 { unsafe { workspace.offset(ByteOffset(0x6000 + index)).write_byte(0) }; }
    let mut valid = 0u32;
    for index in 0..count {
        let a = unsafe { source.offset(ByteOffset(index * 4)).word() }; let b = unsafe { target.offset(ByteOffset(index * 4)).word() };
        unsafe { workspace.offset(ByteOffset(0x6200 + index)).write_byte(0) };
        let active = a >> 16 != 0xffff && b >> 16 != 0xffff;
        for (i, value) in [a >> 16, a & 0xffff, b >> 16, b & 0xffff].into_iter().enumerate() {
            unsafe { work.put(RAW[i], index, if active { value.wrapping_add((if i & 1 == 0 { offset_x } else { offset_y } as u32).wrapping_shl(3)) } else { 0xffff }) };
        }
        if active { valid += 1; unsafe { work.flag(index, 1) }; }
    }
    if valid < unsafe { config.half(2) } { return false; }
    let mut stationary = 0;
    for index in 0..count {
        if unsafe { work.get(Plane::SourceX, index) } != 0xffff && unsafe { work.get(Plane::TargetX, index) } != 0xffff && distance(unsafe { work.get(Plane::SourceX, index) }, unsafe { work.get(Plane::TargetX, index) }) < unsafe { config.byte(6) } && distance(unsafe { work.get(Plane::SourceY, index) }, unsafe { work.get(Plane::TargetY, index) }) < unsafe { config.byte(6) } { stationary += 1; }
    }
    if stationary * 100 > unsafe { config.byte(7) } * valid { unsafe { identity(matrix, 0x10000) }; return true; }
    let mut rejected = 0u32; let sparse = unsafe { config.word(8) } == 1;
    for index in 0..count {
        if unsafe { work.get(Plane::SourceX, index) } == 0xffff || unsafe { work.get(Plane::TargetX, index) } == 0xffff { unsafe { work.invalidate(index) }; continue; }
        if sparse {
            let row = index / columns; let column = index % columns; let radius = unsafe { config.byte(12) };
            let left = column.min(radius) as i32; let right = if column >= columns.wrapping_sub(radius) { columns - 1 - column } else { radius } as i32;
            let top = row.min(radius) as i32; let bottom = if row >= rows.wrapping_sub(radius) { rows - 1 - row } else { radius } as i32;
            let mut agreeing = 0i32; let mut neighbors = 0i32;
            for dx in -left..=right { for dy in -top..=bottom {
                let neighbor = (row.wrapping_add(dy as u32)).wrapping_mul(columns).wrapping_add(column).wrapping_add(dx as u32);
                if dx == 0 && dy == 0 || unsafe { work.get(Plane::SourceX, neighbor) } == 0xffff || unsafe { work.get(Plane::TargetX, neighbor) } == 0xffff { continue; }
                let mx = unsafe { work.get(Plane::TargetX, index).wrapping_sub(work.get(Plane::SourceX, index)) }; let my = unsafe { work.get(Plane::TargetY, index).wrapping_sub(work.get(Plane::SourceY, index)) };
                if (distance(unsafe { work.get(Plane::TargetX, neighbor).wrapping_sub(work.get(Plane::SourceX, neighbor)) }, mx) as i32) < unsafe { config.half(14) } as i32 && (distance(unsafe { work.get(Plane::TargetY, neighbor).wrapping_sub(work.get(Plane::SourceY, neighbor)) }, my) as i32) < unsafe { config.half(14) } as i32 { agreeing += 1; }
                neighbors += 1;
            } }
            unsafe { workspace.offset(ByteOffset(0x6000 + index)).write_byte(if neighbors != 0 { (agreeing * 127 / neighbors - 128) as u8 } else { 0 }) };
            if neighbors > 2 && agreeing * 100 < neighbors * unsafe { config.byte(16) } as i32 { unsafe { work.invalidate(index) }; rejected += 1; continue; }
        }
        for i in 0..4 { unsafe { work.put(ROUNDED[i], index, work.get(RAW[i], index).wrapping_add(4) >> 3) }; }
        if sparse { unsafe { work.flag(index, 2) }; }
    }
    if sparse && (valid.wrapping_sub(rejected) < unsafe { config.half(18) } || rejected * 100 > 100u32.wrapping_sub(unsafe { config.byte(20) }).wrapping_mul(valid)) { return false; }
    if unsafe { config.word(24) } == 1 {
        let (row_step, column_step) = if selected_count != count {
            (unsafe { __aeabi_uidiv(rows, selected_rows) } & 255, unsafe { __aeabi_uidiv(columns, selected_columns) } & 255)
        } else { (0, 0) };
        for row in 0..selected_rows { for column in 0..selected_columns {
            let destination = column * selected_rows + row;
            if selected_count == count { unsafe { work.select(destination, row * selected_columns + column) }; continue; }
            let mut found = None; let mut best = 0;
            'search: for y in row * row_step..(row + 1) * row_step { for x in column * column_step..(column + 1) * column_step {
                let candidate = y * columns + x;
                if unsafe { work.get(Plane::RoundedU, candidate) } == 0xffff { continue; }
                if unsafe { config.word(8) } == 0 { found = Some(candidate); break 'search; }
                let confidence = unsafe { workspace.offset(ByteOffset(0x6000 + candidate)).byte() };
                if unsafe { work.get(Plane::RoundedX, candidate) } != 0xffff && confidence > best { found = Some(candidate); best = confidence; }
            } }
            if let Some(index) = found { unsafe { work.select(destination, index) }; } else { for plane in SELECTED { unsafe { work.put(plane, destination, 0xffff) }; } }
        } }
        let mut compact = 0;
        for index in 0..count { if unsafe { work.get(Plane::RoundedX, index) } != 0xffff { for plane in ROUNDED { unsafe { work.put(plane, compact, work.get(plane, index)) }; } unsafe { work.put(Plane::OriginalIndex, compact, index) }; compact += 1; } }
        for index in compact..count { unsafe { work.invalidate(index) }; }
        let mut best = 0i32; let mut chosen = [0x4000, 0, 0, 0x4000, 0, 0];
        let limit = unsafe { config.byte(28) } << 14;
        for first in 0..selected_count.saturating_sub(3) {
            let x = unsafe { work.get(Plane::SelectedX, first) }; if x == 0xffff { continue; }
            let y = unsafe { work.get(Plane::SelectedY, first) }; let u = unsafe { work.get(Plane::SelectedU, first) }; let v = unsafe { work.get(Plane::SelectedV, first) };
            let separated = |index: u32| (index / selected_rows).wrapping_sub(first / selected_rows).wrapping_add(1) > 2 || (index % selected_rows).wrapping_sub(first % selected_rows).wrapping_add(1) > 2;
            for second in first + 1..selected_count.saturating_sub(1) {
                if unsafe { work.get(Plane::SelectedX, second) } == 0xffff || !separated(second) { continue; }
                let dx = unsafe { work.get(Plane::SelectedX, second) }.wrapping_sub(x); let dy = unsafe { work.get(Plane::SelectedY, second) }.wrapping_sub(y);
                for third in second + 1..selected_count {
                    if unsafe { work.get(Plane::SelectedX, third) } == 0xffff || !separated(third) { continue; }
                    let ex = unsafe { work.get(Plane::SelectedX, third) }.wrapping_sub(x); let ey = unsafe { work.get(Plane::SelectedY, third) }.wrapping_sub(y);
                    let mut area = ey.wrapping_mul(dx).wrapping_sub(ex.wrapping_mul(dy)) as i32;
                    let threshold = unsafe { config.byte(35) } as i32; if area < threshold && area > -threshold { continue; }
                    let du = unsafe { work.get(Plane::SelectedU, second) }.wrapping_sub(u); let dv = unsafe { work.get(Plane::SelectedV, second) }.wrapping_sub(v);
                    let eu = unsafe { work.get(Plane::SelectedU, third) }.wrapping_sub(u); let ev = unsafe { work.get(Plane::SelectedV, third) }.wrapping_sub(v);
                    let mut numerators = [du.wrapping_mul(ey).wrapping_sub(eu.wrapping_mul(dy)), eu.wrapping_mul(dx).wrapping_sub(du.wrapping_mul(ex)), dv.wrapping_mul(ey).wrapping_sub(ev.wrapping_mul(dy)), ev.wrapping_mul(dx).wrapping_sub(dv.wrapping_mul(ex))];
                    if area < 0 { area = area.wrapping_neg(); for value in &mut numerators { *value = value.wrapping_neg(); } }
                    let mut hypothesis = [0; 6]; let mut usable = true;
                    for i in 0..4 { let value = numerators[i] as i32; if (magnitude(value as u32) as i32) > area.wrapping_mul(8) { usable = false; break; } let scaled = (value as i64) * 0x4000 + if value < 0 { -((area >> 1) as i64) } else { (area >> 1) as i64 }; hypothesis[i] = unsafe { __aeabi_ldivmod(scaled, area as i64) } as u32; }
                    if !usable { continue; }
                    let bx = (u.wrapping_shl(14) as i32 as i64) - hypothesis[0] as i32 as i64 * x as i32 as i64 - hypothesis[1] as i32 as i64 * y as i32 as i64;
                    let by = (v.wrapping_shl(14) as i32 as i64) - hypothesis[2] as i32 as i64 * x as i32 as i64 - hypothesis[3] as i32 as i64 * y as i32 as i64;
                    if !(-0x7fffffff..=0x7fffffff).contains(&bx) || !(-0x7fffffff..=0x7fffffff).contains(&by) { continue; }
                    hypothesis[4] = bx as u32; hypothesis[5] = by as u32;
                    if hypothesis[0] as i32 >= 0x8000 || hypothesis[3] as i32 >= 0x8000 || hypothesis[1].wrapping_add(0x1fff) > 0x3ffe || hypothesis[2].wrapping_add(0x1fff) > 0x3ffe { continue; }
                    let mut support = 0;
                    for index in 0..compact { let error = unsafe { residual(work, index, &hypothesis, 14) }; if (magnitude(error[0]) as i32) < limit as i32 && (magnitude(error[1]) as i32) < limit as i32 { support += 1; } }
                    if support > best { chosen = hypothesis; best = support; }
                }
            }
        }
        unsafe { store_matrix(matrix, chosen) };
        for index in 0..compact { let error = unsafe { residual(work, index, &chosen, 14) }; if magnitude(error[0]) < limit && magnitude(error[1]) < limit { unsafe { work.flag(work.get(Plane::OriginalIndex, index), 4) }; } else { unsafe { work.invalidate(index) }; rejected += 1; } }
        if valid.wrapping_sub(rejected) <= unsafe { config.byte(29) } || rejected * 100 >= 100u32.wrapping_sub(unsafe { config.byte(30) }).wrapping_mul(valid) { return false; }
        for (i, value) in chosen.iter_mut().enumerate() { *value = value.wrapping_mul(4); if i < 4 { *value = clamp(*value); } }
        unsafe { store_matrix(matrix, chosen) };
    }
    for _ in 0..unsafe { config.byte(31) } {
        let mut sums = [0u64; 11]; let mut used = 0u32;
        for index in 0..count {
            for plane in RAW { unsafe { work.put(plane, index, 0xffff) }; }
            if unsafe { work.get(Plane::RoundedU, index) } == 0xffff { continue; }
            let x = unsafe { work.get(Plane::RoundedX, index) }; let y = unsafe { work.get(Plane::RoundedY, index) }; let u = unsafe { work.get(Plane::RoundedU, index) }; let v = unsafe { work.get(Plane::RoundedV, index) };
            for (plane, value) in RAW.into_iter().zip([x, y, u, v]) { unsafe { work.put(plane, used, value) }; } used += 1;
            for (sum, value) in sums.iter_mut().zip([x.wrapping_mul(x), x.wrapping_mul(y), y.wrapping_mul(y), x, y, x.wrapping_mul(u), y.wrapping_mul(u), x.wrapping_mul(v), y.wrapping_mul(v), u, v]) { *sum = sum.wrapping_add(value as u64); }
        }
        let [xx, xy, yy, x, y, xu, yu, xv, yv, u, v] = sums.map(|value| value as f64); let n = used as f64;
        let cxx = n * yy - y * y; let cxy = x * y - n * xy; let cxt = y * xy - x * yy;
        let determinant = (cxx * xx + xy * cxy) + x * cxt; let threshold = unsafe { config.byte(35) } as f64;
        if determinant < threshold && determinant > -threshold { return false; }
        let cyy = n * xx - x * x; let cyt = x * xy - y * xx;
        let a = ((cxx * xu + cxy * yu) + cxt * u) / determinant; let b = ((cxy * xu + cyy * yu) + cyt * u) / determinant;
        let c = ((cxx * xv + cxy * yv) + cxt * v) / determinant; let d = ((cxy * xv + cyy * yv) + cyt * v) / determinant;
        if used == 0 { return false; }
        let fit = [a, b, c, d, ((u - x * a) - y * b) / n, ((v - x * c) - y * d) / n];
        let candidate = fit.map(|value| if value > 2147483647.0 { 0x7fffffff } else if value < -2147483647.0 { 0x80000001 } else { (value * 65536.0) as i32 as u32 });
        if candidate[0].wrapping_mul(2) as i32 > 0x30000 || (candidate[0] as i32) < 0x8000 || candidate[3].wrapping_mul(2) as i32 > 0x30000 || (candidate[3] as i32) < 0x8000 || candidate[1].wrapping_add(0x8000) > 0x10000 || candidate[2].wrapping_add(0x8000) > 0x10000 || candidate[4].wrapping_add(0x1fff0000) > 0x3ffe0000 || candidate[5].wrapping_add(0x1fff0000) > 0x3ffe0000 { return false; }
        for index in 0..count { if unsafe { work.get(Plane::RoundedU, index) } == 0xffff { continue; } let error = unsafe { residual(work, index, &candidate, 16) }; let limit = unsafe { config.byte(32) } << 16; if magnitude(error[0]) > limit || magnitude(error[1]) > limit { unsafe { work.invalidate(index) }; rejected += 1; } }
        if valid.wrapping_sub(rejected) <= unsafe { config.byte(33) } || rejected * 100 >= 100u32.wrapping_sub(unsafe { config.byte(34) }).wrapping_mul(valid) { return false; }
        unsafe { store_matrix(matrix, core::array::from_fn(|i| if i < 4 { clamp(candidate[i]) } else { candidate[i] })) };
    }
    if unsafe { config.word(36) } == 1 { *completion = unsafe { xm1_pipeline_image_affine_grid_valid(source, target, matrix, workspace, options) }; }
    true
}

unsafe extern "C" {
    fn __aeabi_ldivmod(numerator: i64, denominator: i64) -> i64;
    fn __aeabi_uidiv(numerator: u32, denominator: u32) -> u32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_estimate_transform(source: Address32, target: Address32, offset_x: i32, offset_y: i32, matrix: Address32, workspace: Address32, options: Address32) -> i32 {
    let mut completion = 1;
    if unsafe { estimate(source, target, offset_x, offset_y, matrix, workspace, options, &mut completion) } { completion } else { unsafe { identity(matrix, 0x10000) }; 0 }
}
