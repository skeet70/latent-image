#![no_std]
use core::ptr::{read_volatile, write_volatile};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

unsafe fn get(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

#[no_mangle]
#[link_section = ".text.copy"]
pub unsafe extern "C" fn copy(dst: *mut u8, src: *const u8, length: usize) -> *mut u8 {
    for index in 0..length {
        unsafe { write_volatile(dst.add(index), read_volatile(src.add(index))) };
    }
    dst
}

#[no_mangle]
#[link_section = ".text.append"]
pub unsafe extern "C" fn append(dst: *mut u8, src: *const u8, length: usize) -> *mut u8 {
    let mut end = 0;
    while unsafe { read_volatile(dst.add(end)) } != 0 { end += 1; }
    let mut copied = 0;
    while copied < length {
        let byte = unsafe { read_volatile(src.add(copied)) };
        unsafe { write_volatile(dst.add(end + copied), byte) };
        if byte == 0 { return dst; }
        copied += 1;
    }
    unsafe { write_volatile(dst.add(end + copied), 0) };
    dst
}
#[no_mangle]
#[link_section = ".text.decimal"]
pub unsafe extern "C" fn decimal(dst: *mut u8, value: u32, width: u32, flags: u32) -> u32 {
    let mut digits = [0u8; 10];
    let mut remaining = value;
    let mut count = 0;
    while remaining != 0 {
        digits[count] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        count += 1;
    }
    if width < count as u32 { return u32::MAX; }
    let offset = if flags < 4 { width as usize - count } else { 0 };
    if flags < 4 {
        for index in 0..width as usize {
            unsafe { write_volatile(dst.add(index), if flags < 2 { b'0' } else { b' ' }) };
        }
    }
    for index in 0..count {
        unsafe { write_volatile(dst.add(offset + index), digits[count - index - 1]) };
    }
    if flags & 1 == 0 {
        unsafe { write_volatile(dst.add(offset + count), 0) };
    }
    count as u32
}

#[no_mangle]
#[link_section = ".text.receive"]
pub unsafe extern "C" fn receive(channel: u32, frame: *mut u8, timeout: u32) {
    if channel != 9 || timeout != u32::MAX { loop {} }
    let count = unsafe { get(0x009a3000) };
    unsafe { put(0x009a3000, count + 1) };
    if count != 0 {
        unsafe { core::arch::asm!("bx {exit}", exit = in(reg) 0x009dfff0u32, options(noreturn)) }
    }
    let destination = unsafe { read_volatile(frame.add(8).cast::<*mut u8>()) };
    unsafe { copy(destination, 0x009a2000 as *const u8, 56) };
}

#[no_mangle]
#[link_section = ".text.fill"]
pub unsafe extern "C" fn fill(dst: *mut u8, value: u32, length: usize) {
    for index in 0..length { unsafe { write_volatile(dst.add(index), value as u8) }; }
}

#[inline(never)]
unsafe fn storage(mode: u32, descriptor: *mut u8, callback: unsafe extern "C" fn(i32)) {
    if mode != 1 || descriptor as usize as u32 != unsafe { get(0x009a3024) } || callback as usize as u32 != unsafe { get(0x009a3014) } { loop {} }
    unsafe { put(0x009a301c, get(0x009a301c) + 1) };
    unsafe { callback(get(0x009a3010) as i32) };
}

#[no_mangle]
#[link_section = ".text.storage_direct"]
pub unsafe extern "C" fn storage_direct(mode: u32, descriptor: *mut u8, callback: unsafe extern "C" fn(i32)) {
    unsafe { storage(mode, descriptor, callback) };
}

#[no_mangle]
#[link_section = ".text.storage_alternate"]
pub unsafe extern "C" fn storage_alternate(mode: u32, descriptor: *mut u8, callback: unsafe extern "C" fn(i32)) {
    unsafe { storage(mode, descriptor, callback) };
}

#[no_mangle]
#[link_section = ".text.storage_release"]
pub unsafe extern "C" fn storage_release(callback: unsafe extern "C" fn()) {
    if callback as usize as u32 != unsafe { get(0x009a3018) } { loop {} }
    unsafe { put(0x009a3020, get(0x009a3020) + 1) };
    unsafe { callback() };
}

#[repr(transparent)]
struct FrameSlot(u32);

#[repr(transparent)]
struct FrameKind(u32);

#[no_mangle]
#[link_section = ".text.frame_kind"]
pub unsafe extern "C" fn frame_kind(slot: u32) -> u32 {
    let slot = FrameSlot(slot);
    let count = unsafe { get(0x009a3030) };
    if slot.0 >= 201 || count >= 201 { loop {} }
    let kind = FrameKind(unsafe { get(0x009a3040 + slot.0 as usize * 4) });
    let row = 0x009a3400 + count as usize * 8;
    unsafe {
        put(row, slot.0);
        put(row + 4, kind.0);
        put(0x009a3030, count + 1);
    }
    kind.0
}

