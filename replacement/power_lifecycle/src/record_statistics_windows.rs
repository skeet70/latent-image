use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Mode(u32);
#[repr(C)]
struct CropInput { mode: u32, kind: u32, first: u32, second: u32, ratio: u32, enabled: u32 }

extern "C" {
    fn xm1_power_record_helper_ef6030(input: *const CropInput, output: *mut u32);
    fn xm1_power_record_helper_ee81bc(input: *const u32, dimensions: *const u32, output: *mut u32);
    fn xm1_power_record_helper_ef5ab8(mode: u32, index: u32) -> u32;
    fn xm1_power_record_helper_ef5cc4(mode: u32, index: u32) -> u32;
    fn xm1_power_record_helper_eda668(kind: u32, profile: *const u8, options: *const u16, first: u32, second: u32, third: u32, output: *mut u8) -> u32;
    fn xm1_power_record_helper_4d978(numerator: u32, denominator: u32) -> u32;
}

unsafe fn residue<const N: usize>() -> [u32; N] {
    let slots = core::mem::MaybeUninit::<[u32; N]>::uninit();
    let mut values = [0; N];
    for (index, value) in values.iter_mut().enumerate() {
        unsafe { core::arch::asm!("ldr {value}, [{slot}]", value = out(reg) *value, slot = in(reg) slots.as_ptr().cast::<u32>().add(index), options(readonly, nostack, preserves_flags)); }
    }
    values
}
unsafe fn load(input: *const u32, index: usize) -> u32 { unsafe { read_volatile(input.add(index)) } }
unsafe fn store(output: *mut u32, index: usize, value: u32) { unsafe { write_volatile(output.add(index), value); } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eed9c4(input: *const u32, output: *mut u32) {
    let dimensions = unsafe { [read_volatile(0x4171_7790 as *const u32), read_volatile(0x4171_7794 as *const u32)] };
    let mut rectangle = unsafe { residue::<4>() };
    unsafe { xm1_power_record_helper_ee81bc(input, dimensions.as_ptr(), rectangle.as_mut_ptr()); }
    for (index, value) in rectangle.iter().enumerate() { unsafe { store(output, index, *value); } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eeda24(output: *mut u32, mode: u32, query: *const u32) {
    let mode = Mode(mode);
    let request = CropInput { mode: mode.0, kind: unsafe { load(query, 0) }, first: unsafe { load(query, 1) }, second: unsafe { load(query, 2) }, ratio: 0x0004_0003, enabled: 1 };
    let mut crop = unsafe { residue::<8>() };
    unsafe { xm1_power_record_helper_ef6030(&request, crop.as_mut_ptr()); }
    let mut scaled = unsafe { residue::<4>() };
    unsafe { xm1_power_record_helper_eed9c4(crop.as_ptr(), scaled.as_mut_ptr()); }
    unsafe { store(output, 0, crop[2]); store(output, 1, crop[3]); }
    for (index, value) in scaled.iter().enumerate() {
        unsafe { store(output, index + 2, *value); store(output, index + 6, *value); }
    }
    let first = unsafe { xm1_power_record_helper_ef5ab8(mode.0, 1) };
    let second = unsafe { xm1_power_record_helper_ef5cc4(mode.0, 1) };
    let packed = unsafe { load(query, 3) };
    let low = packed & 0xffff;
    let high = packed >> 16;
    if packed == 0x0001_0001 {
        let numerator = second.wrapping_mul(unsafe { load(output, 8) }).wrapping_mul(high);
        let value = unsafe { xm1_power_record_helper_4d978(numerator, low.wrapping_mul(first)) }.min(unsafe { load(output, 9) });
        unsafe {
            store(output, 5, value);
            store(output, 2, load(output, 2).wrapping_add(load(output, 9).wrapping_sub(value) >> 1));
        }
    } else {
        let numerator = first.wrapping_mul(unsafe { load(output, 9) }).wrapping_mul(low);
        let value = unsafe { xm1_power_record_helper_4d978(numerator, high.wrapping_mul(second)) }.min(unsafe { load(output, 8) });
        unsafe {
            store(output, 4, value);
            store(output, 3, load(output, 3).wrapping_add(load(output, 8).wrapping_sub(value) >> 1));
        }
    }
    unsafe {
        store(output, 2, load(output, 2).wrapping_sub(crop[0]));
        store(output, 3, load(output, 3).wrapping_sub(crop[1]));
        store(output, 6, load(output, 6).wrapping_sub(crop[0]));
        store(output, 7, load(output, 7).wrapping_sub(crop[1]));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_eedb98(output: *mut u32, mode: u32, query: *const u32) {
    unsafe { xm1_power_record_helper_eeda24(output, mode, query); }
    let mut profile = [0u32; 18];
    profile[0] = mode;
    profile[4] = unsafe { load(query, 0) };
    profile[5] = unsafe { load(query, 1) };
    profile[6] = unsafe { load(query, 2) };
    unsafe { core::ptr::write_unaligned(profile.as_mut_ptr().cast::<u8>().add(30).cast::<u32>(), load(query, 3)); }
    let options = [100u16, 100, 16, 0, 0, 0];
    let mut result = unsafe { residue::<46>() };
    unsafe { xm1_power_record_helper_eda668(2, profile.as_ptr().cast(), options.as_ptr(), 0, 0, 0, result.as_mut_ptr().cast()); }
    let result_bytes = result.as_ptr().cast::<u8>();
    let width = result[16].wrapping_mul(unsafe { read_volatile(result_bytes.add(85)) } as u32).min(unsafe { load(output, 4) });
    let height = result[17].wrapping_mul(unsafe { read_volatile(result_bytes.add(84)) } as u32).min(unsafe { load(output, 5) });
    unsafe {
        store(output, 4, width);
        store(output, 5, height);
        store(output, 2, load(output, 1).wrapping_sub(height) >> 1);
        store(output, 3, load(output, 0).wrapping_sub(width) >> 1);
    }
}
