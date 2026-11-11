core::arch::global_asm!(include_str!("DebugStorage_stock.S"));

use super::shared::{Address32, ByteOffset};
use core::mem::MaybeUninit;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DebugFileKind(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SuffixNumber(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct StorageStatus(pub u32);

extern "C" {
    fn xm1_buffers_configuration_byte(key: u32) -> u32;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_wait_all(mask: u32, output: *mut u32, timeout: i32) -> i32;
    fn xm1_pipeline_image_send_notification(task: u32, value: u32) -> i32;
    fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32;
    fn xm1_pipeline_image_diagnostic_event(category: u32, code: u32);
    fn xm1_pipeline_debug_kernel_error(operation: u32, object: u32, status: i32);
}

fn at(base: Address32, offset: u32) -> Address32 { base.offset(ByteOffset(offset)) }
unsafe fn context() -> Address32 { Address32(unsafe { Address32(0x910204).word() }) }
pub(crate) unsafe fn copy(destination: Address32, source: Address32, count: u32) {
    if count == 0 { return; }
    unsafe {
        core::ptr::copy_nonoverlapping(
            source.0 as *const MaybeUninit<u8>,
            destination.0 as *mut MaybeUninit<u8>,
            count as usize,
        );
    }
}
pub(crate) unsafe fn append(destination: Address32, source: Address32, maximum: Option<u32>) {
    let mut end = 0;
    while unsafe { at(destination, end).byte() } != 0 { end += 1; }
    let mut index = 0;
    loop {
        if maximum == Some(index) { break; }
        let byte = unsafe { at(source, index).byte() };
        if byte == 0 { break; }
        unsafe { at(destination, end + index).write_byte(byte) };
        index += 1;
    }
    unsafe { at(destination, end + index).write_byte(0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_decimal(destination: Address32, value: u32, width: u32, flags: u32) -> i32 {
    let mut digits = [0u8; 10];
    let mut count = 0;
    let mut remaining = value;
    let mut divisor = 1_000_000_000;
    while divisor != 0 {
        let digit = remaining / divisor;
        if count != 0 || digit != 0 { digits[count] = b'0' + digit as u8; count += 1; }
        remaining %= divisor;
        divisor /= 10;
    }
    if width < count as u32 { return -1; }
    let padding = if flags <= 3 { width - count as u32 } else { 0 };
    if flags <= 3 {
        for index in 0..width { unsafe { at(destination, index).write_byte(if flags <= 1 { b'0' } else { b' ' }) }; }
    }
    for (index, byte) in digits[..count].iter().enumerate() { unsafe { at(destination, padding + index as u32).write_byte(*byte) }; }
    if flags & 1 == 0 { unsafe { at(destination, padding + count as u32).write_byte(0) }; }
    count as i32
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_validate_name(text: Address32, size: u32) -> StorageStatus {
    for index in 0..size {
        let byte = unsafe { at(text, index).byte() };
        if !byte.is_ascii_alphanumeric() && byte != b'_' { return StorageStatus(8); }
    }
    StorageStatus(0)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_ready(mode: u32) -> StorageStatus {
    let state = unsafe { context() };
    if state.0 == 0 || unsafe { at(state, 0x114).word() } == u32::MAX { return StorageStatus(5); }
    let flags = unsafe { at(state, 0x16bd8).byte() };
    let (status, code) = if flags & 1 != 0 { (1, 0x50) }
        else if flags & 2 != 0 { if mode >= 2 { return StorageStatus(0); } (5, 0x55) }
        else if flags & 4 != 0 { (5, 0x53) }
        else if flags & 8 != 0 && mode == 0 { (2, 0x52) }
        else { return StorageStatus(0); };
    unsafe { xm1_pipeline_image_diagnostic_event(2, code) };
    StorageStatus(status)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_group(mask: u32) -> u32 {
    let count = unsafe { at(context(), 0x2600).byte() } as u32;
    for index in 0..count {
        if index < 32 && mask & (1 << index) != 0 { return index; }
    }
    255
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_drive_for_device(device: u32) -> u32 {
    let routes = Address32(0x814730);
    let index = if unsafe { at(routes, 1).byte() } == 1 && unsafe { at(routes, 2).byte() } as u32 == device { 0 }
        else if unsafe { at(routes, 9).byte() } == 1 && unsafe { at(routes, 10).byte() } as u32 == device { 1 }
        else { 2 };
    unsafe { at(routes, index * 8).byte() as u32 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_current_drive() -> u32 {
    let state = unsafe { context() };
    if state.0 == 0 { return 0; }
    let device = unsafe { at(state, 0x114).word() };
    if device > 1 { return 0; }
    unsafe { xm1_pipeline_image_storage_drive_for_device(device) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_drive_prefix(destination: Address32) {
    let device = unsafe { at(context(), 0x114).word() };
    unsafe {
        destination.write_byte(xm1_pipeline_image_storage_drive_for_device(device) as u8);
        at(destination, 1).write_byte(b':');
        at(destination, 2).write_byte(0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_counter() -> u32 {
    let count = unsafe { Address32(0x409c281c).half() };
    if count > 9999 { 0 } else { count as u32 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_directory() -> u32 {
    let directory = unsafe { Address32(0x409c281e).half() };
    if directory.wrapping_sub(100) > 899 { 100 } else { directory as u32 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_choose_pair(_device: u32, group: u32, directory: *mut u16, file: *mut u16) -> StorageStatus {
    let state = unsafe { context() };
    let record = at(state, 0x21d0 + group * 16);
    let mut dir = unsafe { at(record, 4).half() } as u32;
    let mut number = unsafe { at(record, 6).half() };
    if dir < 100 {
        let largest = unsafe { at(state, 0x2168).word() };
        if largest <= 100 && unsafe { at(state, 0x2794).word() } == 0 { dir = 100; number = 0; }
        else { dir = largest as u16 as u32; number = ((dir % 10) * 1000 + 999) as u16; }
    }
    let (mut saved_dir, mut saved_file) = if unsafe { at(state, 0x16c6c).word() } == 0 { (100, 0) }
        else { (unsafe { xm1_pipeline_image_storage_directory() }, unsafe { xm1_pipeline_image_storage_counter() } as u16) };
    if dir < saved_dir || (dir == saved_dir && number < saved_file) {
        let selected = unsafe { at(context(), saved_dir + 0x2218).byte() } as u32;
        if selected != group && selected != 255 {
            saved_dir = unsafe { at(context(), 0x2168).half() } as u32;
            saved_file = ((saved_dir % 10) * 1000 + 999) as u16;
        }
    }
    if dir < saved_dir || (dir == saved_dir && number < saved_file) { dir = saved_dir; number = saved_file; }
    if group == 0 {
        let state = unsafe { context() };
        let largest = unsafe { at(state, 0x2168).word() };
        if dir < largest { dir = largest as u16 as u32; number = ((dir % 10) * 1000 + 999) as u16; }
        else if dir == largest && dir != unsafe { at(state, 0x21b8).word() } && (dir > 100 || unsafe { at(state, 0x2794).word() } != 0) { number = ((dir % 10) * 1000 + 999) as u16; }
    }
    unsafe { directory.write(dir as u16); file.write(number) };
    StorageStatus(0)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_read_numbers(directory: *mut u16, file: *mut u16) -> StorageStatus {
    let identifier = unsafe { at(context(), 0x2210).word() };
    let status = unsafe { xm1_pipeline_image_storage_ready(0) };
    if status.0 != 0 { return status; }
    let device = unsafe { xm1_pipeline_image_storage_current_drive() };
    if device == 0 { return StorageStatus(8); }
    let group = unsafe { xm1_pipeline_image_storage_group(identifier) };
    if group == 255 { return StorageStatus(8); }
    unsafe { xm1_pipeline_image_storage_choose_pair(device, group, directory, file) }
}

unsafe fn next_directory(directory: u16) -> u16 {
    let largest = unsafe { at(context(), 0x2168).word() };
    (core::cmp::max(directory as u32, largest).wrapping_add(1)) as u16
}
unsafe fn reset_sequence() { unsafe { at(context(), 0x127d4).write_half(1) }; }
unsafe fn limit_error() -> StorageStatus { unsafe { xm1_pipeline_image_diagnostic_event(2, 0x51) }; StorageStatus(6) }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_debug_basename(path: Address32, directory_name: Address32, prefix: Address32) -> StorageStatus {
    let status = unsafe { xm1_pipeline_image_storage_ready(0) };
    if status.0 != 0 { return status; }
    let group = unsafe { xm1_pipeline_image_storage_group(at(context(), 0x2210).word()) };
    if group == 255 { return StorageStatus(8); }
    let state = unsafe { context() };
    if unsafe { at(state, 0x16bd0).byte() } == 2 {
        unsafe { at(state, 0x127d4).write_half(at(state, 0x127d4).half().wrapping_add(1)); at(state, 0x16bd0).write_byte(1) };
    }
    let mut sequence = unsafe { at(state, 0x127d4).half() };
    let mut original_directory = 0;
    let mut number = 0u16;
    let status = unsafe { xm1_pipeline_image_storage_read_numbers(&mut original_directory, &mut number) };
    if status.0 != 0 { return status; }
    let mut directory = original_directory;
    if directory % 10 == number / 1000 {
        number = number.wrapping_add(1);
        if number % 1000 == 0 || (unsafe { at(context(), 0x16bd0).byte() } != 0 && sequence > 99) {
            directory = unsafe { next_directory(directory) };
            if directory > 999 { return unsafe { limit_error() }; }
            number = (directory % 10) * 1000 + 1;
            sequence = 1;
            unsafe { reset_sequence() };
        }
    } else {
        number = number.wrapping_add(1);
        let mut advance = false;
        if number >= 10000 {
            number = 1;
            directory = unsafe { next_directory(directory) };
            if directory > 999 { return unsafe { limit_error() }; }
            sequence = 1;
            unsafe { reset_sequence() };
            advance = true;
        } else if number % 1000 == 0 { number = number.wrapping_add(1); }
        if number / 1000 != directory % 10 {
            advance |= number / 1000 > directory % 10;
            if advance {
                directory = unsafe { next_directory(original_directory) };
                while directory % 10 != (number % 10000) / 1000 { directory = directory.wrapping_add(1); }
                if directory > 999 { return unsafe { limit_error() }; }
                sequence = 1;
                unsafe { reset_sequence() };
            } else if unsafe { at(context(), 0x16bd0).byte() } != 0 && sequence > 99 {
                directory = unsafe { next_directory(original_directory) };
                if directory > 999 { return unsafe { limit_error() }; }
                number = (directory % 10) * 1000 + 1;
                sequence = 1;
                unsafe { reset_sequence() };
            } else { number = (directory % 10) * 1000 + 1; }
        }
    }
    unsafe {
        xm1_pipeline_image_storage_drive_prefix(path);
        for (index, byte) in b"\\DCIM\\".iter().enumerate() { at(path, 2 + index as u32).write_byte(*byte); }
        xm1_pipeline_image_storage_decimal(at(path, 8), directory as u32, 3, 1);
        let default_name = at(context(), 0x21da + group * 16);
        let name = if directory_name.0 == 0 || xm1_pipeline_image_storage_validate_name(directory_name, 5).0 != 0 { default_name } else { directory_name };
        copy(at(path, 11), name, 5);
        at(path, 16).write_byte(b'\\');
        if prefix.0 != 0 {
            let selected = if xm1_pipeline_image_storage_validate_name(prefix, 4).0 != 0 { context() } else { prefix };
            copy(at(path, 17), selected, 4);
        } else {
            match at(context(), 0x16bd4).word() {
                mode @ (1 | 2) => {
                    let key = 0x142 + (mode - 1) * 2;
                    at(path, 17).write_byte(xm1_buffers_configuration_byte(key) as u8);
                    at(path, 18).write_byte(xm1_buffers_configuration_byte(key + 1) as u8);
                    xm1_pipeline_image_storage_decimal(at(path, 19), sequence as u32, 2, 1);
                }
                0 => copy(at(path, 17), context(), 4),
                _ => {}
            }
        }
        xm1_pipeline_image_storage_decimal(at(path, 21), number as u32, 4, 1);
        at(path, 25).write_byte(0);
    }
    StorageStatus(0)
}

#[repr(C)]
pub struct StorageMessage { code: u16, task: u8, destination: u8, length: u32, payload: Address32 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_open_complete(_status: StorageStatus, handle: u32) {
    unsafe { Address32(0x154c744).write_word(handle); xm1_pipeline_image_debug_complete() };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_complete() {
    let task = unsafe { Address32(0x154d898).word() };
    if task != 0 { unsafe { xm1_pipeline_image_send_notification(task, 0x10) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_debug_file(kind: DebugFileKind, data: Address32, byte_count: u32, number: SuffixNumber, suffix: Address32) {
    unsafe { xm1_pipeline_image_wait_reserved(0x66, u32::MAX) };
    let basename = Address32(0x154cdb0);
    let path = Address32(0x154cce0);
    unsafe {
        xm1_pipeline_image_prepare_debug_basename(basename, Address32(0), Address32(0));
        path.write_byte(0);
        append(path, basename, None);
        append(path, Address32(0xeeb2d0), None);
    }
    let suffix = if suffix.0 == 0 { Address32(0xe540b5u32.wrapping_add(kind.0.wrapping_mul(12))) } else { suffix };
    let width = if (number.0.wrapping_sub(1) as u8) < 9 { 1 } else if (number.0.wrapping_sub(10) as u8) < 90 { 2 } else { 0 };
    if width != 0 {
        let mut formatted = MaybeUninit::<[u8; 4]>::uninit();
        let formatted_address = Address32(formatted.as_mut_ptr() as u32);
        unsafe {
            append(path, suffix, Some(3 - width));
            xm1_pipeline_image_storage_decimal(formatted_address, number.0, width, 4);
            append(path, formatted_address, None);
        }
    } else { unsafe { append(path, suffix, None) }; }
    unsafe { Address32(0x154d898).write_word(xm1_pipeline_image_current_task()) };
    let mut events = 0;
    unsafe {
        xm1_pipeline_image_debug_open(Some(xm1_pipeline_image_debug_open_complete), path, 1);
        xm1_pipeline_image_wait_all(0x10, &mut events, -1);
        events = 0;
        xm1_pipeline_image_debug_write(Some(xm1_pipeline_image_debug_write_complete), Address32(0x154c744).word(), byte_count, data);
        xm1_pipeline_image_wait_all(0x10, &mut events, -1);
        events = 0;
        xm1_pipeline_image_debug_entry(Some(xm1_pipeline_image_debug_entry_complete), Address32(0x154c744).word(), Address32(0), Address32(0));
        xm1_pipeline_image_wait_all(0x10, &mut events, -1);
        xm1_pipeline_image_signal_reserved(0x66);
    }
}

type OpenCompletion = Option<unsafe extern "C" fn(StorageStatus, u32)>;
type EntryCompletion = Option<unsafe extern "C" fn(StorageStatus)>;

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_write_complete(_status: StorageStatus, _count: u32) {
    unsafe { xm1_pipeline_image_debug_complete() };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_entry_complete(_status: StorageStatus) {
    unsafe { xm1_pipeline_image_debug_complete() };
}

unsafe fn submit(code: u16, payload: Address32, length: u32) {
    let message = StorageMessage { code, task: unsafe { xm1_pipeline_image_current_task() } as u8, destination: 0x65, length, payload };
    unsafe { xm1_pipeline_image_storage_submit(&message) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_submit(message: *const StorageMessage) {
    let message = unsafe { &*message };
    let copied = StorageMessage { code: message.code, task: message.task, destination: message.destination, length: message.length, payload: message.payload };
    unsafe { xm1_pipeline_image_send_message(0x65, &copied) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_token() -> u32 {
    if unsafe { xm1_pipeline_image_wait_reserved(0x3c, u32::MAX) } != 0 { return 0; }
    let mut token = unsafe { Address32(0x910200).word() };
    loop {
        token = token.wrapping_add(1);
        if (token as i32) < 0 { token = 1; }
        if !(0..16).any(|index| unsafe { Address32(0x9101c0 + index * 4).word() } == token) { break; }
    }
    unsafe { Address32(0x910200).write_word(token) };
    let mut allocated = 0;
    for index in 0..16 {
        let slot = Address32(0x9101c0 + index * 4);
        if unsafe { slot.word() } == 0 {
            unsafe { slot.write_word(token) };
            allocated = token;
            break;
        }
    }
    unsafe { xm1_pipeline_image_signal_reserved(0x3c) };
    allocated
}

#[repr(C)]
struct OpenPayload {
    callback: OpenCompletion,
    path: [MaybeUninit<u8>; 48],
    position: u32,
    count: u32,
    token: u32,
    wait: u32,
    status: Address32,
    result: Address32,
    flags: u32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_open(callback: OpenCompletion, path: Address32, count: u32) {
    unsafe { xm1_pipeline_image_current_task() };
    if unsafe { Address32(0x8fc8a4).word() } == 1 {
        let matches = (0..21).all(|index| unsafe { at(path, index).byte() == Address32(0x8299fc + index).byte() });
        if matches { unsafe { Address32(0x8fc8b0).write_word(0) }; }
        if let Some(callback) = callback { unsafe { callback(StorageStatus(if matches { 0 } else { 6 }), matches as u32) }; }
        return;
    }
    let status = unsafe { xm1_pipeline_image_storage_ready(0) };
    if status.0 != 0 {
        if let Some(callback) = callback { unsafe { callback(status, 0) }; }
        return;
    }
    if unsafe { at(path, 1).byte() } == b':' {
        let drive = unsafe { path.byte() }.to_ascii_uppercase();
        let status = if !drive.is_ascii_uppercase() { 8 } else {
            let routes = Address32(0x814730);
            let mut device = u32::MAX;
            for index in 0..2 {
                let route = at(routes, index * 8);
                if unsafe { at(route, 1).byte() } == 1 && unsafe { route.byte() } == drive {
                    device = unsafe { at(route, 2).byte() } as u32;
                    break;
                }
            }
            if device == unsafe { at(context(), 0x114).word() } { 0 } else { 5 }
        };
        if status != 0 {
            if let Some(callback) = callback { unsafe { callback(StorageStatus(status), 0) }; }
            return;
        }
    }
    let token = unsafe { xm1_pipeline_image_storage_token() };
    let mut payload = OpenPayload { callback, path: [MaybeUninit::uninit(); 48], position: 0, count, token, wait: 0, status: Address32(0), result: Address32(0), flags: 1 };
    let destination = Address32(payload.path.as_mut_ptr() as u32);
    let mut index = 0;
    loop {
        let byte = unsafe { at(path, index).byte() };
        unsafe { at(destination, index).write_byte(byte) };
        index += 1;
        if byte == 0 { break; }
    }
    unsafe { submit(0x2c17, Address32(&payload as *const OpenPayload as u32), 0x50) };
}

#[repr(C)]
struct WritePayload { callback: OpenCompletion, handle: u32, count: u32, data: Address32, wait: u32, status: Address32, result: Address32 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_write(callback: OpenCompletion, handle: u32, count: u32, data: Address32) {
    let status = unsafe { xm1_pipeline_image_storage_ready(0) };
    if status.0 != 0 {
        if let Some(callback) = callback { unsafe { callback(status, 0) }; }
        return;
    }
    unsafe { xm1_pipeline_image_current_task() };
    let payload = WritePayload { callback, handle, count, data, wait: 0, status: Address32(0), result: Address32(0) };
    unsafe { submit(0x2c19, Address32(&payload as *const WritePayload as u32), 0x1c) };
}

#[repr(C)]
struct EntryPayload { callback: EntryCompletion, handle: u32, request: Address32, path: [MaybeUninit<u8>; 20], wait: u32, status: Address32 }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_debug_entry(callback: EntryCompletion, handle: u32, request: Address32, path: Address32) {
    if unsafe { Address32(0x8fc8a4).word() } == 1 {
        if let Some(callback) = callback { unsafe { callback(StorageStatus(0)) }; }
        return;
    }
    let status = unsafe { xm1_pipeline_image_storage_ready(0) };
    if status.0 != 0 {
        if let Some(callback) = callback { unsafe { callback(status) }; }
        return;
    }
    unsafe { xm1_pipeline_image_current_task() };
    let mut payload = EntryPayload { callback, handle, request, path: [MaybeUninit::uninit(); 20], wait: 0, status: Address32(0) };
    if path.0 == 0 { payload.path[0].write(0); }
    else {
        let destination = Address32(payload.path.as_mut_ptr() as u32);
        let mut index = 0;
        loop {
            let byte = unsafe { at(path, index).byte() };
            unsafe { at(destination, index).write_byte(byte) };
            index += 1;
            if byte == 0 { break; }
        }
    }
    unsafe { submit(0x2c1a, Address32(&payload as *const EntryPayload as u32), 0x28) };
}

unsafe fn interface_method(interface: u32, offset: u32) -> u32 {
    let methods = Address32(unsafe { Address32(0x96bd20 + interface * 4).word() });
    unsafe { at(methods, offset).word() }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_storage_dispatch_error(status: i32) {
    let report: unsafe extern "C" fn(i32) = unsafe { core::mem::transmute(Address32(0x4089ea8c).word()) };
    unsafe { report(status) };
}

unsafe fn queue_error(channel: u32, status: i32) -> i32 {
    unsafe {
        xm1_pipeline_debug_kernel_error(0x1c, channel, status);
        Address32(0x40a0f82c).write_word(status as u32);
        xm1_pipeline_image_storage_dispatch_error(status);
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_send_message(channel: u32, message: *const StorageMessage) -> i32 {
    let current: unsafe extern "C" fn() -> u32 = unsafe { core::mem::transmute(interface_method(0, 0x1618)) };
    let interface = unsafe { current() };
    let message = unsafe { &*message };
    let limit = unsafe { Address32(0x40a0fa4c + channel * 4).word() }.wrapping_sub(8);
    if message.length > limit { return unsafe { queue_error(channel, -60) }; }
    let trace: unsafe extern "C" fn(*const StorageMessage) = unsafe { core::mem::transmute(Address32(0x4089ea94).word()) };
    unsafe { trace(message) };
    let id = channel as u16 as u32;
    let namespace = unsafe { Address32(0x7cad9c + id.wrapping_sub(1) * 4).word() };
    let tag = id | namespace.wrapping_shl(16);
    let allocate: unsafe extern "C" fn(u32, *mut Address32) -> i32 = unsafe { core::mem::transmute(interface_method(interface, 0x170)) };
    let mut buffer = Address32(0);
    let status = unsafe { allocate(tag, &mut buffer) };
    if status != 0 { return unsafe { queue_error(channel, if status == -50 { -62 } else { status }) }; }
    unsafe {
        at(buffer, 4).write_half(message.code);
        at(buffer, 6).write_byte(message.task);
        at(buffer, 7).write_byte(message.destination);
        at(buffer, 8).write_word(message.length);
        copy(at(buffer, 12), message.payload, message.length);
    }
    let mode: unsafe extern "C" fn() -> i32 = unsafe { core::mem::transmute(interface_method(interface, 0x148)) };
    let method = if unsafe { mode() } == 1 { 0xb0 } else { 0xac };
    let send: unsafe extern "C" fn(u32, Address32) -> i32 = unsafe { core::mem::transmute(interface_method(interface, method)) };
    let status = unsafe { send(tag, buffer) };
    if status != 0 {
        unsafe { xm1_pipeline_debug_kernel_error(0x1c, channel, status) };
        let release: unsafe extern "C" fn(u32, Address32) -> i32 = unsafe { core::mem::transmute(interface_method(interface, 0xf8)) };
        unsafe {
            release(tag, buffer);
            Address32(0x40a0f82c).write_word(status as u32);
            xm1_pipeline_image_storage_dispatch_error(status);
        }
    }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_receive_message(channel: u32, message: *mut StorageMessage, timeout: i32) -> i32 {
    let current: unsafe extern "C" fn() -> u32 = unsafe { core::mem::transmute(interface_method(0, 0x1618)) };
    let interface = unsafe { current() };
    let id = channel as u16 as u32;
    let namespace = unsafe { Address32(0x7cad9c + id.wrapping_sub(1) * 4).word() };
    let tag = id | namespace.wrapping_shl(16);
    let receive: unsafe extern "C" fn(u32, *mut Address32, i32) -> i32 = unsafe { core::mem::transmute(interface_method(interface, 0xc8)) };
    let mut buffer = Address32(0);
    let status = unsafe { receive(tag, &mut buffer, timeout) };
    if status == -50 { return status; }
    if status != 0 {
        unsafe {
            xm1_pipeline_debug_kernel_error(0x1f, channel, status);
            Address32(0x40a0f82c).write_word(status as u32);
            xm1_pipeline_image_storage_dispatch_error(status);
        }
        return status;
    }
    let destination = Address32(message as u32);
    unsafe {
        destination.write_half(at(buffer, 4).half());
        at(destination, 2).write_byte(at(buffer, 6).byte());
        at(destination, 3).write_byte(at(buffer, 7).byte());
        at(destination, 4).write_word(at(buffer, 8).word());
        let length = at(buffer, 8).word();
        if length != 0 { copy(Address32(at(destination, 8).word()), at(buffer, 12), length); }
    }
    let release: unsafe extern "C" fn(u32, Address32) -> i32 = unsafe { core::mem::transmute(interface_method(interface, 0xf8)) };
    unsafe { release(tag, buffer) };
    0
}
