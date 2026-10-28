#![no_std]
#![allow(unexpected_cfgs)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { xm1_panic_loop() }
}

extern "C" {
    fn xm1_panic_loop() -> !;
}

#[repr(C)]
pub struct Pair {
    pub first: u32,
    pub second: u32,
}

#[no_mangle]
pub extern "C" fn xm1_pilot_add_u32(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

#[cfg(not(pilot_abi_defect))]
#[no_mangle]
pub extern "C" fn xm1_pilot_add_u64(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
}

#[cfg(pilot_abi_defect)]
#[no_mangle]
pub extern "C" fn xm1_pilot_add_u64(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

#[no_mangle]
pub extern "C" fn xm1_pilot_mixed(a: u32, b: u64, c: u32) -> u64 {
    (a as u64).wrapping_add(b).wrapping_add(c as u64)
}

#[no_mangle]
pub extern "C" fn xm1_pilot_many_args(
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
    f: u32,
    g: u32,
    h: u32,
) -> u32 {
    a ^ b ^ c ^ d ^ e ^ f ^ g ^ h
}

#[no_mangle]
pub extern "C" fn xm1_pilot_pair(a: u32, b: u32) -> Pair {
    Pair {
        first: a.wrapping_mul(3),
        second: b.wrapping_add(7),
    }
}

#[no_mangle]
pub extern "C" fn xm1_pilot_pair_sum(p: Pair) -> u32 {
    p.first.wrapping_add(p.second)
}

#[no_mangle]
pub extern "C" fn xm1_pilot_pair_swap(p: Pair) -> Pair {
    Pair {
        first: p.second,
        second: p.first,
    }
}

/// # Safety
/// `bytes` must be readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn xm1_pilot_checksum(bytes: *const u8, len: u32) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    let mut index = 0u32;
    while index < len {
        let byte = unsafe { core::ptr::read_volatile(bytes.add(index as usize)) };
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
        index += 1;
    }
    hash
}

/// # Safety
/// `out` must be writable for one `u32`.
#[no_mangle]
pub unsafe extern "C" fn xm1_pilot_store(out: *mut u32, value: u32) {
    unsafe { core::ptr::write_volatile(out, value) }
}

mod pilot_extra;

#[inline(never)]
fn mix(value: u32) -> u32 {
    value.rotate_left(5) ^ 0x9e37_79b9
}

#[no_mangle]
pub extern "C" fn xm1_pilot_preserve_pressure(a: u32, b: u32, c: u32) -> u32 {
    let x = mix(a);
    let y = mix(b);
    let z = mix(c);
    x.wrapping_add(y).wrapping_add(z)
}
