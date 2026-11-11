use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Workspace(Address32);
impl Workspace {
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn word(self, offset: u32) -> i32 { (unsafe { self.field(offset).word() }) as i32 }
    unsafe fn store(self, offset: u32, value: i32) { unsafe { self.field(offset).write_word(value as u32) }; }
    unsafe fn half(self, offset: u32) -> i16 { (unsafe { self.field(offset).half() }) as i16 }
    unsafe fn store_half(self, offset: u32, value: i16) { unsafe { self.field(offset).write_half(value as u16) }; }
}

#[derive(Clone, Copy, Default)]
#[repr(transparent)]
struct Q4(i32);
impl Q4 { fn integer(self) -> i32 { self.0 / 16 } }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_divide(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        let saturated = if numerator > 0 { i32::MAX } else if numerator < 0 { i32::MIN } else { 0 };
        unsafe { redeye_divide_fault(saturated) };
    }
    numerator.wrapping_div(denominator)
}

unsafe fn divide(numerator: i32, denominator: i32) -> i32 { unsafe { xm1_pipeline_image_redeye_divide(numerator, denominator) } }

fn mask_fraction(position: Q4) -> (i32, i32) {
    let integer = position.integer();
    (integer, position.0.wrapping_mul(1024).wrapping_div(16).wrapping_sub(integer.wrapping_mul(1024)))
}

unsafe fn sample_mask(mask: Address32, stride: i32, x: Q4, y: Q4) -> i32 {
    let (column, horizontal) = mask_fraction(x);
    let (row, vertical) = mask_fraction(y);
    let address = mask.offset(ByteOffset(row.wrapping_mul(stride).wrapping_add(column) as u32));
    let a = unsafe { address.byte() } as i32;
    let b = unsafe { address.offset(ByteOffset(1)).byte() } as i32;
    let c = unsafe { address.offset(ByteOffset(stride as u32)).byte() } as i32;
    let d = unsafe { address.offset(ByteOffset(stride.wrapping_add(1) as u32)).byte() } as i32;
    let top = (1024i32.wrapping_sub(horizontal)).wrapping_mul(a).wrapping_add(horizontal.wrapping_mul(b));
    let bottom = (1024i32.wrapping_sub(horizontal)).wrapping_mul(c).wrapping_add(horizontal.wrapping_mul(d));
    (1024i32.wrapping_sub(vertical)).wrapping_mul(top).wrapping_add(vertical.wrapping_mul(bottom)) / 0x100000
}

unsafe fn luma_weight(pixels: Address32, pair_row: i32, column: i32, mask: i32) -> i32 {
    let address = pixels.offset(ByteOffset(pair_row.wrapping_add(column >> 1).wrapping_mul(4) as u32));
    let y = unsafe { address.offset(ByteOffset((column & 1) as u32)).byte() } as i32;
    let u = unsafe { address.offset(ByteOffset(2)).byte() } as i32;
    let v = unsafe { address.offset(ByteOffset(3)).byte() } as i32;
    let red = (v.wrapping_mul(198).wrapping_sub(y.wrapping_mul(98)).wrapping_sub(u.wrapping_mul(43)) >> 8).wrapping_add(99);
    let value = ((mask.wrapping_mul(red) >> 8).wrapping_sub(128)).wrapping_mul(2).max(0);
    value.wrapping_mul(value) >> 6
}

unsafe fn blend_byte(address: Address32, gain: i32, weight: i32, chroma: bool) {
    let original = unsafe { address.byte() } as i32;
    let adjusted = if chroma { (gain.wrapping_mul(original.wrapping_sub(128)) >> 8).wrapping_add(128) } else { gain.wrapping_mul(original) >> 8 };
    let output = (original.wrapping_mul(256i32.wrapping_sub(weight)).wrapping_add(adjusted.wrapping_mul(weight)) >> 8).clamp(0, 255);
    unsafe { address.write_byte(output as u8) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_apply_pixels(edits: *const u8, pixels: u32, stride_pixels: i32, _height: i32, scale: i16) {
    let edits = Workspace(Address32(edits as u32));
    let pixels = Address32(pixels);
    let mut index = 0;
    while index < unsafe { edits.word(0x460) } {
        let edit = Workspace(edits.field(index as u32 * 0x38));
        let diameter = unsafe { divide(edit.word(16).wrapping_mul(0x2800), i32::from(scale)) };
        let center_x = unsafe { divide(edit.word(12).wrapping_mul(0x2800), i32::from(scale)) };
        let center_y = unsafe { divide(edit.word(20).wrapping_mul(0x2800), i32::from(scale)) };
        let radius = diameter.wrapping_sub(1) >> 1;
        let left = Q4(center_x).integer().wrapping_sub(radius);
        let top = Q4(center_y).integer().wrapping_sub(radius);
        let aligned_left = left & !1;
        let row_width = (left.wrapping_add(diameter).wrapping_add(1) & !1).wrapping_sub(aligned_left);
        let mut row = 0;
        while row < diameter {
            let y = 8i32.wrapping_sub(center_y).wrapping_add(top.wrapping_add(row).wrapping_mul(16));
            let pair_row = (stride_pixels >> 1).wrapping_mul(top.wrapping_add(row));
            let mut column = 0;
            while column < row_width {
                let x = 8i32.wrapping_sub(center_x).wrapping_add(aligned_left.wrapping_add(column).wrapping_mul(16));
                let mask_width = unsafe { edit.word(40) };
                let mask_height = unsafe { edit.word(36) };
                let mask_x = unsafe { divide(mask_width.wrapping_mul(x), diameter) }.wrapping_add((mask_width.wrapping_sub(1) >> 1).wrapping_mul(16));
                let mask_y = unsafe { divide(mask_height.wrapping_mul(y), diameter) }.wrapping_add((mask_height.wrapping_sub(1) >> 1).wrapping_mul(16));
                if mask_x >= 0 && mask_y >= 0 {
                    let next_x = unsafe { divide(mask_width.wrapping_mul(x.wrapping_add(16)), diameter) }.wrapping_add((mask_width.wrapping_sub(1) >> 1).wrapping_mul(16));
                    if next_x < mask_width.wrapping_sub(1).wrapping_mul(16) && mask_y < mask_height.wrapping_sub(1).wrapping_mul(16) {
                        let mask = Address32(unsafe { edit.word(32) } as u32);
                        let first = unsafe { sample_mask(mask, mask_width, Q4(mask_x), Q4(mask_y)) };
                        let second = unsafe { sample_mask(mask, mask_width, Q4(next_x), Q4(mask_y)) };
                        let chroma = first.wrapping_add(second) >> 1;
                        let source_column = left.wrapping_add(column);
                        let first_weight = unsafe { luma_weight(pixels, pair_row, source_column, first) };
                        let second_weight = unsafe { luma_weight(pixels, pair_row, source_column.wrapping_add(1), second) };
                        let output = pixels.offset(ByteOffset(stride_pixels.wrapping_mul(top.wrapping_add(row)).wrapping_add(aligned_left).wrapping_mul(2).wrapping_add(column.wrapping_mul(2)) as u32));
                        unsafe {
                            blend_byte(output, edit.word(0), first_weight, false);
                            blend_byte(output.offset(ByteOffset(1)), edit.word(0), second_weight, false);
                            blend_byte(output.offset(ByteOffset(2)), edit.word(4), chroma, true);
                            blend_byte(output.offset(ByteOffset(3)), edit.word(8), chroma, true);
                        }
                    }
                }
                column = column.wrapping_add(2);
            }
            row = row.wrapping_add(1);
        }
        index += 1;
    }
}

unsafe fn record_at(records: Workspace, index: i32) -> Workspace {
    if index < unsafe { records.word(8) } {
        Workspace(Address32(unsafe { records.word(0) }.wrapping_add(unsafe { records.word(4) }.wrapping_mul(index)) as u32))
    } else { Workspace(Address32(0)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_scale_edits(records: *const u8, edits: *mut u8, scale: i32) {
    let records = Workspace(Address32(records as u32));
    let edits = Workspace(Address32(edits as u32));
    unsafe { edits.store(0x460, 0) };
    let mut count = 0;
    let mut index = 0;
    while index < unsafe { records.word(12) } {
        let record = unsafe { record_at(records, index) };
        for side in 0..2 {
            let selection = unsafe { record.half(20) };
            if i32::from(selection) != side && selection != 2 { continue; }
            let edit = Workspace(edits.field(count * 0x38));
            let raw_diameter = unsafe { divide(0x4924, i32::from(record.half(60 + side as u32 * 2))) }.wrapping_add(1) & !1;
            let diameter = unsafe { divide(raw_diameter.wrapping_shl(10), scale) };
            unsafe {
                edit.store(16, diameter); edit.store(24, diameter);
                edit.store(12, divide(record.word(44 + side as u32 * 4).wrapping_shl(10), scale).wrapping_sub(8));
                edit.store(20, divide(record.word(52 + side as u32 * 4).wrapping_shl(10), scale).wrapping_sub(8));
                edit.store_half(48, record.half(64 + side as u32 * 2));
                edit.store_half(50, record.half(68)); edit.store_half(52, side as i16);
            }
            count += 1;
            unsafe { edits.store(0x460, count as i32) };
            if count > 19 { return; }
        }
        index += 1;
    }
}

unsafe extern "C" { fn redeye_divide_fault(value: i32) -> !; }
core::arch::global_asm!(include_str!("../../RedEyeBackend_stock.S"));

unsafe fn descriptor(workspace: Workspace, offset: u32, arena: u32, width: i32, height: i32) {
    unsafe { workspace.store(offset, arena as i32); workspace.store(offset + 4, width); workspace.store(offset + 8, height); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_init_buffers(arena: u32, workspace: *mut u8) -> u32 {
    let workspace = Workspace(Address32(workspace as u32));
    unsafe { workspace.store(0x460, 0) };
    for index in 0..20 {
        let edit = Workspace(workspace.field(index * 0x38));
        unsafe {
            edit.store(44, 1); edit.store(40, 64); edit.store(36, 64);
            edit.store(32, arena.wrapping_add(index * 0x1000) as i32);
        }
    }
    unsafe {
        workspace.store(0x47c, 100); workspace.store(0x478, 24);
        workspace.store(0x474, arena.wrapping_add(0x14000) as i32);
        workspace.store(0x46c, 20); workspace.store(0x468, 72);
        workspace.store(0x464, arena.wrapping_add(0x14960) as i32);
    }
    0x14f00
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_primary_buffer(arena: u32, workspace: *mut u8) -> u32 {
    unsafe { descriptor(Workspace(Address32(workspace as u32)), 0x55c, arena, 24, 1000) };
    0x5dc0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_secondary_buffer(arena: u32, workspace: *mut u8) -> u32 {
    unsafe { descriptor(Workspace(Address32(workspace as u32)), 0x594, arena, 72, 500) };
    0x8ca0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_stage_buffers(arena: u32, workspace: *mut u8) -> u32 {
    let workspace = Workspace(Address32(workspace as u32));
    for (offset, displacement, extent) in [(0x5a4, 0, 20), (0x5b0, 800, 40), (0x5bc, 4000, 64), (0x5c8, 0x2fa0, 64)] {
        unsafe { descriptor(workspace, offset, arena.wrapping_add(displacement), extent, extent) };
    }
    unsafe { workspace.store(0x5d4, 1); descriptor(workspace, 0x9d8, arena.wrapping_add(0x3fa0), 24, 200); }
    0x5260
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_pyramid_buffers(arena: u32, workspace: *mut u8) -> u32 {
    let workspace = Workspace(Address32(workspace as u32));
    unsafe { descriptor(workspace, 0x9e8, arena, 64, 64) };
    for index in 0..4 {
        let offset = 0x9f4 + index * 16;
        unsafe { descriptor(workspace, offset, arena.wrapping_add(0x2000 + index * 0x1000), 64, 64); workspace.store(offset + 12, 1); }
    }
    0x6000
}

unsafe fn copy_bytes(destination: Address32, source: Address32, length: i32) {
    let mut offset = 0;
    while offset < length {
        unsafe { destination.offset(ByteOffset(offset as u32)).write_byte(source.offset(ByteOffset(offset as u32)).byte()) };
        offset += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_reset_records(list: *mut u8) {
    unsafe { Workspace(Address32(list as u32)).store(12, 0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_record_at(list: *const u8, index: i32) -> u32 {
    unsafe { record_at(Workspace(Address32(list as u32)), index) }.0.0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_append_record(list: *mut u8, source: *const u8) {
    let list = Workspace(Address32(list as u32));
    let count = unsafe { list.word(12) };
    if count == unsafe { list.word(8) } { return; }
    let stride = unsafe { list.word(4) };
    let destination = Address32(unsafe { list.word(0) }.wrapping_add(stride.wrapping_mul(count)) as u32);
    unsafe { copy_bytes(destination, Address32(source as u32), stride); destination.write_word(1); list.store(12, count.wrapping_add(1)); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_compact_records(list: *mut u8) {
    let list = Workspace(Address32(list as u32));
    let base = Address32(unsafe { list.word(0) } as u32);
    let stride = unsafe { list.word(4) };
    let mut kept: i32 = 0;
    let mut index: i32 = 0;
    while index < unsafe { list.word(12) } {
        let source = base.offset(ByteOffset(index.wrapping_mul(stride) as u32));
        if unsafe { source.word() } != 0 {
            let destination = base.offset(ByteOffset(kept.wrapping_mul(stride) as u32));
            if destination != source { unsafe { copy_bytes(destination, source, stride) }; }
            kept += 1;
        }
        index += 1;
    }
    unsafe { list.store(12, kept) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_copy_records(source: *const u8, destination: *mut u8) {
    unsafe { xm1_pipeline_image_redeye_reset_records(destination) };
    let source_list = Workspace(Address32(source as u32));
    let mut index = 0;
    while index < unsafe { source_list.word(12) } {
        let record = unsafe { record_at(source_list, index) };
        unsafe { xm1_pipeline_image_redeye_append_record(destination, record.0.0 as *const u8) };
        index += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_spin_delay(milliseconds: u32) {
    let timer = Workspace(Address32(0xfff9_1000));
    let period = unsafe { timer.field(4).word() };
    let divisor = unsafe { unsigned_divide(10_000_000, period) };
    let target = unsafe { unsigned_divide(milliseconds.wrapping_mul(1000), divisor) };
    let mut previous = unsafe { timer.field(4).word() }.wrapping_sub(unsafe { timer.field(0).word() });
    let mut elapsed: u32 = 0;
    loop {
        let current = unsafe { timer.field(4).word() }.wrapping_sub(unsafe { timer.field(0).word() });
        elapsed = if previous >= current { elapsed.wrapping_add(previous.wrapping_sub(current)) } else { elapsed.wrapping_add(period.wrapping_add(previous).wrapping_add(1).wrapping_sub(current)) };
        if elapsed >= target { break; }
        previous = current;
    }
}

const TASK: Workspace = Workspace(Address32(0x014d_fa90));

#[derive(Clone, Copy)]
enum RedEyeEvent { Detect, Apply }
impl RedEyeEvent {
    fn parse(value: u16) -> Option<Self> {
        match value { 0x4800 => Some(Self::Detect), 0x4801 => Some(Self::Apply), _ => None }
    }
}

unsafe fn clear(destination: Address32, length: u32) {
    for offset in 0..length { unsafe { destination.offset(ByteOffset(offset)).write_byte(0) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_report_error() {
    unsafe {
        TASK.store(0x26a0, TASK.word(0x26a0).wrapping_add(1));
        redeye_append_text(0x38, 0x0107_28e0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_abort_requested() -> u32 {
    let saved = unsafe { redeye_enter_critical() };
    let aborted = unsafe { TASK.field(0x269c).byte() } == 1;
    unsafe { redeye_leave_critical(saved) };
    u32::from(aborted)
}

unsafe fn prepare_faces() {
    let count = unsafe { TASK.field(16).byte() };
    unsafe { TASK.field(0x1d90).write_byte(count) };
    for index in 0..u32::from(count) {
        let face = Workspace(TASK.field(16 + index * 92));
        let prepared = Workspace(TASK.field(0x1d92 + index * 18));
        let left = unsafe { face.word(4) };
        let top = unsafe { face.word(8) };
        let width = unsafe { face.word(12) };
        let height = unsafe { face.word(16) };
        unsafe {
            prepared.store_half(0, left.wrapping_add((width as u32 >> 1) as i32).wrapping_mul(16) as i16);
            prepared.store_half(2, top.wrapping_add((height as u32 >> 1) as i32).wrapping_mul(16) as i16);
            prepared.store_half(6, left as i16); prepared.store_half(8, top as i16);
            prepared.store_half(10, width as i16); prepared.store_half(12, height as i16);
            prepared.store_half(4, divide(0x8000, i32::from(width as u16)) as i16);
            prepared.store_half(14, face.half(20));
        }
        let kind = unsafe { face.word(24) };
        if (0..=2).contains(&kind) { unsafe { prepared.field(16).write_byte(kind as u8) }; } else { unsafe { xm1_pipeline_image_redeye_report_error() }; }
        let orientation = unsafe { face.word(28) };
        if (0..=11).contains(&orientation) { unsafe { prepared.field(17).write_byte(orientation as u8) }; } else { unsafe { xm1_pipeline_image_redeye_report_error() }; }
    }
    unsafe { TASK.field(0x1e48).write_byte(count) };
    for index in 0..u32::from(count) {
        let source = Workspace(TASK.field(16 + index * 92));
        let group = Workspace(TASK.field(0x1e4c + index * 52));
        unsafe { group.field(0).write_byte(3) };
        for feature in 0..3 {
            let source = Workspace(source.field(32 + feature * 16));
            let target = Workspace(group.field(4 + feature * 12));
            unsafe {
                target.store(0, source.word(0));
                target.store_half(4, source.word(4) as i16);
                target.store_half(6, source.word(8) as i16);
                target.store_half(8, source.word(12) as i16);
            }
        }
    }
}

unsafe fn collect_results() {
    for index in 0..10 { unsafe { TASK.store(0x2058 + index * 44, 0) }; }
    unsafe { TASK.field(0x2054).write_byte(TASK.field(16).byte()) };
    let mut index = 0;
    while index < unsafe { TASK.word(0x81c) } {
        let edit = Workspace(TASK.field(0x3bc + index as u32 * 56));
        let face_index = unsafe { edit.half(50) } as i32;
        let face = Workspace(TASK.field(0x2058u32.wrapping_add(face_index.wrapping_mul(44) as u32)));
        let side = u32::from(unsafe { edit.half(52) } != 0);
        let existing = unsafe { face.word(0) };
        let selected = if side == 0 { 1 } else { 2 };
        if existing == 0 { unsafe { face.store(0, selected) }; }
        else if existing == (3 ^ selected) { unsafe { face.store(0, 3) }; }
        else { unsafe { xm1_pipeline_image_redeye_report_error() }; }
        let eye = Workspace(face.field(4 + side * 20));
        unsafe {
            eye.store(0, Q4(edit.word(12)).integer().wrapping_sub(edit.word(16) / 2));
            eye.store(4, Q4(edit.word(20)).integer().wrapping_sub(edit.word(24) / 2));
            eye.store(8, edit.word(16)); eye.store(12, edit.word(24)); eye.store_half(16, edit.half(48));
        }
        index += 1;
    }
}

unsafe fn export_edits() {
    unsafe { clear(TASK.field(0x2210), 0x464) };
    let count = unsafe { TASK.field(0x81c).byte() };
    unsafe { TASK.field(0x2210).write_byte(count) };
    for index in 0..u32::from(count) {
        let source = Workspace(TASK.field(0x3bc + index * 56));
        let target = Workspace(TASK.field(0x2214 + index * 56));
        for offset in [0,4,8,12,16,20,24,28,32,44] { unsafe { target.store(offset, source.word(offset)) }; }
        unsafe { target.store(36, source.word(40)); target.store(40, source.word(36)); }
        unsafe {
            target.store_half(48, source.half(48));
            target.field(50).write_byte(source.half(50) as u8);
            target.store(52, i32::from(source.half(52) != 0));
        }
    }
}

unsafe fn begin_phase(text: u32, operation: u32) -> u32 {
    unsafe { xm1_pipeline_image_redeye_spin_delay(1); redeye_append_text(0x38, text); }
    let mut ticks = 0;
    unsafe { redeye_get_ticks(&mut ticks); redeye_trace(operation); }
    ticks
}

unsafe fn end_phase(start: u32, operation: u32, timer_offset: u32) {
    unsafe { redeye_trace(operation | 0x1000_0000); TASK.store(timer_offset, redeye_elapsed(start) as i32); }
}

unsafe fn detect_packet() {
    unsafe { clear(TASK.field(0x2054), 0x1bc); clear(TASK.field(0x2674), 20); clear(TASK.field(0x2210), 0x464); }
    let success = if unsafe { xm1_pipeline_image_redeye_abort_requested() } == 1 { 0 } else {
        unsafe { xm1_pipeline_image_redeye_spin_delay(1); redeye_append_text(0x38, 0x0107_28f8); }
        let arena = unsafe { TASK.word(0x2694) } as u32;
        let setup = TASK.field(0x3bc).0 as *mut u8;
        let size = unsafe { xm1_pipeline_image_redeye_init_buffers(arena, setup) };
        let arena = arena.wrapping_add(size);
        unsafe {
            TASK.store(0x84c, TASK.word(0x3ac));
            TASK.store(0x850, TASK.word(0x3b4));
            TASK.store(0x854, TASK.word(0x3b8).wrapping_add(TASK.word(0x3b0)));
            TASK.store(0x870, 1);
            xm1_pipeline_image_redeye_prepare_primary_buffer(arena, setup);
            xm1_pipeline_image_redeye_prepare_secondary_buffer(arena, setup);
            xm1_pipeline_image_redeye_prepare_stage_buffers(arena, setup);
            xm1_pipeline_image_redeye_prepare_pyramid_buffers(arena, setup);
            prepare_faces();
        }
        if unsafe { xm1_pipeline_image_redeye_abort_requested() } == 1 { 0 } else {
            let start = unsafe { begin_phase(0x0107_290c, 0x0602_0800) };
            unsafe { redeye_detect(TASK.field(0x84c).0, TASK.field(0x830).0, TASK.field(0x8ec).0, 0, 0, 0, 0, 0, TASK.field(0x1d90).0, TASK.field(0x1e48).0); end_phase(start, 0x0602_0800, 0x2674); }
            if unsafe { xm1_pipeline_image_redeye_abort_requested() } == 1 { 0 } else {
                let start = unsafe { begin_phase(0x0107_2920, 0x0602_0900) };
                unsafe { xm1_pipeline_image_redeye_prepare_history(0, TASK.field(0x820).0 as *mut u8, TASK.field(0x830).0 as *mut u8, TASK.field(0x928).0 as *mut u8, 0,0,0,0,0,0,TASK.field(0x1d90).0 as *const u8); end_phase(start, 0x0602_0900, 0x2678); }
                if unsafe { xm1_pipeline_image_redeye_abort_requested() } == 1 { 0 } else {
                    let start = unsafe { begin_phase(0x0107_2930, 0x0602_0a00) };
                    unsafe { xm1_pipeline_image_redeye_configure_pair_filters(TASK.field(0x84c).0 as *const u8, TASK.field(0x820).0 as *mut u8, TASK.field(0x960).0 as *mut u8, 0); end_phase(start, 0x0602_0a00, 0x267c); xm1_pipeline_image_redeye_spin_delay(1); }
                    let scale = unsafe { unsigned_divide(0xc0000, TASK.word(8) as u32) } as i16;
                    unsafe { xm1_pipeline_image_redeye_scale_edits(TASK.field(0x820).0 as *const u8, setup, i32::from(scale)); collect_results(); }
                    1
                }
            }
        }
    };
    let callback: unsafe extern "C" fn(i32, u32, u32) = unsafe { core::mem::transmute(TASK.word(0x2698) as usize) };
    unsafe { callback(success, TASK.field(0x2054).0, TASK.field(0x2674).0) };
}

unsafe fn apply_packet() {
    let success = if unsafe { xm1_pipeline_image_redeye_abort_requested() } == 1 { 0 } else {
        let start = unsafe { begin_phase(0x0107_293c, 0x0602_0b00) };
        unsafe { xm1_pipeline_image_redeye_prepare_edits(TASK.field(0x84c).0 as *const u8, TASK.field(0x820).0 as *const u8, TASK.field(0x3bc).0 as *mut u8, TASK.field(0xda4).0 as *mut u8); end_phase(start, 0x0602_0b00, 0x2680); }
        if unsafe { xm1_pipeline_image_redeye_abort_requested() } == 1 { 0 } else {
            let start = unsafe { begin_phase(0x0107_294c, 0x0602_0c00) };
            let scale = unsafe { unsigned_divide(0x780000, TASK.word(8) as u32) } as i16;
            unsafe { xm1_pipeline_image_redeye_apply_pixels(TASK.field(0x3bc).0 as *const u8, TASK.word(0) as u32, TASK.word(12).wrapping_add(TASK.word(4)), TASK.word(8), scale); end_phase(start, 0x0602_0c00, 0x2684); xm1_pipeline_image_redeye_spin_delay(1); export_edits(); }
            1
        }
    };
    let callback: unsafe extern "C" fn(i32, u32, u32) = unsafe { core::mem::transmute(TASK.word(0x2694) as usize) };
    unsafe { callback(success, TASK.field(0x2680).0, TASK.field(0x2210).0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_run_task() -> ! {
    unsafe { TASK.store(0x26a0, 0); TASK.store(0x2690, TASK.field(0x2694).0 as i32); }
    loop {
        unsafe { redeye_receive_message(0x38, TASK.field(0x2688).0 as *mut u8, -1) };
        match RedEyeEvent::parse(unsafe { TASK.field(0x2688).half() }) {
            Some(RedEyeEvent::Detect) => unsafe { detect_packet() },
            Some(RedEyeEvent::Apply) => unsafe { apply_packet() },
            None => unsafe { xm1_pipeline_image_redeye_report_error() },
        }
    }
}

unsafe extern "C" {
    #[link_name = "xm1_develop_append_stream_text"]
    fn redeye_append_text(task: u32, text: u32);
    #[link_name = "xm1_develop_enter_critical"]
    fn redeye_enter_critical() -> u32;
    #[link_name = "xm1_develop_leave_critical"]
    fn redeye_leave_critical(saved: u32);
    #[link_name = "xm1_pipeline_image_get_ticks"]
    fn redeye_get_ticks(output: *mut u32);
    #[link_name = "xm1_pipeline_image_elapsed"]
    fn redeye_elapsed(start: u32) -> u32;
    #[link_name = "xm1_pipeline_image_trace_operation"]
    fn redeye_trace(operation: u32);
    #[link_name = "xm1_pipeline_image_receive_message"]
    fn redeye_receive_message(channel: u32, message: *mut u8, timeout: i32) -> i32;
    #[link_name = "xm1_pipeline_image_redeye_detect"]
    fn redeye_detect(image: u32, parameters: u32, detector: u32, arg3: u32, arg4: u32, options: u32, arg6: u32, arg7: u32, faces: u32, groups: u32);
}

#[derive(Clone, Copy)]
struct DivisionRegisters { quotient: i32, divisor: u32 }

unsafe fn divide_registers(numerator: i32, denominator: i32) -> DivisionRegisters {
    if denominator == 0 { unsafe { xm1_pipeline_image_redeye_divide(numerator, 0) }; }
    let negative = (numerator ^ denominator) < 0;
    let mut divisor = denominator.unsigned_abs();
    let magnitude = numerator.unsigned_abs();
    let unsigned;
    if divisor == 1 { unsigned = magnitude; }
    else if magnitude <= divisor { unsigned = u32::from(magnitude == divisor); }
    else if divisor.is_power_of_two() {
        let mut shift = 0;
        if divisor >= 0x10000 { divisor >>= 16; shift = 16; }
        if divisor >= 0x100 { divisor >>= 8; shift += 8; }
        if divisor >= 16 { divisor >>= 4; shift += 4; }
        shift += if divisor > 4 { 3 } else { divisor >> 1 };
        unsigned = magnitude >> shift;
    } else {
        let mut bit = if divisor & 0xe000_0000 == 0 { divisor <<= 3; 8 } else { 1 };
        while divisor < 0x1000_0000 && divisor < magnitude { divisor <<= 4; bit <<= 4; }
        while divisor < 0x8000_0000 && divisor < magnitude { divisor <<= 1; bit <<= 1; }
        let mut remainder = magnitude;
        let mut quotient = 0;
        loop {
            for shift in 0..4 {
                if remainder >= divisor >> shift { remainder = remainder.wrapping_sub(divisor >> shift); quotient |= bit >> shift; }
            }
            if remainder == 0 { break; }
            bit >>= 4;
            if bit == 0 { break; }
            divisor >>= 4;
        }
        unsigned = quotient;
    }
    DivisionRegisters { quotient: if negative { unsigned.wrapping_neg() as i32 } else { unsigned as i32 }, divisor }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_edits(image: *const u8, records: *const u8, edits: *mut u8, mask_context: *mut u8) {
    for (key, address) in [(0xa449,0x014e2143),(0xa44a,0x014e2144),(0xa44b,0x014e2142),(0xa44c,0x014e213f),(0xa44d,0x014e2145),(0xa45a,0x014e2146),(0xa45b,0x014e213e),(0xa45c,0x014e2140),(0xa45d,0x014e2147),(0xa45e,0x014e2141)] {
        unsafe { Address32(address).write_byte(redeye_configuration_byte(key) as u8) };
    }
    let records = Workspace(Address32(records as u32));
    let edits = Workspace(Address32(edits as u32));
    unsafe { edits.store(0x460, 0) };
    let mut count = 0;
    let mut index = 0;
    while index < unsafe { records.word(12) } {
        let record = unsafe { record_at(records, index) };
        let selected = unsafe { record.half(20) };
        let (x, y) = match selected {
            0 => unsafe { (record.word(44).wrapping_sub(record.word(28)), record.word(52).wrapping_sub(record.word(36))) },
            1 => unsafe { (record.word(24).wrapping_sub(record.word(48)), record.word(32).wrapping_sub(record.word(56))) },
            2 => unsafe { (record.word(44).wrapping_sub(record.word(48)), record.word(52).wrapping_sub(record.word(56))) },
            _ => return,
        };
        let distance = x.wrapping_mul(x / 16).wrapping_div(16).wrapping_add(y.wrapping_mul(y / 16).wrapping_div(16));
        if distance < 0 { return; }
        for side in 0..2 {
            if i32::from(selected) != side && selected != 2 { continue; }
            let edit = Workspace(edits.field(count * 56));
            let registers = unsafe { divide_registers(0x4924, i32::from(record.half(60 + side as u32 * 2))) };
            let diameter = registers.quotient.wrapping_add(1) & !1;
            let sampling = unsafe { divide(0x10000, registers.divisor as i32) } as i16;
            unsafe {
                edit.store(16, diameter); edit.store(24, diameter);
                edit.store(12, record.word(44 + side as u32 * 4)); edit.store(20, record.word(52 + side as u32 * 4));
                edit.store(28, i32::from(sampling)); edit.store_half(48, record.half(64 + side as u32 * 2));
                edit.store_half(50, record.half(68)); edit.store_half(52, side as i16);
                redeye_prepare_edit_mask(image, edit.0.0 as *mut u8, distance, mask_context);
            }
            count += 1;
            unsafe { edits.store(0x460, count as i32) };
            if count > 19 { return; }
        }
        index += 1;
    }
}

unsafe extern "C" {
    #[link_name = "xm1_buffers_configuration_byte"]
    fn redeye_configuration_byte(key: u32) -> u32;
    #[link_name = "xm1_pipeline_image_redeye_prepare_edit_mask"]
    fn redeye_prepare_edit_mask(image: *const u8, edit: *mut u8, distance: i32, context: *mut u8);
}

#[derive(Default)]
struct SampleSums { count: i32, x: i32, y: i32, scale: i32, quality: i32 }

unsafe fn sample_sums(samples: Workspace) -> SampleSums {
    let count = unsafe { samples.word(12) };
    let mut sums = SampleSums { count, ..SampleSums::default() };
    for index in 0..count {
        let sample = unsafe { record_at(samples, index) };
        sums.x = sums.x.wrapping_add(unsafe { sample.word(4) });
        sums.y = sums.y.wrapping_add(unsafe { sample.word(8) });
        sums.scale = sums.scale.wrapping_add(i32::from(unsafe { sample.half(12) }));
        sums.quality = sums.quality.wrapping_add(i32::from(unsafe { sample.half(14) }));
    }
    sums
}

impl SampleSums {
    fn accepted(&self, minimum: i32) -> bool {
        self.count >= minimum && self.quality > self.count.wrapping_mul(500) && (self.count > 10 || self.quality > self.count.wrapping_mul(1500))
    }
}

unsafe fn search_sample(image: *const u8, context: Workspace, sample_offset: u32, x: i32, y: i32, scale: i16, options: u32) {
    let sample = Workspace(context.field(sample_offset));
    let list = context.field(0x434).0;
    unsafe {
        redeye_sample_image(image, 0, i32::from(scale), x, y, sample.word(8), sample.word(4), sample.0.0 as *const u32);
        redeye_search_local(sample.0.0, list, i32::from(scale), options);
    }
}

unsafe fn sweep_samples(image: *const u8, context: Workspace, sample_offset: u32, x: i32, y: i32, scale: u16, options: u32) -> SampleSums {
    let samples = context.field(0x434).0 as *mut u8;
    unsafe { xm1_pipeline_image_redeye_reset_records(samples) };
    let mut current = scale;
    let mut iteration = 0;
    let mut gain;
    loop {
        unsafe { search_sample(image, context, sample_offset, x, y, current as i16, options) };
        gain = unsafe { Address32(0x014e_2137).byte() } as i32;
        current = unsafe { divide(i32::from(current as i16).wrapping_mul(100), gain) } as u16;
        iteration += 1;
        if iteration > i32::from(unsafe { Address32(0x014e_2136).byte() } >> 1) { break; }
    }
    if unsafe { Address32(0x014e_2136).byte() } >> 1 != 0 {
        let mut scaled = gain.wrapping_mul(i32::from(scale as i16));
        let mut iteration = 0;
        loop {
            let current = (scaled / 100) as i16;
            unsafe { search_sample(image, context, sample_offset, x, y, current, options) };
            scaled = i32::from(unsafe { Address32(0x014e_2137).byte() }).wrapping_mul(i32::from(current));
            iteration += 1;
            if iteration >= i32::from(unsafe { Address32(0x014e_2136).byte() } >> 1) { break; }
        }
    }
    unsafe { sample_sums(Workspace(context.field(0x434))) }
}

unsafe fn average_pair_scale(record: Workspace) {
    if unsafe { record.half(20) } == 2 {
        let sum = i32::from(unsafe { record.half(60) }).wrapping_add(i32::from(unsafe { record.half(62) }));
        let scale = ((sum as u32 >> 1) & 0xffff) as i16;
        unsafe { record.store_half(60, scale); record.store_half(62, scale); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_refine_pairs(image: *const u8, records: *mut u8, context: *mut u8, options: u32) {
    let list = Workspace(Address32(records as u32));
    let context = Workspace(Address32(context as u32));
    let mut index = 0;
    while index < unsafe { list.word(12) } {
        let record = unsafe { record_at(list, index) };
        let selected = unsafe { record.half(20) };
        let base_scale = if selected == 2 {
            ((i32::from(unsafe { record.half(60) }).wrapping_add(i32::from(unsafe { record.half(62) })) as u32 >> 1) & 0xffff) as u16
        } else { unsafe { record.field(60u32.wrapping_add((i32::from(selected) * 2) as u32)).half() } };
        for side in 0..2 {
            let selected = unsafe { record.half(20) };
            if (i32::from(selected) != side && selected != 2) || unsafe { record.half(64 + side as u32 * 2) } >= 5000 { continue; }
            let x = unsafe { record.word(44 + side as u32 * 4) };
            let y = unsafe { record.word(52 + side as u32 * 4) };
            let sums = unsafe { sweep_samples(image, context, 0, x, y, base_scale, options) };
            if sums.accepted(4) {
                let mean_x = unsafe { divide(sums.x, sums.count) };
                let mean_y = unsafe { divide(sums.y, sums.count) };
                let mean_scale = unsafe { divide(sums.scale, sums.count) } as i16;
                let quality = unsafe { divide(sums.quality, sums.count) }.min(0x7f00) as i16;
                unsafe {
                    record.store_half(64 + side as u32 * 2, quality);
                    record.store(44 + side as u32 * 4, x.wrapping_add(mean_x));
                    record.store(52 + side as u32 * 4, y.wrapping_add(mean_y));
                    record.store_half(60 + side as u32 * 2, mean_scale);
                }
            } else if unsafe { record.half(20) } == 2 {
                unsafe { record.store(44 + side as u32 * 4, 0); record.store(52 + side as u32 * 4, 0); record.store_half(60 + side as u32 * 2, 0); record.store_half(20, (1 - side) as i16); }
            } else { unsafe { record.store(0, 0) }; }
        }
        unsafe { average_pair_scale(record) };
        index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(records) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_recover_pair(image: *const u8, records: *mut u8, context: *mut u8, options: u32) {
    let list = Workspace(Address32(records as u32));
    let context = Workspace(Address32(context as u32));
    let mut index = 0;
    while index < unsafe { list.word(12) } {
        let record = unsafe { record_at(list, index) };
        let selected = unsafe { record.half(20) };
        if selected != 2 {
            let base_scale = unsafe { record.field(60u32.wrapping_add((i32::from(selected) * 2) as u32)).half() };
            for iteration in 1..3 {
                if i32::from(unsafe { record.half(20) }) != iteration - 1 { continue; }
                let side = iteration % 2;
                let x = unsafe { record.word(24 + side as u32 * 4) };
                let y = unsafe { record.word(32 + side as u32 * 4) };
                let sums = unsafe { sweep_samples(image, context, 12, x, y, base_scale, options) };
                if sums.accepted(3) {
                    let mean_x = unsafe { divide(sums.x, sums.count) };
                    let mean_y = unsafe { divide(sums.y, sums.count) };
                    let mean_scale = unsafe { divide(sums.scale, sums.count) } as i16;
                    let quality = unsafe { divide(sums.quality, sums.count) }.min(0x7f00) as i16;
                    unsafe {
                        record.store_half(64 + side as u32 * 2, quality);
                        record.store(44 + side as u32 * 4, x.wrapping_add(mean_x));
                        record.store(52 + side as u32 * 4, y.wrapping_add(mean_y));
                        record.store_half(60 + side as u32 * 2, mean_scale); record.store_half(20, 2);
                    }
                }
            }
            unsafe { average_pair_scale(record) };
        }
        index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(records) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_filter_pair_geometry(records: *mut u8) {
    let list = Workspace(Address32(records as u32));
    let mut index = 0;
    while index < unsafe { list.word(12) } {
        let record = unsafe { record_at(list, index) };
        let mut keep = false;
        if unsafe { record.half(20) } == 2 {
            let x = unsafe { record.word(48).wrapping_sub(record.word(44)) };
            let y = unsafe { record.word(56).wrapping_sub(record.word(52)) };
            let baseline = unsafe { record.word(40) };
            let source_x = unsafe { record.word(28).wrapping_sub(record.word(24)) };
            let source_y = unsafe { record.word(36).wrapping_sub(record.word(32)) };
            let projection = unsafe { divide(x.wrapping_mul(source_x).wrapping_add(y.wrapping_mul(source_y)), baseline) } / 16;
            if projection == 0 { return; }
            let first = unsafe { record.half(64) } as i32;
            let second = unsafe { record.half(66) } as i32;
            let score = unsafe { record.half(14) };
            if (score >= 0 || (first.max(second) >= 800 && first.min(second) >= 500)) && first.max(second) > 249 && (first.max(second) >= 500 || first.min(second) >= 50) {
                let square = x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y));
                if unsafe { divide(square, projection.wrapping_mul(projection)) } <= 316 {
                    let baseline_square = if (baseline as u32).wrapping_add(15) <= 30 { 0 } else { (baseline / 16).wrapping_mul(baseline / 16) };
                    let ratio = unsafe { divide(square, baseline_square) };
                    if ratio > 163 || first.min(second) > 684 {
                        let ratio = unsafe { divide(square, baseline_square) };
                        if ratio <= 538 { keep = unsafe { divide(square, baseline_square) } > 124; }
                    }
                }
            }
        }
        if !keep { unsafe { record.store(0, 0) }; }
        index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(records) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_dark_density(image: *const u8, histogram: *mut i32, selection: i32) -> i32 {
    let image = Workspace(Address32(image as u32));
    let histogram = Workspace(Address32(histogram as u32));
    unsafe { clear(histogram.0, 0x400) };
    let width = unsafe { image.word(8) };
    let height = unsafe { image.word(4) };
    let radius = unsafe { Address32(0x014e_2135).byte() } as i32;
    let radius_square = radius.wrapping_mul(radius);
    let pixels = Address32(unsafe { image.word(0) } as u32);
    for row in 0..height {
        for column in 0..width {
            let pixel = unsafe { pixels.offset(ByteOffset(row.wrapping_mul(width).wrapping_add(column) as u32)).byte() };
            let offset = u32::from(pixel) * 4;
            unsafe { histogram.store(offset, histogram.word(offset).wrapping_add(1)) };
        }
    }
    let target = height.wrapping_mul(width).wrapping_mul(20) / 100;
    let mut cumulative = unsafe { histogram.word(255 * 4) };
    let mut threshold = 255;
    while cumulative <= target && threshold > 0 {
        threshold -= 1;
        cumulative = cumulative.wrapping_add(unsafe { histogram.word(threshold * 4) });
    }
    let center_x = width.wrapping_sub(1) / 2;
    let center_y = height.wrapping_sub(1) / 2;
    let mut dark: i32 = 0;
    for row in 0..height {
        for column in 0..width {
            let x = column.wrapping_sub(center_x);
            let y = row.wrapping_sub(center_y);
            if x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y)) < radius_square && u32::from(unsafe { pixels.offset(ByteOffset(row.wrapping_mul(width).wrapping_add(column) as u32)).byte() }) < threshold {
                dark = dark.wrapping_add(1);
            }
        }
    }
    let fraction = unsafe { Address32(if selection == 1 { 0x014e_2134 } else { 0x014e_2138 }).byte() } as i32;
    i32::from(dark > fraction.wrapping_mul(radius_square.wrapping_mul(314) / 100) / 100)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_filter_pair_samples(image: *const u8, records: *mut u8, context: *mut u8) {
    let list = Workspace(Address32(records as u32));
    let context = Workspace(Address32(context as u32));
    let mut index = 0;
    while index < unsafe { list.word(12) } {
        let record = unsafe { record_at(list, index) };
        for side in 0..2 {
            let scale = i32::from(unsafe { record.half(60 + side * 2) });
            let first = unsafe { divide(0x8000, i32::from(record.half(12))) };
            let second = unsafe { divide(0x2c00, scale) };
            let selection = i32::from(first < second.wrapping_mul(12));
            let diameter = unsafe { divide(0x4924, scale) }.wrapping_add(1) & !1;
            let sampling = unsafe { divide(0x10000, diameter) } as i16;
            unsafe {
                redeye_sample_image(image, 0, i32::from(sampling), record.word(44 + side * 4), record.word(52 + side * 4), context.word(32), context.word(28), context.field(24).0 as *const u32);
                redeye_color_to_mono(context.field(24).0 as *const u8, context.field(36).0 as *mut u8);
            }
            if unsafe { xm1_pipeline_image_redeye_dark_density(context.field(36).0 as *const u8, context.field(52).0 as *mut i32, selection) } == 1 { unsafe { record.store(0, 0) }; break; }
        }
        index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(records) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_configure_pair_filters(image: *const u8, records: *mut u8, context: *mut u8, options: u32) {
    for (key, address) in [(0xa454,0x014e2136),(0xa455,0x014e2137),(0xa457,0x014e2135),(0xa458,0x014e2134),(0xa459,0x014e2138)] {
        unsafe { Address32(address).write_byte(redeye_configuration_byte(key) as u8) };
    }
    let flags = unsafe { redeye_configuration_byte(0xa453) };
    unsafe { Address32(0x014e_2139).write_byte(flags as u8); xm1_pipeline_image_redeye_refine_pairs(image, records, context, options); }
    if flags & 2 != 0 { unsafe { xm1_pipeline_image_redeye_recover_pair(image, records, context, options) }; }
    unsafe { xm1_pipeline_image_redeye_filter_pair_geometry(records) };
    if flags & 1 != 0 { unsafe { xm1_pipeline_image_redeye_filter_pair_samples(image, records, context) }; }
}

unsafe extern "C" {
    #[link_name = "xm1_pipeline_image_redeye_dispatch_sample"]
    fn redeye_sample_image(image: *const u8, operation: i32, scale: i32, x: i32, y: i32, width: i32, height: i32, configuration: *const u32);
    #[link_name = "xm1_pipeline_image_redeye_chroma_score"]
    fn redeye_color_to_mono(image: *const u8, mono: *mut u8);
    #[link_name = "xm1_pipeline_image_redeye_search_local"]
    fn redeye_search_local(image: u32, records: u32, scale: i32, options: u32);
}

unsafe fn unsigned_divide(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { unsafe { redeye_divide_fault(if numerator == 0 { 0 } else { -1 }) }; }
    numerator / denominator
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct PairRecord {
    active: i32,
    x: Q4,
    y: Q4,
    scale: i16,
    quality: i16,
    orientation: i16,
    assignment: i16,
    selected: i16,
    reserved: i16,
    source_x: [Q4; 2],
    source_y: [Q4; 2],
    projection: Q4,
    pair_x: [Q4; 2],
    pair_y: [Q4; 2],
    pair_scale: [i16; 2],
    pair_quality: [i16; 2],
    face_index: i16,
    reserved_end: i16,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_append_faces(list: *mut u8, faces: *const u8) {
    let faces = Workspace(Address32(faces as u32));
    let count = unsafe { faces.field(0).byte() };
    for index in 0..u32::from(count) {
        let face = Workspace(faces.field(index * 18));
        let scale = i32::from(unsafe { face.field(6).half() });
        let x = i32::from(unsafe { face.field(2).half() });
        let y = i32::from(unsafe { face.field(4).half() });
        let orientation = u32::from(unsafe { face.field(19).byte() });
        let table = Workspace(Address32(0x0107_4244 + ((orientation + 1) % 12) * 8));
        let mut record = PairRecord::default();
        record.x = Q4(x); record.y = Q4(y);
        record.scale = scale as i16;
        record.quality = unsafe { face.half(16) };
        record.source_x[1] = Q4(x.wrapping_add(unsafe { divide(i32::from(table.half(0)).wrapping_shl(10), scale) }));
        record.source_y[1] = Q4(y.wrapping_add(unsafe { divide(i32::from(table.half(2)).wrapping_shl(10), scale) }));
        record.source_x[0] = Q4(x.wrapping_add(unsafe { divide(i32::from(table.half(4)).wrapping_shl(10), scale) }));
        record.source_y[0] = Q4(y.wrapping_add(unsafe { divide(i32::from(table.half(6)).wrapping_shl(10), scale) }));
        record.projection = Q4(unsafe { divide(0x33400, scale) });
        record.orientation = orientation.wrapping_mul(30) as i16;
        record.assignment = i16::from(unsafe { face.field(18).byte() });
        record.selected = -1;
        record.face_index = index as i16;
        unsafe { xm1_pipeline_image_redeye_append_record(list, core::ptr::addr_of!(record).cast::<u8>()) };
    }
}

fn multiply_add(a: i32, b: i32, c: i32, d: i32) -> i32 {
    a.wrapping_mul(b).wrapping_add(c.wrapping_mul(d))
}

fn absolute(value: i32) -> i32 { if value < 0 { value.wrapping_neg() } else { value } }

struct PairDistances { cross: i32, along: [i32; 2] }

unsafe fn pair_distances(pair: Workspace, observation: Workspace) -> PairDistances {
    let x0 = unsafe { pair.word(24) };
    let x1 = unsafe { pair.word(28) };
    let y0 = unsafe { pair.word(32) };
    let y1 = unsafe { pair.word(36) };
    let x = unsafe { observation.word(4) };
    let y = unsafe { observation.word(8) };
    let dx = x1.wrapping_sub(x0);
    let dy = y1.wrapping_sub(y0);
    let px = x.wrapping_sub(x0);
    let py = y.wrapping_sub(y0);
    PairDistances {
        cross: absolute(multiply_add(py, dx, px.wrapping_neg(), dy)) / 256,
        along: [
            absolute(multiply_add(px, dx, py, dy)) / 256,
            absolute(multiply_add(x.wrapping_sub(x1), dx.wrapping_neg(), y.wrapping_sub(y1), dy.wrapping_neg())) / 256,
        ],
    }
}

unsafe fn observation_fits(pair: Workspace, observation: Workspace, distances: &PairDistances, side: u32) -> bool {
    let scale = i32::from(unsafe { pair.half(12) });
    let observation_scale = i32::from(unsafe { observation.half(12) }).wrapping_mul(32);
    let baseline = unsafe { pair.word(40) };
    observation_scale >= scale.wrapping_mul(60) && observation_scale <= scale.wrapping_mul(350)
        && scale.wrapping_mul(distances.cross) <= (baseline.wrapping_mul(64) / 256).wrapping_mul(1024)
        && scale.wrapping_mul(distances.along[side as usize]) <= (baseline.wrapping_mul(80) / 256).wrapping_mul(1024)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_match_faces(pairs: *mut u8, observations: *const u8) {
    let list = Workspace(Address32(pairs as u32));
    let observations = Workspace(Address32(observations as u32));
    let original_count = unsafe { list.word(12) };
    for index in 0..original_count { unsafe { record_at(list, index).store(0, 0) }; }
    let mut observation_index = 0;
    while observation_index < unsafe { observations.word(12) } {
        let observation = unsafe { record_at(observations, observation_index) };
        let mut best = None;
        let mut best_quality: i16 = -10000;
        for index in 0..original_count {
            let pair = unsafe { record_at(list, index) };
            let quality = unsafe { pair.half(14) };
            if quality <= best_quality || unsafe { pair.half(18) } > 0 { continue; }
            let distances = unsafe { pair_distances(pair, observation) };
            for side in 0..2 {
                if quality > best_quality && unsafe { observation_fits(pair, observation, &distances, side) } {
                    best = Some((pair, side)); best_quality = quality;
                }
            }
        }
        if let Some((pair, side)) = best {
            let mut record = unsafe { core::ptr::read_volatile(pair.0.0 as *const PairRecord) };
            record.active = 1;
            record.selected = side as i16;
            record.pair_x[side as usize] = Q4(unsafe { observation.word(4) });
            record.pair_y[side as usize] = Q4(unsafe { observation.word(8) });
            record.pair_scale[side as usize] = unsafe { observation.half(12) };
            record.pair_quality[side as usize] = unsafe { observation.half(14) };
            unsafe { xm1_pipeline_image_redeye_append_record(pairs, core::ptr::addr_of!(record).cast::<u8>()) };
        }
        observation_index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(pairs) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_complete_pairs(pairs: *mut u8, observations: *const u8) {
    let list = Workspace(Address32(pairs as u32));
    let observations = Workspace(Address32(observations as u32));
    let mut pair_index = 0;
    while pair_index < unsafe { list.word(12) } {
        let pair = unsafe { record_at(list, pair_index) };
        let selected = i32::from(unsafe { pair.half(20) });
        let side = if selected as u32 <= 1 { (1 - selected) as u32 } else { 0 };
        let mut best = None;
        let mut best_quality: i16 = -10000;
        let mut index = 0;
        while index < unsafe { observations.word(12) } {
            let observation = unsafe { record_at(observations, index) };
            let quality = unsafe { observation.half(14) };
            if quality > best_quality {
                let distances = unsafe { pair_distances(pair, observation) };
                if unsafe { observation_fits(pair, observation, &distances, u32::from(selected == 0)) } {
                    best = Some(observation); best_quality = quality;
                }
            }
            index += 1;
        }
        if let Some(observation) = best {
            unsafe {
                pair.store(44 + side * 4, observation.word(4)); pair.store(52 + side * 4, observation.word(8));
                pair.store_half(60 + side * 2, observation.half(12)); pair.store_half(64 + side * 2, observation.half(14)); pair.store_half(20, 2);
            }
        }
        pair_index += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_deduplicate_pairs(pairs: *mut u8) {
    let list = Workspace(Address32(pairs as u32));
    let mut first_index = 0;
    while first_index < unsafe { list.word(12) }.wrapping_sub(1) {
        let first = unsafe { record_at(list, first_index) };
        let mut second_index = first_index + 1;
        while second_index < unsafe { list.word(12) } {
            let second = unsafe { record_at(list, second_index) };
            let same_center = unsafe { first.word(12) == second.word(12) && first.word(8) == second.word(8) && first.word(4) == second.word(4) };
            let selected = unsafe { first.half(20) };
            if same_center && selected == unsafe { second.half(20) } {
                let same_position = if selected == 2 {
                    unsafe { first.word(52) == second.word(52) && first.word(56) == second.word(56) && first.word(44) == second.word(44) && first.word(48) == second.word(48) }
                } else {
                    let offset = i32::from(selected).wrapping_mul(4) as u32;
                    unsafe { first.word(52u32.wrapping_add(offset)) == second.word(52u32.wrapping_add(offset)) && first.word(44u32.wrapping_add(offset)) == second.word(44u32.wrapping_add(offset)) }
                };
                if same_position { unsafe { second.store(0, 0) }; }
            }
            second_index += 1;
        }
        first_index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(pairs) };
}

struct PairDisplacement { square: i32, projection: i32 }

unsafe fn displacement(pair: Workspace) -> PairDisplacement {
    let selected = unsafe { pair.half(20) };
    let (start_x, start_y, end_x, end_y) = match selected {
        0 => unsafe { (pair.word(44), pair.word(52), pair.word(28), pair.word(36)) },
        1 => unsafe { (pair.word(24), pair.word(32), pair.word(48), pair.word(56)) },
        _ => unsafe { (pair.word(44), pair.word(52), pair.word(48), pair.word(56)) },
    };
    let x = end_x.wrapping_sub(start_x);
    let y = end_y.wrapping_sub(start_y);
    let source_x = unsafe { pair.word(28).wrapping_sub(pair.word(24)) };
    let source_y = unsafe { pair.word(36).wrapping_sub(pair.word(32)) };
    let projection = unsafe { divide(multiply_add(x, source_x, y, source_y), pair.word(40)) } / 16;
    PairDisplacement { square: x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y)), projection }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_remove_overlapping_pairs(pairs: *mut u8) {
    let list = Workspace(Address32(pairs as u32));
    let mut first_index = 0;
    while first_index < unsafe { list.word(12) }.wrapping_sub(1) {
        let first = unsafe { record_at(list, first_index) };
        let mut second_index = first_index + 1;
        while second_index < unsafe { list.word(12) } {
            let second = unsafe { record_at(list, second_index) };
            let first_scale = i32::from(unsafe { first.half(12) });
            let second_scale = i32::from(unsafe { second.half(12) });
            if first_scale < second_scale.wrapping_mul(2) && second_scale < first_scale.wrapping_mul(2) {
                let x = unsafe { first.word(4).wrapping_sub(second.word(4)) };
                let y = unsafe { first.word(8).wrapping_sub(second.word(8)) };
                let distance = x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y)).wrapping_mul(16);
                let radius = unsafe { divide(0x80000, first_scale) }.wrapping_add(unsafe { divide(0x80000, second_scale) });
                if radius.wrapping_mul(radius) > distance {
                    let first_selected = unsafe { first.half(20) };
                    let second_selected = unsafe { second.half(20) };
                    let remove_first = if first_selected == 2 && second_selected != 2 { false }
                    else if first_selected != 2 && second_selected == 2 { true }
                    else {
                        let first_quality = unsafe { first.half(14) };
                        let second_quality = unsafe { second.half(14) };
                        if first_quality != second_quality { first_quality < second_quality }
                        else {
                            let first_displacement = unsafe { displacement(first) };
                            let second_displacement = unsafe { displacement(second) };
                            if first_displacement.projection <= 0 || first_displacement.square < 0 || second_displacement.projection <= 0 || second_displacement.square < 0 { return; }
                            let second_ratio = unsafe { divide(second_displacement.square, second_displacement.projection.wrapping_mul(second_displacement.projection)) };
                            let first_ratio = unsafe { divide(first_displacement.square, first_displacement.projection.wrapping_mul(first_displacement.projection)) };
                            second_ratio < first_ratio
                        }
                    };
                    unsafe { if remove_first { first.store(0, 0); } else { second.store(0, 0); } }
                }
            }
            second_index += 1;
        }
        first_index += 1;
    }
    unsafe { xm1_pipeline_image_redeye_compact_records(pairs) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_associate_candidates(pairs: *mut u8, observations: *const u8) {
    unsafe {
        xm1_pipeline_image_redeye_match_faces(pairs, observations);
        xm1_pipeline_image_redeye_complete_pairs(pairs, observations);
        xm1_pipeline_image_redeye_deduplicate_pairs(pairs);
        xm1_pipeline_image_redeye_remove_overlapping_pairs(pairs);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_history(_unused: u32, destination: *mut u8, observations: *mut u8, history: *mut u8, _reserved0: u32, _reserved1: u32, _reserved2: u32, _reserved3: u32, _reserved4: u32, _reserved5: u32, faces: *const u8) {
    let staged = (history as u32).wrapping_add(40) as *mut u8;
    unsafe { xm1_pipeline_image_redeye_reset_records(staged) };
    if unsafe { Workspace(Address32(observations as u32)).word(12) } != 0 {
        unsafe { xm1_pipeline_image_redeye_append_faces(staged, faces); xm1_pipeline_image_redeye_associate_candidates(staged, observations); }
    }
    unsafe { xm1_pipeline_image_redeye_copy_records(staged, destination) };
}
