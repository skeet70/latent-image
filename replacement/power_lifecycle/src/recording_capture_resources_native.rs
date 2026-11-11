use crate::recording_capture_resources::{self as pools, CapturePoolCount};

const POOLS: u32 = 0x015b_7428;
const ROOT: u32 = 0x015b_5be8;

fn word(address: u32) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
fn put(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value) }; }
fn half(address: u32) -> i16 { unsafe { core::ptr::read_volatile(address as *const i16) } }
fn put_half(address: u32, value: i16) { unsafe { core::ptr::write_volatile(address as *mut i16, value) }; }
fn halt() -> ! { loop { core::hint::spin_loop(); } }
fn clear(address: u32, length: u32) {
    for index in 0..length { unsafe { core::ptr::write_volatile((address + index) as *mut u8, 0) }; }
}

fn acquire(pool: u32) -> i16 {
    let count = half(pool) as u16;
    let Some(count) = CapturePoolCount::new(count) else { halt(); };
    let bitmap = unsafe { &mut *((pool + 8) as *mut [u32; 10]) };
    pools::acquire_bitmap(count, bitmap).map_or(-1, |index| index.0 as i16)
}

fn resolve(index: i16, offset: u32) -> u32 {
    if index == -1 { 0 } else if index == 0 { ROOT + offset } else {
        word(ROOT + 0x17d0).wrapping_add((i32::from(index) - 1).wrapping_mul(0x17d0) as u32).wrapping_add(offset)
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb25d8() {
    for pool in 0..13 {
        let address = POOLS + pool * 0x30;
        let count = if pool == 12 { 1 } else { 202 };
        put_half(address, count);
        pools::initialize_bitmap(CapturePoolCount::new(count as u16).unwrap(), unsafe { &mut *((address + 8) as *mut [u32; 10]) });
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb26ac(secondary: u32) -> u32 {
    let index = acquire(POOLS);
    if secondary == 0 && index != 0 || secondary != 0 && index == 0 { halt(); }
    let address = if secondary == 0 { ROOT } else { resolve(index, 0) };
    if address == 0 { return 0; }
    put_half(address, index);
    for offset in (2..=0x18).step_by(2) { put_half(address + offset, -1); }
    put(address + 0x1c, 0);
    put(address + 0x20, 0x80);
    put(address + 0x24, 0);
    put(address + 0x28, 2);
    address
}

fn acquire_resource(handle: u32, field: u32, pool: u32, offset: u32, length: u32) -> u32 {
    let current = half(handle + field);
    if current != -1 { return resolve(current, offset); }
    let allocated = acquire(POOLS + pool * 0x30);
    put_half(handle + field, allocated);
    let address = resolve(allocated, offset);
    if address != 0 { clear(address, length); }
    address
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb276c(handle: u32) -> u32 { acquire_resource(handle, 2, 1, 0x30, 0x90) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb288c(handle: u32) -> u32 { acquire_resource(handle, 4, 2, 0xc0, 0xc) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb2838(handle: u32) -> u32 { resolve(half(handle + 2), 0x30) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb2958(handle: u32) -> u32 { resolve(half(handle + 4), 0xc0) }

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb34bc(handle: u32) -> u32 {
    let current = half(handle + 0x18);
    if current != -1 { return 0x015b_73bc_u32.wrapping_add(i32::from(current).wrapping_mul(0x68) as u32); }
    let allocated = acquire(POOLS + 12 * 0x30);
    put_half(handle + 0x18, allocated);
    if allocated == -1 { return 0; }
    let address = 0x015b_73bc_u32.wrapping_add(i32::from(allocated).wrapping_mul(0x68) as u32);
    clear(address, 0x68);
    address
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_eb35ec(handle: u32) -> u32 {
    let index = half(handle + 0x18);
    if index == -1 { 0 } else { 0x015b_73bc_u32.wrapping_add(i32::from(index).wrapping_mul(0x68) as u32) }
}
