use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Plane {
    pub format: u32,
    pub address: u32,
    pub length: u32,
    pub stride: u32,
    pub line_size: u32,
    pub lines: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Frame(u32);
impl Frame {
    unsafe fn word(self, offset: u32) -> u32 {
        unsafe { read_volatile(self.0.wrapping_add(offset) as *const u32) }
    }
    unsafe fn plane(self, offset: u32) -> Plane {
        unsafe { read_volatile(self.0.wrapping_add(offset) as *const Plane) }
    }
}
unsafe fn frame(context: *mut u8) -> Frame {
    Frame(unsafe { read_volatile(context as *const u32) })
}
fn aligned(value: u32, mask: u32) -> u32 { value.wrapping_add(mask) & !mask }

#[derive(Clone, Copy)]
#[repr(transparent)]
struct CriticalToken(u32);

unsafe fn enter_critical() -> CriticalToken {
    use super::image_planes::{current_interface, interface};
    use super::shared::ByteOffset;
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let enter: unsafe extern "C" fn() -> u32 = unsafe { core::mem::transmute(table.offset(ByteOffset(0x164)).word()) };
    let owner: unsafe extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(table.offset(ByteOffset(0x1624)).word()) };
    let acquire: unsafe extern "C" fn(u32, u32) -> u32 = unsafe { core::mem::transmute(table.offset(ByteOffset(0x161c)).word()) };
    let saved = unsafe { enter() };
    if unsafe { owner(0) } != ((index.0 + 1) | 0x10) {
        let tag = (index.0 + 1) | 0x20;
        if unsafe { owner(0) } != tag {
            while unsafe { acquire(0, tag) } != 0 {}
        }
        let nesting = 0x40a0f824 as *mut u32;
        unsafe { write_volatile(nesting, read_volatile(nesting).wrapping_add(1)) };
    }
    CriticalToken(saved)
}

unsafe fn leave_critical(saved: CriticalToken) {
    use super::image_planes::{current_interface, interface};
    use super::shared::ByteOffset;
    let index = unsafe { current_interface() };
    let table = unsafe { interface(index) };
    let owner: unsafe extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(table.offset(ByteOffset(0x1624)).word()) };
    let release: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(table.offset(ByteOffset(0x1620)).word()) };
    let leave: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(table.offset(ByteOffset(0x168)).word()) };
    if unsafe { owner(0) } != ((index.0 + 1) | 0x10) {
        let nesting = 0x40a0f824 as *mut u32;
        let count = unsafe { read_volatile(nesting) };
        if count != 0 {
            let remaining = count - 1;
            unsafe { write_volatile(nesting, remaining) };
            if remaining == 0 { unsafe { release(0) }; }
        }
    }
    unsafe { leave(saved.0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_enter_critical() -> u32 {
    unsafe { enter_critical().0 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_leave_critical(token: u32) {
    unsafe { leave_critical(CriticalToken(token)) };
}

unsafe extern "C" {
    fn xm1_develop_capture_option_selection() -> u32;
    fn xm1_develop_transform_required(context: *mut u8, mode: u32) -> u32;
    fn xm1_develop_acquire_object(group: u32, kind: u32, owner: i32, inherited: i32, slot: *mut i16, ordered: i32, size: u32) -> u32;
    fn xm1_develop_release_object(group: u32, slot: i32, size: u32) -> u32;
    fn xm1_develop_notify_object_waiters();
    fn xm1_develop_trace_event(text: u32);
    fn xm1_pipeline_image_fill_pattern(cancel: *const u8, plane: *const Plane, mode: u32, auxiliary: u32, task: u32, restore: u32) -> u32;
    fn xm1_develop_append_stream_text(task: u32, text: u32);
    fn xm1_buffers_cache_endpoint(address: u32, length: u32);
}

/// # Safety
/// The configuration global must name its live 0x420000-byte RAM allocation.
pub(super) unsafe fn configuration_view() -> Result<crate::develop::calibration_core::configuration::Configuration<'static>, crate::develop::calibration_core::configuration::ConfigurationError<core::convert::Infallible>> {
    use crate::develop::calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationRegion, CONFIGURATION_RAM_SIZE};
    let base = ConfigAddress(unsafe { read_volatile(0x0096ca50 as *const u32) });
    let region = unsafe { ConfigurationRegion::physical(base, CONFIGURATION_RAM_SIZE) }?;
    Ok(Configuration::new(region, base))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_buffers_configuration_byte(key: u32) -> u32 {
    use crate::develop::calibration_core::configuration::CfgKey;
    match unsafe { configuration_view() }.and_then(|mut configuration| configuration.read_byte(CfgKey(key))) {
        Ok(value) => u32::from(value),
        Err(_) => loop { core::hint::spin_loop(); },
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_buffers_configuration_half(key: u32) -> u16 {
    let low = unsafe { xm1_buffers_configuration_byte(key) } as u8;
    let high = unsafe { xm1_buffers_configuration_byte(key.wrapping_add(1)) } as u8;
    crate::develop::calibration_core::configuration::decode_half(low, high)
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct GroupId(u32);

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct ObjectRecord {
    owner: i16,
    owner_reserved: u16,
    group: u32,
    kind: u32,
    slot: i16,
    slot_reserved: u16,
    plane: Plane,
}

fn registry(group: GroupId) -> (*mut ObjectRecord, usize) {
    if group.0 < 20 { (0x0154a9bc as *mut ObjectRecord, 20) } else { (0x0154ad2c as *mut ObjectRecord, 128) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_group(group: u32) -> u32 {
    if group < 20 { 0x00e48370u32.wrapping_add(group.wrapping_mul(16)) } else {
        let mode = unsafe { read_volatile(0x015480d4 as *const u32) };
        0x00e4a948u32.wrapping_add(mode.wrapping_mul(73).wrapping_sub(20).wrapping_add(group).wrapping_mul(16))
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_find_object_entry_index(objects: u32, group: u32, kind: u32) -> i32 {
    let owner = unsafe { read_volatile(objects as *const i16) };
    let (entries, limit) = registry(GroupId(group));
    for index in 0..limit {
        let entry = unsafe { read_volatile(entries.add(index)) };
        if entry.owner == 0 { break; }
        if entry.owner == owner && entry.group == group && entry.kind == kind { return i32::from(entry.slot); }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_entry(objects: u32, group: u32, kind: u32) -> u32 {
    let descriptor = Frame(unsafe { xm1_develop_object_group(group) });
    let token = unsafe { enter_critical() };
    let index = unsafe { xm1_develop_find_object_entry_index(objects, group, kind) };
    unsafe { leave_critical(token) };
    if index < 1 { 0 } else { unsafe { descriptor.word(12) }.wrapping_add((index as u32 - 1).wrapping_mul(28)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_store_object(objects: u32, group: u32, kind: u32, plane: *const Plane) {
    let owner = unsafe { read_volatile(objects as *const i16) };
    let entries = 0x0154ad2c as *mut ObjectRecord;
    let token = unsafe { enter_critical() };
    for index in 0..128 {
        let entry = unsafe { read_volatile(entries.add(index)) };
        if entry.owner == 0 { break; }
        if entry.owner == owner && entry.group == group && entry.kind == kind {
            unsafe { write_volatile(core::ptr::addr_of_mut!((*entries.add(index)).plane), read_volatile(plane)) };
            break;
        }
    }
    unsafe { leave_critical(token) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_registry_lookup(owner: i32, group: u32, kind: u32, slot: i32, output: *mut ObjectRecord) {
    unsafe { write_volatile(output, ObjectRecord::default()) };
    let (entries, limit) = registry(GroupId(group));
    let token = unsafe { enter_critical() };
    for index in 0..limit {
        let entry = unsafe { read_volatile(entries.add(index)) };
        if entry.owner == 0 { break; }
        if entry.group == group && ((entry.owner == -1 && i32::from(entry.slot) == slot) || (i32::from(entry.owner) == owner && entry.kind == kind)) {
            unsafe { write_volatile(output, entry) };
            break;
        }
    }
    unsafe { leave_critical(token) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_registry_insert(record: *const ObjectRecord) {
    let record = unsafe { read_volatile(record) };
    let (entries, limit) = registry(GroupId(record.group));
    let token = unsafe { enter_critical() };
    for index in 0..limit {
        if unsafe { read_volatile(entries.add(index)) }.owner == 0 {
            unsafe { write_volatile(entries.add(index), record); leave_critical(token); }
            return;
        }
    }
    unsafe { leave_critical(token) };
    loop { core::hint::spin_loop(); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_registry_remove(record: *const ObjectRecord) {
    let record = unsafe { read_volatile(record) };
    let (entries, limit) = registry(GroupId(record.group));
    let token = unsafe { enter_critical() };
    for index in 0..limit {
        let entry = unsafe { read_volatile(entries.add(index)) };
        if entry.owner == 0 { break; }
        if entry.group == record.group && ((entry.owner == -1 && entry.slot == record.slot) || (entry.owner == record.owner && entry.kind == record.kind)) {
            let mut destination = index;
            while destination < limit - 1 {
                let next = unsafe { read_volatile(entries.add(destination + 1)) };
                if next.owner == 0 { break; }
                unsafe { write_volatile(entries.add(destination), next) };
                destination += 1;
            }
            unsafe { write_volatile(entries.add(destination), ObjectRecord::default()) };
            break;
        }
    }
    unsafe { leave_critical(token) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_open(objects: u32, group: u32, kind: u32, argument: u32, size: u32) -> u32 {
    let descriptor = Frame(unsafe { xm1_develop_object_group(group) });
    let owner = unsafe { read_volatile(objects as *const i16) };
    let ordered = unsafe { read_volatile(0x015480d8 as *const u32) } != 0 && argument != 0;
    let mut record = ObjectRecord::default();
    unsafe { xm1_develop_object_registry_lookup(i32::from(owner), group, kind, 0, &mut record) };
    if record.slot != 0 {
        let task = unsafe { super::image_planes::current_task() };
        unsafe { xm1_develop_append_stream_text(task.0 & 255, 0xe498e8); xm1_develop_trace_event(0xe498f8); }
        return 0;
    }
    if unsafe { descriptor.word(0) } == 0 { return 0; }
    let mut inherited = 0;
    if unsafe { descriptor.word(8) } as i32 > 0 && group.wrapping_sub(1) >= 20 {
        let mut previous = group - 1;
        loop {
            unsafe { xm1_develop_object_registry_lookup(i32::from(owner), previous, kind, 0, &mut record) };
            let other = Frame(unsafe { xm1_develop_object_group(previous) });
            if unsafe { other.word(8) } == unsafe { descriptor.word(8) } && record.slot > 0 {
                inherited = i32::from(record.slot);
                break;
            }
            previous = previous.wrapping_sub(1);
            if previous < 20 { break; }
        }
    }
    let mut slot = 0i16;
    let result = unsafe { xm1_develop_acquire_object(group, kind, i32::from(owner), inherited, &mut slot, i32::from(ordered), size) };
    if result != 0 { return result; }
    record.owner = unsafe { read_volatile(objects as *const i16) };
    record.group = group;
    record.kind = kind;
    record.slot = slot;
    let planes = unsafe { descriptor.word(12) };
    if planes != 0 {
        record.plane = unsafe { read_volatile(planes.wrapping_add((i32::from(slot).wrapping_sub(1) as u32).wrapping_mul(28)) as *const Plane) };
        if group == 33 { record.plane.length = size; }
    }
    unsafe { xm1_develop_object_registry_insert(&record) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_object_close(objects: u32, group: u32, kind: u32, size: u32) -> u32 {
    let owner = unsafe { read_volatile(objects as *const i16) };
    let mut record = ObjectRecord::default();
    unsafe { xm1_develop_object_registry_lookup(i32::from(owner), group, kind, 0, &mut record) };
    if record.slot == 0 { return 0; }
    let size = if group == 33 { record.plane.length } else { size };
    let result = unsafe { xm1_develop_release_object(record.group, i32::from(record.slot), size) };
    if result != 0 { return result; }
    unsafe { xm1_develop_object_registry_remove(&record) };
    if group >= 20 { unsafe { xm1_develop_notify_object_waiters() }; }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_canonical_object_group(group: u32) -> u32 {
    let handle = unsafe { Frame(xm1_develop_object_group(group)).word(0) };
    for candidate in 0..93 {
        if unsafe { Frame(xm1_develop_object_group(candidate)).word(0) } == handle { return candidate; }
    }
    93
}


unsafe fn append_trace_text(destination: *mut u8, source: u32, limit: u32) {
    let mut end = destination;
    while unsafe { read_volatile(end) } != 0 { end = unsafe { end.add(1) }; }
    for index in 0..limit {
        let value = unsafe { read_volatile(source.wrapping_add(index) as *const u8) };
        unsafe { write_volatile(end, value) };
        if value == 0 { return; }
        end = unsafe { end.add(1) };
    }
    unsafe { write_volatile(end, 0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_trace_object_text(group: u32, _slot: i32, action: u32) {
    let mut text = [0u8; 32];
    unsafe { append_trace_text(text.as_mut_ptr(), 0xe49874, u32::MAX) };
    if action > 3 { return; }
    unsafe { append_trace_text(text.as_mut_ptr(), 0xe4987c + action * 4, u32::MAX) };
    if action == 3 {
        let task = unsafe { super::image_planes::current_task() };
        unsafe { xm1_develop_append_stream_text(task.0 & 255, 0xe4988c) };
    }
    let name = unsafe { read_volatile(0x00e4a208u32.wrapping_add(group.wrapping_mul(4)) as *const u32) };
    unsafe { append_trace_text(text.as_mut_ptr(), name, 23) };
    let task = unsafe { super::image_planes::current_task() };
    unsafe { xm1_develop_append_stream_text(task.0 & 255, text.as_ptr() as u32) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_trace_object_event(group: u32, slot: i32, action: u32) {
    let mut text = [0u8; 9];
    let source = unsafe { read_volatile(0x00e47e60u32.wrapping_add(group.wrapping_mul(4)) as *const u32) };
    for index in 0..5u32 {
        text[index as usize] = unsafe { read_volatile(source.wrapping_add(index) as *const u8) };
    }
    let tens = slot / 10;
    text[5] = tens.wrapping_add(48) as u8;
    text[6] = slot.wrapping_sub(tens.wrapping_mul(10)).wrapping_add(48) as u8;
    text[7] = match action { 0 => b'W', 1 => b'S', 2 => b'E', _ => return };
    unsafe { xm1_develop_trace_event(text.as_ptr() as u32) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_trace_object_action(group: u32, slot: i32, action: u32) {
    if unsafe { read_volatile(0x015480d4 as *const u32) } == 3 { return; }
    unsafe { xm1_develop_trace_object_text(group, slot, action); xm1_develop_trace_object_event(group, slot, action); }
}


unsafe fn copy_diagnostic_text(destination: u32, source: u32) {
    let mut ended = false;
    for index in 0..15u32 {
        let value = if ended { 0 } else { unsafe { read_volatile(source.wrapping_add(index) as *const u8) } };
        ended |= value == 0;
        unsafe { write_volatile(destination.wrapping_add(index) as *mut u8, value) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_report_error(file: u32, line: u32, reason: u32, _detail: u32) {
    if unsafe { read_volatile(0x00eb1750 as *const u8) } == 0 { return; }
    if file != 0 {
        let mut last_slash = 0u32;
        let mut index = 0u32;
        loop {
            let value = unsafe { read_volatile(file.wrapping_add(index) as *const u8) };
            if value == 0 { break; }
            if value == b'/' { last_slash = index; }
            index = index.wrapping_add(1);
        }
        unsafe { copy_diagnostic_text(0x015b5bc4, file.wrapping_add(last_slash).wrapping_add(1)); xm1_develop_append_stream_text(3, 0x015b5bc4); }
    }
    let mut remaining = line;
    for index in (0..15usize).rev() {
        let value = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        unsafe { write_volatile((0x015b5bb4usize + index) as *mut u8, value) };
    }
    unsafe { xm1_develop_append_stream_text(3, 0x015b5bb4) };
    if reason != 0 {
        unsafe { copy_diagnostic_text(0x015b5bd4, reason); xm1_develop_append_stream_text(3, 0x015b5bd4); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_expand_planes(_context: *mut u8, planes: *mut Plane, source: *const Plane, auxiliary: *const Plane, count: u8, parity: u32) {
    unsafe { write_volatile(planes, read_volatile(source)) };
    for index in 1..count as usize {
        let previous = unsafe { read_volatile(planes.add(index - 1)) };
        let mut next = if index == 1 { unsafe { read_volatile(auxiliary) } } else { previous };
        if index == 1 {
            if parity != 0 { next.address = next.address.wrapping_add(aligned(previous.stride >> 1, 31)); }
            next.line_size = aligned(previous.line_size >> 2, 31) & 0x7fff_ffff;
        } else if index == 3 {
            next.address = previous.address.wrapping_add(previous.lines.wrapping_mul(previous.stride));
            next.line_size = aligned(previous.line_size >> 1, 31);
        } else {
            next.address = previous.address.wrapping_add(previous.line_size);
            next.line_size = aligned(previous.line_size >> 1, 31);
        }
        next.stride = previous.stride;
        next.lines = (previous.lines >> 1).wrapping_add(1) & !1;
        next.length = next.stride.wrapping_mul(next.lines);
        unsafe { write_volatile(planes.add(index), next) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_split_planes(_context: *mut u8, planes: *mut Plane, source: *const Plane, auxiliary: *const Plane, count: u8) {
    unsafe { write_volatile(planes, read_volatile(source)) };
    for index in 1..count as usize {
        let previous = unsafe { read_volatile(planes.add(index - 1)) };
        let mut next = if index == 1 { unsafe { read_volatile(auxiliary) } } else { previous };
        if index == 4 {
            next.address = previous.address.wrapping_add(previous.lines.wrapping_mul(previous.stride));
        } else if index != 1 {
            next.address = previous.address.wrapping_add(previous.line_size);
        }
        next.line_size = aligned(previous.line_size >> 1, 63);
        next.stride = previous.stride;
        next.lines = (previous.lines >> 1).wrapping_add(1) & !1;
        next.length = next.stride.wrapping_mul(next.lines);
        unsafe { write_volatile(planes.add(index), next) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_lookup_width(profile: u32, aspect: u32) -> u32 {
    let column = if aspect < 3 { aspect } else { 3 };
    unsafe { read_volatile(0x0081_6f60u32.wrapping_add(profile.wrapping_mul(16)).wrapping_add(column * 4) as *const u32) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_lookup_height(profile: u32, aspect: u32) -> u32 {
    let column = if aspect < 3 { aspect } else { 3 };
    unsafe { read_volatile(0x0081_63a8u32.wrapping_add(profile.wrapping_mul(16)).wrapping_add(column * 4) as *const u32) }
}
fn fit(width: u32, height: u32, numerator: u32, denominator: u32) -> Crop {
    let difference = height.wrapping_mul(numerator).wrapping_sub(width.wrapping_mul(denominator)) as i32;
    let mut crop = Crop { width, height, ..Crop::default() };
    if difference > 0 {
        crop.height = width.wrapping_mul(denominator) / numerator;
        let border = height.wrapping_sub(crop.height);
        crop.top = border >> 1;
        crop.bottom = border.wrapping_sub(crop.top);
    } else if difference < 0 {
        crop.width = height.wrapping_mul(numerator) / denominator;
        let border = width.wrapping_sub(crop.width);
        crop.left = border >> 1;
        crop.right = border.wrapping_sub(crop.left);
    }
    crop
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_fit_ratio(width: u32, height: u32, numerator: u32, denominator: u32, crop: *mut Crop) {
    unsafe { write_volatile(crop, fit(width, height, numerator, denominator)) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_fit_aspect(width: u32, height: u32, aspect: u32, crop: *mut Crop) {
    let (numerator, denominator) = match aspect { 1 => (3, 2), 2 => (16, 9), 3 => (1, 1), _ => (4, 3) };
    unsafe { write_volatile(crop, fit(width, height, numerator, denominator)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_fill_border_regions(task: u32, restore: u32, plane: *const Plane, _unused: *const Plane, amounts: *const u32) -> u32 {
    let mut cancel = 0;
    let mut result = 0;
    for side in 0..4 {
        let mut region = unsafe { read_volatile(plane) };
        let amount = unsafe { read_volatile(amounts.add(side)) };
        if amount == 0 { continue; }
        if side < 2 {
            if side == 1 { region.address = region.address.wrapping_add(region.stride.wrapping_mul(region.lines.wrapping_sub(amount))); }
            region.lines = amount;
        } else {
            region.line_size = amount.wrapping_mul(2);
            if side == 3 { region.address = region.address.wrapping_add(region.stride).wrapping_sub(region.line_size); }
        }
        region.length = region.lines.wrapping_mul(region.line_size);
        result = unsafe { xm1_pipeline_image_fill_pattern(&mut cancel, &region, 0, 0, task, restore) };
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_fill_border(context: *mut u8, plane: *const Plane, lookup: u32, selector: u32, mode: u32) -> u32 {
    let mut region = unsafe { read_volatile(plane) };
    if region.address != 0 {
        let state = unsafe { frame(context) };
        let crop = if unsafe { state.word(0x4b4) } == 0x1d {
            let height = unsafe { state.word(0x198) }.wrapping_mul(region.line_size >> 1) / (unsafe { state.word(0x194) } >> 1);
            let border = region.lines.wrapping_sub(height);
            Crop { width: region.line_size, height, top: border >> 1, bottom: border.wrapping_sub(border >> 1), left: 0, right: 0 }
        } else if mode == 1 {
            fit(region.line_size >> 1, region.lines, unsafe { xm1_develop_lookup_width(lookup, selector) }, unsafe { xm1_develop_lookup_height(lookup, selector) })
        } else {
            let mut crop = Crop::default();
            unsafe { xm1_develop_fit_aspect(region.line_size >> 1, region.lines, selector, &mut crop) };
            crop
        };
        region.line_size = crop.width.wrapping_mul(2);
        region.lines = crop.height;
        let amounts = [crop.top, crop.bottom, crop.left, crop.right];
        let ids = Frame(context as u32);
        unsafe { xm1_develop_fill_border_regions(ids.word(0x34), ids.word(0x3c), plane, &region, amounts.as_ptr()) };
    }
    1
}

#[no_mangle]
pub extern "C" fn xm1_develop_buffer_policy_disabled() -> u32 { 0 }

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_output_matches(context: *mut u8, plane: *const Plane, output: u32, mode: u32) -> u32 {
    if unsafe { xm1_develop_capture_option_selection() } != 0 { return 1; }
    let state = unsafe { frame(context) };
    let plane = unsafe { read_volatile(plane) };
    let special = unsafe { state.word(0x4b4) } == 0x19 || unsafe { state.word(0x710) } & 0x400 != 0;
    let (width, height) = if special && output == 0 {
        if plane.flags & 0x0200_0000 == 0 { return 0; }
        let (profile, aspect) = if mode == 2 { (0x21, 1) } else { (unsafe { state.word(0x644) }, unsafe { state.word(0x4f0) }) };
        (unsafe { xm1_develop_lookup_width(profile, aspect) }.wrapping_mul(2), unsafe { xm1_develop_lookup_height(profile, aspect) })
    } else if output == 1 { (320, 120) } else if output == 2 { (640, 240) } else {
        let (profile, aspect) = if mode == 2 { (0x21, 1) } else { (unsafe { state.word(0x644) }, unsafe { state.word(0x4f0) }) };
        (unsafe { xm1_develop_lookup_width(profile, aspect) }.wrapping_mul(2), unsafe { xm1_develop_lookup_height(profile, aspect) })
    };
    u32::from(plane.line_size == plane.stride && plane.line_size == width && plane.lines == height)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_conversion_required(context: *mut u8, plane: *const Plane, mode: u32) -> u32 {
    let state = unsafe { frame(context) };
    let capture = unsafe { state.word(0x4b4) };
    if capture == 0x20 || capture == 0x1d { return 1; }
    if unsafe { xm1_develop_output_matches(context, plane, 0, mode) } == 1 { return 0; }
    if mode == 1 && unsafe { state.word(0x5a8) } == 1 { return 1; }
    if unsafe { xm1_develop_transform_required(context, mode) } == 1 { return 1; }
    if unsafe { state.word(0x4ac) } == 1 || unsafe { state.word(0x78) } == 0xb { return 1; }
    if unsafe { state.word(0x4a8) } != 0 { return 0; }
    u32::from(unsafe { state.word(0x550) }.wrapping_sub(0x97) < 2)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_operation_memory_size(context: *mut u8, group: u32, kind: u32) -> u32 {
    let state = unsafe { frame(context) };
    if unsafe { state.word(0x4b4) } == 0x1d || unsafe { state.word(0x604) } == 1 || group != 0x21 { return 0; }
    if unsafe { state.word(0x27ac) } == 1 && unsafe { state.word(0x600) } == 1 { return 0x26d160; }
    if kind == 0x10 { return 0x144d60; }
    let mut entry = Frame(0x00ea1da0);
    while unsafe { entry.word(0) } != 10 {
        if unsafe { entry.word(0) } == unsafe { state.word(0x644) } && unsafe { entry.word(4) } == unsafe { state.word(0x50c) } && unsafe { entry.word(8) } == unsafe { state.word(0x4f0) } { return unsafe { entry.word(12) }; }
        entry.0 = entry.0.wrapping_add(16);
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_open_object(context: *mut u8, group: u32, kind: u32, argument: u32) -> u32 {
    let size = unsafe { xm1_develop_operation_memory_size(context, group, kind) };
    let result = unsafe { xm1_develop_object_open(frame(context).0.wrapping_add(0x7c), group, kind, argument, size) };
    if result != 0 { unsafe { xm1_develop_report_error(0xea1fa0, 0, 0xea1fac, 0) }; }
    result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_close_object(context: *mut u8, group: u32, kind: u32) -> u32 {
    let size = unsafe { xm1_develop_operation_memory_size(context, group, kind) };
    let result = unsafe { xm1_develop_object_close(frame(context).0.wrapping_add(0x7c), group, kind, size) };
    if result != 0 { unsafe { xm1_develop_report_error(0xea1fa0, 0, 0xea1fb8, 0) }; }
    result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_remap_object(context: *mut u8, object: u32) -> u32 {
    let mut entry = Frame(0xea1eec);
    loop {
        let matched = if unsafe { entry.word(0) } == object { Some(0) } else if unsafe { entry.word(4) } == object { Some(1) } else if unsafe { entry.word(8) } == object { Some(2) } else { None };
        if let Some(column) = matched {
            if unsafe { entry.word(column * 4) } == 0x5d { return object; }
            let state = unsafe { frame(context) };
            if unsafe { state.word(0x4b4) } == 0x19 || unsafe { state.word(0x710) } & 0x400 != 0 {
                return if column == 0 { unsafe { entry.word(4) } } else { object };
            }
            return if unsafe { state.word(0x4b4) } == 0x1a && column == 1 { unsafe { entry.word(0) } } else { object };
        }
        entry.0 = entry.0.wrapping_add(8);
        if unsafe { entry.word(8) } == 0x5d { return object; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_geometry(source: *const Plane, plane: *mut Plane, borders: *mut u32, profile: u32, aspect: u32, capture: u32, mode: u32, anchor: u32) {
    let mut region = unsafe { read_volatile(plane) };
    let (width, height) = if capture == 0x1d {
        let source = unsafe { read_volatile(source) };
        let width = region.line_size >> 1;
        (width, source.lines.wrapping_mul(width) / (source.line_size >> 1))
    } else if mode == 1 && aspect != 3 {
        let width = region.line_size >> 1;
        (width, width.wrapping_mul(unsafe { xm1_develop_lookup_height(profile, aspect) }) / unsafe { xm1_develop_lookup_width(profile, aspect) })
    } else if aspect == 3 { (region.lines, region.lines) } else {
        let width = region.line_size >> 1;
        (width, match aspect { 1 => width.wrapping_mul(2) / 3, 2 => width.wrapping_mul(9) >> 4, _ => width.wrapping_mul(3) >> 2 })
    };
    let vertical = region.lines.wrapping_sub(height);
    let top = if anchor == 1 { 0 } else { vertical >> 1 };
    let bottom = vertical.wrapping_sub(top);
    let horizontal = (region.line_size >> 1).wrapping_sub(width);
    let left = horizontal >> 1;
    region.address = region.address.wrapping_add(left.wrapping_mul(2)).wrapping_add(region.stride.wrapping_mul(top));
    region.line_size = region.line_size.wrapping_sub(horizontal.wrapping_mul(2));
    region.lines = height;
    region.length = region.line_size.wrapping_mul(height);
    unsafe { write_volatile(plane, region) };
    if !borders.is_null() {
        for (index, value) in [top, bottom, left, horizontal.wrapping_sub(left)].iter().enumerate() { unsafe { write_volatile(borders.add(index), *value) }; }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_dimensions(profile: u32, aspect: u32, _mode: u32, width: *mut u32, height: *mut u32) {
    let (w, h) = match profile {
        2 | 3 => match aspect { 1 => (272,180), 2 => (272,153), 3 => (204,204), _ => (272,204) },
        5 => match aspect { 1 => (176,118), 2 => (176,99), 3 => (132,132), _ => (176,132) },
        _ => (0,0),
    };
    unsafe { write_volatile(width, w); write_volatile(height, h); }
}
#[no_mangle]
pub extern "C" fn xm1_develop_orientation(selection: u32, orientation: u32) -> u32 {
    match (selection, orientation) {
        (2,1) => 3, (2,2) => 4, (2,3) => 2, (2,_) => 1,
        (3,1) => 4, (3,2) => 1, (3,3) => 3, (3,_) => 2,
        (4,1) => 1, (4,2) => 2, (4,3) => 4, (4,_) => 3,
        (_,1) => 2, (_,2) => 3, (_,3) => 1, _ => 4,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_descriptor_list(context: *mut u8, plane: *mut Plane, enlarged: *mut u32) -> u32 {
    unsafe { write_volatile(enlarged, 0) };
    let state = unsafe { frame(context) };
    let mut region = unsafe { state.plane(if state.word(0x4a8) == 0 { 0x184 } else { 0x2b8 }) };
    let (width, x, y) = if unsafe { state.word(0x277c) } == 1 {
        let width = unsafe { state.word(0x30ac) }.wrapping_add(1).wrapping_sub(unsafe { state.word(0x30a8) });
        (width, unsafe { state.word(0x30a8) }.wrapping_add(width >> 1), unsafe { state.word(0x30b0) }.wrapping_add(unsafe { state.word(0x30b4) }.wrapping_add(1).wrapping_sub(unsafe { state.word(0x30b0) }) >> 1))
    } else {
        let key = if unsafe { state.word(0x2778) } == 1 { 0xa8ef } else { 0xa8e6 };
        let scale = unsafe { xm1_buffers_configuration_byte(key) }.wrapping_mul(640) / 320;
        let x_scale = unsafe { xm1_buffers_configuration_byte(key + 1) }.wrapping_mul(640) / 320;
        let y_scale = unsafe { xm1_buffers_configuration_byte(key + 2) }.wrapping_mul(480) / 240;
        ((region.line_size & !1).wrapping_mul(scale) / 640, (unsafe { state.word(0x194) } >> 1).wrapping_mul(x_scale) / 640, unsafe { state.word(0x198) }.wrapping_mul(y_scale) / 480)
    };
    let mut bytes = (width >> 1) << 2;
    let triple = width.wrapping_mul(3);
    let mut candidate = (triple >> 6) << 5;
    if candidate != triple >> 1 { candidate = candidate.wrapping_add(32); }
    while candidate >= width {
        let half = candidate >> 1;
        if x.wrapping_sub(half) as i32 >= 0 && x.wrapping_sub(1).wrapping_add(half) <= (bytes >> 1).wrapping_sub(1) && y.wrapping_sub(half) as i32 >= 0 && y.wrapping_sub(1).wrapping_add(half) <= region.lines.wrapping_sub(1) {
            unsafe { write_volatile(enlarged, 1) };
            bytes = candidate << 1;
            break;
        }
        candidate = candidate.wrapping_sub(32);
    }
    region.address = region.address.wrapping_add(region.stride.wrapping_mul(y.wrapping_sub(bytes >> 2))).wrapping_add((x.wrapping_sub(bytes >> 2) >> 1) << 2);
    region.length = bytes.wrapping_mul(bytes >> 1);
    region.line_size = bytes;
    region.lines = bytes >> 1;
    unsafe { write_volatile(plane, region) };
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_rotate_copy(_frame: u32, source: *const Plane, target: *mut Plane) {
    let task = unsafe { super::image_planes::current_task() };
    unsafe { xm1_buffers_cache_endpoint(0, 0) };
    let source = unsafe { read_volatile(source) };
    let target = unsafe { read_volatile(target) };
    unsafe { xm1_develop_append_stream_text(task.0 & 255, 0xea1f54); super::image_planes::configure_priority(task, super::image_planes::TaskPriority(13)); }
    let pixels = source.line_size >> 1;
    let words = source.line_size >> 2;
    let source_gap = (source.stride >> 1).wrapping_sub(pixels).wrapping_mul(2);
    let target_gap = ((target.stride >> 1).wrapping_sub(pixels) & 0x7fff_fffe).wrapping_mul(2);
    let mut input = source.address.wrapping_add((source.stride >> 1).wrapping_mul(source.lines).wrapping_mul(2)).wrapping_sub(4).wrapping_sub(source_gap);
    let mut output = target.address;
    for _ in 0..source.lines {
        for _ in 0..words {
            let value = unsafe { read_volatile(input as *const u32) };
            let swapped = (value & 0xffff0000) | ((value & 255) << 8) | ((value >> 8) & 255);
            unsafe { write_volatile(output as *mut u32, swapped) };
            input = input.wrapping_sub(4);
            output = output.wrapping_add(4);
        }
        input = input.wrapping_sub(source_gap & !3);
        output = output.wrapping_add(target_gap);
    }
    unsafe { super::image_planes::configure_priority(task, super::image_planes::TaskPriority(6)); xm1_develop_append_stream_text(task.0 & 255, 0xea1f70); xm1_buffers_cache_endpoint(0, 0); }
}
