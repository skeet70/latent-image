use core::ptr::{read_volatile, write_volatile};

extern "C" { fn xm1_power_configuration_byte(offset: u32) -> u32; }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f5eba0(raw: *mut u32, tenths: *mut u32, doubled: *mut u32, copy: *mut u32, width: u32, height: u32) {
    if raw.is_null() || tenths.is_null() || doubled.is_null() || copy.is_null() { return; }
    let area = width.wrapping_mul(height);
    unsafe {
        let high = xm1_power_configuration_byte(0x639c);
        let middle = xm1_power_configuration_byte(0x639b);
        let low = xm1_power_configuration_byte(0x639a);
        let rate = (high << 16) | (middle << 8) | low;
        write_volatile(raw, rate);
        let scaled = rate / 10;
        write_volatile(tenths, scaled);
        write_volatile(doubled, scaled << 1);
        write_volatile(copy, read_volatile(tenths));
        write_volatile(raw, area.wrapping_mul(read_volatile(raw)));
        write_volatile(tenths, area.wrapping_mul(read_volatile(tenths)));
        write_volatile(doubled, area.wrapping_mul(read_volatile(doubled)));
        write_volatile(copy, area.wrapping_mul(read_volatile(copy)));
    }
}
