#![no_std]
#![allow(unexpected_cfgs)]

const MAILBOX_ADDR: usize = 0x0021_5000;

#[cfg(xm1_storage_model_faulty)]
const KIND_THREE_VARIANT: u32 = 0;
#[cfg(not(xm1_storage_model_faulty))]
const KIND_THREE_VARIANT: u32 = 1;

#[cfg(xm1_freestanding)]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[inline(always)]
unsafe fn mailbox(index: usize) -> u32 {
    unsafe { core::ptr::read_volatile((MAILBOX_ADDR as *mut u32).add(index)) }
}

#[inline(always)]
unsafe fn set_mailbox(index: usize, value: u32) {
    unsafe { core::ptr::write_volatile((MAILBOX_ADDR as *mut u32).add(index), value) }
}

#[inline(always)]
unsafe fn fault(code: u32) {
    unsafe { set_mailbox(14, code) }
}

#[inline(always)]
unsafe fn descriptor_ok(descriptor: *mut u32) -> bool {
    !descriptor.is_null()
        && (descriptor as usize) % 4 == 0
        && descriptor as usize == unsafe { mailbox(13) } as usize
}

#[inline(always)]
fn callback_word(cb: Option<unsafe extern "C" fn(i32)>) -> u32 {
    match cb {
        Some(callback) => callback as usize as u32,
        None => 0,
    }
}

#[inline(always)]
unsafe fn deliver(cb: Option<unsafe extern "C" fn(i32)>, code: i32) {
    if let Some(callback) = cb {
        unsafe { callback(code) }
    }
}

#[link_section = ".svc5600"]
#[no_mangle]
pub unsafe extern "C" fn svc_5600(
    mode: u32,
    descriptor: *mut u32,
    cb: Option<unsafe extern "C" fn(i32)>,
) {
    unsafe { storage_common(0, mode, descriptor, cb) }
}

#[link_section = ".svc5688"]
#[no_mangle]
pub unsafe extern "C" fn svc_5688(
    mode: u32,
    descriptor: *mut u32,
    cb: Option<unsafe extern "C" fn(i32)>,
) {
    unsafe { storage_common(KIND_THREE_VARIANT, mode, descriptor, cb) }
}

#[link_section = ".svc5a00"]
#[inline(never)]
unsafe fn storage_common(
    variant: u32,
    mode: u32,
    descriptor: *mut u32,
    cb: Option<unsafe extern "C" fn(i32)>,
) {
    let descriptor_ok = unsafe { descriptor_ok(descriptor) };
    if mode != 1 {
        unsafe { fault(3) };
        return;
    }
    if !descriptor_ok {
        unsafe { fault(4) };
        return;
    }
    if cb.is_none() {
        unsafe { fault(2) };
        return;
    }
    unsafe { set_mailbox(18, callback_word(cb)) };
    unsafe { core::ptr::write_volatile(descriptor.add(0x14c / 4), variant) };
    let count = unsafe { mailbox(3) };
    let mut index = 0;
    while index < count {
        let code = if index == 0 {
            unsafe { mailbox(4) as i32 }
        } else {
            unsafe { mailbox(5) as i32 }
        };
        unsafe { deliver(cb, code) };
        index += 1;
    }
}

#[link_section = ".svc5714"]
#[no_mangle]
pub unsafe extern "C" fn svc_5714(cb: Option<unsafe extern "C" fn()>) {
    if cb.is_none() {
        unsafe { fault(9) };
        return;
    }
    let pointer = match cb {
        Some(callback) => callback as usize as u32,
        None => 0,
    };
    unsafe { set_mailbox(19, pointer) };
    let count = unsafe { mailbox(6) };
    let mut index = 0;
    while index < count {
        if let Some(callback) = cb {
            unsafe { callback() }
        }
        index += 1;
    }
}

#[link_section = ".svcwait"]
#[no_mangle]
pub unsafe extern "C" fn svc_wait(mask: u32, out: *mut u32, timeout: u32) -> i32 {
    let base = unsafe { mailbox(16) } as usize;
    let top = unsafe { mailbox(17) } as usize;
    let address = out as usize;
    if mask != 0x30 && mask != 0x10 {
        unsafe { fault(5) };
        return 0;
    }
    if timeout != 0xffff_ffff {
        unsafe { fault(6) };
        return 0;
    }
    if out.is_null() {
        unsafe { fault(10) };
        return 0;
    }
    if address % 4 != 0 {
        unsafe { fault(11) };
        return 0;
    }
    if address < base || address + 4 > top {
        unsafe { fault(12) };
        return 0;
    }
    let phase_one = mask == 0x30;
    let flags = if phase_one {
        unsafe { mailbox(7) }
    } else {
        unsafe { mailbox(12) }
    };
    unsafe { core::ptr::write_volatile(out, flags) };
    0
}

#[link_section = ".svcstage"]
#[no_mangle]
pub unsafe extern "C" fn svc_stage(a: u32, b: u32, _c: u32, _d: u32) {
    if a != 1 || (b != 8 && b != 10) {
        unsafe { fault(8) };
        return;
    }
    if b == 8 {
        unsafe { set_mailbox(9, (a << 8) | b) };
    } else {
        unsafe { set_mailbox(10, (a << 8) | b) };
    }
}
