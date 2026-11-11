use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ScanRequest(Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ScanWorkspace(Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ConfigurationKey(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct DiagnosticSlot(u32);

#[repr(C)]
struct RawSampleDescriptor {
    samples: Address32,
    count: u16,
    padding: MaybeUninit<u16>,
}

unsafe extern "C" {
    fn xm1_pipeline_image_scan_select_region(request: ScanRequest, workspace: ScanWorkspace);
    fn xm1_pipeline_image_scan_measure_region(plane: Address32, region: Address32, table: Address32, levels: Address32, mode: u32, workspace: ScanWorkspace, scratch: Address32) -> i32;
    fn xm1_pipeline_image_scan_adjust_components(plane: Address32, region: Address32, profile: Address32, levels: Address32, workspace: ScanWorkspace, fallback: Address32, special: u32, settings: Address32, selection: u32, operation: u32, scratch: Address32) -> i32;
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_pipeline_image_configuration_signed_byte(key: ConfigurationKey) -> i32;
    fn xm1_pipeline_image_scan_divide_fault(numerator: u32) -> !;
}

fn at(base: Address32, offset: u32) -> Address32 { base.offset(ByteOffset(offset)) }
unsafe fn word(base: Address32, offset: u32) -> u32 { unsafe { at(base, offset).word() } }
unsafe fn half(base: Address32, offset: u32) -> u16 { unsafe { at(base, offset).half() } }
unsafe fn put(base: Address32, offset: u32, value: u32) { unsafe { at(base, offset).write_word(value) }; }
unsafe fn put_half(base: Address32, offset: u32, value: u16) { unsafe { at(base, offset).write_half(value) }; }
unsafe fn clear(base: Address32, length: u32) {
    for offset in 0..length { unsafe { at(base, offset).write_byte(0) }; }
}
unsafe fn quotient(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 { unsafe { xm1_pipeline_image_scan_divide_fault(numerator) } }
    numerator / denominator
}
unsafe fn config(key: ConfigurationKey) -> u32 { unsafe { xm1_buffers_configuration_byte(key) } }
unsafe fn copy_components(source: Address32, destination: Address32) {
    for offset in [0, 2, 4] { unsafe { put_half(destination, offset, half(source, offset)) }; }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_components(context: Address32, components: Address32) {
    unsafe { copy_components(at(context, 0x238), components) };
    if unsafe { word(context, 0) } == 0 { return; }
    let request = Address32(0x0146_94b0);
    unsafe { xm1_pipeline_image_scan_prepare_components(context, request) };
    unsafe { xm1_pipeline_image_scan_update_components(request, components) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_update_components(request: Address32, components: Address32) {
    let workspace = Address32(0x0146_87f8);
    unsafe { clear(workspace, 0xcb8) };
    let status = unsafe { xm1_pipeline_image_scan_process_components(ScanRequest(request), ScanWorkspace(workspace)) };
    let source = if status == 0 { workspace } else { at(request, 0x218) };
    unsafe { copy_components(source, components) };
    if unsafe { word(request, 0x2a8) } != 1 { return; }
    let output = at(Address32(unsafe { word(request, 0x28c) }), 0xd7e54);
    unsafe { xm1_pipeline_image_scan_export_diagnostics(request, workspace, output) };
    if status != 0 { return; }
    for offset in [0xa040c, 0xa0410, 0xa0414] {
        let debug = Address32(unsafe { word(request, 0x28c) });
        unsafe { put(debug, offset, u32::MAX) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_process_components(request: ScanRequest, workspace: ScanWorkspace) -> i32 {
    if unsafe { xm1_pipeline_image_scan_validate_regions(request) } == 0 { return 1; }
    unsafe { xm1_pipeline_image_scan_select_region(request, workspace) };
    let selected = unsafe { half(workspace.0, 0xc28) } as i16;
    if selected < 0 { return 1; }
    let region = at(request.0, 4 + selected as u32 * 52);
    if unsafe { xm1_pipeline_image_scan_measure_region(at(request.0, 0x258), region, Address32(word(request.0, 0x254)), at(request.0, 0x20c), word(request.0, 0x220), workspace, Address32(word(request.0, 0x298))) } != 1 { return 1; }
    for offset in [0xc14, 0xc16] {
        if !(1..=511).contains(&unsafe { half(workspace.0, offset) }) { return 1; }
    }
    let status = unsafe { xm1_pipeline_image_scan_adjust_components(at(request.0, 0x258), region, at(request.0, 0x220), at(request.0, 0x20c), workspace, at(request.0, 0x212), word(request.0, 0x29c), at(request.0, 0x290), word(request.0, 0x2a0), word(request.0, 0x2a4), Address32(word(request.0, 0x298))) };
    i32::from(status != 1)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_validate_regions(request: ScanRequest) -> i32 {
    let request = request.0;
    let mut valid = false;
    let mut index = 0u8;
    while index < unsafe { request.byte() } {
        let region = at(request, u32::from(index) * 52);
        let accepted = if unsafe { word(request, 0x280) } != 8 { true } else {
            let mut left = unsafe { word(region, 4) };
            let mut top = unsafe { word(region, 8) };
            let mut width = unsafe { word(region, 12) };
            let mut height = unsafe { word(region, 16) };
            let minimum = width.min(height);
            if unsafe { config(ConfigurationKey(0xa754)) } >= minimum { false } else {
                let maximum = unsafe { config(ConfigurationKey(0xa755)) };
                let expanded = if maximum >= minimum {
                    let margin = unsafe { config(ConfigurationKey(0xa755)) }.wrapping_sub(minimum).wrapping_add(1) >> 1;
                    if left < margin || top < margin || left.wrapping_add(width).wrapping_add(margin) > unsafe { word(request, 0x25c) } || top.wrapping_add(height).wrapping_add(margin) > unsafe { word(request, 0x260) } { false } else {
                        left = left.wrapping_sub(margin); top = top.wrapping_sub(margin);
                        width = width.wrapping_add(margin.wrapping_mul(2)); height = height.wrapping_add(margin.wrapping_mul(2)); true
                    }
                } else { true };
                if !expanded { false } else {
                    let grid = unsafe { config(ConfigurationKey(0xa57a)) }.wrapping_mul(6);
                    let phase_x = ((left.wrapping_sub(u32::from(unsafe { half(request, 0x270) })) % 6) as u8 as u32 + u32::from(unsafe { at(request, 0x288).byte() })) % 6;
                    let phase_y = ((top.wrapping_sub(u32::from(unsafe { half(request, 0x274) })) % 6) as u8 as u32 + u32::from(unsafe { at(request, 0x289).byte() })) % 6;
                    let shift_x = (6 - phase_x) % 6;
                    let shift_y = (6 - phase_y) % 6;
                    left = left.wrapping_add(shift_x); top = top.wrapping_add(shift_y);
                    width = width.wrapping_sub(shift_x); height = height.wrapping_sub(shift_y);
                    if width < grid || height < grid { false } else {
                        if grid == 0 { unsafe { xm1_pipeline_image_scan_divide_fault(width) } }
                        let remainder_x = width % grid; let remainder_y = height % grid;
                        unsafe { put(region, 0x14, left.wrapping_add((remainder_x / 12) * 6)); put(region, 0x18, top.wrapping_add((remainder_y / 12) * 6)); put(region, 0x1c, width.wrapping_sub(remainder_x)); put(region, 0x20, height.wrapping_sub(remainder_y)); }
                        true
                    }
                }
            }
        };
        unsafe { put(region, 0x34, u32::from(accepted)) };
        valid |= accepted;
        index = index.wrapping_add(1);
    }
    i32::from(valid)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_translate_small_kind(output: Address32, kind: u32) {
    unsafe { put(output, 0, if (2..=5).contains(&kind) { kind - 1 } else { 0 }) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_translate_operation(output: Address32, kind: u32) {
    let (key, enabled) = match kind {
        48 | 49 | 84 | 85 => (0x7955, 7),
        50 | 86 => (0x7951, 9),
        51 | 87 => (0x7951, 10),
        52 | 53 | 88 | 89 => (0x7954, 13),
        54 | 55 | 90 | 91 => (0x7953, 14),
        56 | 57 | 92 | 93 => (0x7952, 16),
        _ => (0, 0),
    };
    let result = if key != 0 {
        if unsafe { config(ConfigurationKey(key)) } == 0 { 5 } else { enabled }
    } else {
        match kind {
            1..=4 => kind,
            5..=7 | 65 | 66 | 68 | 69 | 71 | 72 | 76 | 77 | 95 | 96 | 98 | 99 | 101 | 102 => 5,
            8..=10 | 78 => 7,
            11 => 8, 12 | 79 => 9, 13 | 80 => 10,
            14 => 11, 15 => 12, 16 | 81 => 13, 17 | 82 => 14,
            18 => 15, 19 | 83 => 16, 20 => 17, 21 => 18, 22 => 19,
            23 => 20, 24 => 21, 25 | 63 => 22, 26 => 23, 27 => 24,
            28 => 25, 29 => 26, 31 | 33 => 27, 34 => 28, 35 => 29,
            36 => 30, 37 => 31, _ => 0,
        }
    };
    unsafe { put(output, 0, result) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_translate_selection(kind: u32, output: Address32) {
    let result = match kind { 1..=3 | 5 | 6 => kind, 4 | 7..=10 => 4, _ => 0 };
    unsafe { put(output, 0, result) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_packing(flags: u32, packing: Address32, numerator: Address32, denominator: Address32) {
    let wide = flags & 0x10 != 0;
    unsafe { put(packing, 0, u32::from(wide)); put(numerator, 0, if wide { 3 } else { 2 }); put(denominator, 0, if wide { 2 } else { 1 }); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_channel_layout(flags: u32, output: Address32) {
    unsafe { put(output, 0, if flags & 0x3000 == 0x3000 { 2 } else { (flags >> 12) & 1 }) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_frame_mode(_flags: u32, output: Address32) {
    unsafe { put(output, 0, 2) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_component_kind(mode: u32, selection: i32, output: Address32) {
    let result = match (mode, selection) {
        (0, 0) => 0, (0, 2) => 1, (1, 0) => 2, (1, 2) => 3,
        (2, 0) => 4, (2, 2) => 5, (2, 1) => 6, (2, 3) => 7,
        (0..=2, _) => 8, _ => 4,
    };
    unsafe { put(output, 0, result) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_initialize_pair(count: Address32, kind: Address32) {
    unsafe { put_half(count, 0, 0); put(kind, 0, 4); }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_scan_zero_one(value: i32) -> i32 {
    match value { 0 => 0, 1 => 1, _ => 2 }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_scan_one_two(value: i32) -> i32 {
    match value { 1 => 1, 2 => 2, _ => 0 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_enabled_value(value: i32, kind: u32) -> i32 {
    if kind == 1 && unsafe { xm1_pipeline_image_configuration_signed_byte(ConfigurationKey(0x8300)) } != 0 { value } else { 0 }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_select_word_table(mode: u32, output: Address32) {
    let key = ConfigurationKey(0x8060 + if (1..=4).contains(&mode) { mode } else { 0 });
    let index = unsafe { config(key) };
    let mut first = 0;
    let mut second = 0;
    let mut values = Address32(0);
    unsafe { xm1_pipeline_image_scan_word_lookup(&mut first, &mut second, &mut values, index); put(output, 16, values.0); }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_prepare_components(context: Address32, request: Address32) {
    unsafe {
        clear(request, 0x2ac);
        let debug = Address32(word(context, 0x200));
        let profile = Address32(word(context, 0x1f8));
        xm1_pipeline_image_scan_translate_operation(at(request, 0x2a4), word(context, 0x224));
        xm1_pipeline_image_scan_translate_selection(word(context, 0x228), at(request, 0x2a0));
        let mode = word(context, 0x1d0);
        put(request, 0x220, if mode < 4 { mode } else { 4 });
        put(request, 0x224, profile.0);
        put(request, 0x228, profile.0.wrapping_add(0x400));
        put_half(request, 0x22c, word(debug, 0xa30a4) as u16);
        for (destination, source) in [(0x230, 0x1dc), (0x234, 0x1e0), (0x238, 0x1ec), (0x23c, 0x1f0)] {
            put(request, destination, word(context, source));
        }
        put(request, 0x240, word(context, 0x1e4).wrapping_sub(word(context, 0x1d4)));
        put(request, 0x244, word(context, 0x1e8).wrapping_sub(word(context, 0x1d8)));
        put_half(request, 0x248, 16);
        put(request, 0x24c, xm1_pipeline_image_scan_zero_one(word(context, 0x24c) as i32) as u32);
        let enabled = xm1_pipeline_image_scan_enabled_value(word(context, 0x250) as i32, mode);
        put(request, 0x250, xm1_pipeline_image_scan_one_two(enabled) as u32);
        put_half(request, 0x22e, half(context, 0x1f6));
        let plane = Address32(word(context, 0x204));
        let flags = word(plane, 24);
        let (numerator, denominator) = if flags & 0x10 != 0 { (3, 2) } else { (2, 1) };
        put(request, 0x278, u32::from(flags & 0x10 != 0));
        let stride = quotient(word(plane, 12).wrapping_mul(denominator), numerator);
        put(request, 0x25c, stride);
        put(request, 0x260, word(plane, 20));
        put_half(request, 0x274, 0); put_half(request, 0x276, 0); put_half(request, 0x270, 0);
        put_half(request, 0x272, stride.wrapping_sub(quotient(word(plane, 16).wrapping_mul(denominator), numerator)) as u16);
        put(request, 0x258, word(plane, 4));
        at(request, 0x264).write_byte(14);
        let origin = Address32(word(context, 0x21c));
        let raw_samples = read_volatile(origin.0 as *const RawSampleDescriptor);
        write_volatile(at(request, 0x268).0 as *mut RawSampleDescriptor, raw_samples);
        xm1_pipeline_image_scan_channel_layout(flags, at(request, 0x27c));
        xm1_pipeline_image_scan_component_kind(2, 4, at(request, 0x280));
        put(request, 0x284, xm1_pipeline_image_scan_zero_one(word(context, 0x248) as i32) as u32);
        at(request, 0x288).write_byte(word(context, 0x210) as u8);
        at(request, 0x289).write_byte(word(context, 0x214) as u8);
        request.write_byte(word(context, 0) as u8);
        put(request, 0x29c, u32::from(word(context, 0x224) == 24));
        for index in 0..10 {
            let source = at(context, 4 + index * 44);
            let destination = at(request, 4 + index * 52);
            put(destination, 0, word(source, 0)); put(destination, 4, word(source, 8));
            put(destination, 8, word(source, 4).wrapping_add(1).wrapping_sub(word(source, 0)));
            put(destination, 12, word(source, 12).wrapping_add(1).wrapping_sub(word(source, 8)));
            match word(source, 16) {
                0 => put_half(destination, 34, 0),
                1 => put_half(destination, 34, 180),
                2 => put_half(destination, 34, (-180i16) as u16),
                _ => {}
            }
            let angle = word(source, 20);
            if angle < 12 { put_half(destination, 32, (angle * 30) as u16); }
            put(destination, 36, u32::from(half(source, 24)));
            put_half(destination, 40, half(source, 26));
            put(destination, 44, u32::from(word(source, 40) == 1));
        }
        for (destination, source) in [(0x20c,0x1ca),(0x20e,0x1cc),(0x210,0x1ce),(0x218,0x238),(0x21a,0x23a),(0x21c,0x23c),(0x212,0x1c2),(0x214,0x1c4),(0x216,0x1c6)] {
            put_half(request, destination, half(context, source));
        }
        at(request, 0x21e).write_byte(10);
        put(request, 0x290, word(context, 0x208)); put(request, 0x294, word(context, 0x20c));
        let statistics = at(debug, 0x9e018);
        let scratch = Address32(word(context, 0x1fc));
        let ratios = at(scratch, 0x4720);
        for index in 0..64 {
            let count = word(statistics, index * 4);
            if count == 0 {
                put_half(ratios, index * 2, 0); put_half(ratios, 128 + index * 2, 0);
            } else {
                put_half(ratios, index * 2, quotient(word(statistics, 256 + index * 4).wrapping_mul(100), count) as u16);
                put_half(ratios, 128 + index * 2, quotient(word(statistics, 512 + index * 4).wrapping_mul(100), count) as u16);
            }
        }
        let kind = word(context, 0x230);
        let small = if (2..=5).contains(&kind) { kind - 1 } else { 0 };
        let index = config(ConfigurationKey(0x8060 + small));
        let mut first = 0; let mut second = 0; let mut values = Address32(0);
        xm1_pipeline_image_scan_word_lookup(&mut first, &mut second, &mut values, index);
        put(request, 0x254, values.0);
        put(request, 0x298, scratch.0); put(request, 0x28c, debug.0);
        put(request, 0x2a8, u32::from(word(context, 0x240) == 1));
    }
}

#[derive(Clone, Copy)]
enum DiagnosticField {
    Half(u32),
    SignedHalf(u32),
    Word(u32),
    Byte(u32),
    RequestHalf(u32),
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_export_diagnostics(request: Address32, workspace: Address32, output: Address32) {
    use DiagnosticField::{Byte, Half, RequestHalf, SignedHalf, Word};
    unsafe { clear(output, 0xd1c); put(output, 0, 0x5354_4644); }
    const FIELDS: &[(u32, DiagnosticField)] = &[
        (1,Half(0xc2a)),(2,Byte(0xc34)),(3,Byte(0xc35)),
        (4,Word(0xc38)),(5,Word(0xc3c)),(8,Word(0xc40)),(9,Word(0xc44)),(6,Word(0xc48)),(7,Word(0xc4c)),
        (10,Half(0xc50)),(11,Half(0xc52)),(12,Half(0xc54)),(13,Half(0xc56)),(14,Half(0xc58)),(15,Half(0xc5a)),(16,Half(0xc5c)),(17,Half(0xc5e)),
        (20,Word(0xc60)),(18,Half(0xc14)),(19,Half(0xc16)),
        (63,Half(0xc18)),(64,Half(0xc1a)),(65,Word(0xc1c)),(66,Half(0xc20)),(67,Half(0xc22)),(68,Half(0xc24)),(69,Half(0xc26)),
        (21,Half(0xc68)),(22,Half(0xc6a)),(23,Word(0xc6c)),(24,Word(0xc70)),
        (25,Word(0xc7c)),(26,Word(0xc78)),(27,Half(0xc80)),(28,Half(0xc82)),
        (31,Half(0xc84)),(32,Half(0xc86)),(33,Half(0xc88)),(34,SignedHalf(0xc8a)),(35,SignedHalf(0xc8c)),(37,SignedHalf(0xc96)),
        (29,Half(0xca2)),(30,Half(0xca4)),(36,Half(0xca6)),(38,Half(0xca8)),(39,Half(0xcaa)),
        (52,Word(0xc90)),(53,SignedHalf(0xc94)),(54,SignedHalf(0xc98)),(55,SignedHalf(0xc9a)),(56,SignedHalf(0xc9c)),(57,SignedHalf(0xc9e)),(58,Half(0xca0)),
        (40,RequestHalf(0x20c)),(41,RequestHalf(0x20e)),(42,RequestHalf(0x210)),
        (43,Half(12)),(44,Half(14)),(45,Half(16)),(46,Half(6)),(47,Half(8)),(48,Half(10)),
        (49,RequestHalf(0x212)),(50,RequestHalf(0x214)),(51,RequestHalf(0x216)),
        (59,Half(0)),(60,Half(2)),(61,Half(4)),(62,Word(0xcb0)),
    ];
    for &(slot, field) in FIELDS {
        let slot = DiagnosticSlot(slot);
        let value = unsafe { match field {
            Half(offset) => u32::from(half(workspace, offset)),
            SignedHalf(offset) => half(workspace, offset) as i16 as i32 as u32,
            Word(offset) => word(workspace, offset),
            Byte(offset) => u32::from(at(workspace, offset).byte()),
            RequestHalf(offset) => u32::from(half(request, offset)),
        } };
        unsafe { put(output, slot.0 * 4, value) };
        if slot.0 == 7 {
            for index in 0..256 {
                unsafe {
                    put(output, 0x118 + index * 4, word(workspace, 0x414 + index * 4));
                    put(output, 0x518 + index * 4, word(workspace, 0x14 + index * 4));
                    put(output, 0x918 + index * 4, word(workspace, 0x814 + index * 4));
                }
            }
        }
    }
    unsafe { put(output, 0xd18, 0x4c41_4644) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scan_word_lookup(first: *mut u32, second: *mut u32, values: *mut Address32, index: u32) {
    let record = at(Address32(0x0082_1bac), index.wrapping_mul(4104));
    if !first.is_null() { unsafe { Address32(first as u32).write_word(record.word()) }; }
    if !second.is_null() { unsafe { Address32(second as u32).write_word(word(record, 4)) }; }
    if !values.is_null() { unsafe { Address32(values as u32).write_word(record.0.wrapping_add(8)) }; }
}
